# FRIDGE

#### Video Demo: [URL](https://youtu.be/TX9sHuCarkE?si=C2Ftl6In8h--TiD5)

#### Description

*A personal note first: I picked this project because I was curious to play with
a REPL — and it turned out to be just an infinite loop.* 😃 That joke is truer
than it sounds: the whole program really is one loop that reads a line, runs a
command, and prints a result, forever, until you type `exit`. Everything
interesting lives in what happens *inside* that loop.

FRIDGE is a command-line "what's in my fridge?" tool written in Rust. It keeps
track of the food you actually have — how much of each item, in what unit, and
by when it needs to be used — across multiple password-protected user profiles,
each of which may own several fridges. Food is stored in **lots**: a lot is a
quantity of one product sharing a single use-by date. When you take food out,
FRIDGE consumes from the lots that expire soonest (FEFO — first-expire,
first-out). When a lot goes bad, it is swept into a **bin** so you can see what
was wasted and why. Everything lives in a local SQLite database, so your fridge
survives between runs.

The program is a small interactive shell. You start it, log in, open a fridge,
and then type commands like `add "Milk 3.2%" 1l --use-by 2026-08-01`, `list`, or
`take Milk 200ml`. Prices, calories, and shopping lists are all out of scope on
purpose — the tool does one thing, inventory of perishables, and tries to do it
correctly.

#### Running it

```
cargo run
```

The database defaults to `.tmp/fridge.sqlite` in the project directory (kept out
of git via `.gitignore`); set `FRIDGE_DB` to point elsewhere. On first launch
there are no users, so it prompts you to create one with `user create <name>`.

#### Data model (ERD)

```
users 1───∞ fridges 1───∞ lots
                     └────∞ bin
```

- **users** — one row per profile: a unique name and an argon2id password hash.
- **fridges** — each belongs to one user (`user_id`), unique per user by name.
- **lots** — the live inventory; each belongs to one fridge.
- **bin** — a post-mortem log of food that left a fridge as waste.

Every arrow is a foreign key with `ON DELETE CASCADE`: delete a user and their
fridges, lots, and bin entries all go with them. This is why `db.rs` runs
`PRAGMA foreign_keys = ON` on **every** connection — SQLite leaves it off by
default and won't persist it, and without it `fridge rm` would silently orphan
rows.

**Why the UNIQUE index on `lots (fridge_id, name, unit_kind, use_by)`.** Those
four columns are what I call a lot's *identity*. Two cartons of the same milk,
in the same unit, with the same use-by date are not two facts — they are one
pile of 2 litres. The index makes the database enforce that: you cannot end up
with duplicate rows that should have been summed. `add` and `edit` both look up
this tuple first and merge onto the existing "twin" instead of inserting, so the
index is both a guarantee and the thing that makes merging well-defined.

#### The files

The project is a **Cargo workspace** split into four crates, so pure logic,
storage, auth, and the interface can each be tested in isolation.

**`crates/core`** — the domain library, no database or terminal, all unit-tested.
- `model.rs` — the core types: `UnitKind` (Count / Volume / Mass), the `Reason`
  a lot was binned (Expired / Discarded), and the `Lot` struct.
- `units.rs` — parses amounts like `1.5l`, `250g`, or a bare `10` and reduces
  each to whole *base units* (pcs, ml, g). All arithmetic uses `rust_decimal`,
  not `f64`, because `2.675 kg` through a float becomes 2674 g — a fridge that
  silently loses a gram is broken. It also rejects fractional base units: no
  half an egg, no tenth of a millilitre.
- `dates.rs` — ISO `YYYY-MM-DD` dates, with the `--no-expiry` sentinel stored as
  the ordinary date `9999-12-31` (see design notes).
- `names.rs` — validates and normalizes names, collapsing whitespace so
  `"Milk  Korovka"` and `"Milk Korovka"` don't split past the UNIQUE index.

**`crates/auth`** (`lib.rs`) — password hashing with **argon2id** (salt and
parameters live inside the stored PHC string), verification, and a no-echo
prompt via `rpassword`.

**`crates/store`** — the SQLite layer. `schema.sql` is the DDL, embedded into the
binary with `include_str!`; `db.rs` opens the database and enables foreign keys;
`store.rs` holds the reusable lot/bin queries.

**`crates/cli`** — the front end. `main.rs` opens the database and starts the
REPL; `repl.rs` is the loop, tokenizer, and session state; `root_cmds.rs` has
the pre-fridge commands (user/fridge management and `open`); `fridge_cmds.rs`
has the in-fridge commands (`list`, `add`, `take`, `edit`, `rm`, `bin`).

#### Design choices I debated

**Should merging be automatic?** `add` merges silently — two identical cartons
really are one pile. But `edit` refuses to merge unless you pass `--merge`,
because editing one lot into another destroys a row you didn't name, and that
should be a choice, not a surprise.

**The bin is a snapshot, not a link.** When a lot expires or is discarded, its
values are *copied* into `bin` and the lot row is deleted — the bin has no
foreign key back to `lots`. This is deliberate: waste is history, and history
shouldn't change if you later reorganize your fridge. It also means there is no
"restore," which matches how a real trash can works.

**The REPL captures the date once, at `open`.** When you open a fridge, `today`
is read from the clock and frozen into the session. Every query inside that
fridge — the expiry sweep, use-by validation — uses that one value. Without it,
a fridge left open across midnight could see food expire mid-session, and two
commands in the same breath could disagree about what "today" is. Freezing the
date makes a session internally consistent.

**`--no-expiry` is a sentinel date, not a nullable column.** "Never expires" is
stored as `9999-12-31`. Because ISO date strings sort chronologically as plain
text, every comparison — including the sweep — just works, with no `if is_forever`
branch anywhere in the code.

**Multi-row changes run in a transaction.** Any command that touches more than
one row — the sweep, a `take` that drains several lots, an `rm` or a merging
`edit` — wraps its writes in a SQLite transaction and commits only at the end.
Either the whole operation lands or none of it does. This matters most for
`take` and the sweep: each first writes a bin snapshot and *then* deletes the
lot, and a crash between those two steps would otherwise lose food off the
books. The transaction makes "snapshot, then delete" a single indivisible fact.

**Sweeping happens once, at `open`, and nowhere else.** Expiry is not checked on
every command — that would make results depend on how long you sat at the
prompt. Instead, opening a fridge is the one moment the clock is consulted:
every lot whose `use_by` is strictly *before* today's frozen date is snapshotted
into the bin as `EXPIRED`, then deleted from `lots`, and each removal is printed
so you see what was lost. "Strictly before" is deliberate — food is still good
*on* its use-by date, so a lot due today survives the sweep and only falls in the
next time you open after that date passes. Because the sweep and every later
query share the single `today` captured at `open`, what you see in `list` always
matches what the sweep just did.
