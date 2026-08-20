//! Search commands.

use tauri::State;

use crate::app_state::AppState;
use crate::error::Result;
use crate::search::{self, SearchHit};

use super::parse_volume_id;

/// How many hits one query returns.
///
/// A writer scans the first handful and refines; a longer list is slower to
/// produce and no more useful.
const DEFAULT_LIMIT: i64 = 40;

#[tauri::command]
pub async fn search_query(
    state: State<'_, AppState>,
    query: String,
    volume_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<SearchHit>> {
    // Validated here so a malformed id fails at the boundary rather than
    // silently searching the whole library.
    let scope = match volume_id {
        Some(raw) => Some(parse_volume_id(&raw)?.to_string()),
        None => None,
    };
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 200);

    state
        .read(move |conn| search::search(conn, &query, scope.as_deref(), limit))
        .await
}

/// Rebuilds the index from the manuscript.
///
/// Offered in Settings as a repair: a library whose index was lost, or one
/// written before a change to how text is segmented, is fixed by this rather
/// than by re-typing anything.
#[tauri::command]
pub async fn search_rebuild(state: State<'_, AppState>) -> Result<usize> {
    state.write(|tx| search::rebuild(tx)).await
}
