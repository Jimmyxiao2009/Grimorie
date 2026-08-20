//! Schema migrations.
//!
//! Rules this module exists to guarantee:
//!
//! * Migrations run in order, each inside its own transaction. A migration that
//!   fails leaves the database exactly as it was.
//! * A migration that has already been applied is never re-run.
//! * A migration whose text has changed since it was applied is a *hard error*.
//!   Editing shipped SQL means the schema on disk no longer matches what the
//!   code believes, and continuing from there is how user data gets destroyed.
//! * Nothing here drops or rewrites user data. Adding a table or a column is
//!   fine; anything destructive must be written as an explicit, reviewed
//!   migration that backs up first.

use rusqlite::Connection;

use crate::domain::annotation::hash;
use crate::error::{AppError, ErrorCode, Result};

pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

/// Every migration Grimoire has ever shipped, in order.
///
/// Append only. Never edit an entry that has been released.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "manuscript",
    sql: include_str!("../../migrations/001_manuscript.sql"),
}];

#[derive(Debug, Default, PartialEq)]
pub struct MigrationReport {
    pub applied: Vec<i64>,
    pub version: i64,
}

/// Brings the database up to the current schema version.
pub fn migrate(conn: &mut Connection) -> Result<MigrationReport> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    INTEGER PRIMARY KEY NOT NULL,
            name       TEXT NOT NULL,
            checksum   TEXT NOT NULL,
            applied_at TEXT NOT NULL
        ) STRICT;",
    )?;

    verify_applied(conn)?;

    let mut report = MigrationReport::default();

    for migration in MIGRATIONS {
        let already: Option<i64> = conn
            .query_row(
                "SELECT version FROM schema_migrations WHERE version = ?1",
                [migration.version],
                |row| row.get(0),
            )
            .ok();
        if already.is_some() {
            continue;
        }

        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql).map_err(|err| {
            AppError::new(
                ErrorCode::Migration,
                "Grimoire couldn't update its library file to the current version. \
                 Nothing has been changed, and your manuscripts are untouched.",
            )
            .with_detail(format!(
                "migration {} ({}) failed: {err}",
                migration.version, migration.name
            ))
        })?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                migration.version,
                migration.name,
                hash(migration.sql),
                crate::domain::now().to_rfc3339(),
            ],
        )?;
        tx.commit()?;

        tracing::info!(
            version = migration.version,
            name = migration.name,
            "migration applied"
        );
        report.applied.push(migration.version);
    }

    report.version = current_version(conn)?;
    Ok(report)
}

/// Refuses to continue if a previously applied migration's text has changed.
fn verify_applied(conn: &Connection) -> Result<()> {
    let mut statement = conn.prepare("SELECT version, checksum FROM schema_migrations")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for (version, recorded) in rows {
        let Some(migration) = MIGRATIONS.iter().find(|m| m.version == version) else {
            // The database was written by a newer build. Older code cannot know
            // what that schema means, so it must not touch it.
            return Err(AppError::new(
                ErrorCode::Migration,
                "This library was created by a newer version of Grimoire. \
                 Update Grimoire to open it — your manuscripts are safe.",
            )
            .with_detail(format!("unknown migration version {version} on disk")));
        };

        if hash(migration.sql) != recorded {
            return Err(AppError::new(
                ErrorCode::Migration,
                "Grimoire's library file does not match this build and was not opened. \
                 Your manuscripts have not been changed.",
            )
            .with_detail(format!(
                "checksum mismatch for migration {version} ({})",
                migration.name
            )));
        }
    }

    Ok(())
}

pub fn current_version(conn: &Connection) -> Result<i64> {
    let version: Option<i64> =
        conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
    Ok(version.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Connection {
        Connection::open_in_memory().expect("in-memory database")
    }

    #[test]
    fn migrations_are_ordered_and_uniquely_numbered() {
        let mut previous = 0;
        for migration in MIGRATIONS {
            assert!(
                migration.version > previous,
                "migration {} is out of order",
                migration.version
            );
            previous = migration.version;
        }
    }

    #[test]
    fn no_migration_is_empty() {
        for migration in MIGRATIONS {
            assert!(
                !migration.sql.trim().is_empty(),
                "migration {} is empty",
                migration.version
            );
        }
    }

    #[test]
    fn migrating_a_fresh_database_applies_everything() {
        let mut conn = fresh();
        let report = migrate(&mut conn).unwrap();
        assert_eq!(report.applied.len(), MIGRATIONS.len());
        assert_eq!(report.version, MIGRATIONS.last().unwrap().version);
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let mut conn = fresh();
        migrate(&mut conn).unwrap();
        let second = migrate(&mut conn).unwrap();
        assert!(second.applied.is_empty(), "re-applied {:?}", second.applied);
    }

    #[test]
    fn a_tampered_migration_refuses_to_open_rather_than_guessing() {
        let mut conn = fresh();
        migrate(&mut conn).unwrap();

        // Simulate shipped SQL having been edited after release.
        conn.execute(
            "UPDATE schema_migrations SET checksum = 'different' WHERE version = 1",
            [],
        )
        .unwrap();

        let error = migrate(&mut conn).unwrap_err();
        assert_eq!(error.code, ErrorCode::Migration);
        assert!(
            error.message.contains("not been changed"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_database_from_a_newer_build_is_left_alone() {
        let mut conn = fresh();
        migrate(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (version, name, checksum, applied_at)
             VALUES (9999, 'from the future', 'x', '2099-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        let error = migrate(&mut conn).unwrap_err();
        assert_eq!(error.code, ErrorCode::Migration);
        assert!(error.message.contains("newer version"), "{}", error.message);
    }

    #[test]
    fn a_failing_migration_leaves_no_partial_schema() {
        let mut conn = fresh();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY NOT NULL, name TEXT NOT NULL,
                checksum TEXT NOT NULL, applied_at TEXT NOT NULL) STRICT;",
        )
        .unwrap();

        let tx = conn.transaction().unwrap();
        let outcome = tx.execute_batch(
            "CREATE TABLE good (id TEXT PRIMARY KEY); CREATE TABLE bad (this is not sql);",
        );
        assert!(outcome.is_err());
        drop(tx); // rolls back

        let exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='good'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 0, "a failed migration left a table behind");
    }

    #[test]
    fn the_schema_has_the_tables_the_app_depends_on() {
        let mut conn = fresh();
        migrate(&mut conn).unwrap();
        for table in ["volumes", "chapters", "pages", "settings"] {
            let count: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing table {table}");
        }
    }
}
