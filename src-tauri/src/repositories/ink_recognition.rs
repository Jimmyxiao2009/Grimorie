//! Ink recognition persistence.
//!
//! The only place that touches the `ink_recognition` table. Like the strokes
//! repository, the row granularity matches the access pattern: one row per ink
//! annotation, read in bulk for a Page and written one at a time as recognition
//! proceeds.
//!
//! Two invariants are enforced here:
//!
//! * **A result is tied to an ink state.** Every recognised row carries a
//!   `content_hash`; when strokes are added or erased the caller invalidates the
//!   row, which moves it to `stale` and clears the stale transcript's claim to
//!   be current — without erasing it, so the writer still sees the old text
//!   while a refresh runs.
//! * **A hand-edited transcript is never silently overwritten.** Recognition
//!   writes go through [`record_result`], which refuses to clobber a
//!   `user-edited` transcript; only an explicit re-recognition does.

use rusqlite::{Connection, Row, params};

use crate::domain::ids::{AnnotationId, PageId};
use crate::domain::ink_recognition::{
    InkRecognitionRecord, LanguageHint, RecognitionStatus, TranscriptSource,
};
use crate::domain::manuscript::now;
use crate::error::{AppError, Result};
use crate::search::{self, EntityKind};

/// The transcript source column default, mirrored for new-row construction.
const DEFAULT_SOURCE: TranscriptSource = TranscriptSource::Recognized;

