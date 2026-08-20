//! Repositories — the only place SQL is written.
//!
//! Each function takes a `&Connection`, which a `Transaction` derefs to, so the
//! same code composes into a larger transaction or runs standalone. Callers
//! that mutate more than one row are expected to wrap the call in
//! [`crate::database::Database::transaction`].

pub mod chapters;
pub mod pages;
pub mod settings;
pub mod volumes;

use rusqlite::Connection;

use crate::error::Result;

/// Rewrites a sibling group's positions to 0..n-1 in their current order.
///
/// Called after every structural change. Gaps and duplicates are not merely
/// untidy — reordering by drag depends on positions being dense and unique, and
/// a duplicate makes the resulting order depend on SQLite's tiebreak, which is
/// not something a user should be able to observe.
fn normalise_positions(
    conn: &Connection,
    table: &str,
    parent_column: &str,
    parent_id: &str,
) -> Result<()> {
    // The table and column names are compile-time constants from this module,
    // never user input; the parent id is bound as a parameter.
    let sql = format!(
        "WITH ordered AS (
             SELECT id, ROW_NUMBER() OVER (ORDER BY position, created_at, id) - 1 AS seat
             FROM {table} WHERE {parent_column} = ?1
         )
         UPDATE {table} SET position = (SELECT seat FROM ordered WHERE ordered.id = {table}.id)
         WHERE {parent_column} = ?1"
    );
    conn.execute(&sql, [parent_id])?;
    Ok(())
}

/// The position a newly created sibling should take: the end of the list.
fn next_position(
    conn: &Connection,
    table: &str,
    parent_column: &str,
    parent_id: &str,
) -> Result<i64> {
    let sql =
        format!("SELECT COALESCE(MAX(position) + 1, 0) FROM {table} WHERE {parent_column} = ?1");
    Ok(conn.query_row(&sql, [parent_id], |row| row.get(0))?)
}

/// Applies an explicit order to a sibling group.
///
/// Ids not in `ordered` keep their relative order and follow the listed ones,
/// so a reorder computed against a slightly stale tree cannot drop a Page.
fn apply_order(
    conn: &Connection,
    table: &str,
    parent_column: &str,
    parent_id: &str,
    ordered: &[String],
) -> Result<()> {
    let sql = format!("UPDATE {table} SET position = ?1 WHERE id = ?2 AND {parent_column} = ?3");
    for (seat, id) in ordered.iter().enumerate() {
        // Listed items take the low seats; anything unlisted sorts after them
        // because normalisation preserves relative order.
        conn.execute(&sql, rusqlite::params![seat as i64, id, parent_id])?;
    }
    let bump = format!(
        "UPDATE {table} SET position = position + ?1
         WHERE {parent_column} = ?2 AND id NOT IN (SELECT value FROM json_each(?3))"
    );
    let listed = serde_json::to_string(ordered)?;
    conn.execute(
        &bump,
        rusqlite::params![ordered.len() as i64, parent_id, listed],
    )?;

    normalise_positions(conn, table, parent_column, parent_id)
}
