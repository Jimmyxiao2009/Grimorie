//! Ink annotation commands.
//!
//! Thin adapters, like the rest of this module: parse identifiers, call one
//! repository function, return. The drawing, anchoring, and cascade rules live
//! in the domain and repositories; this is only the IPC surface.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::app_state::AppState;
use crate::domain::annotation::Annotation;
use crate::domain::ids::{AnnotationId, StrokeId};
use crate::domain::ink::{InkStroke, InkTool};
use crate::error::{AppError, Result};
use crate::repositories;

use super::parse_page_id;

fn parse_tool(raw: &str) -> Result<InkTool> {
    InkTool::parse(raw).ok_or_else(|| AppError::invalid("Ink tool must be 'pen' or 'highlighter'."))
}

/// A stroke as it crosses the IPC boundary, with the points drawn locally.
///
/// The id is minted by the frontend so an optimistic stroke already on screen
/// can be matched to its persisted counterpart; the server trusts it because the
/// id is only ever used as a key, never to read state. `createdAt` is supplied
/// so drawing order is preserved exactly as the pen produced it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokeInput {
    pub id: String,
    pub tool: String,
    pub color: String,
    pub width: f32,
    pub points: Vec<PointInput>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointInput {
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub pressure: Option<f32>,
    #[serde(default)]
    pub timestamp: Option<u64>,
}

impl StrokeInput {
    pub(crate) fn into_domain(self) -> Result<InkStroke> {
        let id = StrokeId::parse(&self.id)?;
        let tool = parse_tool(&self.tool)?;
        let points = self
            .points
            .into_iter()
            .map(|p| crate::domain::ink::InkPoint {
                x: p.x,
                y: p.y,
                pressure: p.pressure,
                timestamp: p.timestamp,
            })
            .collect();
        let created_at = self
            .created_at
            .and_then(|raw| chrono::DateTime::parse_from_rfc3339(&raw).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(crate::domain::manuscript::now);

        Ok(InkStroke {
            id,
            tool,
            color: self.color,
            width: self.width,
            points,
            created_at,
        })
    }
}

/// The payload returned for one ink note: the annotation it hangs on, and its
/// strokes in drawing order.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InkNote {
    pub annotation: Annotation,
    pub strokes: Vec<InkStroke>,
}

/// Creates a whole-Page ink note and returns it ready to draw into.
#[tauri::command]
pub async fn ink_create(state: State<'_, AppState>, page_id: String) -> Result<Annotation> {
    let page = parse_page_id(&page_id)?;
    state
        .write(move |tx| repositories::ink::create_ink_annotation(tx, page))
        .await
}

/// Creates an ink note anchored to a range of the Page's plain text.
#[tauri::command]
pub async fn ink_create_anchored(
    state: State<'_, AppState>,
    page_id: String,
    from: i64,
    to: i64,
) -> Result<Annotation> {
    let page = parse_page_id(&page_id)?;
    state
        .write(move |tx| repositories::ink::create_anchored_ink_annotation(tx, page, from, to))
        .await
}

/// Every ink note on a Page, with its strokes — the payload the Margin paints
/// from when a Page opens.
#[tauri::command]
pub async fn ink_notes_for_page(
    state: State<'_, AppState>,
    page_id: String,
) -> Result<Vec<InkNote>> {
    let page = parse_page_id(&page_id)?;
    state
        .read(move |conn| {
            let mut by_id: Vec<(AnnotationId, Annotation)> =
                repositories::ink::list_ink_annotations(conn, page)?
                    .into_iter()
                    .map(|a| (a.id, a))
                    .collect();
            let strokes = repositories::ink::strokes_for_page(conn, page)?;

            // Keep the Margin's order: annotations come newest-first from the
            // repository, and strokes are grouped under them.
            by_id.sort_by_key(|(id, _)| {
                strokes
                    .iter()
                    .position(|(aid, _)| aid == id)
                    .unwrap_or(usize::MAX)
            });

            Ok(by_id
                .into_iter()
                .map(|(id, annotation)| InkNote {
                    annotation,
                    strokes: strokes
                        .iter()
                        .find(|(aid, _)| *aid == id)
                        .map(|(_, s)| s.clone())
                        .unwrap_or_default(),
                })
                .collect())
        })
        .await
}

/// Persists strokes drawn locally, on `pointerup`. One call per finished stroke
/// keeps the write rate to "once per pen lift" rather than once per point.
///
/// Adding strokes changes the ink, so any existing recognition is invalidated:
/// the old transcript no longer describes the handwriting on screen, and a fresh
/// recognition is due.
#[tauri::command]
pub async fn ink_add_strokes(
    state: State<'_, AppState>,
    annotation_id: String,
    strokes: Vec<StrokeInput>,
) -> Result<()> {
    let annotation = AnnotationId::parse(&annotation_id)?;
    let domain_strokes = strokes
        .into_iter()
        .map(StrokeInput::into_domain)
        .collect::<Result<Vec<_>>>()?;
    state
        .write(move |tx| {
            repositories::ink::add_strokes(tx, annotation, &domain_strokes)?;
            repositories::ink_recognition::invalidate(tx, annotation)?;
            Ok(())
        })
        .await
}

/// Removes a single stroke — the eraser's unit of work.
///
/// Erasing changes the ink, so the note's recognition is invalidated just as an
/// add does. The annotation is resolved before the delete, because the stroke
/// row is gone once the delete runs.
#[tauri::command]
pub async fn ink_delete_stroke(state: State<'_, AppState>, id: String) -> Result<()> {
    let stroke = StrokeId::parse(&id)?;
    state
        .write(move |tx| {
            let annotation = repositories::ink::annotation_of_stroke(tx, stroke)?;
            repositories::ink::delete_stroke(tx, stroke)?;
            repositories::ink_recognition::invalidate(tx, annotation)?;
            Ok(())
        })
        .await
}

/// Deletes an ink note and every stroke under it. Used when a note is cleared
/// or removed from the Margin.
#[tauri::command]
pub async fn ink_delete_annotation(state: State<'_, AppState>, id: String) -> Result<()> {
    let annotation = AnnotationId::parse(&id)?;
    state
        .write(move |tx| repositories::annotations::delete(tx, annotation))
        .await
}
