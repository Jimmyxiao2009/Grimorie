//! Grimoire — a local-first writing environment.
//!
//! The Rust side owns durability: the database, migrations, backups, credential
//! storage, and AI transport. It does no presentation work. See
//! `docs/architecture.md` for the boundary this crate is held to.

mod logging;

use tauri::{Manager, Window};

/// Reported by the frontend once the first paint is complete, so the window can
/// be revealed without a flash of unstyled white.
#[tauri::command]
fn app_ready(window: Window) {
    if let Err(err) = window.show() {
        tracing::warn!(error = %err, "could not reveal the main window");
    }
}

/// Build metadata, so the About screen states facts rather than a hardcoded string.
#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "sqliteVersion": rusqlite::version(),
        "debug": cfg!(debug_assertions),
    })
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let log_dir = app.path().app_log_dir()?;
            // The guard must outlive the app, so it is parked in managed state.
            if let Some(guard) = logging::init(&log_dir) {
                app.manage(guard);
            }
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "grimoire starting");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![app_ready, app_info])
        .run(tauri::generate_context!())
        .expect("Grimoire failed to start");

    tracing::info!("grimoire shutting down");
}
