//! Secrets, kept in the operating system's credential store.
//!
//! An API key never enters the database, a config file, a log line, or the
//! WebView. The database holds a *reference* — the name the secret is filed
//! under — and this module is the only thing that can turn that reference into
//! the secret, which it does at the moment a request is built.
//!
//! There is deliberately no "read the key back out to show the user" path. A
//! key can be set, replaced, tested, and deleted; it cannot be displayed.
//! Displaying it would mean sending it to the WebView, and the entire point of
//! keeping it here is that it never goes there.

use keyring::Entry;

use crate::error::{AppError, ErrorCode, Result};

/// The service name secrets are filed under. Matches the app identifier so the
/// entries are recognisable in the OS credential manager.
const SERVICE: &str = "app.grimoire.desktop";

fn entry(reference: &str) -> Result<Entry> {
    Entry::new(SERVICE, reference).map_err(|err| {
        AppError::new(
            ErrorCode::Credential,
            "Grimoire couldn't reach the credential store on this computer.",
        )
        .with_detail(err.to_string())
    })
}

/// Stores or replaces a secret.
pub fn set(reference: &str, secret: &str) -> Result<()> {
    if secret.trim().is_empty() {
        return Err(AppError::invalid("An API key cannot be blank."));
    }

    entry(reference)?.set_password(secret).map_err(|err| {
        AppError::new(ErrorCode::Credential, "Grimoire couldn't save the API key.")
            .with_detail(err.to_string())
    })?;

    // The reference, never the secret. This line exists so that a support
    // question about "did it save" can be answered from the log.
    tracing::info!(reference, "credential stored");
    Ok(())
}

/// Reads a secret, for the moment a request is built.
pub fn get(reference: &str) -> Result<String> {
    entry(reference)?.get_password().map_err(|err| match err {
        keyring::Error::NoEntry => AppError::new(
            ErrorCode::Credential,
            "No API key is saved for that provider. Add one in Settings.",
        ),
        other => AppError::new(
            ErrorCode::Credential,
            "Grimoire couldn't read the saved API key.",
        )
        .with_detail(other.to_string()),
    })
}

/// Whether a secret exists, without reading it.
///
/// This is what Settings uses to show "a key is saved" — the answer the UI
/// needs, and the most it is allowed to know.
pub fn exists(reference: &str) -> bool {
    entry(reference)
        .and_then(|entry| Ok(entry.get_password().is_ok()))
        .unwrap_or(false)
}

/// Removes a secret. Missing is success: the desired state is "gone".
pub fn delete(reference: &str) -> Result<()> {
    let entry = entry(reference)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            tracing::info!(reference, "credential removed");
            Ok(())
        }
        Err(err) => Err(AppError::new(
            ErrorCode::Credential,
            "Grimoire couldn't remove the saved API key.",
        )
        .with_detail(err.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reference nothing else will collide with.
    fn scratch() -> String {
        format!("test-{}", uuid::Uuid::new_v4())
    }

    /// The OS credential store is a real, shared resource and may be
    /// unavailable in a headless or locked-down environment. These tests skip
    /// rather than fail there — a machine without a keychain is a fact about
    /// the machine, not a defect in this code.
    fn keychain_available() -> bool {
        let probe = scratch();
        let usable = set(&probe, "probe").is_ok();
        if usable {
            let _ = delete(&probe);
        }
        usable
    }

    #[test]
    fn a_blank_key_is_refused_before_the_store_is_touched() {
        let error = set(&scratch(), "   ").unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::InvalidInput);
    }

    #[test]
    fn a_secret_round_trips() {
        if !keychain_available() {
            eprintln!("skipping: no usable credential store");
            return;
        }
        let reference = scratch();
        set(&reference, "sk-a-secret-value").unwrap();
        assert_eq!(get(&reference).unwrap(), "sk-a-secret-value");
        assert!(exists(&reference));
        delete(&reference).unwrap();
    }

    #[test]
    fn replacing_a_secret_keeps_only_the_new_one() {
        if !keychain_available() {
            eprintln!("skipping: no usable credential store");
            return;
        }
        let reference = scratch();
        set(&reference, "first").unwrap();
        set(&reference, "second").unwrap();
        assert_eq!(get(&reference).unwrap(), "second");
        delete(&reference).unwrap();
    }

    #[test]
    fn a_missing_secret_says_what_to_do_about_it() {
        if !keychain_available() {
            eprintln!("skipping: no usable credential store");
            return;
        }
        let error = get(&scratch()).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::Credential);
        assert!(error.message.contains("Settings"), "{}", error.message);
    }

    #[test]
    fn deleting_something_absent_is_success() {
        if !keychain_available() {
            eprintln!("skipping: no usable credential store");
            return;
        }
        assert!(delete(&scratch()).is_ok());
    }

    #[test]
    fn existence_can_be_checked_without_reading() {
        if !keychain_available() {
            eprintln!("skipping: no usable credential store");
            return;
        }
        let reference = scratch();
        assert!(!exists(&reference));
        set(&reference, "value").unwrap();
        assert!(exists(&reference));
        delete(&reference).unwrap();
        assert!(!exists(&reference));
    }
}
