//! Annotation persistence and re-anchoring.

use rusqlite::{Connection, Row, params};

use crate::domain::annotation::{
    Annotation, AnnotationAnchor, AnnotationKind, AnnotationStatus, AnnotationTarget,
};
use crate::domain::ids::{AnnotationId, PageId};
use crate::domain::manuscript::now;
use crate::error::{AppError, Result};
use crate::search::{self, EntityKind};

fn reindex(conn: &Connection, annotation: &Annotation) -> Result<()> {
    let chapter_id = super::pages::get(conn, annotation.page_id)?.chapter_id;
    let volume_id = super::chapters::get(conn, chapter_id)?.volume_id;
    search::index(
        conn,
        EntityKind::Annotation,
        &annotation.id.to_string(),
        Some(&volume_id.to_string()),
        "",
        &annotation.body,
    )
}

fn map(row: &Row<'_>) -> rusqlite::Result<Annotation> {
    let target = if row.get::<_, String>("target_kind")? == "range" {
        AnnotationTarget::Range(AnnotationAnchor {
            from: row.get::<_, Option<i64>>("anchor_from")?.unwrap_or(0),
            to: row.get::<_, Option<i64>>("anchor_to")?.unwrap_or(0),
            selected_text: row
                .get::<_, Option<String>>("anchor_text")?
                .unwrap_or_default(),
            context_before: row
                .get::<_, Option<String>>("context_before")?
                .unwrap_or_default(),
            context_after: row
                .get::<_, Option<String>>("context_after")?
                .unwrap_or_default(),
            base_revision: row.get::<_, Option<i64>>("base_revision")?.unwrap_or(0),
            text_hash: row
                .get::<_, Option<String>>("text_hash")?
                .unwrap_or_default(),
        })
    } else {
        AnnotationTarget::Page
    };

    Ok(Annotation {
        id: row.get("id")?,
        page_id: row.get("page_id")?,
        // An unrecognised kind or status loads as the safest equivalent rather
        // than failing the read. A row from a newer build must not make the
        // Margin unreadable.
        kind: AnnotationKind::parse(&row.get::<_, String>("kind")?).unwrap_or(AnnotationKind::Note),
        status: AnnotationStatus::parse(&row.get::<_, String>("status")?)
            .unwrap_or(AnnotationStatus::Active),
        body: row.get("body")?,
        target,
        author_profile: row.get("author_profile")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn insert(conn: &Connection, annotation: &Annotation) -> Result<()> {
    let anchor = annotation.anchor();
    conn.execute(
        "INSERT INTO annotations (id, page_id, kind, status, body, target_kind,
                                  anchor_from, anchor_to, anchor_text, context_before,
                                  context_after, base_revision, text_hash, author_profile,
                                  created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            annotation.id,
            annotation.page_id,
            annotation.kind.as_str(),
            annotation.status.as_str(),
            annotation.body,
            if anchor.is_some() { "range" } else { "page" },
            anchor.map(|a| a.from),
            anchor.map(|a| a.to),
            anchor.map(|a| a.selected_text.clone()),
            anchor.map(|a| a.context_before.clone()),
            anchor.map(|a| a.context_after.clone()),
            anchor.map(|a| a.base_revision),
            anchor.map(|a| a.text_hash.clone()),
            annotation.author_profile,
            annotation.created_at,
            annotation.updated_at,
        ],
    )?;
    Ok(())
}

/// Creates an annotation anchored to a range of the Page's plain text.
///
/// The anchor is captured here, from the text as stored, rather than trusted
/// from the caller: the frontend supplies offsets, and the surrounding context
/// that makes relocation possible is read from the authoritative copy.
pub fn create_anchored(
    conn: &Connection,
    page_id: PageId,
    kind: AnnotationKind,
    body: &str,
    from: i64,
    to: i64,
) -> Result<Annotation> {
    let page = super::pages::get(conn, page_id)?;
    let anchor = AnnotationAnchor::capture(&page.plain_text, from, to, page.revision_number);

    if anchor.is_empty() {
        return Err(AppError::invalid(
            "Select some text to annotate, or add a note to the whole Page.",
        ));
    }

    let annotation = Annotation::create(page_id, kind, body, AnnotationTarget::Range(anchor));
    insert(conn, &annotation)?;
    reindex(conn, &annotation)?;
    Ok(annotation)
}

pub fn create_for_page(
    conn: &Connection,
    page_id: PageId,
    kind: AnnotationKind,
    body: &str,
) -> Result<Annotation> {
    super::pages::get(conn, page_id)?;
    let annotation = Annotation::create(page_id, kind, body, AnnotationTarget::Page);
    insert(conn, &annotation)?;
    reindex(conn, &annotation)?;
    Ok(annotation)
}

pub fn get(conn: &Connection, id: AnnotationId) -> Result<Annotation> {
    conn.query_row("SELECT * FROM annotations WHERE id = ?1", params![id], map)
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => AppError::not_found("annotation"),
            other => other.into(),
        })
}

