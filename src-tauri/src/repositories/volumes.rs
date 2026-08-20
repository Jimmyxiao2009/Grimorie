//! Volume persistence.

use rusqlite::{Connection, Row, params};

use crate::domain::manuscript::{CoverTint, Volume, VolumeSummary, now};
use crate::domain::text::normalise_title;
use crate::domain::{ChapterId, PageId, VolumeId};
use crate::error::{AppError, Result};
use crate::search::{self, EntityKind};

fn reindex(conn: &Connection, volume: &Volume) -> Result<()> {
    search::index(
        conn,
        EntityKind::Volume,
        &volume.id.to_string(),
        Some(&volume.id.to_string()),
        &volume.title,
        volume.subtitle.as_deref().unwrap_or(""),
    )
}

const COLUMNS: &str =
    "id, title, subtitle, description, tint, created_at, updated_at, last_opened_at, archived_at";

fn map(row: &Row<'_>) -> rusqlite::Result<Volume> {
    Ok(Volume {
        id: row.get("id")?,
        title: row.get("title")?,
        subtitle: row.get("subtitle")?,
        description: row.get("description")?,
        tint: CoverTint::parse(&row.get::<_, String>("tint")?),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        last_opened_at: row.get("last_opened_at")?,
        archived_at: row.get("archived_at")?,
    })
}

pub fn insert(conn: &Connection, volume: &Volume) -> Result<()> {
    conn.execute(
        "INSERT INTO volumes (id, title, subtitle, description, tint,
                              created_at, updated_at, last_opened_at, archived_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            volume.id,
            volume.title,
            volume.subtitle,
            volume.description,
            volume.tint.as_str(),
            volume.created_at,
            volume.updated_at,
            volume.last_opened_at,
            volume.archived_at,
        ],
    )?;
    Ok(())
}

pub fn create(
    conn: &Connection,
    title: &str,
    subtitle: Option<&str>,
    description: Option<&str>,
) -> Result<Volume> {
    let volume = Volume::create(title, subtitle, description);
    insert(conn, &volume)?;
    reindex(conn, &volume)?;
    Ok(volume)
}

pub fn get(conn: &Connection, id: VolumeId) -> Result<Volume> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM volumes WHERE id = ?1"),
        params![id],
        map,
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => AppError::not_found("Volume"),
        other => other.into(),
    })
}

/// Which slice of the Library to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    Active,
    Archived,
    All,
}

impl Shelf {
    fn predicate(&self) -> &'static str {
        match self {
            Shelf::Active => "WHERE v.archived_at IS NULL",
            Shelf::Archived => "WHERE v.archived_at IS NOT NULL",
            Shelf::All => "",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        Ok(match raw {
            "active" => Shelf::Active,
            "archived" => Shelf::Archived,
            "all" => Shelf::All,
            _ => return Err(AppError::invalid("Unknown Library shelf.")),
        })
    }
}

