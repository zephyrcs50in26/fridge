mod fridge_cmds;
mod repl;
mod root_cmds;

use anyhow::Result;

fn main() -> Result<()> {
    let conn = fridge_store::db::open()?;
    let mut repl = repl::Repl::new(conn);
    repl.run()?;
    Ok(())
}
