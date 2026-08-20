//! Crash-safe draft journal.
//!
//! Autosave commits a Page when typing pauses. This shortens the window before
//! that: the editor journals its document on a much shorter debounce into a
//! single row, which is a cheap upsert rather than a full save with derived
//! fields and revision bookkeeping.
//!
//! The recovery rules exist to make one guarantee — **recovery never overwrites
//! newer content**:
//!
//! * A draft is deleted the instant its Page saves. A row here always means
//!   "typed but not committed".
//! * A draft whose `base_revision` is behind the Page's is from before a later
//!   successful save and is discarded without asking.
//! * A draft whose text already matches the Page is discarded without asking.
//! * Only what survives both checks is offered back, and even then the writer
//!   decides. Nothing is restored automatically.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::ids::PageId;
use crate::domain::manuscript::{Timestamp, now};
use crate::domain::text;
use crate::error::Result;

/// A draft worth offering back, with enough context to describe the choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverableDraft {
    pub page_id: PageId,
    pub page_title: String,
    pub volume_title: String,
    pub document: Value,
    /// What the recovered text says, for the preview in the prompt.
    pub preview: String,
    pub word_count: i64,
    /// What the Page currently holds, so the two can be compared.
    pub saved_preview: String,
    pub saved_word_count: i64,
    pub captured_at: Timestamp,
}

/// Journals the in-progress document for a Page.
pub fn write(conn: &Connection, page_id: PageId, document: &Value) -> Result<()> {
    let page = super::pages::get(conn, page_id)?;
    let plain_text = text::plain_text_from_document(document);

    conn.execute(
        "INSERT INTO drafts (page_id, document_json, plain_text, base_revision, captured_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (page_id) DO UPDATE SET
             document_json = excluded.document_json,
             plain_text    = excluded.plain_text,
             base_revision = excluded.base_revision,
             captured_at   = excluded.captured_at",
        params![
            page_id,
            serde_json::to_string(document)?,
            plain_text,
            page.revision_number,
            now(),
        ],
    )?;
    Ok(())
}

/// Drops the journal for a Page. Called on every successful save.
pub fn clear(conn: &Connection, page_id: PageId) -> Result<()> {
    conn.execute("DELETE FROM drafts WHERE page_id = ?1", params![page_id])?;
    Ok(())
}