/// Every annotation on a Page, in the order the Margin shows them.
///
/// Anchored notes come first, ordered down the page, so the Margin reads
/// alongside the manuscript. Whole-Page notes follow, newest first.
pub fn list(conn: &Connection, page_id: PageId) -> Result<Vec<Annotation>> {
    let mut statement = conn.prepare(
        "SELECT * FROM annotations WHERE page_id = ?1
          ORDER BY target_kind DESC, anchor_from ASC, created_at DESC",
    )?;
    let rows = statement
        .query_map(params![page_id], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// How many unresolved notes sit on each Page of a Volume.
///
/// One query for the whole tree, so a Volume with hundreds of Pages does not
/// cost a round trip each to show its counts.
pub fn open_counts(
    conn: &Connection,
    volume_id: crate::domain::VolumeId,
) -> Result<Vec<(PageId, i64)>> {
    let mut statement = conn.prepare(
        "SELECT a.page_id, count(*) AS open_count
           FROM annotations a
           JOIN pages p    ON p.id = a.page_id
           JOIN chapters c ON c.id = p.chapter_id
          WHERE c.volume_id = ?1 AND a.status <> 'resolved'
          GROUP BY a.page_id",
    )?;
    let rows = statement
        .query_map(params![volume_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn update_body(conn: &Connection, id: AnnotationId, body: &str) -> Result<Annotation> {
    let changed = conn.execute(
        "UPDATE annotations SET body = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, body.trim(), now()],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("annotation"));
    }
    let updated = get(conn, id)?;
    reindex(conn, &updated)?;
    Ok(updated)
}

pub fn set_status(
    conn: &Connection,
    id: AnnotationId,
    status: AnnotationStatus,
) -> Result<Annotation> {
    let changed = conn.execute(
        "UPDATE annotations SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status.as_str(), now()],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("annotation"));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: AnnotationId) -> Result<()> {
    search::remove(conn, &id.to_string())?;
    let changed = conn.execute("DELETE FROM annotations WHERE id = ?1", params![id])?;
    if changed == 0 {
        return Err(AppError::not_found("annotation"));
    }
    Ok(())
}

fn write_back(conn: &Connection, annotation: &Annotation) -> Result<()> {
    let anchor = annotation.anchor();
    conn.execute(
        "UPDATE annotations
            SET status = ?2, anchor_from = ?3, anchor_to = ?4, anchor_text = ?5,
                context_before = ?6, context_after = ?7, base_revision = ?8,
                text_hash = ?9, updated_at = ?10
          WHERE id = ?1",
        params![
            annotation.id,
            annotation.status.as_str(),
            anchor.map(|a| a.from),
            anchor.map(|a| a.to),
            anchor.map(|a| a.selected_text.clone()),
            anchor.map(|a| a.context_before.clone()),
            anchor.map(|a| a.context_after.clone()),
            anchor.map(|a| a.base_revision),
            anchor.map(|a| a.text_hash.clone()),
            annotation.updated_at,
        ],
    )?;
    Ok(())
}

/// Re-places every annotation on a Page against its current text.
///
/// Runs inside the same transaction as the save that changed the text, so the
/// Margin can never be observed pointing at a version of the Page that no
/// longer exists. Returns how many records actually moved or changed state.
pub fn reanchor_page(conn: &Connection, page_id: PageId) -> Result<usize> {
    let page = super::pages::get(conn, page_id)?;
    let mut touched = 0;

    for mut annotation in list(conn, page_id)? {
        if annotation.reanchor(&page.plain_text, page.revision_number) {
            write_back(conn, &annotation)?;
            touched += 1;
        }
    }

    if touched > 0 {
        tracing::debug!(page = %page_id, touched, "annotations re-anchored");
    }
    Ok(touched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testing::TempDatabase;
    use serde_json::json;

    const TEXT: &str = "The road had been salt once, or so the carters said.";

    fn document(text: &str) -> serde_json::Value {
        json!({
            "type": "doc",
            "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }]
        })
    }

    fn page_with(conn: &Connection, text: &str) -> PageId {
        let volume = super::super::volumes::create(conn, "A", None, None).unwrap();
        let chapter = super::super::chapters::create(conn, volume.id, "One").unwrap();
        let page = super::super::pages::create(conn, chapter.id, "First").unwrap();
        super::super::pages::save_document(conn, page.id, document(text)).unwrap();
        page.id
    }

    fn offsets(text: &str, phrase: &str) -> (i64, i64) {
        let start = text
            .find(phrase)
            .map(|b| text[..b].chars().count())
            .expect("phrase");
        (start as i64, (start + phrase.chars().count()) as i64)
    }

    #[test]
    fn an_anchored_note_records_the_text_it_points_at() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page_with(&conn, TEXT);
        let (from, to) = offsets(TEXT, "carters");

        let note = create_anchored(
            &conn,
            page_id,
            AnnotationKind::Question,
            "Whose voice is this?",
            from,
            to,
        )
        .unwrap();

        let anchor = note.anchor().expect("anchored");
        assert_eq!(anchor.selected_text, "carters");
        assert!(anchor.context_before.ends_with("or so the "));
        assert_eq!(note.status, AnnotationStatus::Active);
        assert_eq!(get(&conn, note.id).unwrap().body, "Whose voice is this?");
    }

    #[test]
    fn an_empty_selection_is_refused_with_a_useful_message() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page_with(&conn, TEXT);

        let error = create_anchored(&conn, page_id, AnnotationKind::Note, "x", 5, 5).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::InvalidInput);
        assert!(error.message.contains("whole Page"), "{}", error.message);
    }

    #[test]
    fn a_whole_page_note_needs_no_anchor() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page_with(&conn, TEXT);

        let note =
            create_for_page(&conn, page_id, AnnotationKind::Note, "This chapter drags.").unwrap();
        assert!(note.anchor().is_none());
        assert!(matches!(
            get(&conn, note.id).unwrap().target,
            AnnotationTarget::Page
        ));
    }

    #[test]
    fn editing_around_a_note_moves_it_rather_than_breaking_it() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page_with(&conn, TEXT);
        let (from, to) = offsets(TEXT, "carters");
        let note = create_anchored(&conn, page_id, AnnotationKind::Note, "note", from, to).unwrap();

        let edited = format!("A new opening sentence. {TEXT}");
        super::super::pages::save_document(&conn, page_id, document(&edited)).unwrap();
        assert_eq!(reanchor_page(&conn, page_id).unwrap(), 1);

        let moved = get(&conn, note.id).unwrap();
        assert_eq!(moved.status, AnnotationStatus::Active);
        let anchor = moved.anchor().unwrap();
        let found: String = edited
            .chars()
            .skip(anchor.from as usize)
            .take((anchor.to - anchor.from) as usize)
            .collect();
        assert_eq!(found, "carters");
    }

    #[test]
    fn deleting_the_anchored_text_marks_the_note_stale_rather_than_moving_it() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page_with(&conn, TEXT);
        let (from, to) = offsets(TEXT, "carters");
        let note = create_anchored(&conn, page_id, AnnotationKind::Note, "note", from, to).unwrap();

        super::super::pages::save_document(
            &conn,
            page_id,
            document(&TEXT.replace("carters", "drovers")),
        )
        .unwrap();
        reanchor_page(&conn, page_id).unwrap();

        assert_eq!(get(&conn, note.id).unwrap().status, AnnotationStatus::Stale);
    }

    #[test]
    fn a_whole_page_note_never_goes_stale() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page_with(&conn, TEXT);
        let note = create_for_page(&conn, page_id, AnnotationKind::Note, "note").unwrap();

        super::super::pages::save_document(&conn, page_id, document("something else entirely"))
            .unwrap();
        reanchor_page(&conn, page_id).unwrap();

        assert_eq!(
            get(&conn, note.id).unwrap().status,
            AnnotationStatus::Active
        );
    }

    #[test]
    fn resolving_hides_a_note_from_the_open_counts() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page_with(&conn, TEXT);
        let (from, to) = offsets(TEXT, "carters");
        let note = create_anchored(&conn, page_id, AnnotationKind::Note, "note", from, to).unwrap();

        let volume_id = {
            let chapter_id = super::super::pages::get(&conn, page_id).unwrap().chapter_id;
            super::super::chapters::get(&conn, chapter_id)
                .unwrap()
                .volume_id
        };

        assert_eq!(open_counts(&conn, volume_id).unwrap(), vec![(page_id, 1)]);
        set_status(&conn, note.id, AnnotationStatus::Resolved).unwrap();
        assert!(open_counts(&conn, volume_id).unwrap().is_empty());
    }

    #[test]
    fn anchored_notes_are_listed_in_page_order_and_page_notes_last() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page_with(&conn, TEXT);

        let (late_from, late_to) = offsets(TEXT, "carters");
        let (early_from, early_to) = offsets(TEXT, "road");

        create_anchored(
            &conn,
            page_id,
            AnnotationKind::Note,
            "late",
            late_from,
            late_to,
        )
        .unwrap();
        create_for_page(&conn, page_id, AnnotationKind::Note, "whole page").unwrap();
        create_anchored(
            &conn,
            page_id,
            AnnotationKind::Note,
            "early",
            early_from,
            early_to,
        )
        .unwrap();

        let listed = list(&conn, page_id).unwrap();
        assert_eq!(
            listed.iter().map(|a| a.body.as_str()).collect::<Vec<_>>(),
            ["early", "late", "whole page"]
        );
    }

    #[test]
    fn re_anchoring_reports_nothing_when_the_text_is_unchanged() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page_with(&conn, TEXT);
        let (from, to) = offsets(TEXT, "carters");
        create_anchored(&conn, page_id, AnnotationKind::Note, "note", from, to).unwrap();

        assert_eq!(reanchor_page(&conn, page_id).unwrap(), 0);
    }

    #[test]
    fn deleting_a_page_takes_its_annotations_with_it() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page_with(&conn, TEXT);
        create_for_page(&conn, page_id, AnnotationKind::Note, "note").unwrap();

        super::super::pages::delete(&conn, page_id).unwrap();
        let left: i64 = conn
            .query_row("SELECT count(*) FROM annotations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn an_unrecognised_kind_loads_as_a_note_rather_than_failing() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page_with(&conn, TEXT);
        let note = create_for_page(&conn, page_id, AnnotationKind::Note, "note").unwrap();

        conn.execute(
            "UPDATE annotations SET kind = 'from-the-future' WHERE id = ?1",
            params![note.id],
        )
        .unwrap();

        assert_eq!(get(&conn, note.id).unwrap().kind, AnnotationKind::Note);
    }
}
