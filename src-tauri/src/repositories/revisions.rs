//! Revision persistence.

use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::ids::{PageId, RevisionId};
use crate::domain::manuscript::{Timestamp, now};
use crate::domain::text;
use crate::error::{AppError, Result};

/// How many snapshots a Page keeps.
///
/// Bounded deliberately. This is a safety net for "what did that paragraph say
/// yesterday", not an archive, and an unbounded history on a Page edited for
/// months would quietly become the largest thing in the library.
pub const MAX_REVISIONS_PER_PAGE: i64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RevisionReason {
    /// Periodic snapshot during a long editing session.
    Checkpoint,
    /// Taken before restoring a different revision over the current text.
    BeforeRestore,
    /// Taken before an AI suggestion was applied.
    BeforeAi,
    /// Taken before an import replaced existing content.
    BeforeImport,
}

impl RevisionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            RevisionReason::Checkpoint => "checkpoint",
            RevisionReason::BeforeRestore => "before-restore",
            RevisionReason::BeforeAi => "before-ai",
            RevisionReason::BeforeImport => "before-import",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "before-restore" => RevisionReason::BeforeRestore,
            "before-ai" => RevisionReason::BeforeAi,
            "before-import" => RevisionReason::BeforeImport,
            // Anything unrecognised is treated as an ordinary checkpoint rather
            // than failing the load. A row written by a newer build must not
            // make history unreadable.
            _ => RevisionReason::Checkpoint,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub id: RevisionId,
    pub page_id: PageId,
    pub revision_number: i64,
    pub document: Value,
    pub plain_text: String,
    pub word_count: i64,
    pub reason: RevisionReason,
    pub created_at: Timestamp,
}

/// A history row: everything the list needs, without the documents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionSummary {
    pub id: RevisionId,
    pub revision_number: i64,
    pub word_count: i64,
    /// Change in words against the revision immediately before it.
    pub word_delta: i64,
    pub reason: RevisionReason,
    pub preview: String,
    pub created_at: Timestamp,
}

fn map(row: &Row<'_>) -> rusqlite::Result<Revision> {
    let raw: String = row.get("document_json")?;
    Ok(Revision {
        id: row.get("id")?,
        page_id: row.get("page_id")?,
        revision_number: row.get("revision_number")?,
        document: serde_json::from_str(&raw).unwrap_or(Value::Null),
        plain_text: row.get("plain_text")?,
        word_count: row.get("word_count")?,
        reason: RevisionReason::parse(&row.get::<_, String>("reason")?),
        created_at: row.get("created_at")?,
    })
}

/// Snapshots a Page's current content.
pub fn capture(conn: &Connection, page_id: PageId, reason: RevisionReason) -> Result<RevisionId> {
    let page = super::pages::get(conn, page_id)?;
    let id = RevisionId::new();

    conn.execute(
        "INSERT INTO revisions (id, page_id, revision_number, document_json, plain_text,
                                word_count, reason, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            page_id,
            page.revision_number,
            serde_json::to_string(&page.document)?,
            page.plain_text,
            page.word_count,
            reason.as_str(),
            now(),
        ],
    )?;

    prune(conn, page_id)?;
    Ok(id)
}

/// Drops the oldest snapshots beyond the per-Page limit.
///
/// The very first snapshot is kept whatever happens: "what did this look like
/// when I started" is the one point in history worth protecting from a bound.
fn prune(conn: &Connection, page_id: PageId) -> Result<()> {
    conn.execute(
        "DELETE FROM revisions
          WHERE page_id = ?1
            AND id NOT IN (
                SELECT id FROM revisions WHERE page_id = ?1
                 ORDER BY created_at DESC LIMIT ?2
            )
            AND id <> (
                SELECT id FROM revisions WHERE page_id = ?1
                 ORDER BY created_at ASC LIMIT 1
            )",
        params![page_id, MAX_REVISIONS_PER_PAGE],
    )?;
    Ok(())
}

