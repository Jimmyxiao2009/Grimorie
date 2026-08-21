//! Repositories — the only place SQL is written.
//!
//! Each function takes a `&Connection`, which a `Transaction` derefs to, so the
//! same code composes into a larger transaction or runs standalone. Callers
//! that mutate more than one row are expected to wrap the call in
//! [`crate::database::Database::transaction`].

pub mod ai;
pub mod annotations;
pub mod bookmarks;
pub mod chapters;
pub mod drafts;
pub mod ink;
pub mod pages;
pub mod revisions;
pub mod settings;
pub mod volumes;

use rusqlite::Connection;

use crate::domain::{Page, PageId};
use crate::error::Result;

/// Saves a Page and does the bookkeeping that must happen with it.
///
/// Three things belong in one transaction, and this is the only place that
/// knows they belong together:
///
/// 1. A checkpoint revision, if enough time has passed since the last one.
///    Taken *before* the write, so it captures what is being replaced.
/// 2. The write itself.
/// 3. Clearing the crash-recovery draft, because a committed Page has nothing
///    left to recover.
/// 4. Re-placing every annotation against the new text, so the Margin can never
///    be observed pointing at a version of the Page that no longer exists.
///
/// The checkpoint interval comes from settings rather than a constant, so a
/// writer who wants a denser history can have one.
pub fn save_page(conn: &Connection, page_id: PageId, document: serde_json::Value) -> Result<Page> {
    let interval = settings::load(conn)?.revision_interval_seconds;

    // Only worth a snapshot once the Page has something in it — the empty
    // state a Page is created in is not a draft anyone wants back.
    let existing = pages::get(conn, page_id)?;
    if !existing.plain_text.is_empty() && revisions::should_checkpoint(conn, page_id, interval)? {
        revisions::capture(conn, page_id, revisions::RevisionReason::Checkpoint)?;
    }

    let saved = pages::save_document(conn, page_id, document)?;
    drafts::clear(conn, page_id)?;
    annotations::reanchor_page(conn, page_id)?;
    Ok(saved)
}

/// Rewrites a sibling group's positions to 0..n-1 in their current order.
///
/// Called after every structural change. Gaps and duplicates are not merely
/// untidy — reordering by drag depends on positions being dense and unique, and
/// a duplicate makes the resulting order depend on SQLite's tiebreak, which is
/// not something a user should be able to observe.
fn normalise_positions(
    conn: &Connection,
    table: &str,
    parent_column: &str,
    parent_id: &str,
) -> Result<()> {
    // The table and column names are compile-time constants from this module,
    // never user input; the parent id is bound as a parameter.
    let sql = format!(
        "WITH ordered AS (
             SELECT id, ROW_NUMBER() OVER (ORDER BY position, created_at, id) - 1 AS seat
             FROM {table} WHERE {parent_column} = ?1
         )
         UPDATE {table} SET position = (SELECT seat FROM ordered WHERE ordered.id = {table}.id)
         WHERE {parent_column} = ?1"
    );
    conn.execute(&sql, [parent_id])?;
    Ok(())
}

/// The position a newly created sibling should take: the end of the list.
fn next_position(
    conn: &Connection,
    table: &str,
    parent_column: &str,
    parent_id: &str,
) -> Result<i64> {
    let sql =
        format!("SELECT COALESCE(MAX(position) + 1, 0) FROM {table} WHERE {parent_column} = ?1");
    Ok(conn.query_row(&sql, [parent_id], |row| row.get(0))?)
}

/// Applies an explicit order to a sibling group.
///
/// Ids not in `ordered` keep their relative order and follow the listed ones,
/// so a reorder computed against a slightly stale tree cannot drop a Page.
fn apply_order(
    conn: &Connection,
    table: &str,
    parent_column: &str,
    parent_id: &str,
    ordered: &[String],
) -> Result<()> {
    let sql = format!("UPDATE {table} SET position = ?1 WHERE id = ?2 AND {parent_column} = ?3");
    for (seat, id) in ordered.iter().enumerate() {
        // Listed items take the low seats; anything unlisted sorts after them
        // because normalisation preserves relative order.
        conn.execute(&sql, rusqlite::params![seat as i64, id, parent_id])?;
    }
    let bump = format!(
        "UPDATE {table} SET position = position + ?1
         WHERE {parent_column} = ?2 AND id NOT IN (SELECT value FROM json_each(?3))"
    );
    let listed = serde_json::to_string(ordered)?;
    conn.execute(
        &bump,
        rusqlite::params![ordered.len() as i64, parent_id, listed],
    )?;

    normalise_positions(conn, table, parent_column, parent_id)
}

/// A Volume's whole structure, without any documents.
///
/// Two queries regardless of size: the chapter list, and every page summary in
/// the Volume. Assembling the tree per chapter would be N+1 queries for
/// something drawn on every navigation.
pub fn outline(
    conn: &Connection,
    volume_id: crate::domain::VolumeId,
) -> Result<crate::domain::Outline> {
    use crate::domain::{ChapterOutline, Outline};

    let volume = volumes::get(conn, volume_id)?;
    let chapters = chapters::list(conn, volume_id)?;
    let mut summaries = pages::summaries_for_volume(conn, volume_id)?;

    let chapters = chapters
        .into_iter()
        .map(|chapter| {
            // `summaries` arrives in manuscript order, so each chapter's pages
            // are a contiguous run that can be split off rather than searched.
            let taken = summaries
                .iter()
                .position(|page| page.chapter_id != chapter.id)
                .unwrap_or(summaries.len());
            let pages = summaries.drain(..taken).collect();
            ChapterOutline { chapter, pages }
        })
        .collect();

    Ok(Outline { volume, chapters })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testing::TempDatabase;

    #[test]
    fn an_outline_groups_pages_under_their_chapters() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();

        let volume = volumes::create(&conn, "A", None, None).unwrap();
        let one = chapters::create(&conn, volume.id, "One").unwrap();
        let two = chapters::create(&conn, volume.id, "Two").unwrap();

        pages::create(&conn, one.id, "1a").unwrap();
        pages::create(&conn, one.id, "1b").unwrap();
        pages::create(&conn, two.id, "2a").unwrap();

        let outline = outline(&conn, volume.id).unwrap();
        assert_eq!(outline.volume.id, volume.id);
        assert_eq!(outline.chapters.len(), 2);
        assert_eq!(
            outline.chapters[0]
                .pages
                .iter()
                .map(|p| p.title.as_str())
                .collect::<Vec<_>>(),
            ["1a", "1b"]
        );
        assert_eq!(
            outline.chapters[1]
                .pages
                .iter()
                .map(|p| p.title.as_str())
                .collect::<Vec<_>>(),
            ["2a"]
        );
    }

    #[test]
    fn an_empty_chapter_appears_with_no_pages() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();

        let volume = volumes::create(&conn, "A", None, None).unwrap();
        chapters::create(&conn, volume.id, "Empty").unwrap();
        let full = chapters::create(&conn, volume.id, "Full").unwrap();
        pages::create(&conn, full.id, "p").unwrap();

        let outline = outline(&conn, volume.id).unwrap();
        assert!(outline.chapters[0].pages.is_empty());
        assert_eq!(outline.chapters[1].pages.len(), 1);
    }

    #[test]
    fn a_volume_with_no_chapters_outlines_cleanly() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume = volumes::create(&conn, "A", None, None).unwrap();

        let outline = outline(&conn, volume.id).unwrap();
        assert!(outline.chapters.is_empty());
    }
}
