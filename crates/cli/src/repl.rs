use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use jiff::civil::Date;
use rusqlite::Connection;
use std::io::{BufRead, Write};

/// Whether the main loop should keep going or wind down.
pub enum Flow {
    Continue,
    Exit,
}

/// The logged-in profile. Identity is by id everywhere, so that is all we keep.
pub struct UserCtx {
    pub id: i64,
}

/// State that only exists while a fridge is open. `today` is captured once, at
/// `open`, and lives until `close` — every query inside the fridge is naive.
pub struct FridgeCtx {
    pub id: i64,
    pub name: String,
    pub today: Date,
}

pub enum Scope {
    Root,
    Fridge(FridgeCtx),
}

pub struct Repl {
    pub conn: Connection,
    reader: Box<dyn BufRead>,
    pub user: Option<UserCtx>,
    pub scope: Scope,
}

impl Repl {
    pub fn new(conn: Connection) -> Self {
        let stdin = std::io::stdin();
        Repl {
            conn,
            reader: Box::new(std::io::BufReader::new(stdin)),
            user: None,
            scope: Scope::Root,
        }
    }

    /// Read a password. On a real terminal this is a no-echo rpassword prompt;
    /// when stdin is piped (tests, scripts) we read a plain line through the
    /// same buffered reader, so nothing desyncs.
    pub fn read_secret(&mut self, prompt: &str) -> Result<String> {
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() {
            Ok(fridge_auth::prompt(prompt)?)
        } else {
            Ok(self.next_line(prompt)?.unwrap_or_default())
        }
    }

    /// Print a prompt and read one line. Returns `None` at end of input.
    pub fn next_line(&mut self, prompt: &str) -> Result<Option<String>> {
        print!("{prompt}");
        std::io::stdout().flush()?;
        let mut buf = String::new();
        let n = self.reader.read_line(&mut buf)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(buf.trim_end_matches(['\n', '\r']).to_string()))
    }

    pub fn run(&mut self) -> Result<()> {
        self.greet()?;
        loop {
            let prompt = match &self.scope {
                Scope::Root => "> ".to_string(),
                Scope::Fridge(f) => format!("{}> ", f.name),
            };
            let line = match self.next_line(&prompt)? {
                Some(l) => l,
                None => {
                    println!();
                    break;
                }
            };
            let tokens = match tokenize(&line) {
                Ok(t) => t,
                Err(e) => {
                    println!("Error: {e}");
                    continue;
                }
            };
            if tokens.is_empty() {
                continue;
            }
            let flow = match &self.scope {
                Scope::Root => self.dispatch_root(tokens),
                Scope::Fridge(_) => self.dispatch_fridge(tokens),
            };
            match flow {
                Ok(Flow::Continue) => {}
                Ok(Flow::Exit) => break,
                Err(e) => println!("Error: {e}"),
            }
        }
        Ok(())
    }

    fn greet(&mut self) -> Result<()> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        if count == 0 {
            println!("No users yet. Create one: user create <name>");
        }
        Ok(())
    }

    fn dispatch_root(&mut self, tokens: Vec<String>) -> Result<Flow> {
        let parsed = match RootLine::try_parse_from(tokens) {
            Ok(p) => p,
            Err(e) => {
                print!("{e}");
                return Ok(Flow::Continue);
            }
        };
        match parsed.cmd {
            RootCmd::User { action } => self.cmd_user(action),
            RootCmd::Fridge { action } => self.cmd_fridge(action),
            RootCmd::Open { name } => self.cmd_open(&name),
            RootCmd::Help => {
                print_root_help();
                Ok(Flow::Continue)
            }
            RootCmd::Exit => Ok(Flow::Exit),
        }
    }

    fn dispatch_fridge(&mut self, mut tokens: Vec<String>) -> Result<Flow> {
        // Accept `clear bin` as a synonym for `bin clear`.
        if tokens.len() == 2 && tokens[0] == "clear" && tokens[1] == "bin" {
            tokens = vec!["bin".to_string(), "clear".to_string()];
        }
        let parsed = match FridgeLine::try_parse_from(tokens) {
            Ok(p) => p,
            Err(e) => {
                print!("{e}");
                return Ok(Flow::Continue);
            }
        };
        match parsed.cmd {
            InFridgeCmd::List => self.cmd_list(),
            InFridgeCmd::Add {
                name,
                amount,
                use_by,
                no_expiry,
            } => self.cmd_add(&name, &amount, use_by.as_deref(), no_expiry),
            InFridgeCmd::Take { target, amount } => self.cmd_take(&target, &amount),
            InFridgeCmd::Edit {
                target,
                name,
                amount,
                use_by,
                no_expiry,
                merge,
            } => self.cmd_edit(&target, name.as_deref(), amount.as_deref(), use_by.as_deref(), no_expiry, merge),
            InFridgeCmd::Rm { target } => self.cmd_rm(&target),
            InFridgeCmd::Bin { action } => match action {
                BinCmd::List => self.cmd_bin_list(),
                BinCmd::Clear => self.cmd_bin_clear(),
            },
            InFridgeCmd::Close => {
                self.scope = Scope::Root;
                Ok(Flow::Continue)
            }
            InFridgeCmd::Help => {
                print_fridge_help();
                Ok(Flow::Continue)
            }
            InFridgeCmd::Exit => Ok(Flow::Exit),
        }
    }

    /// The fridge currently open. Only call from inside a fridge command.
    pub fn fridge(&self) -> &FridgeCtx {
        match &self.scope {
            Scope::Fridge(f) => f,
            Scope::Root => unreachable!("fridge command dispatched at root scope"),
        }
    }
}

