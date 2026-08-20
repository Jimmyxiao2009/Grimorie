//! Settings commands.

use tauri::State;

use crate::app_state::AppState;
use crate::domain::settings::AppSettings;
use crate::error::Result;
use crate::repositories;

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<AppSettings> {
    state.read(repositories::settings::load).await
}

/// Saves settings and returns what was actually stored.
///
/// The return value matters: values are clamped on the way in, so the frontend
/// must adopt the result rather than assume its request was taken verbatim.
#[tauri::command]
pub async fn settings_save(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings> {
    state
        .write(move |tx| repositories::settings::save(tx, &settings))
        .await
}
