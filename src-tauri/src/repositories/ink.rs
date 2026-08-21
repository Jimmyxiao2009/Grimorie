//! Ink stroke persistence.
//!
//! Ink is an annotation kind: a handwritten margin note is an `annotations` row
//! of kind `ink`, and its vector content — the strokes — lives in `ink_strokes`.
//! This module is the only place that touches that table.
//!
//! Strokes are written on `pointerup`, one stroke at a time, and read in bulk
//! for a Page. A stroke is a unit: it is drawn whole, erased whole, and never
//! edited point by point, so it is stored as one JSON blob per stroke and the
//! row granularity matches the access pattern.

use rusqlite::{Connection, Row, params};

use crate::domain::annotation::{Annotation, AnnotationKind, AnnotationTarget};
use crate::domain::ids::{AnnotationId, PageId, StrokeId};
use crate::domain::ink::{InkPoint, InkStroke, InkTool};
use crate::domain::manuscript::{Timestamp, now};
use crate::error::{AppError, Result};

/// A versioned envelope for the points payload, so a future point schema can be
/// migrated forward without splitting the table or guessing at old rows.
///
/// Version 1 is the current [`InkPoint`] array.
const POINTS_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct PointsEnvelope {
    version: u32,
    points: Vec<InkPoint>,
}

fn map(row: &Row<'_>) -> rusqlite::Result<InkStroke> {
    let raw: String = row.get("points_json")?;
    let tool_str: String = row.get("tool")?;
    let tool = InkTool::parse(&tool_str).unwrap_or(InkTool::Pen);
    let points = decode_points(&raw).unwrap_or_default();

    Ok(InkStroke {
        id: row.get("id")?,
        tool,
        color: row.get("color")?,
        width: row.get("width")?,
        points,
        created_at: row.get("created_at")?,
    })
}

fn encode_points(points: &[InkPoint]) -> Result<String> {
    serde_json::to_string(&PointsEnvelope {
        version: POINTS_VERSION,
        points: points.to_vec(),
    })
    .map_err(Into::into)
}

/// Decodes a stored points blob, tolerating an unknown shape.
///
/// A row that will not parse as the envelope — corruption, or a newer version
/// this build cannot read — yields an empty stroke rather than failing the whole
/// Page read. Losing one malformed stroke is better than blanking the Margin
/// over it, and the failure is logged so it is not silent.
fn decode_points(raw: &str) -> Option<Vec<InkPoint>> {
    match serde_json::from_str::<PointsEnvelope>(raw) {
        Ok(envelope) => Some(envelope.points),
        Err(err) => {
            tracing::warn!(error = %err, "ink points could not be deserialized");
            None
        }
    }
}

/// Creates an ink annotation on a Page, whole-Page by default.
///
/// An ink note needs no body: its content is the strokes. The `body` column is
/// kept empty so an ink note never reads as text in the Margin, and so an
/// unrecognised kind loading from an older build degrades to an empty note
/// rather than nonsense text.
pub fn create_ink_annotation(conn: &Connection, page_id: PageId) -> Result<Annotation> {
    super::pages::get(conn, page_id)?;
    let annotation = Annotation::create(page_id, AnnotationKind::Ink, "", AnnotationTarget::Page);
    super::annotations::insert(conn, &annotation)?;
    tracing::debug!(page = %page_id, annotation = %annotation.id, "ink annotation created");
    Ok(annotation)
}

/// Creates an ink annotation anchored to a range of the Page's plain text.
///
/// Ink that points at specific prose re-anchors with every save, just like a
/// text note — so handwriting travels with the sentence it was written beside.
pub fn create_anchored_ink_annotation(
    conn: &Connection,
    page_id: PageId,
    from: i64,
    to: i64,
) -> Result<Annotation> {
    let annotation =
        super::annotations::create_anchored(conn, page_id, AnnotationKind::Ink, "", from, to)?;
    tracing::debug!(page = %page_id, annotation = %annotation.id, "anchored ink annotation created");
    Ok(annotation)
}

/// The ink annotations on a Page, newest first — the order the Margin draws in.
pub fn list_ink_annotations(conn: &Connection, page_id: PageId) -> Result<Vec<Annotation>> {
    Ok(super::annotations::list(conn, page_id)?
        .into_iter()
        .filter(|annotation| annotation.kind == AnnotationKind::Ink)
        .collect())
}

