//! Margin annotation commands.

use tauri::State;

use crate::app_state::AppState;
use crate::domain::annotation::{Annotation, AnnotationKind, AnnotationStatus};
use crate::domain::ids::AnnotationId;
use crate::error::{AppError, Result};
use crate::repositories;

use super::{parse_page_id, parse_volume_id};

fn parse_kind(raw: &str) -> Result<AnnotationKind> {
    AnnotationKind::parse(raw).ok_or_else(|| AppError::invalid("Unknown annotation kind."))
}

fn parse_status(raw: &str) -> Result<AnnotationStatus> {
    AnnotationStatus::parse(raw).ok_or_else(|| AppError::invalid("Unknown annotation status."))
}

#[tauri::command]
pub async fn annotations_list(
    state: State<'_, AppState>,
    page_id: String,
) -> Result<Vec<Annotation>> {
    let page = parse_page_id(&page_id)?;
    state
        .read(move |conn| repositories::annotations::list(conn, page))
        .await
}

/// Unresolved counts per Page, for the manuscript tree.
///
/// One query for the whole Volume rather than one per Page.
#[tauri::command]
pub async fn annotations_open_counts(
    state: State<'_, AppState>,
    volume_id: String,
) -> Result<Vec<(String, i64)>> {
    let volume = parse_volume_id(&volume_id)?;
    let counts = state
        .read(move |conn| repositories::annotations::open_counts(conn, volume))
        .await?;
    Ok(counts
        .into_iter()
        .map(|(id, n)| (id.to_string(), n))
        .collect())
}

/// Creates a note anchored to a range of the Page's plain text.
///
/// Only the offsets come from the frontend. The anchored text and the
/// surrounding context that makes relocation possible are read from the stored
/// copy, so an anchor can never describe text the Page does not contain.
#[tauri::command]
pub async fn annotation_create(
    state: State<'_, AppState>,
    page_id: String,
    kind: String,
    body: String,
    from: i64,
    to: i64,
) -> Result<Annotation> {
    let page = parse_page_id(&page_id)?;
    let kind = parse_kind(&kind)?;
    state
        .write(move |tx| {
            repositories::annotations::create_anchored(tx, page, kind, &body, from, to)
        })
        .await
}

#[tauri::command]
pub async fn annotation_create_for_page(
    state: State<'_, AppState>,
    page_id: String,
    kind: String,
    body: String,
) -> Result<Annotation> {
    let page = parse_page_id(&page_id)?;
    let kind = parse_kind(&kind)?;
    state
        .write(move |tx| repositories::annotations::create_for_page(tx, page, kind, &body))
        .await
}

#[tauri::command]
pub async fn annotation_update(
    state: State<'_, AppState>,
    id: String,
    body: String,
) -> Result<Annotation> {
    let id = AnnotationId::parse(&id)?;
    state
        .write(move |tx| repositories::annotations::update_body(tx, id, &body))
        .await
}

#[tauri::command]
pub async fn annotation_set_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
) -> Result<Annotation> {
    let id = AnnotationId::parse(&id)?;
    let status = parse_status(&status)?;
    state
        .write(move |tx| repositories::annotations::set_status(tx, id, status))
        .await
}

#[tauri::command]
pub async fn annotation_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    let id = AnnotationId::parse(&id)?;
    state
        .write(move |tx| repositories::annotations::delete(tx, id))
        .await
}
