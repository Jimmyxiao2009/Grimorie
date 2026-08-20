//! Database access.
//!
//! One pool, opened once at startup, owned by the Rust side. Nothing above this
//! module writes SQL, and nothing below it knows what a Volume is.

pub mod migrations;

use std::path::{Path, PathBuf};

use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

use crate::error::Result;

pub type Conn = PooledConnection<SqliteConnectionManager>;

/// SQLite permits one writer at a time, so a large pool buys nothing but
/// contention. Four leaves room for concurrent reads while a save is in flight.
const POOL_SIZE: u32 = 4;

/// How long a statement waits for a competing write before giving up. Long
/// enough to absorb an autosave landing during a search, short enough that a
/// genuine deadlock surfaces rather than hanging the app.
const BUSY_TIMEOUT_MS: u32 = 5_000;

#[derive(Clone)]
pub struct Database {
    pool: Pool<SqliteConnectionManager>,
    path: PathBuf,
}

impl Database {
    /// Opens the library file, creating and migrating it if necessary.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let manager = SqliteConnectionManager::file(&path).with_init(configure);
        let pool = Pool::builder().max_size(POOL_SIZE).build(manager)?;

        let mut conn = pool.get()?;
        let report = migrations::migrate(&mut conn)?;
        if !report.applied.is_empty() {
            tracing::info!(
                applied = ?report.applied,
                version = report.version,
                "database schema updated"
            );
        }
        ensure_search_index(&conn)?;
        drop(conn);

        Ok(Self { pool, path })
    }

    pub fn get(&self) -> Result<Conn> {
        Ok(self.pool.get()?)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `work` inside a transaction, committing on success and rolling back
    /// on any error. Every multi-statement mutation goes through here.
    pub fn transaction<T>(
        &self,
        work: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let mut conn = self.get()?;
        let tx = conn.transaction()?;
        let value = work(&tx)?;
        tx.commit()?;
        Ok(value)
    }
}

/// Builds the search index if the library has content but the index does not.
///
/// This covers two cases with one check, and deliberately does not test for a
/// particular migration version:
///
/// * A library that predates search. Adding the table does not fill it, and
///   without this a writer would upgrade and find that searching their own
///   manuscript returned nothing until they re-saved every Page.
/// * An index lost or emptied for any other reason.
///
/// The condition is "pages exist and the index is empty", so it cannot loop on
/// a genuinely empty library and costs two counts at startup otherwise.
fn ensure_search_index(conn: &rusqlite::Connection) -> Result<()> {
    let pages: i64 = conn.query_row("SELECT count(*) FROM pages", [], |row| row.get(0))?;
    if pages == 0 {
        return Ok(());
    }

    let indexed: i64 = conn.query_row("SELECT count(*) FROM search_rows", [], |row| row.get(0))?;
    if indexed > 0 {
        return Ok(());
    }

    tracing::info!(
        pages,
        "search index is empty; building it from the manuscript"
    );
    crate::search::rebuild(conn)?;
    Ok(())
}

/// Per-connection pragmas.
fn configure(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        -- Write-ahead logging: readers never block the writer, which is what
        -- keeps a search or a tree redraw from stalling behind an autosave.
        PRAGMA journal_mode = WAL;

        -- With WAL, NORMAL is durable against application crashes and only
        -- risks the most recent transactions on sudden power loss. Grimoire
        -- keeps a separate draft journal for exactly that case, and FULL would
        -- put an fsync in the autosave path on tablet-class storage.
        PRAGMA synchronous = NORMAL;

        -- Cascades are declared in the schema and must actually fire; SQLite
        -- has them off by default, per connection.
        PRAGMA foreign_keys = ON;

        PRAGMA temp_store = MEMORY;

        -- Negative means KiB rather than pages: a 16 MiB page cache, which is
        -- generous for a manuscript and modest for a tablet.
        PRAGMA cache_size = -16000;
        ",
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS.into()))?;
    Ok(())
}

#[cfg(test)]
pub mod testing {
    use super::Database;

    /// A real on-disk database in a temporary directory.
    ///
    /// Tests use a file rather than `:memory:` deliberately: WAL, busy
    /// timeouts, and foreign-key cascades all behave differently in memory, and
    /// those are precisely the behaviours worth testing.
    pub struct TempDatabase {
        pub db: Database,
        _dir: tempfile::TempDir,
    }

    impl TempDatabase {
        pub fn open() -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            let db = Database::open(dir.path().join("grimoire.db")).expect("open database");
            Self { db, _dir: dir }
        }
    }

    impl std::ops::Deref for TempDatabase {
        type Target = Database;
        fn deref(&self) -> &Database {
            &self.db
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::TempDatabase;

    #[test]
    fn opening_creates_and_migrates_the_file() {
        let db = TempDatabase::open();
        assert!(db.path().exists());
        let conn = db.get().unwrap();
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(version >= 1);
    }

    #[test]
    fn reopening_an_existing_file_keeps_its_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grimoire.db");

        let first = super::Database::open(&path).unwrap();
        first
            .get()
            .unwrap()
            .execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('probe', '1', 'now')",
                [],
            )
            .unwrap();
        drop(first);

        let second = super::Database::open(&path).unwrap();
        let value: String = second
            .get()
            .unwrap()
            .query_row(
                "SELECT value FROM settings WHERE key = 'probe'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, "1");
    }

    #[test]
    fn write_ahead_logging_is_on() {
        let db = TempDatabase::open();
        let mode: String = db
            .get()
            .unwrap()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn foreign_keys_are_enforced_on_every_pooled_connection() {
        let db = TempDatabase::open();
        // Exercise more connections than one, since the pragma is per
        // connection and a missed init would only show up on a later checkout.
        for _ in 0..(super::POOL_SIZE + 1) {
            let conn = db.get().unwrap();
            let on: i64 = conn
                .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
                .unwrap();
            assert_eq!(on, 1);
        }
    }

    #[test]
    fn a_failed_transaction_rolls_everything_back() {
        let db = TempDatabase::open();
        let outcome = db.transaction(|tx| {
            tx.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('a', '1', 'now')",
                [],
            )?;
            Err::<(), _>(crate::error::AppError::invalid("deliberate failure"))
        });
        assert!(outcome.is_err());

        let count: i64 = db
            .get()
            .unwrap()
            .query_row("SELECT count(*) FROM settings WHERE key = 'a'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "a rolled-back transaction left a row behind");
    }

    #[test]
    fn a_successful_transaction_commits() {
        let db = TempDatabase::open();
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('b', '2', 'now')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let value: String = db
            .get()
            .unwrap()
            .query_row("SELECT value FROM settings WHERE key = 'b'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(value, "2");
    }
}
