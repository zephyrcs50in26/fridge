use fridge_core::model::{Lot, Reason, UnitKind};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Row};

fn row_to_lot(row: &Row) -> rusqlite::Result<Lot> {
    let kind_str: String = row.get(2)?;
    Ok(Lot {
        id: row.get(0)?,
        name: row.get(1)?,
        unit_kind: UnitKind::parse(&kind_str)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
        unit_amount: row.get(3)?,
        use_by: row.get(4)?,
        added_at: row.get(5)?,
    })
}

const LOT_COLS: &str = "id, name, unit_kind, unit_amount, use_by, added_at";

pub fn load_lot_by_id(conn: &Connection, fridge_id: i64, id: i64) -> Result<Option<Lot>> {
    let sql = format!("SELECT {LOT_COLS} FROM lots WHERE fridge_id = ?1 AND id = ?2");
    let lot = conn
        .query_row(&sql, (fridge_id, id), row_to_lot)
        .optional()?;
    Ok(lot)
}

pub fn lots_by_name(conn: &Connection, fridge_id: i64, name: &str) -> Result<Vec<Lot>> {
    let sql =
        format!("SELECT {LOT_COLS} FROM lots WHERE fridge_id = ?1 AND name = ?2 ORDER BY use_by, id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map((fridge_id, name), row_to_lot)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn lots_by_name_kind(
    conn: &Connection,
    fridge_id: i64,
    name: &str,
    kind: UnitKind,
) -> Result<Vec<Lot>> {
    let sql = format!(
        "SELECT {LOT_COLS} FROM lots \
         WHERE fridge_id = ?1 AND name = ?2 AND unit_kind = ?3 \
         ORDER BY use_by, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map((fridge_id, name, kind.as_str()), row_to_lot)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn all_lots(conn: &Connection, fridge_id: i64) -> Result<Vec<Lot>> {
    let sql = format!(
        "SELECT {LOT_COLS} FROM lots WHERE fridge_id = ?1 ORDER BY use_by, name COLLATE NOCASE, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([fridge_id], row_to_lot)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The twin (if any) occupying a given identity tuple, ignoring `except_id`.
pub fn find_identity(
    conn: &Connection,
    fridge_id: i64,
    name: &str,
    kind: UnitKind,
    use_by: &str,
    except_id: Option<i64>,
) -> Result<Option<Lot>> {
    let sql = format!(
        "SELECT {LOT_COLS} FROM lots \
         WHERE fridge_id = ?1 AND name = ?2 AND unit_kind = ?3 AND use_by = ?4 AND id != ?5"
    );
    let except = except_id.unwrap_or(-1);
    let lot = conn
        .query_row(&sql, (fridge_id, name, kind.as_str(), use_by, except), row_to_lot)
        .optional()?;
    Ok(lot)
}

/// Snapshot a lot's fields into the bin. The bin holds a post-mortem copy, not
/// a reference — this is why there is no restore.
pub fn bin_snapshot(
    conn: &Connection,
    fridge_id: i64,
    name: &str,
    kind: UnitKind,
    unit_amount: i64,
    use_by: &str,
    trashed_at: &str,
    reason: Reason,
) -> Result<()> {
    conn.execute(
        "INSERT INTO bin (fridge_id, name, unit_kind, unit_amount, use_by, trashed_at, reason) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        (
            fridge_id,
            name,
            kind.as_str(),
            unit_amount,
            use_by,
            trashed_at,
            reason.as_str(),
        ),
    )?;
    Ok(())
}
