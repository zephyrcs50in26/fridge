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

/// Where the database lives. `FRIDGE_DB` overrides it; otherwise the default is
/// a `.tmp/` folder in the current working directory, so a run from the project
/// keeps its data inside the project (and out of git — see `.gitignore`).
fn db_path() -> Result<PathBuf> {
    if let Ok(env) = std::env::var("FRIDGE_DB") {
        return Ok(PathBuf::from(env));
    }
    Ok(PathBuf::from(".tmp").join("fridge.sqlite"))
}
