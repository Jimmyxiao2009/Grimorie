//! Bookmarks and tags.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::domain::ids::{BookmarkId, PageId, TagId};
use crate::domain::manuscript::{Timestamp, now};
use crate::domain::text::normalise_title;
use crate::error::{AppError, Result};

// --- Bookmarks --------------------------------------------------------------

/// A bookmark, with enough context to be listed away from its manuscript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: BookmarkId,
    pub page_id: PageId,
    pub label: String,
    pub page_title: String,
    pub chapter_title: String,
    pub volume_id: String,
    pub volume_title: String,
    pub preview: String,
    pub created_at: Timestamp,
}

/// Marks a Page, or clears the mark if it already had one.
///
/// A single toggle rather than separate add and remove: bookmarking is one
/// gesture, and Ctrl+B should undo itself.
pub fn toggle(conn: &Connection, page_id: PageId, label: &str) -> Result<bool> {
    super::pages::get(conn, page_id)?;

    let existing: Option<BookmarkId> = conn
        .query_row(
            "SELECT id FROM bookmarks WHERE page_id = ?1",
            params![page_id],
            |row| row.get(0),
        )
        .ok();

    match existing {
        Some(id) => {
            conn.execute("DELETE FROM bookmarks WHERE id = ?1", params![id])?;
            Ok(false)
        }
        None => {
            conn.execute(
                "INSERT INTO bookmarks (id, page_id, label, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![BookmarkId::new(), page_id, label.trim(), now()],
            )?;
            Ok(true)
        }
    }
}

