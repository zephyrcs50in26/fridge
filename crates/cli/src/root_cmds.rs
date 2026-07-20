use crate::repl::{FridgeCmd, FridgeCtx, Flow, Repl, Scope, UserCmd, UserCtx};
use anyhow::{bail, Result};
use fridge_auth as auth;
use fridge_core::model::Reason;
use fridge_core::{dates, names, units};
use fridge_store::store;
use rusqlite::OptionalExtension;

impl Repl {
    fn require_user(&self) -> Result<&UserCtx> {
        self.user
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("log in first: user use <name>"))
    }

    // -- users --------------------------------------------------------------

    pub fn cmd_user(&mut self, action: UserCmd) -> Result<Flow> {
        match action {
            UserCmd::Create { name } => self.user_create(&name),
            UserCmd::List => self.user_list(),
            UserCmd::Use { name } => self.user_use(&name),
        }
    }

    fn user_create(&mut self, raw: &str) -> Result<Flow> {
        let name = names::validate_account(raw)?;
        let exists: Option<i64> = self
            .conn
            .query_row("SELECT id FROM users WHERE name = ?1", [&name], |r| r.get(0))
            .optional()?;
        if exists.is_some() {
            bail!("a profile named {name} already exists");
        }
        let pw = self.read_secret("Password: ")?;
        if pw.is_empty() {
            bail!("password must not be empty");
        }
        let confirm = self.read_secret("Confirm password: ")?;
        if pw != confirm {
            bail!("passwords did not match");
        }
        let hash = auth::hash(&pw)?;
        self.conn.execute(
            "INSERT INTO users (name, password_hash) VALUES (?1, ?2)",
            (&name, &hash),
        )?;
        let id = self.conn.last_insert_rowid();
        self.user = Some(UserCtx { id });
        self.scope = Scope::Root;
        println!("Created profile {name}. You are logged in.");
        Ok(Flow::Continue)
    }

    fn user_list(&mut self) -> Result<Flow> {
        let mut stmt = self.conn.prepare("SELECT name FROM users ORDER BY name")?;
        let names: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        if names.is_empty() {
            println!("No profiles yet.");
        } else {
            for n in names {
                println!("  {n}");
            }
        }
        Ok(Flow::Continue)
    }

    fn user_use(&mut self, raw: &str) -> Result<Flow> {
        let name = names::validate_account(raw)?;
        let row: Option<(i64, String, String)> = self
            .conn
            .query_row(
                "SELECT id, name, password_hash FROM users WHERE name = ?1",
                [&name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (id, real_name, hash) = match row {
            Some(v) => v,
            None => bail!("no profile named {name}"),
        };
        let pw = self.read_secret("Password: ")?;
        if !auth::verify(&pw, &hash) {
            bail!("wrong password");
        }
        self.user = Some(UserCtx { id });
        self.scope = Scope::Root;
        println!("Logged in as {real_name}.");
        Ok(Flow::Continue)
    }

    // -- fridges ------------------------------------------------------------

    pub fn cmd_fridge(&mut self, action: FridgeCmd) -> Result<Flow> {
        match action {
            FridgeCmd::Create { name } => self.fridge_create(&name),
            FridgeCmd::Rm { name } => self.fridge_rm(&name),
            FridgeCmd::Rename { old, new } => self.fridge_rename(&old, &new),
            FridgeCmd::List => self.fridge_list(),
        }
    }

    /// Resolve a fridge belonging to the current user by name or id.
    fn resolve_fridge(&self, user_id: i64, target: &str) -> Result<(i64, String)> {
        if is_id(target) {
            let id: i64 = target.parse()?;
            let row: Option<String> = self
                .conn
                .query_row(
                    "SELECT name FROM fridges WHERE user_id = ?1 AND id = ?2",
                    (user_id, id),
                    |r| r.get(0),
                )
                .optional()?;
            match row {
                Some(name) => Ok((id, name)),
                None => bail!("fridge {id} not found"),
            }
        } else {
            let name = names::validate_account(target)?;
            let row: Option<(i64, String)> = self
                .conn
                .query_row(
                    "SELECT id, name FROM fridges WHERE user_id = ?1 AND name = ?2",
                    (user_id, &name),
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match row {
                Some(v) => Ok(v),
                None => bail!("no fridge named {name}"),
            }
        }
    }

    fn fridge_create(&mut self, raw: &str) -> Result<Flow> {
        let user_id = self.require_user()?.id;
        let name = names::validate_account(raw)?;
        let dup: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM fridges WHERE user_id = ?1 AND name = ?2",
                (user_id, &name),
                |r| r.get(0),
            )
            .optional()?;
        if dup.is_some() {
            bail!("you already have a fridge named {name}");
        }
        self.conn.execute(
            "INSERT INTO fridges (user_id, name) VALUES (?1, ?2)",
            (user_id, &name),
        )?;
        println!("Created fridge {name}.");
        Ok(Flow::Continue)
    }

    fn fridge_rm(&mut self, raw: &str) -> Result<Flow> {
        let user_id = self.require_user()?.id;
        let (id, name) = self.resolve_fridge(user_id, raw)?;
        self.conn
            .execute("DELETE FROM fridges WHERE id = ?1", [id])?;
        println!("Deleted fridge {name} and everything in it.");
        Ok(Flow::Continue)
    }

    fn fridge_rename(&mut self, old: &str, new: &str) -> Result<Flow> {
        let user_id = self.require_user()?.id;
        let (id, _) = self.resolve_fridge(user_id, old)?;
        let new_name = names::validate_account(new)?;
        let dup: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM fridges WHERE user_id = ?1 AND name = ?2 AND id != ?3",
                (user_id, &new_name, id),
                |r| r.get(0),
            )
            .optional()?;
        if dup.is_some() {
            bail!("you already have a fridge named {new_name}");
        }
        self.conn
            .execute("UPDATE fridges SET name = ?1 WHERE id = ?2", (&new_name, id))?;
        println!("Renamed to {new_name}.");
        Ok(Flow::Continue)
    }

    fn fridge_list(&mut self) -> Result<Flow> {
        let user_id = self.require_user()?.id;
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM fridges WHERE user_id = ?1 ORDER BY name")?;
        let names: Vec<String> = stmt
            .query_map([user_id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        if names.is_empty() {
            println!("No fridges yet. Create one: fridge create <name>");
        } else {
            for n in names {
                println!("  {n}");
            }
        }
        Ok(Flow::Continue)
    }

    // -- open (the only place the sweep runs) -------------------------------

    pub fn cmd_open(&mut self, raw: &str) -> Result<Flow> {
        let user_id = self.require_user()?.id;
        let (fridge_id, fridge_name) = self.resolve_fridge(user_id, raw)?;
        let today = dates::today()?;
        let today_iso = dates::to_storage(today);

        // Sweep: everything expired (use_by strictly before today) is moved to
        // the bin, then physically removed from lots.
        let expired = store::all_lots(&self.conn, fridge_id)?
            .into_iter()
            .filter(|l| l.use_by.as_str() < today_iso.as_str())
            .collect::<Vec<_>>();

        let tx = self.conn.transaction()?;
        for lot in &expired {
            store::bin_snapshot(
                &tx,
                fridge_id,
                &lot.name,
                lot.unit_kind,
                lot.unit_amount,
                &lot.use_by,
                &today_iso,
                Reason::Expired,
            )?;
            tx.execute("DELETE FROM lots WHERE id = ?1", [lot.id])?;
        }
        tx.commit()?;

        for lot in &expired {
            println!(
                "Trashed on open: {} {}, use by {}.",
                lot.name,
                units::display(lot.unit_kind, lot.unit_amount),
                dates::display(&lot.use_by),
            );
        }

        self.scope = Scope::Fridge(FridgeCtx {
            id: fridge_id,
            name: fridge_name,
            today,
        });
        Ok(Flow::Continue)
    }
}

/// An argument made entirely of digits is an id.
pub fn is_id(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}
