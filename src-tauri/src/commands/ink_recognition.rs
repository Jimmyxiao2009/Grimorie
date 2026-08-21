//! Ink intelligence commands — the IPC surface for handwriting recognition.
//!
//! Thin adapters, like the rest of this module: parse identifiers, call the
//! recognition queue or repository, return. The debounce, cancellation, and
//! stale-rejection rules live in [`crate::ai::recognition_queue`]; the
//! persistence rules in [`crate::repositories::ink_recognition`]. This is only
//! the boundary between them and the frontend.
//!
//! Two privacy guarantees are enforced here:
//!
//! * Automatic recognition checks `ai_enabled` and `ink_auto_recognition`
//!   before scheduling, so ink is never sent to a remote model silently.
//! * A manual recognize bypasses those checks — the writer asked — but still
//!   resolves the provider credential through the same secure path as every
//!   other AI action.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::ai::recognition_queue::InkSnapshot;
use crate::ai::recognizers::VisionRecognizer;
use crate::app_state::AppState;
use crate::credentials;
use crate::domain::annotation::AnnotationKind;
use crate::domain::ids::AnnotationId;
use crate::domain::ink_recognition::{
    InkRecognitionRecord, InkRecognizer, LanguageHint, RecognitionStatus,
};
use crate::error::{AppError, ErrorCode, Result};
use crate::repositories;

use super::parse_page_id;

/// A snapshot of an ink note as the frontend knows it: its strokes, and the
/// raster the frontend rendered for recognition (when automatic recognition is
/// on and the frontend chose to render).
///
/// The strokes are the source of truth and always travel; the raster is an
/// optional recognition input artifact that is never persisted. The frontend
/// sends both so the queue can validate against the live strokes even when the
/// recognizer consumes the image.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InkSnapshotInput {
    pub strokes: Vec<super::ink::StrokeInput>,
    pub surface_width: f32,
    #[serde(default)]
    pub png: Option<Vec<u8>>,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

impl InkSnapshotInput {
    fn into_snapshot(self) -> Result<InkSnapshot> {
        let strokes = self
            .strokes
            .into_iter()
            .map(super::ink::StrokeInput::into_domain)
            .collect::<Result<Vec<_>>>()?;
        Ok(InkSnapshot {
            strokes,
            surface_width: self.surface_width,
            png: self.png,
            width: self.width,
            height: self.height,
        })
    }
}

/// One note's recognition state, as the frontend renders it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InkRecognitionView {
    pub annotation_id: String,
    pub status: RecognitionStatus,
    pub recognized_text: Option<String>,
    pub confidence: Option<f32>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub language: Option<String>,
    pub transcript_source: String,
    pub content_hash: Option<String>,
    pub error: Option<String>,
    pub recognized_at: Option<String>,
    pub updated_at: String,
}

impl InkRecognitionView {
    fn from_record(record: InkRecognitionRecord) -> Self {
        Self {
            annotation_id: record.annotation_id.to_string(),
            status: record.status,
            recognized_text: record.recognized_text,
            confidence: record.confidence,
            provider: record.provider,
            model: record.model,
            language: record.language,
            transcript_source: record.transcript_source.as_str().to_string(),
            content_hash: record.content_hash,
            error: record.error,
            recognized_at: record.recognized_at.map(|t| t.to_rfc3339()),
            updated_at: record.updated_at.to_rfc3339(),
        }
    }
}

/// The event a view listens on to follow a recognition job.
///
/// One event carries one note's whole recognition row, so a listener replaces
/// state rather than patching it — there is no ordering hazard if two updates
/// arrive close together.
pub const STATUS_EVENT: &str = "ink:recognition-status";

/// Emits a recognition row to the frontend. Installed as the queue's status
/// sink at startup, so automatic and manual jobs report identically.
///
/// A failed emit is ignored: the window may be closing, and a recognition that
/// already persisted must not be treated as failed because nobody was listening.
pub fn emit_status(app: &AppHandle, record: InkRecognitionRecord) {
    let _ = app.emit(STATUS_EVENT, InkRecognitionView::from_record(record));
}