/// Lists Volumes with the counts the shelf needs.
///
/// The counts are computed in SQL rather than by loading each Volume's
/// manuscript, so drawing the Library is one query regardless of its size.
pub fn list(conn: &Connection, shelf: Shelf, limit: Option<i64>) -> Result<Vec<VolumeSummary>> {
    let order = match shelf {
        // Recent means recently *opened*, falling back to when it changed, so a
        // Volume never vanishes from the top of the shelf just because it has
        // not been opened since it was created.
        Shelf::Archived => "ORDER BY v.archived_at DESC",
        _ => "ORDER BY COALESCE(v.last_opened_at, v.updated_at) DESC",
    };
    let sql = format!(
        "SELECT v.id, v.title, v.subtitle, v.description, v.tint, v.created_at, v.updated_at,
                v.last_opened_at, v.archived_at,
                (SELECT count(*) FROM chapters c WHERE c.volume_id = v.id) AS chapter_count,
                (SELECT count(*) FROM pages p
                   JOIN chapters c ON p.chapter_id = c.id
                  WHERE c.volume_id = v.id) AS page_count,
                (SELECT COALESCE(SUM(p.word_count), 0) FROM pages p
                   JOIN chapters c ON p.chapter_id = c.id
                  WHERE c.volume_id = v.id) AS word_count
           FROM volumes v
           {} {order} LIMIT ?1",
        shelf.predicate()
    );

    let mut statement = conn.prepare(&sql)?;
    let rows = statement
        .query_map([limit.unwrap_or(-1)], |row| {
            Ok(VolumeSummary {
                volume: map(row)?,
                chapter_count: row.get("chapter_count")?,
                page_count: row.get("page_count")?,
                word_count: row.get("word_count")?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn update_details(
    conn: &Connection,
    id: VolumeId,
    title: &str,
    subtitle: Option<&str>,
    description: Option<&str>,
) -> Result<Volume> {
    let normalised = normalise_title(title);
    if normalised.is_empty() {
        return Err(AppError::invalid("A Volume needs a title."));
    }

    let changed = conn.execute(
        "UPDATE volumes SET title = ?2, subtitle = ?3, description = ?4, updated_at = ?5
         WHERE id = ?1",
        params![
            id,
            normalised,
            subtitle.map(str::trim).filter(|s| !s.is_empty()),
            description.map(str::trim).filter(|s| !s.is_empty()),
            now(),
        ],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("Volume"));
    }
    let updated = get(conn, id)?;
    reindex(conn, &updated)?;
    Ok(updated)
}

pub fn set_archived(conn: &Connection, id: VolumeId, archived: bool) -> Result<Volume> {
    let at = now();
    let changed = conn.execute(
        "UPDATE volumes SET archived_at = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, archived.then_some(at), at],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("Volume"));
    }
    get(conn, id)
}

/// Records that a Volume was opened, which is what orders the Recent shelf.
pub fn touch_opened(conn: &Connection, id: VolumeId) -> Result<()> {
    // Deliberately does not touch `updated_at`: opening a manuscript is not
    // editing it, and conflating the two would make "last edited" a lie.
    conn.execute(
        "UPDATE volumes SET last_opened_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: VolumeId) -> Result<()> {
    // The index is a virtual table with no foreign keys into the manuscript, so
    // the cascade that removes chapters and pages cannot reach it.
    search::remove_volume(conn, id)?;
    // Tag attachments carry no foreign key — entity_id points at one of three
    // tables — so they are cleared explicitly, for the Volume and everything
    // beneath it.
    for chapter in super::chapters::list(conn, id)? {
        for page in super::pages::summaries(conn, chapter.id)? {
            super::bookmarks::forget_entity(conn, &page.id.to_string())?;
        }
        super::bookmarks::forget_entity(conn, &chapter.id.to_string())?;
    }
    super::bookmarks::forget_entity(conn, &id.to_string())?;
    let changed = conn.execute("DELETE FROM volumes WHERE id = ?1", params![id])?;
    if changed == 0 {
        return Err(AppError::not_found("Volume"));
    }
    Ok(())
}

/// Copies a Volume and its whole manuscript.
///
/// New identifiers throughout — a duplicate is a separate work, not a second
/// name for the same one.
pub fn duplicate(conn: &Connection, id: VolumeId) -> Result<Volume> {
    let source = get(conn, id)?;
    let mut copy = Volume::create(
        &format!("{} (copy)", source.title),
        source.subtitle.as_deref(),
        source.description.as_deref(),
    );
    copy.tint = source.tint;
    insert(conn, &copy)?;
    reindex(conn, &copy)?;

    let mut chapter_statement = conn.prepare(
        "SELECT id, title, position FROM chapters WHERE volume_id = ?1 ORDER BY position",
    )?;
    let chapters = chapter_statement
        .query_map(params![id], |row| {
            Ok((
                row.get::<_, ChapterId>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for (chapter_id, title, position) in chapters {
        let new_chapter = crate::domain::Chapter::create(copy.id, &title, position);
        super::chapters::insert(conn, &new_chapter)?;

        let mut page_statement = conn.prepare(
            "SELECT id, title, document_json, plain_text, position, word_count, character_count
               FROM pages WHERE chapter_id = ?1 ORDER BY position",
        )?;
        let pages = page_statement
            .query_map(params![chapter_id], |row| {
                Ok((
                    row.get::<_, PageId>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for (_, title, document, plain_text, position, words, characters) in pages {
            let mut page = crate::domain::Page::create(new_chapter.id, &title, position);
            page.plain_text = plain_text;
            page.word_count = words;
            page.character_count = characters;
            super::pages::insert_raw(conn, &page, &document)?;
        }
    }

    get(conn, copy.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testing::TempDatabase;

    #[test]
    fn a_created_volume_can_be_read_back() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();

        let created = create(&conn, "The Salt Road", Some("A novel"), None).unwrap();
        let loaded = get(&conn, created.id).unwrap();

        assert_eq!(loaded.title, "The Salt Road");
        assert_eq!(loaded.subtitle.as_deref(), Some("A novel"));
        assert_eq!(loaded.tint, created.tint);
        assert!(loaded.archived_at.is_none());
    }

    #[test]
    fn timestamps_survive_a_round_trip() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let created = create(&conn, "A", None, None).unwrap();
        let loaded = get(&conn, created.id).unwrap();
        // Millisecond equality is enough; the point is that it is the same
        // instant and not shifted by a timezone.
        assert_eq!(
            loaded.created_at.timestamp(),
            created.created_at.timestamp()
        );
    }

    #[test]
    fn a_missing_volume_reports_not_found_rather_than_a_database_error() {
        let db = TempDatabase::open();
        let error = get(&db.get().unwrap(), VolumeId::new()).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::NotFound);
        assert!(error.message.contains("Volume"));
    }

    #[test]
    fn the_shelf_separates_active_from_archived() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();

        let kept = create(&conn, "Kept", None, None).unwrap();
        let shelved = create(&conn, "Shelved", None, None).unwrap();
        set_archived(&conn, shelved.id, true).unwrap();

        let active = list(&conn, Shelf::Active, None).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].volume.id, kept.id);

        let archived = list(&conn, Shelf::Archived, None).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].volume.id, shelved.id);

        assert_eq!(list(&conn, Shelf::All, None).unwrap().len(), 2);
    }

    #[test]
    fn archiving_is_reversible() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume = create(&conn, "A", None, None).unwrap();

        assert!(set_archived(&conn, volume.id, true).unwrap().is_archived());
        assert!(!set_archived(&conn, volume.id, false).unwrap().is_archived());
    }

    #[test]
    fn summaries_count_the_manuscript_without_loading_it() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();

        let volume = create(&conn, "A", None, None).unwrap();
        let chapter = super::super::chapters::create(&conn, volume.id, "One").unwrap();
        let page = super::super::pages::create(&conn, chapter.id, "First").unwrap();
        super::super::pages::save_document(
            &conn,
            page.id,
            serde_json::json!({
                "type": "doc",
                "content": [{ "type": "paragraph",
                              "content": [{ "type": "text", "text": "one two three" }] }]
            }),
        )
        .unwrap();

        let summary = &list(&conn, Shelf::Active, None).unwrap()[0];
        assert_eq!(summary.chapter_count, 1);
        assert_eq!(summary.page_count, 1);
        assert_eq!(summary.word_count, 3);
    }

    #[test]
    fn an_empty_volume_reports_zero_rather_than_null() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        create(&conn, "Empty", None, None).unwrap();

        let summary = &list(&conn, Shelf::Active, None).unwrap()[0];
        assert_eq!(summary.word_count, 0);
        assert_eq!(summary.page_count, 0);
    }

    #[test]
    fn recent_orders_by_when_a_volume_was_last_opened() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();

        let first = create(&conn, "First", None, None).unwrap();
        let second = create(&conn, "Second", None, None).unwrap();
        touch_opened(&conn, first.id).unwrap();

        let listed = list(&conn, Shelf::Active, None).unwrap();
        assert_eq!(
            listed[0].volume.id, first.id,
            "the just-opened Volume should lead"
        );
        assert_eq!(listed[1].volume.id, second.id);
    }

    #[test]
    fn opening_a_volume_is_not_recorded_as_editing_it() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume = create(&conn, "A", None, None).unwrap();

        touch_opened(&conn, volume.id).unwrap();
        let reloaded = get(&conn, volume.id).unwrap();

        assert_eq!(
            reloaded.updated_at.timestamp_millis(),
            volume.updated_at.timestamp_millis()
        );
        assert!(reloaded.last_opened_at.is_some());
    }

    #[test]
    fn renaming_refuses_an_empty_title() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume = create(&conn, "A", None, None).unwrap();

        let error = update_details(&conn, volume.id, "   ", None, None).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::InvalidInput);
        // And the original title survives the refusal.
        assert_eq!(get(&conn, volume.id).unwrap().title, "A");
    }

    #[test]
    fn deleting_a_volume_takes_its_manuscript_with_it() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();

        let volume = create(&conn, "A", None, None).unwrap();
        let chapter = super::super::chapters::create(&conn, volume.id, "One").unwrap();
        super::super::pages::create(&conn, chapter.id, "First").unwrap();

        delete(&conn, volume.id).unwrap();

        let orphaned_chapters: i64 = conn
            .query_row("SELECT count(*) FROM chapters", [], |row| row.get(0))
            .unwrap();
        let orphaned_pages: i64 = conn
            .query_row("SELECT count(*) FROM pages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(orphaned_chapters, 0);
        assert_eq!(orphaned_pages, 0);
    }

    #[test]
    fn duplicating_copies_the_whole_manuscript_under_new_ids() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();

        let volume = create(&conn, "The Salt Road", None, None).unwrap();
        let chapter = super::super::chapters::create(&conn, volume.id, "One").unwrap();
        let page = super::super::pages::create(&conn, chapter.id, "First").unwrap();
        super::super::pages::save_document(
            &conn,
            page.id,
            serde_json::json!({
                "type": "doc",
                "content": [{ "type": "paragraph",
                              "content": [{ "type": "text", "text": "salt" }] }]
            }),
        )
        .unwrap();

        let copy = duplicate(&conn, volume.id).unwrap();
        assert_ne!(copy.id, volume.id);
        assert_eq!(copy.title, "The Salt Road (copy)");

        let copied_chapters = super::super::chapters::list(&conn, copy.id).unwrap();
        assert_eq!(copied_chapters.len(), 1);
        assert_ne!(copied_chapters[0].id, chapter.id);

        let copied_pages = super::super::pages::summaries(&conn, copied_chapters[0].id).unwrap();
        assert_eq!(copied_pages.len(), 1);
        assert_ne!(copied_pages[0].id, page.id);
        assert_eq!(copied_pages[0].word_count, 1);

        // And editing the copy must not touch the original.
        let original_pages = super::super::pages::summaries(&conn, chapter.id).unwrap();
        assert_eq!(original_pages.len(), 1);
        assert_eq!(original_pages[0].id, page.id);
    }
}