fn map(row: &Row<'_>) -> rusqlite::Result<InkRecognitionRecord> {
    Ok(InkRecognitionRecord {
        annotation_id: row.get("annotation_id")?,
        status: RecognitionStatus::parse(&row.get::<_, String>("status")?),
        recognized_text: row.get("recognized_text")?,
        confidence: row.get("confidence")?,
        provider: row.get("provider")?,
        model: row.get("model")?,
        language: row.get("language")?,
        transcript_source: TranscriptSource::parse(&row.get::<_, String>("transcript_source")?),
        content_hash: row.get("content_hash")?,
        error: row.get("error")?,
        recognized_at: row.get("recognized_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Creates a `pending` row for an ink annotation, if none exists.
///
/// Called when a note first becomes a candidate for recognition. A row that
/// already exists is left alone — it may already hold a transcript, and a
/// fresh `pending` would erase the writer's view of it.
pub fn ensure_pending(conn: &Connection, annotation_id: AnnotationId) -> Result<()> {
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM ink_recognition WHERE annotation_id = ?1",
        [annotation_id],
        |row| row.get(0),
    )?;
    if exists > 0 {
        return Ok(());
    }

    conn.execute(
        "INSERT INTO ink_recognition
            (annotation_id, status, transcript_source, updated_at)
         VALUES (?1, 'pending', ?2, ?3)",
        params![annotation_id, DEFAULT_SOURCE.as_str(), now()],
    )?;
    Ok(())
}

/// The recognition row for one annotation, if any. A note that has never been a
/// recognition candidate has no row and yields `None`.
pub fn get(conn: &Connection, annotation_id: AnnotationId) -> Result<Option<InkRecognitionRecord>> {
    conn.query_row(
        "SELECT * FROM ink_recognition WHERE annotation_id = ?1",
        [annotation_id],
        map,
    )
    .optional()
}

/// Every recognition row on a Page, keyed by annotation. Read in bulk when a
/// Page opens, so the Margin can show each note's status without a query per
/// note.
pub fn list_for_page(
    conn: &Connection,
    page_id: PageId,
) -> Result<Vec<(AnnotationId, InkRecognitionRecord)>> {
    let mut statement = conn.prepare(
        "SELECT r.* FROM ink_recognition r
           JOIN annotations a ON a.id = r.annotation_id
          WHERE a.page_id = ?1",
    )?;
    let rows = statement
        .query_map([page_id], |row| {
            Ok((row.get::<_, AnnotationId>("annotation_id")?, map(row)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Moves a row to `recognizing`, clearing any prior error. The content hash is
/// *not* touched here: it records what a result was generated from, not what a
/// job is running against, and setting it before completion would let a crashed
/// job leave a hash that matched no completed transcript.
pub fn mark_recognizing(conn: &Connection, annotation_id: AnnotationId) -> Result<()> {
    let changed = conn.execute(
        "UPDATE ink_recognition
            SET status = 'recognizing', error = NULL, updated_at = ?2
          WHERE annotation_id = ?1",
        params![annotation_id, now()],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("ink recognition"));
    }
    Ok(())
}

/// Records a recognizer's result, unless the writer has since corrected the
/// transcript by hand.
///
/// This is the guard on automatic overwrite: a `user-edited` transcript is the
/// writer's, and a background recognition that happens to land afterwards must
/// not replace it. The guard is checked at write time, not at schedule time,
/// because the two can be far apart.
///
/// `content_hash` is the hash of the strokes the result was generated from, kept
/// separate from `result` because the result is provider output and the hash is
/// ink state — the two come from different places and conflating them would make
/// staleness checks ambiguous.
///
/// Returns whether the result was applied, so the caller knows whether to emit a
/// status change.
pub fn record_result(
    conn: &Connection,
    annotation_id: AnnotationId,
    content_hash: &str,
    result: &crate::domain::ink_recognition::InkRecognitionResult,
) -> Result<bool> {
    let existing = get(conn, annotation_id)?;
    let Some(row) = existing else {
        return Err(AppError::not_found("ink recognition"));
    };

    // The writer wins. A hand-edited transcript is left untouched by an
    // automatic recognition; only an explicit "recognize again" overwrites it,
    // and that path sets the source back to `recognized` directly.
    if row.transcript_source == TranscriptSource::UserEdited {
        tracing::debug!(
            annotation = %annotation_id,
            "recognition result discarded: transcript was user-edited"
        );
        return Ok(false);
    }

    let at = now();
    conn.execute(
        "UPDATE ink_recognition
            SET status = 'recognized', recognized_text = ?2, confidence = ?3,
                provider = ?4, model = ?5, language = ?6, content_hash = ?7,
                transcript_source = 'recognized', error = NULL, recognized_at = ?8,
                updated_at = ?8
          WHERE annotation_id = ?1",
        params![
            annotation_id,
            result.text,
            result.confidence,
            result.provider,
            result.model,
            result.language,
            content_hash,
            at
        ],
    )?;

    reindex(conn, annotation_id, &result.text)?;
    Ok(true)
}

/// Records a failure, storing a short non-private reason for the retry
/// affordance. A user-edited transcript is preserved even on failure — the
/// writer's correction is not erased because the recognizer could not run.
pub fn record_failure(conn: &Connection, annotation_id: AnnotationId, error: &str) -> Result<()> {
    let existing = get(conn, annotation_id)?;
    let source = existing
        .as_ref()
        .map(|row| row.transcript_source)
        .unwrap_or(DEFAULT_SOURCE);

    // Keep a user-edited transcript and its source; only the status and error
    // change, so the writer still sees their text with a retry offered.
    let keep_text = source == TranscriptSource::UserEdited;
    let at = now();
    conn.execute(
        "UPDATE ink_recognition
            SET status = 'failed', error = ?2, updated_at = ?3
          WHERE annotation_id = ?1",
        params![annotation_id, error, at],
    )?;

    if !keep_text {
        // A failed automatic recognition should not surface stale machine text
        // as if it were current. Clearing the transcript is safe here because
        // the row has no user contribution to lose.
        conn.execute(
            "UPDATE ink_recognition SET recognized_text = NULL WHERE annotation_id = ?1",
            [annotation_id],
        )?;
        reindex(conn, annotation_id, "")?;
    }
    Ok(())
}

/// Saves a writer's hand-correction of a transcript.
///
/// This always overwrites, because the writer asked for it: it sets the source
/// to `user-edited` so subsequent automatic recognition will not clobber it, and
/// it re-indexes the search entry immediately so stale machine text cannot
/// linger in results.
pub fn set_user_transcript(
    conn: &Connection,
    annotation_id: AnnotationId,
    text: &str,
) -> Result<InkRecognitionRecord> {
    ensure_pending(conn, annotation_id)?;
    let at = now();
    let trimmed = text.trim();
    conn.execute(
        "UPDATE ink_recognition
            SET recognized_text = ?2, transcript_source = 'user-edited',
                status = 'recognized', error = NULL, recognized_at = ?3, updated_at = ?3
          WHERE annotation_id = ?1",
        params![annotation_id, trimmed, at],
    )?;

    reindex(conn, annotation_id, trimmed)?;
    get(conn, annotation_id)?.ok_or_else(|| AppError::internal("user transcript did not persist"))
}

/// Invalidates a recognition against a changed ink state.
///
/// Called whenever strokes are added or erased. The row moves to `stale`, its
/// `error` is cleared (a prior failure is no longer the relevant state), and the
/// `disabled` flag is not imposed — a stale note still wants a refresh. The
/// transcript and its hash are *kept*: a stale transcript is out of date, not
/// gone, and the writer should still see the old text while a new recognition is
/// pending.
///
/// A note with no row yet — one that was never a recognition candidate — is
/// left for [`ensure_pending`]; invalidating nothing is correct.
pub fn invalidate(conn: &Connection, annotation_id: AnnotationId) -> Result<()> {
    let changed = conn.execute(
        "UPDATE ink_recognition
            SET status = 'stale', error = NULL, updated_at = ?2
          WHERE annotation_id = ?1
            AND status NOT IN ('stale', 'pending')",
        params![annotation_id, now()],
    )?;
    if changed == 0 {
        return Ok(());
    }
    tracing::debug!(annotation = %annotation_id, "recognition invalidated");
    Ok(())
}

/// Whether a recognition is already current for the given ink state, so a
/// scheduler can skip a needless request. A row whose hash matches the live
/// strokes has nothing to gain from re-recognition.
pub fn is_current_for(
    conn: &Connection,
    annotation_id: AnnotationId,
    content_hash: &str,
) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT content_hash FROM ink_recognition WHERE annotation_id = ?1",
            [annotation_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .is_some_and(|stored| stored == content_hash))
}

/// Resets a row so an explicit re-recognition can run, even over a
/// user-edited transcript.
///
/// Only "recognize again" goes through here: it clears the source back to
/// `recognized` and moves to `pending`, so [`record_result`] will accept the
/// next result.
pub fn reset_for_rerun(conn: &Connection, annotation_id: AnnotationId) -> Result<()> {
    ensure_pending(conn, annotation_id)?;
    conn.execute(
        "UPDATE ink_recognition
            SET status = 'pending', transcript_source = 'recognized', error = NULL,
                content_hash = NULL, updated_at = ?2
          WHERE annotation_id = ?1",
        params![annotation_id, now()],
    )?;
    Ok(())
}

/// Disables automatic recognition for a note — used when the note is below the
/// recognition threshold or when automatic recognition is turned off. A manual
/// recognize can still run; [`reset_for_rerun`] clears this.
pub fn disable(conn: &Connection, annotation_id: AnnotationId) -> Result<()> {
    ensure_pending(conn, annotation_id)?;
    conn.execute(
        "UPDATE ink_recognition
            SET status = 'disabled', error = NULL, updated_at = ?2
          WHERE annotation_id = ?1 AND status NOT IN ('recognized', 'stale')",
        params![annotation_id, now()],
    )?;
    Ok(())
}

/// Repairs rows left mid-flight by an abnormal exit.
///
/// A `recognizing` status can only exist while a job is running; if the app was
/// killed under one, the row is stuck behind a "Recognizing…" that can never
/// finish. On startup these are moved to `pending`, so the writer can retry or
/// the scheduler can pick them up again.
pub fn repair_transient(conn: &Connection) -> Result<usize> {
    let changed = conn.execute(
        "UPDATE ink_recognition SET status = 'pending', updated_at = ?1
          WHERE status = 'recognizing'",
        [now()],
    )?;
    if changed > 0 {
        tracing::info!(changed, "recovered recognition rows stuck mid-flight");
    }
    Ok(changed)
}

/// Removes a recognition row. Called when an annotation is deleted; the cascade
/// from `annotations` already handles this, but the explicit path keeps the
/// search index in step.
pub fn delete(conn: &Connection, annotation_id: AnnotationId) -> Result<()> {
    search::remove(conn, &annotation_id.to_string())?;
    conn.execute(
        "DELETE FROM ink_recognition WHERE annotation_id = ?1",
        [annotation_id],
    )?;
    Ok(())
}

/// Re-indexes an ink note's transcript in full-text search.
///
/// The strokes are never indexed — coordinates are not searchable — so an ink
/// note appears in results only once a transcript exists. The title is left
/// empty: an ink note has no title of its own, and the search card labels it
/// "Ink Note" instead.
fn reindex(conn: &Connection, annotation_id: AnnotationId, transcript: &str) -> Result<()> {
    let volume_id = volume_of_annotation(conn, annotation_id)?;
    search::index(
        conn,
        EntityKind::Ink,
        &annotation_id.to_string(),
        volume_id.as_deref(),
        "",
        transcript,
    )
}

/// Resolves the volume an annotation's page belongs to, for scoping search
/// results to a manuscript. Returns `None` if the annotation (or its page) has
/// been deleted, which is not an error during teardown.
fn volume_of_annotation(conn: &Connection, annotation_id: AnnotationId) -> Result<Option<String>> {
    conn.query_row(
        "SELECT c.volume_id
           FROM annotations a
           JOIN pages p    ON p.id = a.page_id
           JOIN chapters c ON c.id = p.chapter_id
          WHERE a.id = ?1",
        [annotation_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// A hint read from the store of `LanguageHint`, here so the recognition
/// scheduler has one place to read the configured language.
pub fn language_hint(_conn: &Connection) -> LanguageHint {
    // The language hint lives in AppSettings (ink_recognition_language), read by
    // the command layer. This helper exists so repository callers that build a
    // request have a typed default to fall back on without importing the domain
    // enum's default impl.
    LanguageHint::auto()
}

// `option_value` is the rusqlite 0.40 spelling of the old `optional`; pulled in
// here so the queries above read naturally rather than each spelling the helper
// themselves.
trait OptionalValue {
    type Ok;
    fn optional(self) -> Result<Option<Self::Ok>>;
}

impl<T, E> OptionalValue for std::result::Result<T, E>
where
    E: Into<rusqlite::Error>,
{
    type Ok = T;
    fn optional(self) -> Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(err) => {
                let err = err.into();
                if matches!(err, rusqlite::Error::QueryReturnedNoRows) {
                    Ok(None)
                } else {
                    Err(err.into())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testing::TempDatabase;
    use crate::domain::annotation::AnnotationKind;
    use crate::domain::ids::StrokeId;
    use crate::domain::ink::{InkPoint, InkStroke, InkTool};
    use crate::domain::ink_recognition::{
        InkRecognitionResult, TranscriptSource, ink_content_hash,
    };

    fn page(conn: &Connection) -> PageId {
        let volume = super::super::volumes::create(conn, "A", None, None).unwrap();
        let chapter = super::super::chapters::create(conn, volume.id, "One").unwrap();
        super::super::pages::create(conn, chapter.id, "First")
            .unwrap()
            .id
    }

    fn ink_annotation(conn: &Connection) -> AnnotationId {
        super::super::ink::create_ink_annotation(conn, page(conn))
            .unwrap()
            .id
    }

    fn stroke(points: &[(f32, f32)]) -> InkStroke {
        InkStroke::new(
            InkTool::Pen,
            "ink-primary",
            2.0,
            points.iter().map(|(x, y)| InkPoint::new(*x, *y)).collect(),
        )
    }

    fn hash_of(strokes: &[InkStroke]) -> String {
        ink_content_hash(strokes)
    }

    /// Builds a mock recognizer result for the common test shape.
    fn result(text: &str, language: Option<&str>) -> InkRecognitionResult {
        InkRecognitionResult {
            text: text.to_string(),
            confidence: None,
            language: language.map(str::to_string),
            provider: "mock".into(),
            model: "mock-1".into(),
        }
    }

    #[test]
    fn a_note_with_no_recognition_has_no_row() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);

        assert!(get(&conn, id).unwrap().is_none());
        assert!(
            list_for_page(&conn, page_id_of(&conn, id))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn ensure_pending_creates_one_row_and_is_idempotent() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);

        ensure_pending(&conn, id).unwrap();
        ensure_pending(&conn, id).unwrap();

        let row = get(&conn, id).unwrap().unwrap();
        assert_eq!(row.status, RecognitionStatus::Pending);
        assert_eq!(row.transcript_source, TranscriptSource::Recognized);
        assert!(row.recognized_text.is_none());
    }

    #[test]
    fn a_recognized_result_round_trips_with_its_hash() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);
        let strokes = vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])];
        let hash = hash_of(&strokes);

        ensure_pending(&conn, id).unwrap();
        mark_recognizing(&conn, id).unwrap();
        let applied = record_result(&conn, id, &hash, &result("hello world", Some("en"))).unwrap();
        assert!(applied);

        let row = get(&conn, id).unwrap().unwrap();
        assert_eq!(row.status, RecognitionStatus::Recognized);
        assert_eq!(row.recognized_text.as_deref(), Some("hello world"));
        assert_eq!(row.content_hash.as_deref(), Some(hash.as_str()));
        assert_eq!(row.provider.as_deref(), Some("mock"));
        assert_eq!(row.language.as_deref(), Some("en"));
        assert_eq!(row.transcript_source, TranscriptSource::Recognized);
    }

    #[test]
    fn a_result_against_a_different_hash_is_still_stored_but_can_be_detected() {
        // record_result does not itself reject a stale hash — the scheduler
        // checks is_current_for before committing. What it must do is store the
        // hash it was given, so the caller's staleness check has something to
        // compare against. Stale rejection is tested in the command layer.
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);

        ensure_pending(&conn, id).unwrap();
        record_result(&conn, id, "hash-a", &result("first", None)).unwrap();

        let row = get(&conn, id).unwrap().unwrap();
        assert_eq!(row.content_hash.as_deref(), Some("hash-a"));
        assert!(!is_current_for(&conn, id, "hash-b").unwrap());
        assert!(is_current_for(&conn, id, "hash-a").unwrap());
    }

    #[test]
    fn a_user_edited_transcript_is_not_overwritten_by_automatic_recognition() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);
        let strokes = vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])];
        let hash = hash_of(&strokes);

        ensure_pending(&conn, id).unwrap();
        record_result(&conn, id, &hash, &result("machine", None)).unwrap();
        set_user_transcript(&conn, id, "writer's words").unwrap();

        // A later automatic recognition lands. It must not clobber the writer.
        let applied = record_result(&conn, id, &hash, &result("machine again", None)).unwrap();
        assert!(!applied, "automatic recognition overwrote a user edit");

        let row = get(&conn, id).unwrap().unwrap();
        assert_eq!(row.recognized_text.as_deref(), Some("writer's words"));
        assert_eq!(row.transcript_source, TranscriptSource::UserEdited);
    }

    #[test]
    fn explicit_rerun_overwrites_a_user_transcript() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);
        let strokes = vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])];
        let hash = hash_of(&strokes);

        ensure_pending(&conn, id).unwrap();
        set_user_transcript(&conn, id, "writer's words").unwrap();
        reset_for_rerun(&conn, id).unwrap();

        let applied = record_result(&conn, id, &hash, &result("machine", None)).unwrap();
        assert!(applied, "an explicit rerun must be allowed to overwrite");

        let row = get(&conn, id).unwrap().unwrap();
        assert_eq!(row.recognized_text.as_deref(), Some("machine"));
        assert_eq!(row.transcript_source, TranscriptSource::Recognized);
    }

    #[test]
    fn invalidating_moves_a_recognized_row_to_stale_but_keeps_the_transcript() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);
        let strokes = vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])];
        let hash = hash_of(&strokes);

        ensure_pending(&conn, id).unwrap();
        record_result(&conn, id, &hash, &result("hello", None)).unwrap();

        // The writer adds a stroke: the ink changes.
        invalidate(&conn, id).unwrap();
        let row = get(&conn, id).unwrap().unwrap();
        assert_eq!(row.status, RecognitionStatus::Stale);
        // The old transcript is kept, so it is still visible while a refresh runs.
        assert_eq!(row.recognized_text.as_deref(), Some("hello"));
    }

    #[test]
    fn invalidating_a_pending_row_is_a_no_op() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);

        ensure_pending(&conn, id).unwrap();
        invalidate(&conn, id).unwrap();
        let row = get(&conn, id).unwrap().unwrap();
        assert_eq!(row.status, RecognitionStatus::Pending);
    }

    #[test]
    fn invalidating_an_already_stale_row_does_not_touch_it_again() {
        // The WHERE clause excludes 'stale' and 'pending', so a second mutation
        // does not bump updated_at or fire needlessly.
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);
        let strokes = vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])];
        let hash = hash_of(&strokes);

        ensure_pending(&conn, id).unwrap();
        record_result(&conn, id, &hash, &result("hello", None)).unwrap();
        invalidate(&conn, id).unwrap();
        let first = get(&conn, id).unwrap().unwrap();

        invalidate(&conn, id).unwrap();
        let second = get(&conn, id).unwrap().unwrap();
        assert_eq!(first.updated_at, second.updated_at);
    }

    #[test]
    fn a_failure_records_the_error_and_clears_machine_text() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);

        ensure_pending(&conn, id).unwrap();
        mark_recognizing(&conn, id).unwrap();
        record_failure(&conn, id, "network unavailable").unwrap();

        let row = get(&conn, id).unwrap().unwrap();
        assert_eq!(row.status, RecognitionStatus::Failed);
        assert_eq!(row.error.as_deref(), Some("network unavailable"));
        assert!(row.recognized_text.is_none());
    }

    #[test]
    fn a_failure_preserves_a_user_edited_transcript() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);

        ensure_pending(&conn, id).unwrap();
        set_user_transcript(&conn, id, "writer's words").unwrap();
        record_failure(&conn, id, "network unavailable").unwrap();

        let row = get(&conn, id).unwrap().unwrap();
        assert_eq!(row.status, RecognitionStatus::Failed);
        assert_eq!(row.recognized_text.as_deref(), Some("writer's words"));
        assert_eq!(row.transcript_source, TranscriptSource::UserEdited);
    }

    #[test]
    fn repair_transient_moves_stuck_recognizing_rows_to_pending() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);

        ensure_pending(&conn, id).unwrap();
        mark_recognizing(&conn, id).unwrap();
        assert_eq!(
            get(&conn, id).unwrap().unwrap().status,
            RecognitionStatus::Recognizing
        );

        let repaired = repair_transient(&conn).unwrap();
        assert_eq!(repaired, 1);
        assert_eq!(
            get(&conn, id).unwrap().unwrap().status,
            RecognitionStatus::Pending
        );
    }

    #[test]
    fn deleting_an_annotation_cascades_to_its_recognition_row() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);
        ensure_pending(&conn, id).unwrap();

        super::super::annotations::delete(&conn, id).unwrap();

        assert!(get(&conn, id).unwrap().is_none());
        let left: i64 = conn
            .query_row("SELECT count(*) FROM ink_recognition", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn deleting_a_page_cascades_to_its_recognition_rows() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        let id = super::super::ink::create_ink_annotation(&conn, page_id)
            .unwrap()
            .id;
        ensure_pending(&conn, id).unwrap();

        super::super::pages::delete(&conn, page_id).unwrap();

        let left: i64 = conn
            .query_row("SELECT count(*) FROM ink_recognition", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn list_for_page_returns_rows_keyed_by_annotation() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        let a = super::super::ink::create_ink_annotation(&conn, page_id)
            .unwrap()
            .id;
        let b = super::super::ink::create_ink_annotation(&conn, page_id)
            .unwrap()
            .id;

        ensure_pending(&conn, a).unwrap();
        record_result(&conn, a, "h", &result("note a", None)).unwrap();
        ensure_pending(&conn, b).unwrap();

        let rows = list_for_page(&conn, page_id).unwrap();
        assert_eq!(rows.len(), 2);
        let by_id: std::collections::HashMap<AnnotationId, RecognitionStatus> =
            rows.into_iter().map(|(id, r)| (id, r.status)).collect();
        assert_eq!(by_id.get(&a), Some(&RecognitionStatus::Recognized));
        assert_eq!(by_id.get(&b), Some(&RecognitionStatus::Pending));
    }

    #[test]
    fn a_recognized_transcript_is_indexed_for_search() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);
        ensure_pending(&conn, id).unwrap();
        record_result(&conn, id, "h", &result("movethisparagraph", None)).unwrap();

        let hits = crate::search::search(&conn, "movethisparagraph", None, 10).unwrap();
        let kinds: Vec<_> = hits.iter().map(|h| h.kind).collect();
        assert!(
            kinds.contains(&EntityKind::Ink),
            "ink transcript not found in search"
        );
        let ink_hit = hits.iter().find(|h| h.kind == EntityKind::Ink).unwrap();
        assert_eq!(ink_hit.entity_id, id.to_string());
    }

    #[test]
    fn editing_a_transcript_updates_the_search_entry() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);
        ensure_pending(&conn, id).unwrap();
        record_result(&conn, id, "h", &result("old machine text", None)).unwrap();

        set_user_transcript(&conn, id, "corrected by writer").unwrap();

        // The old machine text is gone from the index.
        assert!(
            crate::search::search(&conn, "machine", None, 10)
                .unwrap()
                .is_empty()
        );
        // The corrected text is found.
        assert!(
            crate::search::search(&conn, "corrected", None, 10)
                .unwrap()
                .iter()
                .any(|h| h.kind == EntityKind::Ink)
        );
    }

    #[test]
    fn an_ink_note_with_cjk_transcript_is_searchable() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);
        ensure_pending(&conn, id).unwrap();
        record_result(&conn, id, "h", &result("这里的转折太突然了", Some("zh-CN"))).unwrap();

        let hits = crate::search::search(&conn, "转折", None, 10).unwrap();
        assert!(
            hits.iter()
                .any(|h| h.kind == EntityKind::Ink && h.entity_id == id.to_string())
        );
    }

    #[test]
    fn a_disabled_note_can_still_be_manually_recognized_after_reset() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let id = ink_annotation(&conn);
        let strokes = vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])];
        let hash = hash_of(&strokes);

        ensure_pending(&conn, id).unwrap();
        disable(&conn, id).unwrap();
        assert_eq!(
            get(&conn, id).unwrap().unwrap().status,
            RecognitionStatus::Disabled
        );

        reset_for_rerun(&conn, id).unwrap();
        let applied = record_result(&conn, id, &hash, &result("machine", None)).unwrap();
        assert!(applied);
        assert_eq!(
            get(&conn, id).unwrap().unwrap().status,
            RecognitionStatus::Recognized
        );
    }

    /// Resolves the page an annotation is on, for test setup.
    fn page_id_of(conn: &Connection, annotation_id: AnnotationId) -> PageId {
        conn.query_row(
            "SELECT page_id FROM annotations WHERE id = ?1",
            [annotation_id],
            |row| row.get(0),
        )
        .unwrap()
    }

    // Suppress an unused warning for a symbol only referenced by the doc tests'
    // narrative; the real call sites exercise it.
    #[test]
    fn _stroke_id_is_available() {
        let _ = StrokeId::new();
    }

    // AnnotationKind is referenced in the narrative above; keep the import live.
    #[test]
    fn _annotation_kind_import_is_used() {
        assert_eq!(AnnotationKind::Ink.as_str(), "ink");
    }
}
