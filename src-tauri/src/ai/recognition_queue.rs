//! The recognition queue — the one place that schedules, debounces, and runs
//! handwriting recognition.
//!
//! Recognition is background work that must never interrupt writing, and the
//! rules that make that true are fiddly enough to deserve a single owner:
//!
//! * **Debounce.** A burst of strokes is one recognition, not one per stroke. A
//!   mutation schedules (or reschedules) a job to run after a quiet window.
//! * **Deduplicate.** Only one job per ink note is ever in flight; a new
//!   schedule cancels the old.
//! * **Bounded concurrency.** Rapid page switches cannot launch a storm of
//!   requests. A small semaphore limits how many jobs run at once.
//! * **Stale rejection.** A job captures the content hash of the ink it was
//!   scheduled against; before committing its result it re-reads the strokes and
//!   refuses to write if the hash has changed. A slow response that lands after
//!   the writer added a stroke is discarded, never applied.
//!
//! The bookkeeping — the per-note pending map and the concurrency semaphore —
//! lives in [`RecognitionBookkeeping`], held by [`AppState`] so it persists
//! across requests. The recognizer itself is built per request from the live
//! provider config (base URL, model, key), because a writer may reconfigure
//! their provider between two recognitions. The queue therefore takes a
//! recognizer as an argument to each operation rather than holding one.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, Semaphore};
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::app_state::AppState;
use crate::domain::ids::AnnotationId;
use crate::domain::ink::InkStroke;
use crate::domain::ink_recognition::{
    InkRecognitionInput, InkRecognitionRequest, InkRecognitionResult, InkRecognizer, LanguageHint,
    RecognitionMode, ink_content_hash,
};
use crate::error::{AppError, ErrorCode, Result};
use crate::repositories;

/// How long the queue waits after the last ink mutation before running a
/// recognition. Long enough to gather a burst of strokes into one job, short
/// enough that a finished note is transcribed before the writer moves on.
pub const DEBOUNCE: Duration = Duration::from_millis(1_500);

/// The most recognition jobs that may run at once. A personal, local-first app
/// on a tablet has no call for parallel vision requests; one keeps cost and
/// load predictable, and a second absorbs a page switch landing mid-job.
const MAX_CONCURRENCY: usize = 2;

/// A snapshot of an ink note's strokes at the moment a job was scheduled, paired
/// with the hash of those strokes. The hash is what makes a stale result
/// detectable: a job whose hash no longer matches the live ink is discarded.
#[derive(Clone)]
pub struct InkSnapshot {
    pub strokes: Vec<InkStroke>,
    pub surface_width: f32,
    /// The rendered raster, when the caller already has one. The queue does not
    /// render itself — rendering is a frontend concern that needs layout — so a
    /// snapshot carries the bytes it was given. `None` means "no raster
    /// available"; a vector-only recognizer could still run, but the vision
    /// recognizer will refuse.
    pub png: Option<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

impl InkSnapshot {
    /// The content hash of this snapshot's strokes, recorded so a result can be
    /// tied to the ink state it was generated from.
    pub fn content_hash(&self) -> String {
        ink_content_hash(&self.strokes)
    }

    /// Builds the recognizer input from the snapshot, preferring the raster when
    /// one is present and falling back to vectors for a native recognizer.
    pub fn to_input(&self) -> InkRecognitionInput {
        match &self.png {
            Some(bytes) if !bytes.is_empty() => InkRecognitionInput::Image {
                png: bytes.clone(),
                width: self.width,
                height: self.height,
            },
            _ => InkRecognitionInput::Vectors {
                strokes: self.strokes.clone(),
                surface_width: self.surface_width,
            },
        }
    }
}

/// One scheduled job. Held in the per-note map so a reschedule can cancel its
/// predecessor.
struct PendingJob {
    cancel: CancellationToken,
    snapshot: InkSnapshot,
}

/// The recognition queue's persistent state, held by [`AppState`].
///
/// Holds the per-note pending map (for dedup and cancellation) and the
/// concurrency semaphore. The debounce window is configurable so tests can
/// shorten it; production uses [`DEBOUNCE`].
pub struct RecognitionBookkeeping {
    pending: Mutex<HashMap<AnnotationId, PendingJob>>,
    concurrency: Semaphore,
    debounce: Mutex<Duration>,
}

impl Default for RecognitionBookkeeping {
    fn default() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            concurrency: Semaphore::new(MAX_CONCURRENCY),
            debounce: Mutex::new(DEBOUNCE),
        }
    }
}

