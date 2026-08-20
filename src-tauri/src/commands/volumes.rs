//! Library commands: the shelf, and operations on whole Volumes.

use tauri::State;

use crate::app_state::AppState;
use crate::domain::{Outline, Volume, VolumeSummary};
use crate::error::Result;
use crate::repositories::{self, volumes::Shelf};

use super::parse_volume_id;

#[tauri::command]
pub async fn volumes_list(
    state: State<'_, AppState>,
    shelf: String,
    limit: Option<i64>,
) -> Result<Vec<VolumeSummary>> {
    let shelf = Shelf::parse(&shelf)?;
    state
        .read(move |conn| repositories::volumes::list(conn, shelf, limit))
        .await
}

#[tauri::command]
pub async fn volume_create(
    state: State<'_, AppState>,
    title: String,
    subtitle: Option<String>,
    description: Option<String>,
) -> Result<Volume> {
    state
        .write(move |tx| {
            repositories::volumes::create(tx, &title, subtitle.as_deref(), description.as_deref())
        })
        .await
}

#[tauri::command]
pub async fn volume_get(state: State<'_, AppState>, id: String) -> Result<Volume> {
    let id = parse_volume_id(&id)?;
    state
        .read(move |conn| repositories::volumes::get(conn, id))
        .await
}

/// Opens a Volume: returns its outline and records the visit for the Recent
/// shelf, in one round trip so navigation is a single call.
#[tauri::command]
pub async fn volume_open(state: State<'_, AppState>, id: String) -> Result<Outline> {
    let id = parse_volume_id(&id)?;
    state
        .write(move |tx| {
            repositories::volumes::touch_opened(tx, id)?;
            repositories::outline(tx, id)
        })
        .await
}

#[tauri::command]
pub async fn volume_outline(state: State<'_, AppState>, id: String) -> Result<Outline> {
    let id = parse_volume_id(&id)?;
    state
        .read(move |conn| repositories::outline(conn, id))
        .await
}

#[tauri::command]
pub async fn volume_update(
    state: State<'_, AppState>,
    id: String,
    title: String,
    subtitle: Option<String>,
    description: Option<String>,
) -> Result<Volume> {
    let id = parse_volume_id(&id)?;
    state
        .write(move |tx| {
            repositories::volumes::update_details(
                tx,
                id,
                &title,
                subtitle.as_deref(),
                description.as_deref(),
            )
        })
        .await
}

#[tauri::command]
pub async fn volume_set_archived(
    state: State<'_, AppState>,
    id: String,
    archived: bool,
) -> Result<Volume> {
    let id = parse_volume_id(&id)?;
    state
        .write(move |tx| repositories::volumes::set_archived(tx, id, archived))
        .await
}

#[tauri::command]
pub async fn volume_duplicate(state: State<'_, AppState>, id: String) -> Result<Volume> {
    let id = parse_volume_id(&id)?;
    state
        .write(move |tx| repositories::volumes::duplicate(tx, id))
        .await
}

#[tauri::command]
pub async fn volume_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    let id = parse_volume_id(&id)?;
    state
        .write(move |tx| repositories::volumes::delete(tx, id))
        .await
}
