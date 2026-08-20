//! Chapter and Page commands.

use serde_json::Value;
use tauri::State;

use crate::app_state::AppState;
use crate::domain::{Chapter, Page, PageSummary};
use crate::error::Result;
use crate::repositories;

use super::{parse_chapter_id, parse_chapter_ids, parse_page_id, parse_page_ids, parse_volume_id};

// --- Chapters ---------------------------------------------------------------

#[tauri::command]
pub async fn chapter_create(
    state: State<'_, AppState>,
    volume_id: String,
    title: String,
) -> Result<Chapter> {
    let volume = parse_volume_id(&volume_id)?;
    state
        .write(move |tx| repositories::chapters::create(tx, volume, &title))
        .await
}

#[tauri::command]
pub async fn chapter_rename(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<Chapter> {
    let id = parse_chapter_id(&id)?;
    state
        .write(move |tx| repositories::chapters::rename(tx, id, &title))
        .await
}

#[tauri::command]
pub async fn chapter_duplicate(state: State<'_, AppState>, id: String) -> Result<Chapter> {
    let id = parse_chapter_id(&id)?;
    state
        .write(move |tx| repositories::chapters::duplicate(tx, id))
        .await
}

#[tauri::command]
pub async fn chapter_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    let id = parse_chapter_id(&id)?;
    state
        .write(move |tx| repositories::chapters::delete(tx, id))
        .await
}

#[tauri::command]
pub async fn chapter_reorder(
    state: State<'_, AppState>,
    volume_id: String,
    ordered: Vec<String>,
) -> Result<()> {
    let volume = parse_volume_id(&volume_id)?;
    let ordered = parse_chapter_ids(&ordered)?;
    state
        .write(move |tx| repositories::chapters::reorder(tx, volume, &ordered))
        .await
}

// --- Pages ------------------------------------------------------------------

#[tauri::command]
pub async fn page_create(
    state: State<'_, AppState>,
    chapter_id: String,
    title: String,
) -> Result<Page> {
    let chapter = parse_chapter_id(&chapter_id)?;
    state
        .write(move |tx| repositories::pages::create(tx, chapter, &title))
        .await
}

#[tauri::command]
pub async fn page_get(state: State<'_, AppState>, id: String) -> Result<Page> {
    let id = parse_page_id(&id)?;
    state
        .read(move |conn| repositories::pages::get(conn, id))
        .await
}

#[tauri::command]
pub async fn page_summaries(
    state: State<'_, AppState>,
    volume_id: String,
) -> Result<Vec<PageSummary>> {
    let volume = parse_volume_id(&volume_id)?;
    state
        .read(move |conn| repositories::pages::summaries_for_volume(conn, volume))
        .await
}

/// Persists an editor document.
///
/// Takes only the document: every derived value is recomputed from it in the
/// same transaction, so there is no parameter that could store a word count
/// disagreeing with the text it describes. The same transaction takes a
/// checkpoint revision when one is due and clears the recovery draft.
#[tauri::command]
pub async fn page_save(state: State<'_, AppState>, id: String, document: Value) -> Result<Page> {
    let id = parse_page_id(&id)?;
    state
        .write(move |tx| repositories::pages::save_document(tx, id, document))
        .await
}

#[tauri::command]
pub async fn page_rename(state: State<'_, AppState>, id: String, title: String) -> Result<Page> {
    let id = parse_page_id(&id)?;
    state
        .write(move |tx| repositories::pages::rename(tx, id, &title))
        .await
}

#[tauri::command]
pub async fn page_duplicate(state: State<'_, AppState>, id: String) -> Result<Page> {
    let id = parse_page_id(&id)?;
    state
        .write(move |tx| repositories::pages::duplicate(tx, id))
        .await
}

#[tauri::command]
pub async fn page_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    let id = parse_page_id(&id)?;
    state
        .write(move |tx| repositories::pages::delete(tx, id))
        .await
}

#[tauri::command]
pub async fn page_reorder(
    state: State<'_, AppState>,
    chapter_id: String,
    ordered: Vec<String>,
) -> Result<()> {
    let chapter = parse_chapter_id(&chapter_id)?;
    let ordered = parse_page_ids(&ordered)?;
    state
        .write(move |tx| repositories::pages::reorder(tx, chapter, &ordered))
        .await
}

#[tauri::command]
pub async fn page_move(
    state: State<'_, AppState>,
    id: String,
    chapter_id: String,
    index: Option<i64>,
) -> Result<Page> {
    let id = parse_page_id(&id)?;
    let chapter = parse_chapter_id(&chapter_id)?;
    state
        .write(move |tx| repositories::pages::move_to_chapter(tx, id, chapter, index))
        .await
}