impl RecognitionBookkeeping {
    /// Sets the debounce window. The frontend may tune this from settings in a
    /// future iteration; for now it is the constant, with a setter so tests can
    /// shorten it.
    pub async fn set_debounce(&self, debounce: Duration) {
        *self.debounce.lock().await = debounce;
    }

    /// Schedules (or reschedules) automatic recognition for a note.
    ///
    /// Cancels any job already pending for the note, so a burst of strokes
    /// collapses to one recognition that runs after the writer pauses. The
    /// snapshot is captured here, at schedule time, so the job recognises the
    /// ink as it was when the debounce began — not as it is when the debounce
    /// ends, which would read a half-drawn state.
    ///
    /// `build_recognizer` is an async future invoked once the debounce elapses,
    /// so the recognizer reflects the provider config as it stands at run time,
    /// not at schedule time, and its blocking credential read does not stall the
    /// runtime. The bookkeeping is passed by `Arc` so the spawned task can hold
    /// a reference to the same pending map that a later schedule mutates.
    pub async fn schedule_auto<F>(
        self: &Arc<Self>,
        state: Arc<AppState>,
        annotation_id: AnnotationId,
        snapshot: InkSnapshot,
        build_recognizer: F,
    ) where
        F: std::future::Future<Output = Result<Arc<dyn InkRecognizer>>> + Send + 'static,
    {
        let cancel = {
            let mut pending = self.pending.lock().await;
            if let Some(old) = pending.insert(
                annotation_id,
                PendingJob {
                    cancel: CancellationToken::new(),
                    snapshot: snapshot.clone(),
                },
            ) {
                old.cancel.cancel();
            }
            pending.get(&annotation_id).unwrap().cancel.clone()
        };

        let bookkeeping = Arc::clone(self);
        let debounce = *self.debounce.lock().await;

        tokio::spawn(async move {
            // Wait out the debounce. If a new schedule replaced this job, the
            // token cancels and the sleep returns early — the work is dropped.
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(debounce) => {}
            }

            // Take the snapshot out of the pending map; the job is now running.
            // Reading it from the entry (rather than a captured copy) means the
            // stored snapshot is the single source of the ink state a job runs
            // against, and a reschedule that replaced the entry is what cancels
            // us above.
            let snapshot = {
                let mut pending = bookkeeping.pending.lock().await;
                pending
                    .remove(&annotation_id)
                    .map(|job| job.snapshot)
                    .unwrap_or(snapshot)
            };

            let recognizer = match build_recognizer.await {
                Ok(r) => r,
                Err(err) => {
                    tracing::warn!(annotation = %annotation_id, code = ?err.code, "could not build recognizer");
                    let _ = state
                        .write(move |tx| {
                            repositories::ink_recognition::record_failure(
                                tx,
                                annotation_id,
                                "Recognition is not configured. Add an AI provider in Settings.",
                            )
                        })
                        .await;
                    return;
                }
            };

            let result =
                run_recognition(&state, &recognizer, annotation_id, snapshot, cancel, false).await;

            if let Err(err) = result {
                tracing::warn!(
                    annotation = %annotation_id,
                    code = ?err.code,
                    "automatic recognition failed"
                );
            }
        });
    }

    /// Runs recognition immediately, ignoring the debounce and the automatic
    /// settings. Used by the explicit "Recognize handwriting" action, which the
    /// writer can invoke even when automatic recognition is off.
    ///
    /// Returns the recognised transcript (or an error) so the command can report
    /// the outcome to the writer directly, rather than through polling.
    pub async fn recognize_now(
        &self,
        state: Arc<AppState>,
        recognizer: Arc<dyn InkRecognizer>,
        annotation_id: AnnotationId,
        snapshot: InkSnapshot,
    ) -> Result<InkRecognitionResult> {
        // Cancel any pending automatic job for this note, so the two cannot
        // race to write a result.
        let cancel_auto = {
            let mut pending = self.pending.lock().await;
            pending.remove(&annotation_id).map(|job| job.cancel)
        };
        if let Some(token) = cancel_auto {
            token.cancel();
        }

        let _permit = self
            .concurrency
            .acquire()
            .await
            .map_err(|_| AppError::internal("recognition queue closed"))?;

        run_recognition(
            &state,
            &recognizer,
            annotation_id,
            snapshot,
            CancellationToken::new(),
            true,
        )
        .await
    }

    /// Cancels any pending automatic job for a note, without running one. Used
    /// when a note is deleted or the writer leaves the page, so a stale
    /// recognition does not land and write to a note nobody is looking at.
    pub async fn cancel(&self, annotation_id: AnnotationId) {
        let mut pending = self.pending.lock().await;
        if let Some(job) = pending.remove(&annotation_id) {
            job.cancel.cancel();
        }
    }
}

