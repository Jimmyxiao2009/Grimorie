//! Grimoire — a local-first writing environment.
//!
//! The Rust side owns durability: the database, migrations, backups, credential
//! storage, and AI transport. It does no presentation work. See
//! `docs/architecture.md` for the boundary this crate is held to.

pub mod ai;
mod app_state;
mod commands;
pub mod credentials;
mod logging;

// Public so development tools — the seed example, and any future maintenance
// utility — can build a library without going through the GUI. These are the
// same paths the app itself uses, so a tool cannot drift from the real
// behaviour.
pub mod database;
pub mod domain;
pub mod error;
pub mod repositories;
pub mod search;

#[cfg(test)]
mod integration;

use tauri::{Manager, Window};

use app_state::AppState;
use database::Database;

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
fn app_info(state: tauri::State<'_, AppState>) -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "sqliteVersion": rusqlite::version(),
        "debug": cfg!(debug_assertions),
        "libraryPath": state.database().path().to_string_lossy(),
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

            // The library is one file in the app's data directory: easy to
            // find, easy to back up, easy to take away. That is what "the
            // manuscript belongs to the user" has to mean in practice.
            //
            // GRIMOIRE_LIBRARY points it somewhere else — an external drive, a
            // synced folder, or a scratch copy for development.
            let library = match std::env::var_os("GRIMOIRE_LIBRARY") {
                Some(path) => std::path::PathBuf::from(path),
                None => app.path().app_data_dir()?.join("grimoire.db"),
            };
            let db = Database::open(&library).inspect_err(|err| {
                tracing::error!(error = %err, "could not open the library");
            })?;
            tracing::info!(path = ?db.path(), "library opened");

            // Recover recognition rows left mid-flight by an abnormal exit. A
            // 'recognizing' status can only exist while a job runs; if the app
            // was killed under one, the row is stuck behind a "Recognizing…"
            // that can never finish. Moving them to 'pending' is safe and lets
            // the writer retry — or the scheduler pick them up — on this launch.
            {
                let conn = db.get()?;
                if let Err(err) = repositories::ink_recognition::repair_transient(&conn) {
                    tracing::warn!(error = %err, "could not repair transient recognition rows");
                }
            }

            app.manage(AppState::new(db)?);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_ready,
            app_info,
            commands::volumes::volumes_list,
            commands::volumes::volume_create,
            commands::volumes::volume_get,
            commands::volumes::volume_open,
            commands::volumes::volume_outline,
            commands::volumes::volume_update,
            commands::volumes::volume_set_archived,
            commands::volumes::volume_duplicate,
            commands::volumes::volume_delete,
            commands::manuscript::chapter_create,
            commands::manuscript::chapter_rename,
            commands::manuscript::chapter_duplicate,
            commands::manuscript::chapter_delete,
            commands::manuscript::chapter_reorder,
            commands::manuscript::page_create,
            commands::manuscript::page_get,
            commands::manuscript::page_summaries,
            commands::manuscript::page_save,
            commands::manuscript::page_rename,
            commands::manuscript::page_duplicate,
            commands::manuscript::page_delete,
            commands::manuscript::page_reorder,
            commands::manuscript::page_move,
            commands::annotations::annotations_list,
            commands::annotations::annotations_open_counts,
            commands::annotations::annotation_create,
            commands::annotations::annotation_create_for_page,
            commands::annotations::annotation_update,
            commands::annotations::annotation_set_status,
            commands::annotations::annotation_delete,
            commands::ink::ink_create,
            commands::ink::ink_create_anchored,
            commands::ink::ink_notes_for_page,
            commands::ink::ink_add_strokes,
            commands::ink::ink_delete_stroke,
            commands::ink::ink_delete_annotation,
            commands::ink_recognition::ink_recognize,
            commands::ink_recognition::ink_recognize_manual,
            commands::ink_recognition::ink_recognition_for_page,
            commands::ink_recognition::ink_recognition_status,
            commands::ink_recognition::ink_edit_transcript,
            commands::ink_recognition::ink_convert_to_text,
            commands::ink_recognition::ink_recognize_cancel,
            commands::ink_recognition::ink_recognition_language,
            commands::history::revisions_list,
            commands::history::revision_get,
            commands::history::revision_restore,
            commands::history::draft_write,
            commands::history::drafts_recoverable,
            commands::history::draft_recover,
            commands::history::draft_discard,
            commands::ai::ai_providers,
            commands::ai::ai_provider_save,
            commands::ai::ai_provider_delete,
            commands::ai::ai_profiles,
            commands::ai::ai_profile_save,
            commands::ai::ai_profile_delete,
            commands::ai::ai_actions,
            commands::ai::ai_preview,
            commands::ai::ai_run,
            commands::ai::ai_cancel,
            commands::ai::ai_suggestions,
            commands::ai::ai_suggestion_reevaluate,
            commands::ai::ai_suggestion_apply,
            commands::ai::ai_suggestion_dismiss,
            commands::bookmarks::bookmark_toggle,
            commands::bookmarks::bookmarks_list,
            commands::bookmarks::bookmarked_pages,
            commands::bookmarks::tags_for,
            commands::bookmarks::tags_all,
            commands::bookmarks::tag_attach,
            commands::bookmarks::tag_detach,
            commands::search::search_query,
            commands::search::search_rebuild,
            commands::settings::settings_get,
            commands::settings::settings_save,
        ])
        .run(tauri::generate_context!())
        .expect("Grimoire failed to start");

    tracing::info!("grimoire shutting down");
}
