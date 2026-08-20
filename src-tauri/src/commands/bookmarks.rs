//! Bookmark and tag commands.

use tauri::State;

use crate::app_state::AppState;
use crate::domain::ids::TagId;
use crate::error::Result;
use crate::repositories::bookmarks::{Bookmark, Tag, TaggableKind};
use crate::repositories::{self};

use super::{parse_page_id, parse_volume_id};

// --- Bookmarks --------------------------------------------------------------

/// Marks a Page, or clears the mark. Returns whether it is now bookmarked.
#[tauri::command]
pub async fn bookmark_toggle(
    state: State<'_, AppState>,
    page_id: String,
    label: Option<String>,
) -> Result<bool> {
    let page = parse_page_id(&page_id)?;
    let label = label.unwrap_or_default();
    state
        .write(move |tx| repositories::bookmarks::toggle(tx, page, &label))
        .await
}

#[tauri::command]
pub async fn bookmarks_list(
    state: State<'_, AppState>,
    volume_id: Option<String>,
) -> Result<Vec<Bookmark>> {
    let scope = match volume_id {
        Some(raw) => Some(parse_volume_id(&raw)?.to_string()),
        None => None,
    };
    state
        .read(move |conn| repositories::bookmarks::list(conn, scope.as_deref()))
        .await
}

/// The bookmarked Pages of a Volume, for marking them in the manuscript tree.
#[tauri::command]
pub async fn bookmarked_pages(
    state: State<'_, AppState>,
    volume_id: String,
) -> Result<Vec<String>> {
    let volume = parse_volume_id(&volume_id)?;
    let ids = state
        .read(move |conn| repositories::bookmarks::page_ids_for_volume(conn, volume))
        .await?;
    Ok(ids.into_iter().map(|id| id.to_string()).collect())
}

// --- Tags -------------------------------------------------------------------

#[tauri::command]
pub async fn tags_for(state: State<'_, AppState>, entity_id: String) -> Result<Vec<Tag>> {
    state
        .read(move |conn| repositories::bookmarks::tags_for(conn, &entity_id))
        .await
}

#[tauri::command]
pub async fn tags_all(state: State<'_, AppState>) -> Result<Vec<Tag>> {
    state.read(repositories::bookmarks::all_tags).await
}

#[tauri::command]
pub async fn tag_attach(
    state: State<'_, AppState>,
    kind: String,
    entity_id: String,
    name: String,
) -> Result<Tag> {
    let kind = TaggableKind::parse(&kind)?;
    state
        .write(move |tx| repositories::bookmarks::attach(tx, kind, &entity_id, &name))
        .await
}

#[tauri::command]
pub async fn tag_detach(
    state: State<'_, AppState>,
    entity_id: String,
    tag_id: String,
) -> Result<()> {
    let tag = TagId::parse(&tag_id)?;
    state
        .write(move |tx| repositories::bookmarks::detach(tx, &entity_id, tag))
        .await
}