/// The shared body of automatic and manual recognition.
///
/// `manual` is true for an explicit recognize, which bypasses the
/// `ink_auto_recognition` and `ai_enabled` checks the automatic path enforces at
/// schedule time. By the time this runs, the decision to recognise has been
/// made; this function's job is to execute it safely.
async fn run_recognition(
    state: &Arc<AppState>,
    recognizer: &Arc<dyn InkRecognizer>,
    annotation_id: AnnotationId,
    snapshot: InkSnapshot,
    cancel: CancellationToken,
    _manual: bool,
) -> Result<InkRecognitionResult> {
    let scheduled_hash = snapshot.content_hash();

    // Meaningful-input check: a single accidental dot is not worth a model call.
    if !snapshot.to_input().is_meaningful() {
        state
            .write(move |tx| repositories::ink_recognition::disable(tx, annotation_id))
            .await?;
        return Err(AppError::invalid(
            "There is not enough handwriting there to recognise yet.",
        ));
    }

    // Ensure a row exists, then mark it recognising, so the UI can show the
    // in-flight state. This happens before the request, so a slow first byte
    // still shows "Recognizing…". A note recognised for the first time has no
    // row yet; a manual recognize on a fresh note must create one rather than
    // fail with "not found".
    state
        .write(move |tx| repositories::ink_recognition::ensure_pending(tx, annotation_id))
        .await?;
    state
        .write(move |tx| repositories::ink_recognition::mark_recognizing(tx, annotation_id))
        .await?;

    // Build the request from the snapshot and the configured language/mode.
    let language_hint = state
        .read(|conn| {
            let settings = repositories::settings::load(conn)?;
            Ok::<_, AppError>(LanguageHint::parse(&settings.ink_recognition_language))
        })
        .await
        .unwrap_or(LanguageHint::auto());

    let request = InkRecognitionRequest {
        input: snapshot.to_input(),
        mode: RecognitionMode::Auto,
        language_hint,
    };

    let outcome = recognizer.recognize(request, cancel.clone()).await;

    // If the job was cancelled (a newer schedule superseded it), do not write
    // anything: a newer job owns this note now.
    if cancel.is_cancelled() {
        tracing::debug!(annotation = %annotation_id, "recognition cancelled before commit");
        return Err(AppError::new(
            ErrorCode::Cancelled,
            "Recognition was superseded.",
        ));
    }

    // A recognizer failure must land in the row, not just in the return value.
    // The note was moved to `recognizing` before the request; if the failure
    // only propagated to the caller, an automatic job — whose caller is a
    // spawned task nobody awaits — would leave the note showing "Recognizing…"
    // until the next app start repaired it. Recording the failure is what makes
    // the state retryable and gives the Margin something to say.
    let result = match outcome {
        Ok(result) => result,
        Err(err) => {
            // The message is the reader-facing sentence the error type already
            // owns; `detail` is deliberately not stored, because it can carry
            // provider internals that do not belong in the manuscript database.
            let reason = err.message.clone();
            state
                .write(move |tx| {
                    repositories::ink_recognition::record_failure(tx, annotation_id, &reason)
                })
                .await?;
            return Err(err);
        }
    };

    // Stale rejection: re-read the live strokes and refuse to commit if the ink
    // has changed since the job was scheduled. A slow response that lands after
    // the writer added a stroke must not overwrite the newer state.
    let live_hash = state
        .read(move |conn| {
            let strokes = repositories::ink::strokes_for_annotation(conn, annotation_id)?;
            Ok::<_, AppError>(ink_content_hash(&strokes))
        })
        .await?;

    if live_hash != scheduled_hash {
        tracing::info!(
            annotation = %annotation_id,
            "discarding stale recognition result: ink changed during recognition"
        );
        state
            .write(move |tx| repositories::ink_recognition::invalidate(tx, annotation_id))
            .await?;
        return Err(AppError::stale(
            "The handwriting changed while it was being recognised, so the result was discarded. \
             It will be recognised again.",
        ));
    }

    // Commit. The repository refuses to clobber a user-edited transcript, so an
    // automatic recognition that lands after a manual correction is a no-op.
    let result_for_write = result.clone();
    let applied = state
        .write(move |tx| {
            repositories::ink_recognition::record_result(
                tx,
                annotation_id,
                &scheduled_hash,
                &result_for_write,
            )
        })
        .await?;

    if !applied {
        tracing::debug!(
            annotation = %annotation_id,
            "recognition result not applied: transcript was user-edited"
        );
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::recognizers::MockRecognizer;
    use crate::database::testing::TempDatabase;
    use crate::domain::ink::{InkPoint, InkStroke, InkTool};
    use crate::domain::ink_recognition::{RecognitionStatus, TranscriptSource};

    fn stroke(points: &[(f32, f32)]) -> InkStroke {
        InkStroke::new(
            InkTool::Pen,
            "ink-primary",
            2.0,
            points.iter().map(|(x, y)| InkPoint::new(*x, *y)).collect(),
        )
    }

    fn snapshot(points: &[(f32, f32)]) -> InkSnapshot {
        InkSnapshot {
            strokes: vec![stroke(points)],
            surface_width: 300.0,
            png: None,
            width: 0,
            height: 0,
        }
    }

    #[tokio::test]
    async fn a_manual_recognize_persists_the_transcript() {
        let db = TempDatabase::open();
        let state = Arc::new(AppState::new(db.db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());
        let annotation = {
            let conn = state.database().get().unwrap();
            let volume = repositories::volumes::create(&conn, "A", None, None).unwrap();
            let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
            let page = repositories::pages::create(&conn, chapter.id, "First").unwrap();
            repositories::ink::create_ink_annotation(&conn, page.id)
                .unwrap()
                .id
        };
        // Seed the strokes so the live hash matches the snapshot.
        {
            let conn = state.database().get().unwrap();
            repositories::ink::add_strokes(&conn, annotation, &[stroke(&[(0.1, 5.0), (0.2, 6.0)])])
                .unwrap();
        }

        let recognizer: Arc<dyn InkRecognizer> =
            Arc::new(MockRecognizer::always("hello from the recognizer"));
        let result = bookkeeping
            .recognize_now(
                Arc::clone(&state),
                recognizer,
                annotation,
                snapshot(&[(0.1, 5.0), (0.2, 6.0)]),
            )
            .await
            .unwrap();
        assert_eq!(result.text, "hello from the recognizer");

        let row = {
            let conn = state.database().get().unwrap();
            repositories::ink_recognition::get(&conn, annotation)
                .unwrap()
                .unwrap()
        };
        assert_eq!(row.status, RecognitionStatus::Recognized);
        assert_eq!(
            row.recognized_text.as_deref(),
            Some("hello from the recognizer")
        );
    }

    #[tokio::test]
    async fn a_stale_result_is_discarded_when_ink_changes_during_recognition() {
        let db = TempDatabase::open();
        let state = Arc::new(AppState::new(db.db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());
        let annotation = {
            let conn = state.database().get().unwrap();
            let volume = repositories::volumes::create(&conn, "A", None, None).unwrap();
            let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
            let page = repositories::pages::create(&conn, chapter.id, "First").unwrap();
            repositories::ink::create_ink_annotation(&conn, page.id)
                .unwrap()
                .id
        };
        {
            let conn = state.database().get().unwrap();
            repositories::ink::add_strokes(&conn, annotation, &[stroke(&[(0.1, 5.0), (0.2, 6.0)])])
                .unwrap();
        }

        // A mock that sleeps briefly, so the test can mutate the ink while the
        // job is "in flight" and then observe the stale rejection.
        let slow: Arc<dyn InkRecognizer> = Arc::new(MockRecognizer::responder(|_| {
            std::thread::sleep(std::time::Duration::from_millis(40));
            Ok(InkRecognitionResult {
                text: "stale answer".into(),
                confidence: None,
                language: None,
                provider: "mock".into(),
                model: "mock-1".into(),
            })
        }));

        let state_for_job = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            bookkeeping
                .recognize_now(
                    state_for_job,
                    slow,
                    annotation,
                    snapshot(&[(0.1, 5.0), (0.2, 6.0)]),
                )
                .await
        });

        // While recognition is "in flight", add a stroke so the live hash
        // diverges from the scheduled hash.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        {
            let conn = state.database().get().unwrap();
            repositories::ink::add_strokes(
                &conn,
                annotation,
                &[stroke(&[(0.9, 90.0), (0.95, 91.0)])],
            )
            .unwrap();
        }

        let outcome = handle.await.unwrap();
        assert!(outcome.is_err(), "a stale result should be rejected");
        assert_eq!(outcome.unwrap_err().code, ErrorCode::Stale);

        let row = {
            let conn = state.database().get().unwrap();
            repositories::ink_recognition::get(&conn, annotation)
                .unwrap()
                .unwrap()
        };
        assert_eq!(row.status, RecognitionStatus::Stale);
        assert!(row.recognized_text.is_none());
    }

    #[tokio::test]
    async fn automatic_recognition_does_not_overwrite_a_user_edited_transcript() {
        let db = TempDatabase::open();
        let state = Arc::new(AppState::new(db.db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());
        let annotation = {
            let conn = state.database().get().unwrap();
            let volume = repositories::volumes::create(&conn, "A", None, None).unwrap();
            let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
            let page = repositories::pages::create(&conn, chapter.id, "First").unwrap();
            repositories::ink::create_ink_annotation(&conn, page.id)
                .unwrap()
                .id
        };
        {
            let conn = state.database().get().unwrap();
            repositories::ink::add_strokes(&conn, annotation, &[stroke(&[(0.1, 5.0), (0.2, 6.0)])])
                .unwrap();
            repositories::ink_recognition::ensure_pending(&conn, annotation).unwrap();
            repositories::ink_recognition::set_user_transcript(&conn, annotation, "writer's words")
                .unwrap();
        }

        let recognizer: Arc<dyn InkRecognizer> = Arc::new(MockRecognizer::always("machine answer"));
        let result = bookkeeping
            .recognize_now(
                Arc::clone(&state),
                recognizer,
                annotation,
                snapshot(&[(0.1, 5.0), (0.2, 6.0)]),
            )
            .await
            .unwrap();
        assert_eq!(result.text, "machine answer");

        let row = {
            let conn = state.database().get().unwrap();
            repositories::ink_recognition::get(&conn, annotation)
                .unwrap()
                .unwrap()
        };
        assert_eq!(row.recognized_text.as_deref(), Some("writer's words"));
        assert_eq!(row.transcript_source, TranscriptSource::UserEdited);
    }

    #[tokio::test]
    async fn a_provider_failure_leaves_the_note_retryable_not_stuck_recognizing() {
        let db = TempDatabase::open();
        let state = Arc::new(AppState::new(db.db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());
        let annotation = {
            let conn = state.database().get().unwrap();
            let volume = repositories::volumes::create(&conn, "A", None, None).unwrap();
            let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
            let page = repositories::pages::create(&conn, chapter.id, "First").unwrap();
            repositories::ink::create_ink_annotation(&conn, page.id)
                .unwrap()
                .id
        };
        {
            let conn = state.database().get().unwrap();
            repositories::ink::add_strokes(&conn, annotation, &[stroke(&[(0.1, 5.0), (0.2, 6.0)])])
                .unwrap();
        }

        let failing: Arc<dyn InkRecognizer> =
            Arc::new(MockRecognizer::failing("the provider refused the request"));
        let err = bookkeeping
            .recognize_now(
                Arc::clone(&state),
                failing,
                annotation,
                snapshot(&[(0.1, 5.0), (0.2, 6.0)]),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Provider);

        let row = {
            let conn = state.database().get().unwrap();
            repositories::ink_recognition::get(&conn, annotation)
                .unwrap()
                .unwrap()
        };
        // Not stranded mid-flight: the row says why, so the Margin can offer a
        // retry instead of a spinner that never resolves.
        assert_eq!(row.status, RecognitionStatus::Failed);
        assert_eq!(
            row.error.as_deref(),
            Some("the provider refused the request")
        );
    }

    #[tokio::test]
    async fn a_failure_does_not_erase_a_writers_own_transcript() {
        let db = TempDatabase::open();
        let state = Arc::new(AppState::new(db.db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());
        let annotation = {
            let conn = state.database().get().unwrap();
            let volume = repositories::volumes::create(&conn, "A", None, None).unwrap();
            let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
            let page = repositories::pages::create(&conn, chapter.id, "First").unwrap();
            repositories::ink::create_ink_annotation(&conn, page.id)
                .unwrap()
                .id
        };
        {
            let conn = state.database().get().unwrap();
            repositories::ink::add_strokes(&conn, annotation, &[stroke(&[(0.1, 5.0), (0.2, 6.0)])])
                .unwrap();
            repositories::ink_recognition::ensure_pending(&conn, annotation).unwrap();
            repositories::ink_recognition::set_user_transcript(&conn, annotation, "writer's words")
                .unwrap();
        }

        let failing: Arc<dyn InkRecognizer> = Arc::new(MockRecognizer::failing("network down"));
        let _ = bookkeeping
            .recognize_now(
                Arc::clone(&state),
                failing,
                annotation,
                snapshot(&[(0.1, 5.0), (0.2, 6.0)]),
            )
            .await;

        let row = {
            let conn = state.database().get().unwrap();
            repositories::ink_recognition::get(&conn, annotation)
                .unwrap()
                .unwrap()
        };
        assert_eq!(row.recognized_text.as_deref(), Some("writer's words"));
        assert_eq!(row.transcript_source, TranscriptSource::UserEdited);
    }

    #[tokio::test]
    async fn a_meaningless_snapshot_is_refused_without_a_model_call() {
        let db = TempDatabase::open();
        let state = Arc::new(AppState::new(db.db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());
        let annotation = {
            let conn = state.database().get().unwrap();
            let volume = repositories::volumes::create(&conn, "A", None, None).unwrap();
            let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
            let page = repositories::pages::create(&conn, chapter.id, "First").unwrap();
            repositories::ink::create_ink_annotation(&conn, page.id)
                .unwrap()
                .id
        };

        let mock: Arc<dyn InkRecognizer> = Arc::new(MockRecognizer::always("should not be called"));
        let err = bookkeeping
            .recognize_now(
                Arc::clone(&state),
                mock,
                annotation,
                snapshot(&[(0.5, 5.0)]),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);

        let row = {
            let conn = state.database().get().unwrap();
            repositories::ink_recognition::get(&conn, annotation)
                .unwrap()
                .unwrap()
        };
        assert_eq!(row.status, RecognitionStatus::Disabled);
    }
}