/// Everything worth offering back after an unclean shutdown.
///
/// Drafts that are stale or identical to their Page are deleted here rather
/// than returned, so the caller can show the prompt if and only if this comes
/// back non-empty. Recovery UI for nothing to recover is worse than none.
pub fn recoverable(conn: &Connection) -> Result<Vec<RecoverableDraft>> {
    let mut statement = conn.prepare(
        "SELECT d.page_id, d.document_json, d.plain_text, d.base_revision, d.captured_at,
                p.title AS page_title, p.plain_text AS saved_text,
                p.revision_number, p.word_count AS saved_words,
                v.title AS volume_title
           FROM drafts d
           JOIN pages p    ON p.id = d.page_id
           JOIN chapters c ON c.id = p.chapter_id
           JOIN volumes v  ON v.id = c.volume_id
          ORDER BY d.captured_at DESC",
    )?;

    struct Row {
        page_id: PageId,
        document: String,
        plain_text: String,
        base_revision: i64,
        captured_at: Timestamp,
        page_title: String,
        saved_text: String,
        revision_number: i64,
        saved_words: i64,
        volume_title: String,
    }

    let rows = statement
        .query_map([], |row| {
            Ok(Row {
                page_id: row.get("page_id")?,
                document: row.get("document_json")?,
                plain_text: row.get("plain_text")?,
                base_revision: row.get("base_revision")?,
                captured_at: row.get("captured_at")?,
                page_title: row.get("page_title")?,
                saved_text: row.get("saved_text")?,
                revision_number: row.get("revision_number")?,
                saved_words: row.get("saved_words")?,
                volume_title: row.get("volume_title")?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut offers = Vec::new();
    let mut discard = Vec::new();

    for row in rows {
        // The Page moved on after this draft was captured, so the draft is
        // older than what is stored. Offering it would risk replacing newer
        // content with older text.
        let stale = row.base_revision < row.revision_number;
        // Nothing was actually lost.
        let identical = row.plain_text == row.saved_text;

        let document: Option<Value> = serde_json::from_str(&row.document).ok();

        if stale || identical || document.is_none() {
            discard.push(row.page_id);
            continue;
        }

        offers.push(RecoverableDraft {
            page_id: row.page_id,
            page_title: row.page_title,
            volume_title: row.volume_title,
            preview: text::preview(&row.plain_text, 160),
            word_count: text::count_words(&row.plain_text),
            saved_preview: text::preview(&row.saved_text, 160),
            saved_word_count: row.saved_words,
            captured_at: row.captured_at,
            document: document.unwrap_or(Value::Null),
        });
    }

    for page_id in discard {
        clear(conn, page_id)?;
    }

    if !offers.is_empty() {
        tracing::info!(count = offers.len(), "recoverable drafts found");
    }

    Ok(offers)
}

/// Accepts a draft: writes it over the Page, then clears the journal.
///
/// A revision is taken first by the caller, so the text being replaced remains
/// reachable even if the writer recovers something they did not want.
pub fn recover(conn: &Connection, page_id: PageId) -> Result<crate::domain::Page> {
    let stored: Option<(String, i64)> = conn
        .query_row(
            "SELECT document_json, base_revision FROM drafts WHERE page_id = ?1",
            params![page_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    let Some((raw, base_revision)) = stored else {
        return Err(crate::error::AppError::not_found("draft"));
    };

    let page = super::pages::get(conn, page_id)?;
    if base_revision < page.revision_number {
        // Something saved between the prompt being shown and it being answered.
        clear(conn, page_id)?;
        return Err(crate::error::AppError::stale(
            "That draft is older than the text now on the Page, so it was not restored. \
             Your current text has not been changed.",
        ));
    }

    let document: Value = serde_json::from_str(&raw)?;
    super::revisions::capture(
        conn,
        page_id,
        super::revisions::RevisionReason::BeforeRestore,
    )?;
    let saved = super::pages::save_document(conn, page_id, document)?;
    clear(conn, page_id)?;
    Ok(saved)
}

/// Declines a draft.
pub fn discard(conn: &Connection, page_id: PageId) -> Result<()> {
    clear(conn, page_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testing::TempDatabase;
    use serde_json::json;

    fn document(text: &str) -> Value {
        json!({
            "type": "doc",
            "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }]
        })
    }

    fn page(conn: &Connection) -> PageId {
        let volume = super::super::volumes::create(conn, "A Volume", None, None).unwrap();
        let chapter = super::super::chapters::create(conn, volume.id, "One").unwrap();
        super::super::pages::create(conn, chapter.id, "First")
            .unwrap()
            .id
    }

    #[test]
    fn nothing_to_recover_reports_nothing() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        page(&conn);
        assert!(recoverable(&conn).unwrap().is_empty());
    }

    #[test]
    fn a_journalled_draft_that_never_saved_is_offered_back() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);

        super::super::pages::save_document(&conn, page_id, document("committed text")).unwrap();
        // Typing continues, and the process dies before autosave fires.
        write(
            &conn,
            page_id,
            &document("committed text and more that never saved"),
        )
        .unwrap();

        let offers = recoverable(&conn).unwrap();
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].page_id, page_id);
        assert!(offers[0].preview.contains("never saved"));
        assert_eq!(offers[0].saved_preview, "committed text");
        assert_eq!(offers[0].page_title, "First");
        assert_eq!(offers[0].volume_title, "A Volume");
    }

    #[test]
    fn a_draft_matching_the_saved_text_is_discarded_silently() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);

        super::super::pages::save_document(&conn, page_id, document("same text")).unwrap();
        write(&conn, page_id, &document("same text")).unwrap();

        assert!(recoverable(&conn).unwrap().is_empty());
        // And it is cleaned up rather than being re-examined on every launch.
        let left: i64 = conn
            .query_row("SELECT count(*) FROM drafts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn a_draft_from_before_a_later_save_is_never_offered() {
        // This is the rule that stops recovery overwriting newer content.
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);

        write(&conn, page_id, &document("an old draft")).unwrap();
        // A later save advances the Page past the draft's base revision.
        super::super::pages::save_document(&conn, page_id, document("much newer text")).unwrap();

        assert!(recoverable(&conn).unwrap().is_empty());
        assert_eq!(
            super::super::pages::get(&conn, page_id).unwrap().plain_text,
            "much newer text"
        );
    }

    #[test]
    fn recovering_a_stale_draft_refuses_rather_than_clobbering() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);

        write(&conn, page_id, &document("old draft")).unwrap();
        super::super::pages::save_document(&conn, page_id, document("newer saved text")).unwrap();

        let error = recover(&conn, page_id).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::Stale);
        assert!(error.message.contains("has not been changed"));
        assert_eq!(
            super::super::pages::get(&conn, page_id).unwrap().plain_text,
            "newer saved text"
        );
    }

    #[test]
    fn recovering_restores_the_text_and_keeps_the_replaced_version() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);

        super::super::pages::save_document(&conn, page_id, document("saved")).unwrap();
        write(&conn, page_id, &document("saved plus unsaved work")).unwrap();

        let recovered = recover(&conn, page_id).unwrap();
        assert_eq!(recovered.plain_text, "saved plus unsaved work");

        // The journal is gone, so the prompt does not return next launch.
        assert!(recoverable(&conn).unwrap().is_empty());
        // And the text that was replaced is still reachable.
        let history = super::super::revisions::list(&conn, page_id).unwrap();
        assert!(history.iter().any(|r| r.preview == "saved"));
    }

    #[test]
    fn discarding_removes_the_draft_and_leaves_the_page_alone() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);

        super::super::pages::save_document(&conn, page_id, document("keep this")).unwrap();
        write(&conn, page_id, &document("throw this away")).unwrap();

        discard(&conn, page_id).unwrap();

        assert!(recoverable(&conn).unwrap().is_empty());
        assert_eq!(
            super::super::pages::get(&conn, page_id).unwrap().plain_text,
            "keep this"
        );
    }

    #[test]
    fn journalling_twice_replaces_rather_than_accumulating() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);

        write(&conn, page_id, &document("first")).unwrap();
        write(&conn, page_id, &document("second")).unwrap();

        let rows: i64 = conn
            .query_row("SELECT count(*) FROM drafts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);

        let offers = recoverable(&conn).unwrap();
        assert_eq!(offers.len(), 1);
        assert!(offers[0].preview.contains("second"));
    }

    #[test]
    fn deleting_a_page_takes_its_draft_with_it() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);

        write(&conn, page_id, &document("orphan me")).unwrap();
        super::super::pages::delete(&conn, page_id).unwrap();

        let rows: i64 = conn
            .query_row("SELECT count(*) FROM drafts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn a_corrupted_draft_row_is_discarded_rather_than_offered() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);

        super::super::pages::save_document(&conn, page_id, document("saved")).unwrap();
        write(&conn, page_id, &document("draft")).unwrap();
        conn.execute(
            "UPDATE drafts SET document_json = '{ not json' WHERE page_id = ?1",
            params![page_id],
        )
        .unwrap();

        assert!(recoverable(&conn).unwrap().is_empty());
    }
}
