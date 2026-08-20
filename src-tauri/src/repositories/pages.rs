//! Page persistence.
//!
//! `document_json` and every field derived from it are written together, in one
//! statement, so no reader can observe a Page whose word count belongs to a
//! different draft than its text.

use rusqlite::{Connection, Row, params};
use serde_json::Value;

use crate::domain::ids::PageId;
use crate::domain::manuscript::now;
use crate::domain::text::{self, normalise_title};
use crate::domain::{ChapterId, Page, PageSummary, VolumeId};
use crate::error::{AppError, Result};

use super::{apply_order, next_position, normalise_positions};
use crate::search::{self, EntityKind};

/// Writes a Page's searchable text.
///
/// Called from every path that changes a Page's title or body. The index is a
/// virtual table with no foreign keys into the manuscript, so nothing cascades
/// into it and every write has to be explicit.
fn reindex(conn: &Connection, page: &Page) -> Result<()> {
    let volume_id = super::chapters::get(conn, page.chapter_id)?.volume_id;
    search::index(
        conn,
        EntityKind::Page,
        &page.id.to_string(),
        Some(&volume_id.to_string()),
        &page.title,
        &page.plain_text,
    )
}

fn map(row: &Row<'_>) -> rusqlite::Result<Page> {
    let raw: String = row.get("document_json")?;
    Ok(Page {
        id: row.get("id")?,
        chapter_id: row.get("chapter_id")?,
        title: row.get("title")?,
        // A document that will not parse is a corrupted row. Falling back to an
        // empty document would silently discard the user's writing, so the
        // stored text is kept as a single paragraph instead — recoverable, and
        // visibly wrong rather than invisibly lost.
        document: serde_json::from_str(&raw).unwrap_or_else(|_| recovery_document(&raw)),
        plain_text: row.get("plain_text")?,
        position: row.get("position")?,
        revision_number: row.get("revision_number")?,
        word_count: row.get("word_count")?,
        character_count: row.get("character_count")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn recovery_document(raw: &str) -> Value {
    tracing::error!(
        bytes = raw.len(),
        "page document could not be parsed; recovering as text"
    );
    serde_json::json!({
        "type": "doc",
        "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": raw }] }]
    })
}

fn summary(row: &Row<'_>) -> rusqlite::Result<PageSummary> {
    let plain_text: String = row.get("plain_text")?;
    Ok(PageSummary {
        id: row.get("id")?,
        chapter_id: row.get("chapter_id")?,
        title: row.get("title")?,
        position: row.get("position")?,
        word_count: row.get("word_count")?,
        preview: text::preview(&plain_text, 90),
        updated_at: row.get("updated_at")?,
    })
}

/// Inserts a Page with an already-serialised document. Used by create and by
/// the duplication paths, which carry a document across without re-deriving it.
pub fn insert_raw(conn: &Connection, page: &Page, document_json: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO pages (id, chapter_id, title, document_json, plain_text, position,
                            revision_number, word_count, character_count, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            page.id,
            page.chapter_id,
            page.title,
            document_json,
            page.plain_text,
            page.position,
            page.revision_number,
            page.word_count,
            page.character_count,
            page.created_at,
            page.updated_at,
        ],
    )?;
    Ok(())
}

pub fn create(conn: &Connection, chapter_id: ChapterId, title: &str) -> Result<Page> {
    super::chapters::get(conn, chapter_id)?;

    let position = next_position(conn, "pages", "chapter_id", &chapter_id.to_string())?;
    let page = Page::create(chapter_id, title, position);
    insert_raw(conn, &page, &serde_json::to_string(&page.document)?)?;
    reindex(conn, &page)?;
    Ok(page)
}

pub fn get(conn: &Connection, id: PageId) -> Result<Page> {
    conn.query_row("SELECT * FROM pages WHERE id = ?1", params![id], map)
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => AppError::not_found("Page"),
            other => other.into(),
        })
}

