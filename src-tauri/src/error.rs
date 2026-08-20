//! Application errors.
//!
//! Errors cross the IPC boundary as structured values, not strings, so the
//! frontend can react to *what went wrong* while still having a sentence
//! written for a person.
//!
//! Two rules apply to every message here:
//!
//! 1. It says what happened to the user's work. "Grimoire couldn't save this
//!    Page. Your text is still open and has not been discarded." — not
//!    "Something went wrong."
//! 2. It never contains manuscript text or credentials. Titles are permitted;
//!    body text is not.

use serde::Serialize;

/// A machine-readable classification. The frontend switches on this; it never
/// parses the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    /// The thing being addressed does not exist — usually deleted elsewhere.
    NotFound,
    /// The request was well-formed but is not allowed in the current state.
    Conflict,
    /// The request itself was malformed. Indicates a frontend bug.
    InvalidInput,
    /// The content an operation was computed against has since changed.
    Stale,
    /// Storage failed.
    Database,
    /// Schema migration failed. The database has been left untouched.
    Migration,
    /// A file could not be read or written.
    Io,
    /// An AI provider could not be reached.
    Network,
    /// An AI provider was reached and refused or failed.
    Provider,
    /// The OS credential store could not be used.
    Credential,
    /// The user cancelled, or a newer request superseded this one.
    Cancelled,
    /// An unexpected failure. Always accompanied by a log entry.
    Internal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: ErrorCode,
    /// A complete sentence, written for the person using the app.
    pub message: String,
    /// Technical context for the log and for a "Details" disclosure. Never
    /// shown by default and never contains secrets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn not_found(what: &str) -> Self {
        Self::new(
            ErrorCode::NotFound,
            format!("Grimoire couldn't find that {what}. It may have been deleted."),
        )
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    pub fn stale(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Stale, message)
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        // The detail is for the log; the user gets a sentence that tells them
        // the state of their work rather than a stack trace.
        Self::new(
            ErrorCode::Internal,
            "Grimoire hit an unexpected problem. Your work has not been changed.",
        )
        .with_detail(detail)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(detail) = &self.detail {
            write!(f, " ({detail})")?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        match err {
            rusqlite::Error::QueryReturnedNoRows => AppError::not_found("record"),
            other => AppError::new(
                ErrorCode::Database,
                "Grimoire couldn't reach its library file. Your work has not been changed.",
            )
            .with_detail(other.to_string()),
        }
    }
}

impl From<r2d2::Error> for AppError {
    fn from(err: r2d2::Error) -> Self {
        AppError::new(
            ErrorCode::Database,
            "Grimoire couldn't open its library file. Your work has not been changed.",
        )
        .with_detail(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::internal(format!("json: {err}"))
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::new(ErrorCode::Io, "Grimoire couldn't read or write that file.")
            .with_detail(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_written_for_a_person() {
        let error = AppError::not_found("Page");
        assert!(error.message.starts_with("Grimoire couldn't find"));
        assert!(error.message.ends_with('.'));
    }

    #[test]
    fn internal_errors_hide_the_detail_from_the_message() {
        let error = AppError::internal("poisoned mutex at repositories/pages.rs:214");
        assert!(!error.message.contains("mutex"));
        assert_eq!(
            error.detail.as_deref(),
            Some("poisoned mutex at repositories/pages.rs:214")
        );
        // The user is told the state of their work, which is the thing they
        // actually need to know.
        assert!(error.message.contains("has not been changed"));
    }

    #[test]
    fn missing_rows_become_not_found_rather_than_a_database_failure() {
        let error: AppError = rusqlite::Error::QueryReturnedNoRows.into();
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[test]
    fn codes_serialise_in_kebab_case_for_the_frontend() {
        let json = serde_json::to_string(&ErrorCode::InvalidInput).unwrap();
        assert_eq!(json, "\"invalid-input\"");
    }
}