/// Every stroke belonging to the ink annotations on a Page, keyed by annotation.
///
/// One stroke query for the whole Page: the Margin draws every note at once, so
/// fetching strokes one annotation at a time would be N round trips for what is
/// one paint.
pub fn strokes_for_page(
    conn: &Connection,
    page_id: PageId,
) -> Result<Vec<(AnnotationId, Vec<InkStroke>)>> {
    let annotations = list_ink_annotations(conn, page_id)?;
    if annotations.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<String> = annotations.iter().map(|a| a.id.to_string()).collect();
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, annotation_id, tool, color, width, points_json, position, created_at
           FROM ink_strokes
          WHERE annotation_id IN ({placeholders})
          ORDER BY annotation_id, position"
    );

    let mut by_annotation: Vec<(AnnotationId, Vec<InkStroke>)> = annotations
        .into_iter()
        .map(|a| (a.id, Vec::new()))
        .collect();

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(ids.iter()), |row| {
        let annotation_id: AnnotationId = row.get("annotation_id")?;
        let stroke = map(row)?;
        Ok((annotation_id, stroke))
    })?;

    for row in rows {
        let (annotation_id, stroke) = row?;
        if let Some(entry) = by_annotation
            .iter_mut()
            .find(|(id, _)| *id == annotation_id)
        {
            entry.1.push(stroke);
        }
    }

    Ok(by_annotation)
}

/// Adds a batch of strokes to one ink annotation in a single transaction.
///
/// This is the persistence path for a `pointerup`: the live stroke is kept in
/// memory while drawing and committed here, once, when the pen lifts. Positions
/// continue from where the annotation's strokes already end, so drawing order
/// is preserved across sessions.
pub fn add_strokes(
    conn: &Connection,
    annotation_id: AnnotationId,
    strokes: &[InkStroke],
) -> Result<()> {
    if strokes.is_empty() {
        return Ok(());
    }

    // The annotation must exist and be ink. A non-ink annotation cannot hold
    // strokes, and a missing one means the Margin's state is stale.
    let annotation = super::annotations::get(conn, annotation_id)?;
    if annotation.kind != AnnotationKind::Ink {
        return Err(AppError::invalid("Only an ink note can hold handwriting."));
    }

    let next_position: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM ink_strokes WHERE annotation_id = ?1",
            params![annotation_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for (offset, stroke) in strokes.iter().enumerate() {
        conn.execute(
            "INSERT INTO ink_strokes
                (id, annotation_id, tool, color, width, points_json, position, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                stroke.id,
                annotation_id,
                stroke.tool.as_str(),
                stroke.color,
                stroke.width,
                encode_points(&stroke.points)?,
                next_position + offset as i64,
                stroke.created_at,
            ],
        )?;
    }

    tracing::debug!(annotation = %annotation_id, count = strokes.len(), "strokes persisted");
    Ok(())
}

/// Removes a single stroke. Erasing is stroke-level: a tap on a stroke removes
/// the whole stroke, never a segment of it.
pub fn delete_stroke(conn: &Connection, id: StrokeId) -> Result<()> {
    let changed = conn.execute("DELETE FROM ink_strokes WHERE id = ?1", params![id])?;
    if changed == 0 {
        return Err(AppError::not_found("stroke"));
    }
    tracing::debug!(stroke = %id, "stroke deleted");
    Ok(())
}

/// The annotation a stroke belongs to, so a mutation can invalidate the right
/// recognition row without a separate lookup by the caller.
pub fn annotation_of_stroke(conn: &Connection, id: StrokeId) -> Result<AnnotationId> {
    conn.query_row(
        "SELECT annotation_id FROM ink_strokes WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => AppError::not_found("stroke"),
        other => other.into(),
    })
}

/// Every stroke belonging to one ink annotation, in drawing order.
///
/// Used by the recognition queue to re-read a note's strokes for a
/// stale-rejection hash: recognition captures the ink as it was scheduled, and
/// before committing it re-reads the live strokes to be sure the writer has not
/// added or erased any in the meantime.
pub fn strokes_for_annotation(
    conn: &Connection,
    annotation_id: AnnotationId,
) -> Result<Vec<InkStroke>> {
    let mut statement = conn.prepare(
        "SELECT id, annotation_id, tool, color, width, points_json, position, created_at
           FROM ink_strokes
          WHERE annotation_id = ?1
          ORDER BY position",
    )?;
    let rows = statement
        .query_map(params![annotation_id], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The number of strokes an ink annotation carries, for deciding whether an
/// empty ink note should be pruned.
pub fn stroke_count(conn: &Connection, annotation_id: AnnotationId) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT count(*) FROM ink_strokes WHERE annotation_id = ?1",
        params![annotation_id],
        |row| row.get(0),
    )?)
}

/// Removes every stroke for an annotation. The annotation row itself is owned
/// by `annotations`; this clears only its content.
pub fn delete_strokes_for_annotation(conn: &Connection, annotation_id: AnnotationId) -> Result<()> {
    conn.execute(
        "DELETE FROM ink_strokes WHERE annotation_id = ?1",
        params![annotation_id],
    )?;
    Ok(())
}