/// Tree rows: no documents, so a large Volume costs one small query.
pub fn summaries(conn: &Connection, chapter_id: ChapterId) -> Result<Vec<PageSummary>> {
    let mut statement = conn.prepare(
        "SELECT id, chapter_id, title, plain_text, position, word_count, updated_at
           FROM pages WHERE chapter_id = ?1 ORDER BY position",
    )?;
    let rows = statement
        .query_map(params![chapter_id], summary)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Every summary in a Volume, in manuscript order, in one query.
pub fn summaries_for_volume(conn: &Connection, volume_id: VolumeId) -> Result<Vec<PageSummary>> {
    let mut statement = conn.prepare(
        "SELECT p.id, p.chapter_id, p.title, p.plain_text, p.position, p.word_count, p.updated_at
           FROM pages p JOIN chapters c ON p.chapter_id = c.id
          WHERE c.volume_id = ?1
          ORDER BY c.position, p.position",
    )?;
    let rows = statement
        .query_map(params![volume_id], summary)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list_full(conn: &Connection, chapter_id: ChapterId) -> Result<Vec<Page>> {
    let mut statement =
        conn.prepare("SELECT * FROM pages WHERE chapter_id = ?1 ORDER BY position")?;
    let rows = statement
        .query_map(params![chapter_id], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Persists a new editor document.
///
/// Every derived field is recomputed here — in Rust, from the document that is
/// actually being stored — rather than trusted from the caller. A frontend bug
/// can therefore produce a wrong document, but never a document whose stored
/// word count, plain text, or search index disagree with it.
pub fn save_document(conn: &Connection, id: PageId, document: Value) -> Result<Page> {
    if !document.is_object() || document.get("type").and_then(Value::as_str) != Some("doc") {
        return Err(AppError::invalid("That is not a manuscript document."));
    }

    let plain_text = text::plain_text_from_document(&document);
    let word_count = text::count_words(&plain_text);
    let character_count = text::count_characters(&plain_text);
    let serialised = serde_json::to_string(&document)?;

    let changed = conn.execute(
        "UPDATE pages
            SET document_json = ?2, plain_text = ?3, word_count = ?4, character_count = ?5,
                revision_number = revision_number + 1, updated_at = ?6
          WHERE id = ?1",
        params![
            id,
            serialised,
            plain_text,
            word_count,
            character_count,
            now()
        ],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("Page"));
    }

    let saved = get(conn, id)?;
    reindex(conn, &saved)?;
    Ok(saved)
}

pub fn rename(conn: &Connection, id: PageId, title: &str) -> Result<Page> {
    let normalised = normalise_title(title);
    if normalised.is_empty() {
        return Err(AppError::invalid("A Page needs a title."));
    }
    let changed = conn.execute(
        "UPDATE pages SET title = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, normalised, now()],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("Page"));
    }
    let renamed = get(conn, id)?;
    reindex(conn, &renamed)?;
    Ok(renamed)
}

pub fn delete(conn: &Connection, id: PageId) -> Result<()> {
    let page = get(conn, id)?;
    // Annotations cascade away with the Page, but their index entries do not.
    for annotation in super::annotations::list(conn, id)? {
        search::remove(conn, &annotation.id.to_string())?;
    }
    search::remove(conn, &id.to_string())?;
    super::bookmarks::forget_entity(conn, &id.to_string())?;
    conn.execute("DELETE FROM pages WHERE id = ?1", params![id])?;
    normalise_positions(conn, "pages", "chapter_id", &page.chapter_id.to_string())
}

pub fn reorder(conn: &Connection, chapter_id: ChapterId, ordered: &[PageId]) -> Result<()> {
    let ids: Vec<String> = ordered.iter().map(ToString::to_string).collect();
    apply_order(conn, "pages", "chapter_id", &chapter_id.to_string(), &ids)
}

/// Moves a Page to another Chapter, at `index` within it.
pub fn move_to_chapter(
    conn: &Connection,
    id: PageId,
    target: ChapterId,
    index: Option<i64>,
) -> Result<Page> {
    let page = get(conn, id)?;
    super::chapters::get(conn, target)?;
    let origin = page.chapter_id;

    // Land at the end by default, then let the explicit order settle it.
    let end = next_position(conn, "pages", "chapter_id", &target.to_string())?;
    conn.execute(
        "UPDATE pages SET chapter_id = ?2, position = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, target, end, now()],
    )?;

    if origin != target {
        normalise_positions(conn, "pages", "chapter_id", &origin.to_string())?;
    }

    if let Some(index) = index {
        let mut order: Vec<PageId> = summaries(conn, target)?
            .into_iter()
            .map(|page| page.id)
            .collect();
        order.retain(|existing| *existing != id);
        let seat = (index.max(0) as usize).min(order.len());
        order.insert(seat, id);
        reorder(conn, target, &order)?;
    } else {
        normalise_positions(conn, "pages", "chapter_id", &target.to_string())?;
    }

    get(conn, id)
}

pub fn duplicate(conn: &Connection, id: PageId) -> Result<Page> {
    let source = get(conn, id)?;
    let position = next_position(conn, "pages", "chapter_id", &source.chapter_id.to_string())?;

    let mut copy = Page::create(
        source.chapter_id,
        &format!("{} (copy)", source.title),
        position,
    );
    copy.plain_text = source.plain_text;
    copy.word_count = source.word_count;
    copy.character_count = source.character_count;
    insert_raw(conn, &copy, &serde_json::to_string(&source.document)?)?;
    reindex(conn, &copy)?;

    get(conn, copy.id)
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

    fn chapter(conn: &Connection) -> ChapterId {
        let volume = super::super::volumes::create(conn, "A Volume", None, None).unwrap();
        super::super::chapters::create(conn, volume.id, "One")
            .unwrap()
            .id
    }

    #[test]
    fn a_new_page_is_empty_and_openable() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page = create(&conn, chapter(&conn), "First").unwrap();

        let loaded = get(&conn, page.id).unwrap();
        assert_eq!(loaded.title, "First");
        assert_eq!(loaded.word_count, 0);
        assert_eq!(loaded.document["type"], "doc");
    }

    #[test]
    fn saving_recomputes_every_derived_field_from_the_stored_document() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page = create(&conn, chapter(&conn), "First").unwrap();

        let saved = save_document(&conn, page.id, document("The road had been salt once")).unwrap();

        assert_eq!(saved.plain_text, "The road had been salt once");
        assert_eq!(saved.word_count, 6);
        assert_eq!(saved.character_count, 27);
        assert_eq!(saved.revision_number, page.revision_number + 1);
    }

    #[test]
    fn derived_fields_cannot_be_spoofed_by_the_caller() {
        // save_document takes only the document, so there is no parameter a
        // buggy frontend could use to store a count that contradicts the text.
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page = create(&conn, chapter(&conn), "First").unwrap();
        let saved = save_document(&conn, page.id, document("one two")).unwrap();
        assert_eq!(saved.word_count, 2);
    }

    #[test]
    fn documents_round_trip_unchanged() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page = create(&conn, chapter(&conn), "First").unwrap();

        let rich = json!({
            "type": "doc",
            "content": [
                { "type": "heading", "attrs": { "level": 2 },
                  "content": [{ "type": "text", "text": "Chapter III" }] },
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "salt", "marks": [{ "type": "italic" }] },
                    { "type": "text", "text": " road 手稿" }
                ]}
            ]
        });

        let saved = save_document(&conn, page.id, rich.clone()).unwrap();
        assert_eq!(saved.document, rich);
        assert_eq!(get(&conn, page.id).unwrap().document, rich);
    }

    #[test]
    fn rubbish_is_refused_rather_than_stored() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page = create(&conn, chapter(&conn), "First").unwrap();
        save_document(&conn, page.id, document("real text")).unwrap();

        assert!(save_document(&conn, page.id, json!("just a string")).is_err());
        assert!(save_document(&conn, page.id, json!({ "type": "paragraph" })).is_err());

        // And the previously saved text is untouched.
        assert_eq!(get(&conn, page.id).unwrap().plain_text, "real text");
    }

    #[test]
    fn every_save_advances_the_revision_number() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page = create(&conn, chapter(&conn), "First").unwrap();

        let a = save_document(&conn, page.id, document("one")).unwrap();
        let b = save_document(&conn, page.id, document("two")).unwrap();
        assert_eq!(b.revision_number, a.revision_number + 1);
    }

    #[test]
    fn summaries_carry_a_preview_but_no_document() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let chapter_id = chapter(&conn);
        let page = create(&conn, chapter_id, "First").unwrap();
        save_document(&conn, page.id, document("The road had been salt once")).unwrap();

        let rows = summaries(&conn, chapter_id).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].preview.starts_with("The road"));
        assert_eq!(rows[0].word_count, 6);
    }

    #[test]
    fn moving_a_page_between_chapters_repacks_both_sides() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume = super::super::volumes::create(&conn, "A", None, None).unwrap();
        let one = super::super::chapters::create(&conn, volume.id, "One").unwrap();
        let two = super::super::chapters::create(&conn, volume.id, "Two").unwrap();

        let a = create(&conn, one.id, "A").unwrap();
        let b = create(&conn, one.id, "B").unwrap();
        let c = create(&conn, one.id, "C").unwrap();
        create(&conn, two.id, "Z").unwrap();

        let moved = move_to_chapter(&conn, b.id, two.id, Some(0)).unwrap();
        assert_eq!(moved.chapter_id, two.id);

        let origin = summaries(&conn, one.id).unwrap();
        assert_eq!(
            origin.iter().map(|p| p.position).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(
            origin.iter().map(|p| p.id).collect::<Vec<_>>(),
            [a.id, c.id]
        );

        let target = summaries(&conn, two.id).unwrap();
        assert_eq!(
            target[0].id, b.id,
            "the page should land at the requested index"
        );
        assert_eq!(
            target.iter().map(|p| p.position).collect::<Vec<_>>(),
            [0, 1]
        );
    }

    #[test]
    fn moving_within_the_same_chapter_reseats_without_losing_pages() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let chapter_id = chapter(&conn);

        let a = create(&conn, chapter_id, "A").unwrap();
        let b = create(&conn, chapter_id, "B").unwrap();
        let c = create(&conn, chapter_id, "C").unwrap();

        move_to_chapter(&conn, c.id, chapter_id, Some(0)).unwrap();

        let rows = summaries(&conn, chapter_id).unwrap();
        assert_eq!(
            rows.iter().map(|p| p.id).collect::<Vec<_>>(),
            [c.id, a.id, b.id]
        );
        assert_eq!(
            rows.iter().map(|p| p.position).collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[test]
    fn an_out_of_range_index_clamps_instead_of_failing() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let chapter_id = chapter(&conn);
        create(&conn, chapter_id, "A").unwrap();
        let b = create(&conn, chapter_id, "B").unwrap();

        move_to_chapter(&conn, b.id, chapter_id, Some(9_999)).unwrap();
        let rows = summaries(&conn, chapter_id).unwrap();
        assert_eq!(rows.last().unwrap().id, b.id);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn moving_to_a_missing_chapter_leaves_the_page_where_it_was() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let chapter_id = chapter(&conn);
        let page = create(&conn, chapter_id, "A").unwrap();

        assert!(move_to_chapter(&conn, page.id, ChapterId::new(), None).is_err());
        assert_eq!(get(&conn, page.id).unwrap().chapter_id, chapter_id);
    }

    #[test]
    fn deleting_repacks_the_remaining_positions() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let chapter_id = chapter(&conn);

        let a = create(&conn, chapter_id, "A").unwrap();
        create(&conn, chapter_id, "B").unwrap();
        create(&conn, chapter_id, "C").unwrap();

        delete(&conn, a.id).unwrap();
        let rows = summaries(&conn, chapter_id).unwrap();
        assert_eq!(rows.iter().map(|p| p.position).collect::<Vec<_>>(), [0, 1]);
    }

    #[test]
    fn duplicating_copies_the_document_under_a_new_id() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let chapter_id = chapter(&conn);
        let page = create(&conn, chapter_id, "First").unwrap();
        save_document(&conn, page.id, document("salt road")).unwrap();

        let copy = duplicate(&conn, page.id).unwrap();
        assert_ne!(copy.id, page.id);
        assert_eq!(copy.title, "First (copy)");
        assert_eq!(copy.plain_text, "salt road");

        // Editing the copy must not disturb the original.
        save_document(&conn, copy.id, document("changed")).unwrap();
        assert_eq!(get(&conn, page.id).unwrap().plain_text, "salt road");
    }

    #[test]
    fn volume_wide_summaries_come_back_in_manuscript_order() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume = super::super::volumes::create(&conn, "A", None, None).unwrap();
        let one = super::super::chapters::create(&conn, volume.id, "One").unwrap();
        let two = super::super::chapters::create(&conn, volume.id, "Two").unwrap();

        create(&conn, two.id, "2a").unwrap();
        create(&conn, one.id, "1a").unwrap();
        create(&conn, one.id, "1b").unwrap();

        let rows = summaries_for_volume(&conn, volume.id).unwrap();
        assert_eq!(
            rows.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(),
            ["1a", "1b", "2a"]
        );
    }

    #[test]
    fn a_corrupted_document_row_surfaces_its_text_instead_of_discarding_it() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page = create(&conn, chapter(&conn), "First").unwrap();

        conn.execute(
            "UPDATE pages SET document_json = ?2 WHERE id = ?1",
            params![page.id, "{ this is not json"],
        )
        .unwrap();

        let recovered = get(&conn, page.id).unwrap();
        assert_eq!(recovered.document["type"], "doc");
        assert!(
            recovered.document["content"][0]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("not json"),
            "the stored bytes should still be reachable by the user"
        );
    }
}
