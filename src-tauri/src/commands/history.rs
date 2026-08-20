//! Revision history and crash recovery commands.

use serde_json::Value;
use tauri::State;

use crate::app_state::AppState;
use crate::domain::Page;
use crate::domain::ids::RevisionId;
use crate::error::Result;
use crate::repositories::{self, drafts::RecoverableDraft, revisions::RevisionSummary};

use super::parse_page_id;

// --- Revisions --------------------------------------------------------------

#[tauri::command]
pub async fn revisions_list(
    state: State<'_, AppState>,
    page_id: String,
) -> Result<Vec<RevisionSummary>> {
    let page = parse_page_id(&page_id)?;
    state
        .read(move |conn| repositories::revisions::list(conn, page))
        .await
}

/// The full text of one revision, for preview and comparison.
#[tauri::command]
pub async fn revision_get(
    state: State<'_, AppState>,
    id: String,
) -> Result<repositories::revisions::Revision> {
    let id = RevisionId::parse(&id)?;
    state
        .read(move |conn| repositories::revisions::get(conn, id))
        .await
}

/// Puts a revision back as the Page's current content.
///
/// The text being replaced is snapshotted first, so restoring is itself
/// undoable — a history feature that can lose the present while recovering the
/// past would be worse than none.
#[tauri::command]
pub async fn revision_restore(state: State<'_, AppState>, id: String) -> Result<Page> {
    let id = RevisionId::parse(&id)?;
    state
        .write(move |tx| repositories::revisions::restore(tx, id))
        .await
}

// --- Crash recovery ---------------------------------------------------------

/// Journals an in-progress document.
///
/// Runs on a shorter debounce than autosave and is a single-row upsert, so it
/// narrows the window in which a kill loses typing without putting a full save
/// on the keystroke path.
#[tauri::command]
pub async fn draft_write(
    state: State<'_, AppState>,
    page_id: String,
    document: Value,
) -> Result<()> {
    let page = parse_page_id(&page_id)?;
    state
        .write(move |tx| repositories::drafts::write(tx, page, &document))
        .await
}

/// Drafts worth offering back after an unclean shutdown.
///
/// Returns an empty list when there is nothing to recover — stale and
/// identical drafts are cleaned up rather than reported — so the frontend can
/// show the prompt if and only if this is non-empty.
#[tauri::command]
pub async fn drafts_recoverable(state: State<'_, AppState>) -> Result<Vec<RecoverableDraft>> {
    // A closure rather than the function itself: deref coercion from
    // &Transaction to &Connection does not happen through a function pointer.
    state
        .write(|tx| repositories::drafts::recoverable(tx))
        .await
}

#[tauri::command]
pub async fn draft_recover(state: State<'_, AppState>, page_id: String) -> Result<Page> {
    let page = parse_page_id(&page_id)?;
    state
        .write(move |tx| repositories::drafts::recover(tx, page))
        .await
}

#[tauri::command]
pub async fn draft_discard(state: State<'_, AppState>, page_id: String) -> Result<()> {
    let page = parse_page_id(&page_id)?;
    state
        .write(move |tx| repositories::drafts::discard(tx, page))
        .await
}