/// Builds a vision recognizer from the live provider config, or returns an error
/// explaining what is missing. Used by both automatic and manual recognition, so
/// the credential resolution path is identical.
///
/// Takes the shared transport (`provider`) so the one HTTP client is reused
/// across actions, rather than each recognition negotiating a new connection.
fn build_recognizer(
    conn: &rusqlite::Connection,
    settings: &crate::domain::settings::AppSettings,
    provider: Arc<dyn crate::ai::AiProvider>,
) -> Result<Arc<dyn InkRecognizer>> {
    if !settings.ai_enabled {
        return Err(AppError::new(
            ErrorCode::Conflict,
            "AI is turned off. Turn it on in Settings if you want to recognise handwriting.",
        ));
    }

    let config = repositories::ai::active_provider(conn)?;
    let api_key = credentials::get(&config.credential_ref)?;

    let recognizer = VisionRecognizer::new(
        provider,
        config.base_url,
        config.model,
        api_key,
        config.temperature as f32,
    );

    // A vision-specific model, when the writer chose one for handwriting.
    let recognizer = if settings.ink_recognition_model.trim().is_empty() {
        recognizer
    } else {
        recognizer.with_model(settings.ink_recognition_model.clone())
    };

    Ok(Arc::new(recognizer))
}

/// Schedules automatic recognition for a note, after the debounce window.
///
/// Respects `ai_enabled` and `ink_auto_recognition`: if either is off, nothing
/// is scheduled and no error is raised — the writer simply does not get
/// automatic recognition. The note's recognition row is left as-is (pending or
/// disabled), and the frontend can still recognise manually.
///
/// Returns the content hash of the snapshot, so the frontend can mark a note
/// stale when its ink changes before recognition completes.
#[tauri::command]
pub async fn ink_recognize(
    state: State<'_, AppState>,
    annotation_id: String,
    snapshot: InkSnapshotInput,
) -> Result<String> {
    let annotation = AnnotationId::parse(&annotation_id)?;
    let snapshot = snapshot.into_snapshot()?;

    let (_settings, can_auto) = state
        .read(|conn| {
            let settings = repositories::settings::load(conn)?;
            let can_auto = settings.ai_enabled && settings.ink_auto_recognition;
            Ok::<_, AppError>((settings, can_auto))
        })
        .await?;

    let hash = snapshot.content_hash();

    // Always invalidate, so a note that changed but cannot auto-recognise still
    // shows as stale rather than falsely current.
    state
        .write(move |tx| repositories::ink_recognition::invalidate(tx, annotation))
        .await?;

    if !can_auto {
        // Automatic recognition is off. Mark the note disabled (unless it
        // already has a transcript the writer wants to keep) and stop here.
        state
            .write(move |tx| repositories::ink_recognition::disable(tx, annotation))
            .await?;
        return Ok(hash);
    }

    let bookkeeping = Arc::clone(state.recognition());
    let state_for_build = Arc::new(state.inner().clone());

    // The recognizer is built after the debounce elapses, so it reflects the
    // provider config as it stands at run time — a writer who reconfigured
    // their provider during the debounce gets the new one. The future reads
    // settings and resolves the credential through the blocking DB pool, then
    // builds the vision recognizer against the shared HTTP client.
    let build = async move {
        let provider = state_for_build.provider();
        state_for_build
            .read(move |conn| {
                let settings = repositories::settings::load(conn)?;
                build_recognizer(conn, &settings, provider)
            })
            .await
    };

    bookkeeping
        .schedule_auto(Arc::new(state.inner().clone()), annotation, snapshot, build)
        .await;

    Ok(hash)
}

/// Runs recognition immediately, ignoring the debounce and the automatic
/// settings. Used by the explicit "Recognize handwriting" action.
///
/// Returns the committed row so the caller can show the transcript at once. The
/// queue's status sink has already published the same row on
/// [`STATUS_EVENT`], so any other open view updates without a second,
/// differently-shaped event to reconcile.
#[tauri::command]
pub async fn ink_recognize_manual(
    state: State<'_, AppState>,
    annotation_id: String,
    snapshot: InkSnapshotInput,
) -> Result<InkRecognitionView> {
    let annotation = AnnotationId::parse(&annotation_id)?;
    let snapshot = snapshot.into_snapshot()?;

    let provider = state.provider();
    let recognizer = state
        .read(move |conn| {
            let settings = repositories::settings::load(conn)?;
            build_recognizer(conn, &settings, provider)
        })
        .await?;

    let bookkeeping = Arc::clone(state.recognition());
    bookkeeping
        .recognize_now(
            Arc::new(state.inner().clone()),
            recognizer,
            annotation,
            snapshot,
        )
        .await?;

    // Re-read the committed row rather than returning the recognizer's result:
    // the two differ when the writer's own transcript took priority, and what
    // the Margin should show is what was stored.
    let record = state
        .read(move |conn| repositories::ink_recognition::get(conn, annotation))
        .await?
        .ok_or_else(|| AppError::internal("recognition result did not persist"))?;

    Ok(InkRecognitionView::from_record(record))
}

