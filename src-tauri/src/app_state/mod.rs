//! Shared application state.
//!
//! Every database call goes through [`AppState::read`] or [`AppState::write`],
//! which move the work onto a blocking thread. SQLite calls are synchronous and
//! can wait on a lock; running one on the runtime that also services IPC would
//! stall the UI, and the typing path in particular must never wait on storage.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;

use crate::ai::AiProvider;
use crate::ai::openai::OpenAiCompatible;
use crate::database::Database;
use crate::error::{AppError, Result};

#[derive(Clone)]
pub struct AppState {
    db: Database,
    /// One HTTP client for the life of the app, so connections are reused
    /// rather than renegotiated for every request.
    provider: Arc<dyn AiProvider>,
    /// Cancellation tokens for AI requests still in flight, by request id.
    in_flight: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl AppState {
    pub fn new(db: Database) -> Result<Self> {
        Ok(Self {
            db,
            provider: Arc::new(OpenAiCompatible::new()?),
            in_flight: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn database(&self) -> &Database {
        &self.db
    }

    pub fn provider(&self) -> Arc<dyn AiProvider> {
        Arc::clone(&self.provider)
    }

    /// Registers a request so it can be cancelled, and returns its token.
    pub fn begin_request(&self, id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        if let Ok(mut map) = self.in_flight.lock() {
            map.insert(id.to_string(), token.clone());
        }
        token
    }

    /// Cancels a request if it is still running. Returns whether one was found.
    pub fn cancel_request(&self, id: &str) -> bool {
        let Ok(mut map) = self.in_flight.lock() else {
            return false;
        };
        match map.remove(id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Forgets a finished request. Called however a request ends, so the map
    /// cannot grow across a long session.
    pub fn finish_request(&self, id: &str) {
        if let Ok(mut map) = self.in_flight.lock() {
            map.remove(id);
        }
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
