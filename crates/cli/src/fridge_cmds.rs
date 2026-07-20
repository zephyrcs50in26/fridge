use crate::repl::{Flow, Repl};
use crate::root_cmds::is_id;
use anyhow::{bail, Result};
use fridge_core::dates::{self, NEVER};
use fridge_core::model::{Lot, Reason, UnitKind};
use fridge_core::{names, units};
use fridge_store::store;

impl Repl {
    // -- list ---------------------------------------------------------------

    pub fn cmd_list(&mut self) -> Result<Flow> {
        let fid = self.fridge().id;
        let lots = store::all_lots(&self.conn, fid)?;
        if lots.is_empty() {
            println!("(empty)");
            return Ok(Flow::Continue);
        }
        print_lot_table(&lots);
        Ok(Flow::Continue)
    }

    // -- add ----------------------------------------------------------------

    pub fn cmd_add(
        &mut self,
        raw_name: &str,
        raw_amount: &str,
        use_by: Option<&str>,
        no_expiry: bool,
    ) -> Result<Flow> {
        let fid = self.fridge().id;
        let today = self.fridge().today;
        let today_iso = dates::to_storage(today);

        let name = names::validate_product(raw_name)?;
        let amount = units::parse_amount(raw_amount)?;
        let use_by_iso = resolve_use_by(use_by, no_expiry, today)?;

        // Merge onto a twin if the identity tuple already exists.
        if let Some(existing) =
            store::find_identity(&self.conn, fid, &name, amount.kind, &use_by_iso, None)?
        {
            let merged = existing.unit_amount + amount.base;
            let added = min_iso(&existing.added_at, &today_iso);
            self.conn.execute(
                "UPDATE lots SET unit_amount = ?1, added_at = ?2 WHERE id = ?3",
                (merged, &added, existing.id),
            )?;
            println!(
                "Merged into lot {}: {} {}, use by {}.",
                existing.id,
                name,
                units::display(amount.kind, merged),
                dates::display(&use_by_iso),
            );
        } else {
            self.conn.execute(
                "INSERT INTO lots (fridge_id, name, unit_kind, unit_amount, use_by, added_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                (
                    fid,
                    &name,
                    amount.kind.as_str(),
                    amount.base,
                    &use_by_iso,
                    &today_iso,
                ),
            )?;
            let id = self.conn.last_insert_rowid();
            println!(
                "Added lot {}: {} {}, use by {}.",
                id,
                name,
                units::display(amount.kind, amount.base),
                dates::display(&use_by_iso),
            );
        }
        Ok(Flow::Continue)
    }

    // -- take (FEFO) --------------------------------------------------------

    pub fn cmd_take(&mut self, target: &str, raw_amount: &str) -> Result<Flow> {
        let fid = self.fridge().id;
        let amount = units::parse_amount(raw_amount)?;

        // Collect the lots this take draws from, first-expire-first-out.
        let (display_name, mut lots) = if is_id(target) {
            let id: i64 = target.parse()?;
            let lot = store::load_lot_by_id(&self.conn, fid, id)?
                .ok_or_else(|| anyhow::anyhow!("lot {id} not found."))?;
            (lot.name.clone(), vec![lot])
        } else {
            let name = names::validate_product(target)?;
            let lots = store::lots_by_name_kind(&self.conn, fid, &name, amount.kind)?;
            if lots.is_empty() {
                bail!("no {name} to take");
            }
            (lots[0].name.clone(), lots)
        };

        if let Some(bad) = lots.iter().find(|l| l.unit_kind != amount.kind) {
            bail!(
                "{} is measured in {}, not {}",
                bad.name,
                bad.unit_kind,
                amount.kind
            );
        }

        let total: i64 = lots.iter().map(|l| l.unit_amount).sum();
        if total < amount.base {
            bail!(
                "only {} of {} available",
                units::display(amount.kind, total),
                display_name
            );
        }

        let mut remaining = amount.base;
        let tx = self.conn.transaction()?;
        for lot in lots.iter_mut() {
            if remaining == 0 {
                break;
            }
            if lot.unit_amount <= remaining {
                // consumed in full — vanishes silently, it is not trashed
                remaining -= lot.unit_amount;
                tx.execute("DELETE FROM lots WHERE id = ?1", [lot.id])?;
            } else {
                let left = lot.unit_amount - remaining;
                remaining = 0;
                tx.execute(
                    "UPDATE lots SET unit_amount = ?1 WHERE id = ?2",
                    (left, lot.id),
                )?;
            }
        }
        tx.commit()?;

        println!(
            "Took {} of {}.",
            units::display(amount.kind, amount.base),
            display_name
        );
        Ok(Flow::Continue)
    }

    // -- edit ---------------------------------------------------------------

    pub fn cmd_edit(
        &mut self,
        target: &str,
        new_name: Option<&str>,
        new_amount: Option<&str>,
        use_by: Option<&str>,
        no_expiry: bool,
        merge: bool,
    ) -> Result<Flow> {
        let fid = self.fridge().id;
        let today = self.fridge().today;

        let lot = self.resolve_lot(target)?;
        let kind = lot.unit_kind;

        let name = match new_name {
            Some(n) => names::validate_product(n)?,
            None => lot.name.clone(),
        };
        let amount_base = match new_amount {
            Some(a) => {
                let parsed = units::parse_amount(a)?;
                if parsed.kind != kind {
                    bail!("cannot change the unit kind of a lot; take it out and add it back");
                }
                parsed.base
            }
            None => lot.unit_amount,
        };
        let use_by_iso = match (use_by, no_expiry) {
            (Some(_), true) => bail!("choose one of --use-by / --no-expiry"),
            (_, true) => NEVER.to_string(),
            (Some(s), false) => resolve_use_by(Some(s), false, today)?,
            (None, false) => lot.use_by.clone(),
        };

        let unchanged =
            name == lot.name && amount_base == lot.unit_amount && use_by_iso == lot.use_by;
        if unchanged {
            println!("Nothing to change.");
            return Ok(Flow::Continue);
        }

        // Would this collide with a twin holding the new identity?
        if let Some(twin) =
            store::find_identity(&self.conn, fid, &name, kind, &use_by_iso, Some(lot.id))?
        {
            if !merge {
                bail!(
                    "lot {} already holds {}, use by {}.\n\
                     \x20      Merging is permanent and cannot be undone.\n\
                     \x20      Re-run with --merge to combine them.",
                    twin.id,
                    name,
                    dates::display(&use_by_iso),
                );
            }
            let merged = twin.unit_amount + amount_base;
            let added = min_iso(&twin.added_at, &lot.added_at);
            let tx = self.conn.transaction()?;
            tx.execute(
                "UPDATE lots SET unit_amount = ?1, added_at = ?2 WHERE id = ?3",
                (merged, &added, twin.id),
            )?;
            tx.execute("DELETE FROM lots WHERE id = ?1", [lot.id])?;
            tx.commit()?;
            println!(
                "Merged into lot {}: {} {}, use by {}.",
                twin.id,
                name,
                units::display(kind, merged),
                dates::display(&use_by_iso),
            );
        } else {
            self.conn.execute(
                "UPDATE lots SET name = ?1, unit_amount = ?2, use_by = ?3 WHERE id = ?4",
                (&name, amount_base, &use_by_iso, lot.id),
            )?;
            println!(
                "Updated lot {}: {} {}, use by {}.",
                lot.id,
                name,
                units::display(kind, amount_base),
                dates::display(&use_by_iso),
            );
        }
        Ok(Flow::Continue)
    }

    // -- rm (discard whole lot to the bin) ----------------------------------

    pub fn cmd_rm(&mut self, target: &str) -> Result<Flow> {
        let fid = self.fridge().id;
        let today_iso = dates::to_storage(self.fridge().today);
        let lot = self.resolve_lot(target)?;

        let tx = self.conn.transaction()?;
        store::bin_snapshot(
            &tx,
            fid,
            &lot.name,
            lot.unit_kind,
            lot.unit_amount,
            &lot.use_by,
            &today_iso,
            Reason::Discarded,
        )?;
        tx.execute("DELETE FROM lots WHERE id = ?1", [lot.id])?;
        tx.commit()?;

        println!(
            "Discarded {} {}, use by {}.",
            lot.name,
            units::display(lot.unit_kind, lot.unit_amount),
            dates::display(&lot.use_by),
        );
        Ok(Flow::Continue)
    }

    // -- bin ----------------------------------------------------------------

    pub fn cmd_bin_list(&mut self) -> Result<Flow> {
        let fid = self.fridge().id;
        let mut stmt = self.conn.prepare(
            "SELECT name, unit_kind, unit_amount, use_by, trashed_at, reason \
             FROM bin WHERE fridge_id = ?1 ORDER BY trashed_at, id",
        )?;
        let rows: Vec<(String, String, i64, String, String, String)> = stmt
            .query_map([fid], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        if rows.is_empty() {
            println!("(bin is empty)");
            return Ok(Flow::Continue);
        }
        for (name, kind_str, amount, use_by, trashed_at, reason) in rows {
            let kind = UnitKind::parse(&kind_str)?;
            println!(
                "  {name} {}, use by {} — {} {}",
                units::display(kind, amount),
                dates::display(&use_by),
                reason.to_lowercase(),
                dates::display(&trashed_at),
            );
        }
        Ok(Flow::Continue)
    }

    pub fn cmd_bin_clear(&mut self) -> Result<Flow> {
        let fid = self.fridge().id;
        let n = self
            .conn
            .execute("DELETE FROM bin WHERE fridge_id = ?1", [fid])?;
        println!("Emptied the bin ({n} removed).");
        Ok(Flow::Continue)
    }

    // -- addressing ---------------------------------------------------------

    /// Resolve a single lot by id or name. On multiple name matches, prompt
    /// with the real ids (one numbering scheme for the whole app).
    fn resolve_lot(&mut self, target: &str) -> Result<Lot> {
        let fid = self.fridge().id;
        if is_id(target) {
            let id: i64 = target.parse()?;
            return store::load_lot_by_id(&self.conn, fid, id)?
                .ok_or_else(|| anyhow::anyhow!("lot {id} not found."));
        }
        let name = names::validate_product(target)?;
        let mut lots = store::lots_by_name(&self.conn, fid, &name)?;
        match lots.len() {
            0 => bail!("no lot named {name}"),
            1 => Ok(lots.pop().unwrap()),
            _ => {
                println!("Several lots named {name}:");
                for l in &lots {
                    println!(
                        "  {}  {} {}, use by {}",
                        l.id,
                        l.name,
                        units::display(l.unit_kind, l.unit_amount),
                        dates::display(&l.use_by),
                    );
                }
                let answer = self
                    .next_line("Which lot id? ")?
                    .ok_or_else(|| anyhow::anyhow!("no selection"))?;
                let chosen: i64 = answer
                    .trim()
                    .parse()
                    .map_err(|_| anyhow::anyhow!("{answer:?} is not an id"))?;
                lots.into_iter()
                    .find(|l| l.id == chosen)
                    .ok_or_else(|| anyhow::anyhow!("lot {chosen} is not one of those"))
            }
        }
    }
}