/// Whether enough time has passed to warrant another checkpoint.
///
/// Checkpoints are time-based rather than edit-based: a writer who works for an
/// hour should be able to step back through that hour, and one who fixes a
/// typo should not generate a snapshot for it.
pub fn should_checkpoint(
    conn: &Connection,
    page_id: PageId,
    interval_seconds: i64,
) -> Result<bool> {
    let latest: Option<Timestamp> = conn
        .query_row(
            "SELECT created_at FROM revisions WHERE page_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![page_id],
            |row| row.get(0),
        )
        .ok();

    Ok(match latest {
        // No history yet: the first save after content exists is worth keeping,
        // because it is the only chance to record where the Page began.
        None => true,
        Some(at) => (now() - at).num_seconds() >= interval_seconds,
    })
}

pub fn list(conn: &Connection, page_id: PageId) -> Result<Vec<RevisionSummary>> {
    let mut statement = conn.prepare(
        "SELECT id, revision_number, plain_text, word_count, reason, created_at
           FROM revisions WHERE page_id = ?1 ORDER BY created_at DESC",
    )?;

    let mut rows = statement
        .query_map(params![page_id], |row| {
            let plain_text: String = row.get("plain_text")?;
            Ok(RevisionSummary {
                id: row.get("id")?,
                revision_number: row.get("revision_number")?,
                word_count: row.get("word_count")?,
                word_delta: 0,
                reason: RevisionReason::parse(&row.get::<_, String>("reason")?),
                preview: text::preview(&plain_text, 120),
                created_at: row.get("created_at")?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Newest first, so each row's predecessor is the next one in the list.
    for index in 0..rows.len() {
        let previous = rows.get(index + 1).map(|r| r.word_count).unwrap_or(0);
        if let Some(row) = rows.get_mut(index) {
            row.word_delta = row.word_count - previous;
        }
    }

    Ok(rows)
}

pub fn get(conn: &Connection, id: RevisionId) -> Result<Revision> {
    conn.query_row("SELECT * FROM revisions WHERE id = ?1", params![id], map)
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => AppError::not_found("revision"),
            other => other.into(),
        })
}

/// Puts an old revision back as the Page's current content.
///
/// The current text is snapshotted first, so restoring is itself undoable. A
/// history feature that can lose the present while recovering the past is worse
/// than no history feature.
pub fn restore(conn: &Connection, id: RevisionId) -> Result<crate::domain::Page> {
    let revision = get(conn, id)?;
    if revision.document.is_null() {
        return Err(AppError::new(
            crate::error::ErrorCode::Database,
            "That revision could not be read, so it was not restored. \
             Your current text has not been changed.",
        ));
    }

    capture(conn, revision.page_id, RevisionReason::BeforeRestore)?;
    super::pages::save_document(conn, revision.page_id, revision.document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testing::TempDatabase;
    use crate::domain::ChapterId;
    use serde_json::json;

    fn document(text: &str) -> Value {
        json!({
            "type": "doc",
            "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }]
        })
    }

    fn page(conn: &Connection) -> (ChapterId, PageId) {
        let volume = super::super::volumes::create(conn, "A", None, None).unwrap();
        let chapter = super::super::chapters::create(conn, volume.id, "One").unwrap();
        let page = super::super::pages::create(conn, chapter.id, "First").unwrap();
        (chapter.id, page.id)
    }

    #[test]
    fn a_snapshot_records_the_text_as_it_was() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);

        super::super::pages::save_document(&conn, page_id, document("first draft")).unwrap();
        capture(&conn, page_id, RevisionReason::Checkpoint).unwrap();
        super::super::pages::save_document(&conn, page_id, document("second draft")).unwrap();

        let history = list(&conn, page_id).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].preview, "first draft");
        assert_eq!(history[0].word_count, 2);
    }

    #[test]
    fn restoring_puts_the_old_text_back_and_keeps_the_new_one_in_history() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);

        super::super::pages::save_document(&conn, page_id, document("the original")).unwrap();
        let snapshot = capture(&conn, page_id, RevisionReason::Checkpoint).unwrap();
        super::super::pages::save_document(&conn, page_id, document("a regrettable rewrite"))
            .unwrap();

        let restored = restore(&conn, snapshot).unwrap();
        assert_eq!(restored.plain_text, "the original");

        // The rewrite is not lost — restoring is itself undoable.
        let history = list(&conn, page_id).unwrap();
        assert!(
            history.iter().any(|r| r.preview == "a regrettable rewrite"),
            "the replaced text should have been snapshotted first"
        );
        assert!(
            history
                .iter()
                .any(|r| r.reason == RevisionReason::BeforeRestore)
        );
    }

    #[test]
    fn restoring_advances_the_page_revision_rather_than_rewinding_it() {
        // Anchors and AI suggestions key off revision_number; going backwards
        // would make stale things look current.
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);

        super::super::pages::save_document(&conn, page_id, document("one")).unwrap();
        let snapshot = capture(&conn, page_id, RevisionReason::Checkpoint).unwrap();
        let before = super::super::pages::save_document(&conn, page_id, document("two")).unwrap();

        let restored = restore(&conn, snapshot).unwrap();
        assert!(restored.revision_number > before.revision_number);
    }

    #[test]
    fn the_first_checkpoint_is_always_taken() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);
        assert!(should_checkpoint(&conn, page_id, 300).unwrap());
    }

    #[test]
    fn checkpoints_are_not_taken_again_within_the_interval() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);

        capture(&conn, page_id, RevisionReason::Checkpoint).unwrap();
        assert!(!should_checkpoint(&conn, page_id, 300).unwrap());
        // With a zero interval every save qualifies.
        assert!(should_checkpoint(&conn, page_id, 0).unwrap());
    }

    #[test]
    fn history_is_bounded_but_never_loses_where_the_page_began() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);

        super::super::pages::save_document(&conn, page_id, document("the beginning")).unwrap();
        capture(&conn, page_id, RevisionReason::Checkpoint).unwrap();

        for n in 0..(MAX_REVISIONS_PER_PAGE + 10) {
            super::super::pages::save_document(&conn, page_id, document(&format!("draft {n}")))
                .unwrap();
            capture(&conn, page_id, RevisionReason::Checkpoint).unwrap();
        }

        let history = list(&conn, page_id).unwrap();
        assert!(
            history.len() as i64 <= MAX_REVISIONS_PER_PAGE + 1,
            "history grew to {}",
            history.len()
        );
        assert_eq!(
            history.last().unwrap().preview,
            "the beginning",
            "the oldest snapshot must survive pruning"
        );
    }

    #[test]
    fn word_deltas_describe_the_change_each_revision_made() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);

        super::super::pages::save_document(&conn, page_id, document("one two")).unwrap();
        capture(&conn, page_id, RevisionReason::Checkpoint).unwrap();
        super::super::pages::save_document(&conn, page_id, document("one two three four")).unwrap();
        capture(&conn, page_id, RevisionReason::Checkpoint).unwrap();

        let history = list(&conn, page_id).unwrap();
        assert_eq!(history[0].word_count, 4);
        assert_eq!(history[0].word_delta, 2);
        // The oldest has nothing before it, so its delta is its whole length.
        assert_eq!(history[1].word_delta, 2);
    }

    #[test]
    fn deleting_a_page_takes_its_history_with_it() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);

        capture(&conn, page_id, RevisionReason::Checkpoint).unwrap();
        super::super::pages::delete(&conn, page_id).unwrap();

        let left: i64 = conn
            .query_row("SELECT count(*) FROM revisions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn an_unknown_reason_loads_as_a_checkpoint_rather_than_failing() {
        assert_eq!(
            RevisionReason::parse("something-new"),
            RevisionReason::Checkpoint
        );
        for reason in [
            RevisionReason::Checkpoint,
            RevisionReason::BeforeRestore,
            RevisionReason::BeforeAi,
            RevisionReason::BeforeImport,
        ] {
            assert_eq!(RevisionReason::parse(reason.as_str()), reason);
        }
    }
}
