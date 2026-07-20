use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::PathBuf;

/// Open (creating if needed) the fridge database and apply the schema.
///
/// `PRAGMA foreign_keys = ON` is set on every connection — it is OFF by
/// default in SQLite, and `fridge rm` would silently orphan rows without it.
pub fn open() -> Result<Connection> {
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating data dir {}", parent.display()))?;
    }
    let conn = Connection::open(&path)
        .with_context(|| format!("opening database {}", path.display()))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(include_str!("schema.sql"))
        .context("applying schema")?;
    Ok(conn)
}

/// Where the database lives. `FRIDGE_DB` overrides the platform data dir,
/// which is convenient for tests and scripted sessions.
fn db_path() -> Result<PathBuf> {
    if let Ok(env) = std::env::var("FRIDGE_DB") {
        return Ok(PathBuf::from(env));
    }
    let dirs = directories::ProjectDirs::from("", "", "fridge")
        .context("could not determine a data directory")?;
    Ok(dirs.data_dir().join("fridge.sqlite"))
}