pub fn is_bookmarked(conn: &Connection, page_id: PageId) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM bookmarks WHERE page_id = ?1",
        params![page_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Every bookmark, newest first, optionally confined to one Volume.
pub fn list(conn: &Connection, volume_id: Option<&str>) -> Result<Vec<Bookmark>> {
    let mut statement = conn.prepare(
        "SELECT b.id, b.page_id, b.label, b.created_at,
                p.title, p.plain_text, c.title, v.id, v.title
           FROM bookmarks b
           JOIN pages p    ON p.id = b.page_id
           JOIN chapters c ON c.id = p.chapter_id
           JOIN volumes v  ON v.id = c.volume_id
          WHERE (?1 IS NULL OR v.id = ?1)
          ORDER BY b.created_at DESC",
    )?;

    let rows = statement
        .query_map(params![volume_id], |row| {
            let plain_text: String = row.get(5)?;
            Ok(Bookmark {
                id: row.get(0)?,
                page_id: row.get(1)?,
                label: row.get(2)?,
                created_at: row.get(3)?,
                page_title: row.get(4)?,
                preview: crate::domain::text::preview(&plain_text, 120),
                chapter_title: row.get(6)?,
                volume_id: row.get(7)?,
                volume_title: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The bookmarked Pages of a Volume, for marking them in the tree.
pub fn page_ids_for_volume(
    conn: &Connection,
    volume_id: crate::domain::VolumeId,
) -> Result<Vec<PageId>> {
    let mut statement = conn.prepare(
        "SELECT b.page_id FROM bookmarks b
           JOIN pages p    ON p.id = b.page_id
           JOIN chapters c ON c.id = p.chapter_id
          WHERE c.volume_id = ?1",
    )?;
    let rows = statement
        .query_map(params![volume_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// --- Tags -------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaggableKind {
    Volume,
    Chapter,
    Page,
}

impl TaggableKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaggableKind::Volume => "volume",
            TaggableKind::Chapter => "chapter",
            TaggableKind::Page => "page",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        Ok(match raw {
            "volume" => TaggableKind::Volume,
            "chapter" => TaggableKind::Chapter,
            "page" => TaggableKind::Page,
            _ => return Err(AppError::invalid("That kind of thing cannot be tagged.")),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: TagId,
    pub name: String,
    pub slug: String,
    /// How many things carry this tag, for the tag list.
    pub uses: i64,
}

/// Folds a tag name to its identity.
///
/// Case and surrounding space are not meaningful in a tag, so "Salt", "salt",
/// and " salt " are one tag rather than three that look identical in a list.
fn slugify(name: &str) -> String {
    normalise_title(name).to_lowercase()
}

/// Finds or creates a tag, then attaches it.
pub fn attach(conn: &Connection, kind: TaggableKind, entity_id: &str, name: &str) -> Result<Tag> {
    let display = normalise_title(name);
    let slug = slugify(name);
    if slug.is_empty() {
        return Err(AppError::invalid("A tag needs a name."));
    }

    let existing: Option<TagId> = conn
        .query_row("SELECT id FROM tags WHERE slug = ?1", [&slug], |row| {
            row.get(0)
        })
        .ok();

    let tag_id = match existing {
        Some(id) => id,
        None => {
            let id = TagId::new();
            conn.execute(
                "INSERT INTO tags (id, name, slug, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, display, slug, now()],
            )?;
            id
        }
    };

    conn.execute(
        "INSERT INTO entity_tags (tag_id, entity_kind, entity_id, created_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (tag_id, entity_id) DO NOTHING",
        params![tag_id, kind.as_str(), entity_id, now()],
    )?;

    get_tag(conn, tag_id)
}

pub fn detach(conn: &Connection, entity_id: &str, tag_id: TagId) -> Result<()> {
    conn.execute(
        "DELETE FROM entity_tags WHERE entity_id = ?1 AND tag_id = ?2",
        params![entity_id, tag_id],
    )?;
    // A tag nobody uses is clutter in the tag list, so it goes with its last
    // use rather than lingering as a name with nothing behind it.
    conn.execute(
        "DELETE FROM tags WHERE id = ?1
          AND NOT EXISTS (SELECT 1 FROM entity_tags WHERE tag_id = ?1)",
        params![tag_id],
    )?;
    Ok(())
}

fn get_tag(conn: &Connection, id: TagId) -> Result<Tag> {
    conn.query_row(
        "SELECT t.id, t.name, t.slug,
                (SELECT count(*) FROM entity_tags e WHERE e.tag_id = t.id) AS uses
           FROM tags t WHERE t.id = ?1",
        params![id],
        |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                slug: row.get(2)?,
                uses: row.get(3)?,
            })
        },
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => AppError::not_found("tag"),
        other => other.into(),
    })
}

pub fn tags_for(conn: &Connection, entity_id: &str) -> Result<Vec<Tag>> {
    let mut statement = conn.prepare(
        "SELECT t.id, t.name, t.slug,
                (SELECT count(*) FROM entity_tags e2 WHERE e2.tag_id = t.id) AS uses
           FROM tags t
           JOIN entity_tags e ON e.tag_id = t.id
          WHERE e.entity_id = ?1
          ORDER BY t.name",
    )?;
    let rows = statement
        .query_map([entity_id], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                slug: row.get(2)?,
                uses: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Every tag in the library, most used first.
pub fn all_tags(conn: &Connection) -> Result<Vec<Tag>> {
    let mut statement = conn.prepare(
        "SELECT t.id, t.name, t.slug, count(e.tag_id) AS uses
           FROM tags t LEFT JOIN entity_tags e ON e.tag_id = t.id
          GROUP BY t.id ORDER BY uses DESC, t.name",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                slug: row.get(2)?,
                uses: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Removes every tag attachment for a deleted entity.
///
/// `entity_tags` cannot carry a foreign key, because `entity_id` points at one
/// of three tables. This is the cleanup that stands in for the cascade.
pub fn forget_entity(conn: &Connection, entity_id: &str) -> Result<()> {
    conn.execute("DELETE FROM entity_tags WHERE entity_id = ?1", [entity_id])?;
    conn.execute(
        "DELETE FROM tags WHERE NOT EXISTS
            (SELECT 1 FROM entity_tags e WHERE e.tag_id = tags.id)",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testing::TempDatabase;

    fn page(conn: &Connection) -> (crate::domain::VolumeId, PageId) {
        let volume = super::super::volumes::create(conn, "A Volume", None, None).unwrap();
        let chapter = super::super::chapters::create(conn, volume.id, "One").unwrap();
        let page = super::super::pages::create(conn, chapter.id, "First").unwrap();
        (volume.id, page.id)
    }

    #[test]
    fn bookmarking_toggles() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);

        assert!(toggle(&conn, page_id, "").unwrap());
        assert!(is_bookmarked(&conn, page_id).unwrap());

        assert!(!toggle(&conn, page_id, "").unwrap());
        assert!(!is_bookmarked(&conn, page_id).unwrap());
    }

    #[test]
    fn a_page_can_only_be_bookmarked_once() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);

        toggle(&conn, page_id, "first").unwrap();
        // Toggling off and on again leaves exactly one row, not two.
        toggle(&conn, page_id, "").unwrap();
        toggle(&conn, page_id, "second").unwrap();

        assert_eq!(list(&conn, None).unwrap().len(), 1);
    }

    #[test]
    fn bookmarks_carry_enough_context_to_be_listed_alone() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);
        toggle(&conn, page_id, "come back to this").unwrap();

        let listed = list(&conn, None).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].page_title, "First");
        assert_eq!(listed[0].chapter_title, "One");
        assert_eq!(listed[0].volume_title, "A Volume");
        assert_eq!(listed[0].label, "come back to this");
    }

    #[test]
    fn deleting_a_page_removes_its_bookmark() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);
        toggle(&conn, page_id, "").unwrap();

        super::super::pages::delete(&conn, page_id).unwrap();
        assert!(list(&conn, None).unwrap().is_empty());
    }

    #[test]
    fn bookmarking_a_missing_page_fails_rather_than_creating_an_orphan() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        assert!(toggle(&conn, PageId::new(), "").is_err());
    }

    #[test]
    fn tags_fold_case_and_space_into_one_identity() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);
        let id = page_id.to_string();

        attach(&conn, TaggableKind::Page, &id, "Salt").unwrap();
        attach(&conn, TaggableKind::Page, &id, "  salt  ").unwrap();

        let tags = tags_for(&conn, &id).unwrap();
        assert_eq!(tags.len(), 1, "got {tags:?}");
        // The display form is the one first typed.
        assert_eq!(tags[0].name, "Salt");
    }

    #[test]
    fn attaching_the_same_tag_twice_is_harmless() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);
        let id = page_id.to_string();

        attach(&conn, TaggableKind::Page, &id, "salt").unwrap();
        let tag = attach(&conn, TaggableKind::Page, &id, "salt").unwrap();

        assert_eq!(tag.uses, 1);
        assert_eq!(tags_for(&conn, &id).unwrap().len(), 1);
    }

    #[test]
    fn an_empty_tag_name_is_refused() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);
        assert!(attach(&conn, TaggableKind::Page, &page_id.to_string(), "   ").is_err());
    }

    #[test]
    fn one_tag_can_gather_things_of_different_kinds() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (volume_id, page_id) = page(&conn);

        attach(&conn, TaggableKind::Page, &page_id.to_string(), "salt").unwrap();
        let tag = attach(&conn, TaggableKind::Volume, &volume_id.to_string(), "salt").unwrap();

        assert_eq!(tag.uses, 2);
        assert_eq!(all_tags(&conn).unwrap().len(), 1);
    }

    #[test]
    fn a_tag_disappears_with_its_last_use() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);
        let id = page_id.to_string();

        let tag = attach(&conn, TaggableKind::Page, &id, "transient").unwrap();
        detach(&conn, &id, tag.id).unwrap();

        assert!(
            all_tags(&conn).unwrap().is_empty(),
            "an unused tag lingered"
        );
    }

    #[test]
    fn a_tag_still_in_use_elsewhere_survives_a_detach() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (volume_id, page_id) = page(&conn);

        let tag = attach(&conn, TaggableKind::Page, &page_id.to_string(), "salt").unwrap();
        attach(&conn, TaggableKind::Volume, &volume_id.to_string(), "salt").unwrap();

        detach(&conn, &page_id.to_string(), tag.id).unwrap();

        let remaining = all_tags(&conn).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].uses, 1);
    }

    #[test]
    fn forgetting_an_entity_clears_its_tags() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let (_, page_id) = page(&conn);
        let id = page_id.to_string();

        attach(&conn, TaggableKind::Page, &id, "salt").unwrap();
        forget_entity(&conn, &id).unwrap();

        assert!(tags_for(&conn, &id).unwrap().is_empty());
        assert!(all_tags(&conn).unwrap().is_empty());
    }
}
