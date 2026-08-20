//! Chapter persistence.

use rusqlite::{Connection, Row, params};

use crate::domain::manuscript::now;
use crate::domain::text::normalise_title;
use crate::domain::{Chapter, ChapterId, VolumeId};
use crate::error::{AppError, Result};

use super::{apply_order, next_position, normalise_positions};

const COLUMNS: &str = "id, volume_id, title, position, created_at, updated_at";

fn map(row: &Row<'_>) -> rusqlite::Result<Chapter> {
    Ok(Chapter {
        id: row.get("id")?,
        volume_id: row.get("volume_id")?,
        title: row.get("title")?,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn insert(conn: &Connection, chapter: &Chapter) -> Result<()> {
    conn.execute(
        "INSERT INTO chapters (id, volume_id, title, position, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            chapter.id,
            chapter.volume_id,
            chapter.title,
            chapter.position,
            chapter.created_at,
            chapter.updated_at,
        ],
    )?;
    Ok(())
}

pub fn create(conn: &Connection, volume_id: VolumeId, title: &str) -> Result<Chapter> {
    // Fails cleanly rather than orphaning a Chapter under a Volume that has
    // been deleted in another window.
    super::volumes::get(conn, volume_id)?;

    let position = next_position(conn, "chapters", "volume_id", &volume_id.to_string())?;
    let chapter = Chapter::create(volume_id, title, position);
    insert(conn, &chapter)?;
    Ok(chapter)
}

pub fn get(conn: &Connection, id: ChapterId) -> Result<Chapter> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM chapters WHERE id = ?1"),
        params![id],
        map,
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => AppError::not_found("Chapter"),
        other => other.into(),
    })
}

