//! Settings persistence.
//!
//! Stored as JSON under a single key. That keeps adding a preference free of
//! schema changes, and it means a partially-understood row from another build
//! still loads — `AppSettings` supplies defaults for anything absent.

use rusqlite::{Connection, params};

use crate::domain::manuscript::now;
use crate::domain::settings::AppSettings;
use crate::error::Result;

const KEY: &str = "app";

pub fn load(conn: &Connection) -> Result<AppSettings> {
    let stored: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [KEY], |row| {
            row.get(0)
        })
        .ok();

    let settings = match stored {
        Some(raw) => serde_json::from_str::<AppSettings>(&raw).unwrap_or_else(|err| {
            // A corrupted settings row must not stop someone reaching their
            // manuscripts. Defaults are always usable.
            tracing::warn!(error = %err, "settings could not be read; using defaults");
            AppSettings::default()
        }),
        None => AppSettings::default(),
    };

    Ok(settings.sanitised())
}

pub fn save(conn: &Connection, settings: &AppSettings) -> Result<AppSettings> {
    let sanitised = settings.clone().sanitised();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![KEY, serde_json::to_string(&sanitised)?, now()],
    )?;
    Ok(sanitised)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testing::TempDatabase;

    #[test]
    fn an_untouched_install_gets_defaults() {
        let db = TempDatabase::open();
        assert_eq!(load(&db.get().unwrap()).unwrap(), AppSettings::default());
    }

    #[test]
    fn settings_round_trip() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();

        let settings = AppSettings {
            theme: "night".into(),
            manuscript_measure: 42.0,
            ..Default::default()
        };
        save(&conn, &settings).unwrap();

        let loaded = load(&conn).unwrap();
        assert_eq!(loaded.theme, "night");
        assert_eq!(loaded.manuscript_measure, 42.0);
    }

    #[test]
    fn saving_twice_updates_rather_than_duplicating() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();

        save(
            &conn,
            &AppSettings {
                theme: "dark".into(),
                ..Default::default()
            },
        )
        .unwrap();
        save(
            &conn,
            &AppSettings {
                theme: "paper".into(),
                ..Default::default()
            },
        )
        .unwrap();

        let rows: i64 = conn
            .query_row("SELECT count(*) FROM settings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(load(&conn).unwrap().theme, "paper");
    }

    #[test]
    fn out_of_range_values_are_clamped_on_the_way_in() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();

        let saved = save(
            &conn,
            &AppSettings {
                manuscript_size: 99.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(saved.manuscript_size <= 1.75);
        assert!(load(&conn).unwrap().manuscript_size <= 1.75);
    }

    #[test]
    fn a_corrupted_row_does_not_lock_the_user_out_of_their_library() {
        let db = TempDatabase::open();
        let conn = db.get().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, '{{{ broken', 'now')",
            [KEY],
        )
        .unwrap();

        assert_eq!(load(&conn).unwrap(), AppSettings::default());
    }
}
