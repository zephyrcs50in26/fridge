-- Schema for the fridge tool. See spec §11.
-- foreign_keys is enabled per-connection in db.rs, not here.

CREATE TABLE IF NOT EXISTS users (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  name           TEXT NOT NULL UNIQUE COLLATE NOCASE,
  password_hash  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS fridges (
  id       INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name     TEXT NOT NULL COLLATE NOCASE,
  notes    TEXT NOT NULL DEFAULT '',
  UNIQUE (user_id, name)
);

CREATE TABLE IF NOT EXISTS lots (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  fridge_id    INTEGER NOT NULL REFERENCES fridges(id) ON DELETE CASCADE,
  name         TEXT NOT NULL COLLATE NOCASE,
  unit_kind    TEXT NOT NULL,
  unit_amount  INTEGER NOT NULL,
  use_by       TEXT NOT NULL,          -- '9999-12-31' = --no-expiry
  added_at     TEXT NOT NULL,
  CHECK (unit_amount > 0),
  CHECK (unit_kind IN ('COUNT','VOLUME','MASS')),
  CHECK (use_by > added_at)
);
CREATE UNIQUE INDEX IF NOT EXISTS lot_identity ON lots (fridge_id, name, unit_kind, use_by);

CREATE TABLE IF NOT EXISTS bin (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  fridge_id    INTEGER NOT NULL REFERENCES fridges(id) ON DELETE CASCADE,
  name         TEXT NOT NULL,
  unit_kind    TEXT NOT NULL,
  unit_amount  INTEGER NOT NULL,
  use_by       TEXT NOT NULL,
  trashed_at   TEXT NOT NULL,
  reason       TEXT NOT NULL CHECK (reason IN ('EXPIRED','DISCARDED'))
);