pub fn list(conn: &Connection, volume_id: VolumeId) -> Result<Vec<Chapter>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM chapters WHERE volume_id = ?1 ORDER BY position"
    ))?;
    let rows = statement
        .query_map(params![volume_id], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn rename(conn: &Connection, id: ChapterId, title: &str) -> Result<Chapter> {
    let normalised = normalise_title(title);
    if normalised.is_empty() {
        return Err(AppError::invalid("A Chapter needs a title."));
    }
    let changed = conn.execute(
        "UPDATE chapters SET title = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, normalised, now()],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("Chapter"));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: ChapterId) -> Result<()> {
    let chapter = get(conn, id)?;
    conn.execute("DELETE FROM chapters WHERE id = ?1", params![id])?;
    normalise_positions(
        conn,
        "chapters",
        "volume_id",
        &chapter.volume_id.to_string(),
    )
}

/// Applies an explicit Chapter order within a Volume.
pub fn reorder(conn: &Connection, volume_id: VolumeId, ordered: &[ChapterId]) -> Result<()> {
    let ids: Vec<String> = ordered.iter().map(ToString::to_string).collect();
    apply_order(conn, "chapters", "volume_id", &volume_id.to_string(), &ids)
}

/// Copies a Chapter and its Pages into the same Volume.
pub fn duplicate(conn: &Connection, id: ChapterId) -> Result<Chapter> {
    let source = get(conn, id)?;
    let copy = create(conn, source.volume_id, &format!("{} (copy)", source.title))?;

    for page in super::pages::list_full(conn, source.id)? {
        let mut clone = crate::domain::Page::create(copy.id, &page.title, page.position);
        clone.plain_text = page.plain_text;
        clone.word_count = page.word_count;
        clone.character_count = page.character_count;
        super::pages::insert_raw(conn, &clone, &serde_json::to_string(&page.document)?)?;
    }

    Ok(copy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testing::TempDatabase;

    fn volume(conn: &Connection) -> VolumeId {
        super::super::volumes::create(conn, "A Volume", None, None)
            .unwrap()
            .id
    }

    #[test]
    fn chapters_are_appended_in_creation_order() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume_id = volume(&conn);

        create(&conn, volume_id, "One").unwrap();
        create(&conn, volume_id, "Two").unwrap();
        create(&conn, volume_id, "Three").unwrap();

        let listed = list(&conn, volume_id).unwrap();
        assert_eq!(
            listed.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            ["One", "Two", "Three"]
        );
        assert_eq!(
            listed.iter().map(|c| c.position).collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[test]
    fn creating_under_a_missing_volume_fails_instead_of_orphaning() {
        let db = TempDatabase::open();
        let error = create(&db.get().unwrap(), VolumeId::new(), "One").unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::NotFound);
    }

    #[test]
    fn chapters_from_other_volumes_are_not_listed() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let a = volume(&conn);
        let b = volume(&conn);

        create(&conn, a, "In A").unwrap();
        create(&conn, b, "In B").unwrap();

        assert_eq!(list(&conn, a).unwrap().len(), 1);
        assert_eq!(list(&conn, a).unwrap()[0].title, "In A");
    }

    #[test]
    fn deleting_closes_the_gap_in_positions() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume_id = volume(&conn);

        let one = create(&conn, volume_id, "One").unwrap();
        create(&conn, volume_id, "Two").unwrap();
        create(&conn, volume_id, "Three").unwrap();

        delete(&conn, one.id).unwrap();

        let listed = list(&conn, volume_id).unwrap();
        assert_eq!(
            listed.iter().map(|c| c.position).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(listed[0].title, "Two");
    }

    #[test]
    fn reordering_applies_the_requested_sequence() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume_id = volume(&conn);

        let one = create(&conn, volume_id, "One").unwrap();
        let two = create(&conn, volume_id, "Two").unwrap();
        let three = create(&conn, volume_id, "Three").unwrap();

        reorder(&conn, volume_id, &[three.id, one.id, two.id]).unwrap();

        let listed = list(&conn, volume_id).unwrap();
        assert_eq!(
            listed.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            ["Three", "One", "Two"]
        );
        assert_eq!(
            listed.iter().map(|c| c.position).collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[test]
    fn a_reorder_computed_against_a_stale_tree_cannot_drop_a_chapter() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume_id = volume(&conn);

        let one = create(&conn, volume_id, "One").unwrap();
        let two = create(&conn, volume_id, "Two").unwrap();
        // "Three" was added in another window after the drag started.
        create(&conn, volume_id, "Three").unwrap();

        reorder(&conn, volume_id, &[two.id, one.id]).unwrap();

        let listed = list(&conn, volume_id).unwrap();
        assert_eq!(listed.len(), 3, "a chapter went missing");
        assert_eq!(
            listed.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            ["Two", "One", "Three"]
        );
    }

    #[test]
    fn renaming_refuses_an_empty_title_and_keeps_the_old_one() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume_id = volume(&conn);
        let chapter = create(&conn, volume_id, "One").unwrap();

        assert!(rename(&conn, chapter.id, "  ").is_err());
        assert_eq!(get(&conn, chapter.id).unwrap().title, "One");

        assert_eq!(
            rename(&conn, chapter.id, "  Renamed  ").unwrap().title,
            "Renamed"
        );
    }

    #[test]
    fn duplicating_a_chapter_copies_its_pages() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let volume_id = volume(&conn);
        let chapter = create(&conn, volume_id, "One").unwrap();

        let page = super::super::pages::create(&conn, chapter.id, "First").unwrap();
        super::super::pages::save_document(
            &conn,
            page.id,
            serde_json::json!({
                "type": "doc",
                "content": [{ "type": "paragraph",
                              "content": [{ "type": "text", "text": "salt road" }] }]
            }),
        )
        .unwrap();

        let copy = duplicate(&conn, chapter.id).unwrap();
        let copied = super::super::pages::list_full(&conn, copy.id).unwrap();

        assert_eq!(copied.len(), 1);
        assert_eq!(copied[0].plain_text, "salt road");
        assert_ne!(copied[0].id, page.id);
        assert_eq!(list(&conn, volume_id).unwrap().len(), 2);
    }
}
