//! Shared application state.
//!
//! Every database call goes through [`AppState::read`] or [`AppState::write`],
//! which move the work onto a blocking thread. SQLite calls are synchronous and
//! can wait on a lock; running one on the runtime that also services IPC would
//! stall the UI, and the typing path in particular must never wait on storage.

use crate::database::Database;
use crate::error::{AppError, Result};

#[derive(Clone)]
pub struct AppState {
    db: Database,
}

impl AppState {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Runs a read on a blocking thread.
    pub async fn read<T, F>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let db = self.db.clone();
        join(tauri::async_runtime::spawn_blocking(move || {
            let conn = db.get()?;
            work(&conn)
        }))
        .await
    }

    /// Runs a mutation on a blocking thread, inside a transaction.
    ///
    /// Writes are transactional without the caller having to remember: a
    /// half-applied structural change — a Chapter deleted but its siblings not
    /// repacked — is exactly the kind of corruption this prevents.
    pub async fn write<T, F>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let db = self.db.clone();
        join(tauri::async_runtime::spawn_blocking(move || {
            db.transaction(work)
        }))
        .await
    }
}

async fn join<T>(handle: tauri::async_runtime::JoinHandle<Result<T>>) -> Result<T> {
    match handle.await {
        Ok(value) => value,
        Err(err) => Err(AppError::internal(format!(
            "database task did not finish: {err}"
        ))),
    }
}