/// Shell-like tokenizer: whitespace separates, single and double quotes group.
fn tokenize(line: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                    in_token = true;
                } else if c.is_whitespace() {
                    if in_token {
                        tokens.push(std::mem::take(&mut cur));
                        in_token = false;
                    }
                } else {
                    cur.push(c);
                    in_token = true;
                }
            }
        }
    }
    if quote.is_some() {
        bail!("unclosed quote");
    }
    if in_token {
        tokens.push(cur);
    }
    Ok(tokens)
}

fn print_root_help() {
    println!(
        "Root commands:\n\
         \x20 user create <name>      create a profile (and log in)\n\
         \x20 user use <name>         log in as a profile\n\
         \x20 user list               list profiles\n\
         \x20 user rm                 delete your profile and all its fridges\n\
         \x20 fridge create <name>    create a fridge\n\
         \x20 fridge rename <a> <b>   rename a fridge\n\
         \x20 fridge rm <name|id>     delete a fridge and everything in it\n\
         \x20 fridge list             list your fridges\n\
         \x20 open <name|id>          open a fridge (sweeps expired lots)\n\
         \x20 help, exit"
    );
}

fn print_fridge_help() {
    println!(
        "Fridge commands:\n\
         \x20 list                                     show lots\n\
         \x20 add \"name\" <amount> --use-by D|--no-expiry\n\
         \x20 take \"name|id\" <amount>                  consume (FEFO)\n\
         \x20 edit \"name|id\" [--name X] [--amount A] [--use-by D|--no-expiry] [--merge]\n\
         \x20 rm \"name|id\"                             discard a whole lot to the bin\n\
         \x20 bin list                                 show the bin\n\
         \x20 bin clear                                empty the bin\n\
         \x20 close                                    leave the fridge\n\
         \x20 help, exit"
    );
}

// ---------------------------------------------------------------------------
// clap grammar
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(no_binary_name = true, disable_help_flag = true, disable_help_subcommand = true)]
struct RootLine {
    #[command(subcommand)]
    cmd: RootCmd,
}

#[derive(Subcommand)]
enum RootCmd {
    /// Manage profiles
    User {
        #[command(subcommand)]
        action: UserCmd,
    },
    /// Manage fridges
    Fridge {
        #[command(subcommand)]
        action: FridgeCmd,
    },
    /// Open a fridge
    Open { name: String },
    Help,
    Exit,
}

#[derive(Subcommand)]
pub enum UserCmd {
    Create { name: String },
    List,
    Use { name: String },
    /// Delete your own profile and everything in it (password-confirmed)
    Rm,
}

#[derive(Subcommand)]
pub enum FridgeCmd {
    Create { name: String },
    Rm { name: String },
    Rename { old: String, new: String },
    List,
}

#[derive(Parser)]
#[command(no_binary_name = true, disable_help_flag = true, disable_help_subcommand = true)]
struct FridgeLine {
    #[command(subcommand)]
    cmd: InFridgeCmd,
}

#[derive(Subcommand)]
enum InFridgeCmd {
    List,
    Add {
        name: String,
        amount: String,
        #[arg(long = "use-by")]
        use_by: Option<String>,
        #[arg(long = "no-expiry")]
        no_expiry: bool,
    },
    Take {
        target: String,
        amount: String,
    },
    Edit {
        target: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        amount: Option<String>,
        #[arg(long = "use-by")]
        use_by: Option<String>,
        #[arg(long = "no-expiry")]
        no_expiry: bool,
        #[arg(long)]
        merge: bool,
    },
    Rm {
        target: String,
    },
    Bin {
        #[command(subcommand)]
        action: BinCmd,
    },
    Close,
    Help,
    Exit,
}

#[derive(Subcommand)]
enum BinCmd {
    List,
    Clear,
}
