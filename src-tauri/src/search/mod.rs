//! Full-text search.
//!
//! Two halves that must stay in step: index maintenance, which writes a
//! segmented form of every searchable string, and querying, which applies the
//! identical segmentation to what the user typed.
//!
//! Snippets are produced here rather than by FTS5's `snippet()`, which would
//! hand back the segmented text with a space between every CJK character.
//! Building them from the original string also means highlights can be returned
//! as character ranges instead of markup, so the frontend renders them without
//! ever interpreting text as HTML.

pub mod query;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::domain::VolumeId;
use crate::domain::text::segment_for_index;
use crate::error::Result;

/// What kind of thing a hit points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntityKind {
    Volume,
    Chapter,
    Page,
    Annotation,
}

impl EntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityKind::Volume => "volume",
            EntityKind::Chapter => "chapter",
            EntityKind::Page => "page",
            EntityKind::Annotation => "annotation",
        }
    }

    fn parse(raw: &str) -> Self {
        match raw {
            "volume" => EntityKind::Volume,
            "chapter" => EntityKind::Chapter,
            "annotation" => EntityKind::Annotation,
            _ => EntityKind::Page,
        }
    }
}

/// Adds or replaces an entity's index entry.
///
/// The rowid is looked up rather than deleting by `entity_id`, which FTS5 would
/// answer with a full scan — and a Page is re-indexed on every save.
pub fn index(
    conn: &Connection,
    kind: EntityKind,
    entity_id: &str,
    volume_id: Option<&str>,
    title: &str,
    body: &str,
) -> Result<()> {
    let segmented_title = segment_for_index(title);
    let segmented_body = segment_for_index(body);

    let existing: Option<i64> = conn
        .query_row(
            "SELECT row_id FROM search_rows WHERE entity_id = ?1",
            [entity_id],
            |row| row.get(0),
        )
        .ok();

    match existing {
        Some(row_id) => {
            conn.execute(
                "UPDATE search_index SET title = ?2, body = ?3 WHERE rowid = ?1",
                params![row_id, segmented_title, segmented_body],
            )?;
        }
        None => {
            conn.execute(
                "INSERT INTO search_index (title, body, entity_kind, entity_id, volume_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    segmented_title,
                    segmented_body,
                    kind.as_str(),
                    entity_id,
                    volume_id
                ],
            )?;
            conn.execute(
                "INSERT INTO search_rows (entity_id, row_id, volume_id) VALUES (?1, ?2, ?3)",
                params![entity_id, conn.last_insert_rowid(), volume_id],
            )?;
        }
    }

    Ok(())
}

/// Removes one entity from the index.
pub fn remove(conn: &Connection, entity_id: &str) -> Result<()> {
    let row_id: Option<i64> = conn
        .query_row(
            "SELECT row_id FROM search_rows WHERE entity_id = ?1",
            [entity_id],
            |row| row.get(0),
        )
        .ok();

    if let Some(row_id) = row_id {
        conn.execute("DELETE FROM search_index WHERE rowid = ?1", [row_id])?;
        conn.execute("DELETE FROM search_rows WHERE entity_id = ?1", [entity_id])?;
    }
    Ok(())
}

/// Removes every entry belonging to a Volume.
///
/// Used when a Volume is deleted, where a cascade cannot reach: the index is a
/// virtual table with no foreign keys into the manuscript.
pub fn remove_volume(conn: &Connection, volume_id: VolumeId) -> Result<()> {
    let volume = volume_id.to_string();
    let mut statement =
        conn.prepare("SELECT row_id FROM search_rows WHERE volume_id = ?1 OR entity_id = ?1")?;
    let rows = statement
        .query_map([&volume], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for row_id in rows {
        conn.execute("DELETE FROM search_index WHERE rowid = ?1", [row_id])?;
    }
    conn.execute(
        "DELETE FROM search_rows WHERE volume_id = ?1 OR entity_id = ?1",
        [&volume],
    )?;
    Ok(())
}

/// Rebuilds the whole index from the manuscript.
///
/// Needed after an import, and as a repair for a library whose index was lost
/// or predates a change to how text is segmented.
pub fn rebuild(conn: &Connection) -> Result<usize> {
    conn.execute("DELETE FROM search_index", [])?;
    conn.execute("DELETE FROM search_rows", [])?;

    let mut indexed = 0usize;

    let mut volumes = conn.prepare("SELECT id, title, COALESCE(subtitle, '') FROM volumes")?;
    for row in volumes.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (id, title, subtitle) = row?;
        index(conn, EntityKind::Volume, &id, Some(&id), &title, &subtitle)?;
        indexed += 1;
    }

    let mut chapters = conn.prepare("SELECT id, volume_id, title FROM chapters")?;
    for row in chapters.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (id, volume, title) = row?;
        index(conn, EntityKind::Chapter, &id, Some(&volume), &title, "")?;
        indexed += 1;
    }

    let mut pages = conn.prepare(
        "SELECT p.id, c.volume_id, p.title, p.plain_text
           FROM pages p JOIN chapters c ON c.id = p.chapter_id",
    )?;
    for row in pages.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })? {
        let (id, volume, title, body) = row?;
        index(conn, EntityKind::Page, &id, Some(&volume), &title, &body)?;
        indexed += 1;
    }

    let mut annotations = conn.prepare(
        "SELECT a.id, c.volume_id, a.body
           FROM annotations a
           JOIN pages p    ON p.id = a.page_id
           JOIN chapters c ON c.id = p.chapter_id",
    )?;
    for row in annotations.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (id, volume, body) = row?;
        index(conn, EntityKind::Annotation, &id, Some(&volume), "", &body)?;
        indexed += 1;
    }

    tracing::info!(indexed, "search index rebuilt");
    Ok(indexed)
}

/// One search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub kind: EntityKind,
    pub entity_id: String,
    pub volume_id: String,
    /// Where this sits, for the result card: "The Salt Road › Chapter I".
    pub path: Vec<String>,
    pub title: String,
    /// A window of the original text around the first match.
    pub snippet: String,
    /// Character ranges within `snippet` to mark. Ranges, not markup, so the
    /// frontend never has to interpret a manuscript as HTML.
    pub highlights: Vec<(i64, i64)>,
    /// Lower is better; this is bm25, which is negative.
    pub score: f64,
    /// For an annotation or page hit, the Page to open.
    pub page_id: Option<String>,
}

pub use query::{search, snippet_around};

#[cfg(test)]
mod tests;