/// Turn `--use-by`/`--no-expiry` into the stored ISO string, enforcing that a
/// real date lies strictly after today (you cannot stock already-expired food).
fn resolve_use_by(use_by: Option<&str>, no_expiry: bool, today: jiff::civil::Date) -> Result<String> {
    match (use_by, no_expiry) {
        (Some(_), true) => bail!("choose one of --use-by / --no-expiry"),
        (None, false) => bail!("give --use-by <date> or --no-expiry"),
        (_, true) => Ok(NEVER.to_string()),
        (Some(s), false) => {
            let d = dates::parse(s)?;
            if d <= today {
                bail!(
                    "use-by must be after today ({})",
                    dates::to_storage(today)
                );
            }
            Ok(dates::to_storage(d))
        }
    }
}

/// Minimum of two ISO date strings — lexicographic order is chronological.
fn min_iso(a: &str, b: &str) -> String {
    if a <= b { a.to_string() } else { b.to_string() }
}

fn print_lot_table(lots: &[Lot]) {
    let rows: Vec<(String, String, String)> = lots
        .iter()
        .map(|l| {
            (
                l.id.to_string(),
                format!("{}  {}", l.name, units::display(l.unit_kind, l.unit_amount)),
                dates::display(&l.use_by),
            )
        })
        .collect();

    let id_w = rows.iter().map(|r| r.0.len()).max().unwrap_or(2).max(2);
    let item_w = rows
        .iter()
        .map(|r| r.1.len())
        .max()
        .unwrap_or(4)
        .max("item".len());

    println!("  {:>id_w$}  {:<item_w$}  use by", "id", "item");
    for (id, item, use_by) in rows {
        println!("  {id:>id_w$}  {item:<item_w$}  {use_by}");
    }
}