/// A timestamp at the point of writing, for callers that build strokes outside
/// the domain (tests, future import paths).
pub fn now_timestamp() -> Timestamp {
    now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testing::TempDatabase;
    use crate::domain::ink::InkPoint;

    fn page(conn: &Connection) -> PageId {
        let volume = super::super::volumes::create(conn, "A", None, None).unwrap();
        let chapter = super::super::chapters::create(conn, volume.id, "One").unwrap();
        super::super::pages::create(conn, chapter.id, "First")
            .unwrap()
            .id
    }

    fn stroke(tool: InkTool, width: f32, points: &[(f32, f32)]) -> InkStroke {
        InkStroke::new(
            tool,
            "ink-primary",
            width,
            points.iter().map(|(x, y)| InkPoint::new(*x, *y)).collect(),
        )
    }

    #[test]
    fn an_ink_annotation_has_no_body_and_is_page_targeted() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);

        let annotation = create_ink_annotation(&conn, page_id).unwrap();
        assert_eq!(annotation.kind, AnnotationKind::Ink);
        assert_eq!(annotation.body, "");
        assert!(matches!(annotation.target, AnnotationTarget::Page));
    }

    #[test]
    fn strokes_round_trip_through_the_database() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        let annotation = create_ink_annotation(&conn, page_id).unwrap();

        let strokes = vec![
            stroke(InkTool::Pen, 2.0, &[(0.1, 5.0), (0.2, 6.0), (0.3, 7.0)]),
            stroke(InkTool::Highlighter, 10.0, &[(0.1, 20.0), (0.5, 20.0)]),
        ];
        add_strokes(&conn, annotation.id, &strokes).unwrap();

        let loaded = strokes_for_page(&conn, page_id).unwrap();
        assert_eq!(loaded.len(), 1);
        let (_, read_back) = &loaded[0];
        assert_eq!(read_back.len(), 2);
        assert_eq!(read_back[0].tool, InkTool::Pen);
        assert_eq!(read_back[0].points.len(), 3);
        assert_eq!(read_back[1].tool, InkTool::Highlighter);
        assert!((read_back[1].width - 10.0).abs() < f32::EPSILON);
        // Points survive exactly, including their normalised coordinates.
        assert_eq!(read_back[0].points[0], InkPoint::new(0.1, 5.0));
    }

    #[test]
    fn adding_strokes_continues_the_position_sequence() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        let annotation = create_ink_annotation(&conn, page_id).unwrap();

        add_strokes(
            &conn,
            annotation.id,
            &[stroke(InkTool::Pen, 2.0, &[(0.0, 0.0)])],
        )
        .unwrap();
        add_strokes(
            &conn,
            annotation.id,
            &[stroke(InkTool::Pen, 2.0, &[(0.0, 1.0)])],
        )
        .unwrap();

        let positions: Vec<i64> = conn
            .prepare("SELECT position FROM ink_strokes ORDER BY position")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(positions, vec![0, 1]);
    }

    #[test]
    fn pressure_and_timestamp_survive_round_trip() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        let annotation = create_ink_annotation(&conn, page_id).unwrap();

        let points = vec![InkPoint {
            x: 0.25,
            y: 12.0,
            pressure: Some(0.8),
            timestamp: Some(1_000),
        }];
        let strokes = vec![InkStroke::new(InkTool::Pen, "ink-primary", 2.0, points)];
        add_strokes(&conn, annotation.id, &strokes).unwrap();

        let loaded = strokes_for_page(&conn, page_id).unwrap();
        let point = &loaded[0].1[0].points[0];
        assert_eq!(point.pressure, Some(0.8));
        assert_eq!(point.timestamp, Some(1_000));
    }

    #[test]
    fn deleting_a_stroke_leaves_the_others() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        let annotation = create_ink_annotation(&conn, page_id).unwrap();

        let s1 = stroke(InkTool::Pen, 2.0, &[(0.0, 0.0)]);
        let s2 = stroke(InkTool::Pen, 2.0, &[(0.0, 1.0)]);
        let s3 = stroke(InkTool::Pen, 2.0, &[(0.0, 2.0)]);
        add_strokes(&conn, annotation.id, &[s1, s2.clone(), s3]).unwrap();

        delete_stroke(&conn, s2.id).unwrap();

        let loaded = strokes_for_page(&conn, page_id).unwrap();
        assert_eq!(loaded[0].1.len(), 2);
    }

    #[test]
    fn deleting_a_missing_stroke_fails() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        assert!(delete_stroke(&conn, StrokeId::new()).is_err());
    }

    #[test]
    fn strokes_cascade_when_the_annotation_is_deleted() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        let annotation = create_ink_annotation(&conn, page_id).unwrap();
        add_strokes(
            &conn,
            annotation.id,
            &[stroke(InkTool::Pen, 2.0, &[(0.0, 0.0)])],
        )
        .unwrap();

        super::super::annotations::delete(&conn, annotation.id).unwrap();

        let left: i64 = conn
            .query_row("SELECT count(*) FROM ink_strokes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            left, 0,
            "strokes lingered after their annotation was deleted"
        );
    }

    #[test]
    fn strokes_cascade_when_the_page_is_deleted() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        let annotation = create_ink_annotation(&conn, page_id).unwrap();
        add_strokes(
            &conn,
            annotation.id,
            &[stroke(InkTool::Pen, 2.0, &[(0.0, 0.0)])],
        )
        .unwrap();

        super::super::pages::delete(&conn, page_id).unwrap();

        let left: i64 = conn
            .query_row("SELECT count(*) FROM ink_strokes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0, "strokes lingered after their page was deleted");
    }

    #[test]
    fn a_page_with_no_ink_returns_no_strokes() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        assert!(strokes_for_page(&conn, page_id).unwrap().is_empty());
    }

    #[test]
    fn ink_and_text_annotations_coexist_on_one_page() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);

        super::super::annotations::create_for_page(
            &conn,
            page_id,
            AnnotationKind::Note,
            "A text note",
        )
        .unwrap();
        let ink = create_ink_annotation(&conn, page_id).unwrap();
        add_strokes(&conn, ink.id, &[stroke(InkTool::Pen, 2.0, &[(0.0, 0.0)])]).unwrap();

        // The full annotation list still has both.
        let all = super::super::annotations::list(&conn, page_id).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|a| a.kind == AnnotationKind::Note));
        assert!(all.iter().any(|a| a.kind == AnnotationKind::Ink));

        // The ink list reports only ink.
        let ink_list = list_ink_annotations(&conn, page_id).unwrap();
        assert_eq!(ink_list.len(), 1);
    }

    #[test]
    fn adding_strokes_to_a_non_ink_annotation_is_refused() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        let note = super::super::annotations::create_for_page(
            &conn,
            page_id,
            AnnotationKind::Note,
            "note",
        )
        .unwrap();

        let err =
            add_strokes(&conn, note.id, &[stroke(InkTool::Pen, 2.0, &[(0.0, 0.0)])]).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidInput);
    }

    #[test]
    fn a_malformed_points_blob_loads_as_empty_without_failing_the_page() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        let annotation = create_ink_annotation(&conn, page_id).unwrap();
        add_strokes(
            &conn,
            annotation.id,
            &[stroke(InkTool::Pen, 2.0, &[(0.0, 0.0)])],
        )
        .unwrap();

        // Corrupt the stored points.
        conn.execute(
            "UPDATE ink_strokes SET points_json = ?2 WHERE annotation_id = ?1",
            params![annotation.id, "{ this is not json"],
        )
        .unwrap();

        // The read does not panic; the stroke simply has no points.
        let loaded = strokes_for_page(&conn, page_id).unwrap();
        assert_eq!(loaded[0].1.len(), 1);
        assert!(loaded[0].1[0].points.is_empty());
    }

    #[test]
    fn an_anchored_ink_annotation_relocates_with_the_text() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        let page_id = page(&conn);
        let text = "The road had been salt once.";
        super::super::pages::save_document(
            &conn,
            page_id,
            serde_json::json!({
                "type": "doc",
                "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }]
            }),
        )
        .unwrap();

        let from = text.find("salt").unwrap() as i64;
        let ink = create_anchored_ink_annotation(&conn, page_id, from, from + 4).unwrap();
        add_strokes(&conn, ink.id, &[stroke(InkTool::Pen, 2.0, &[(0.0, 0.0)])]).unwrap();

        // Edit the text before the anchor and re-anchor.
        super::super::pages::save_document(
            &conn,
            page_id,
            serde_json::json!({
                "type": "doc",
                "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "A new line. The road had been salt once." }] }]
            }),
        )
        .unwrap();
        super::super::annotations::reanchor_page(&conn, page_id).unwrap();

        let moved = super::super::annotations::get(&conn, ink.id).unwrap();
        assert_eq!(
            moved.status,
            crate::domain::annotation::AnnotationStatus::Active
        );
        // And the stroke is still there.
        let loaded = strokes_for_page(&conn, page_id).unwrap();
        assert_eq!(loaded[0].1.len(), 1);
    }
}