/// The recognition state for every ink note on a Page, for the Margin to paint
/// when a Page opens.
#[tauri::command]
pub async fn ink_recognition_for_page(
    state: State<'_, AppState>,
    page_id: String,
) -> Result<Vec<InkRecognitionView>> {
    let page = parse_page_id(&page_id)?;
    state
        .read(move |conn| {
            Ok(repositories::ink_recognition::list_for_page(conn, page)?
                .into_iter()
                .map(|(_, record)| InkRecognitionView::from_record(record))
                .collect())
        })
        .await
}

/// The recognition state for one note.
#[tauri::command]
pub async fn ink_recognition_status(
    state: State<'_, AppState>,
    annotation_id: String,
) -> Result<Option<InkRecognitionView>> {
    let annotation = AnnotationId::parse(&annotation_id)?;
    state
        .read(move |conn| {
            Ok(repositories::ink_recognition::get(conn, annotation)?
                .map(InkRecognitionView::from_record))
        })
        .await
}

/// Saves a writer's hand-correction of a transcript.
///
/// Always overwrites (the writer asked), sets the source to `user-edited` so a
/// later automatic recognition will not clobber it, and re-indexes the search
/// entry immediately so stale machine text cannot linger in results.
#[tauri::command]
pub async fn ink_edit_transcript(
    state: State<'_, AppState>,
    annotation_id: String,
    text: String,
) -> Result<InkRecognitionView> {
    let annotation = AnnotationId::parse(&annotation_id)?;
    let record = state
        .write(move |tx| repositories::ink_recognition::set_user_transcript(tx, annotation, &text))
        .await?;
    Ok(InkRecognitionView::from_record(record))
}

/// Converts an ink note's transcript into a typed text annotation, keeping the
/// original ink.
///
/// Creates a `Note` annotation on the same Page whose body is the transcript
/// (the writer's correction preferred over the machine's), and records
/// `source_ink_annotation_id` so the relationship is traceable. The ink note is
/// left exactly as it was — this is never destructive.
#[tauri::command]
pub async fn ink_convert_to_text(
    state: State<'_, AppState>,
    annotation_id: String,
) -> Result<String> {
    let annotation = AnnotationId::parse(&annotation_id)?;

    let (page_id, transcript, anchor) = state
        .read(move |conn| {
            let ink = repositories::annotations::get(conn, annotation)?;
            if ink.kind != AnnotationKind::Ink {
                return Err(AppError::invalid(
                    "Only an ink note can be converted to text.",
                ));
            }
            let transcript = repositories::ink_recognition::get(conn, annotation)?
                .and_then(|r| r.transcript().map(str::to_string))
                .ok_or_else(|| {
                    AppError::invalid(
                        "There is no transcript to convert yet. Recognise the handwriting first.",
                    )
                })?;
            // An anchored ink note passes its anchor to the text note, so the
            // converted note points at the same prose; a whole-Page note becomes
            // a whole-Page note.
            let anchor = ink.anchor().cloned();
            Ok::<_, AppError>((ink.page_id, transcript, anchor))
        })
        .await?;

    let note = state
        .write(move |tx| {
            let note = match anchor {
                Some(anchor) => repositories::annotations::create_anchored(
                    tx,
                    page_id,
                    AnnotationKind::Note,
                    &transcript,
                    anchor.from,
                    anchor.to,
                )?,
                None => repositories::annotations::create_for_page(
                    tx,
                    page_id,
                    AnnotationKind::Note,
                    &transcript,
                )?,
            };
            // Record the lineage: this text note came from that ink note.
            tx.execute(
                "UPDATE annotations SET source_ink_annotation_id = ?2 WHERE id = ?1",
                rusqlite::params![note.id, annotation],
            )?;
            Ok(note.id)
        })
        .await?;

    Ok(note.to_string())
}

/// Cancels any pending automatic recognition for a note. Called when the writer
/// leaves a Page or deletes a note, so a stale recognition does not land.
#[tauri::command]
pub async fn ink_recognize_cancel(state: State<'_, AppState>, annotation_id: String) -> Result<()> {
    let annotation = AnnotationId::parse(&annotation_id)?;
    state.recognition().cancel(annotation).await;
    Ok(())
}

/// The configured recognition language, for the settings UI.
#[tauri::command]
pub async fn ink_recognition_language(state: State<'_, AppState>) -> Result<LanguageHint> {
    state
        .read(|conn| {
            let settings = repositories::settings::load(conn)?;
            Ok(LanguageHint::parse(&settings.ink_recognition_language))
        })
        .await
}
