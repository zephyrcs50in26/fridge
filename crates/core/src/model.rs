use anyhow::{bail, Result};
use std::fmt;

/// The kind of unit a lot is measured in. Determines both the base unit
/// (pcs / ml / g) and how the amount is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Count,
    Volume,
    Mass,
}

impl UnitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitKind::Count => "COUNT",
            UnitKind::Volume => "VOLUME",
            UnitKind::Mass => "MASS",
        }
    }

    /// The base unit a lot of this kind is stored in — for human-facing messages.
    pub fn base_unit(self) -> &'static str {
        match self {
            UnitKind::Count => "pcs",
            UnitKind::Volume => "ml",
            UnitKind::Mass => "g",
        }
    }

    pub fn parse(s: &str) -> Result<UnitKind> {
        Ok(match s {
            "COUNT" => UnitKind::Count,
            "VOLUME" => UnitKind::Volume,
            "MASS" => UnitKind::Mass,
            other => bail!("unknown unit kind {other:?} in database"),
        })
    }
}

impl fmt::Display for UnitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a lot ended up in the bin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Expired,
    Discarded,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Expired => "EXPIRED",
            Reason::Discarded => "DISCARDED",
        }
    }
}

/// A single lot as stored in `lots`. `use_by` and `added_at` are kept as the
/// ISO strings SQLite holds; parse them with the `dates` helpers when needed.
#[derive(Debug, Clone)]
pub struct Lot {
    pub id: i64,
    pub name: String,
    pub unit_kind: UnitKind,
    pub unit_amount: i64,
    pub use_by: String,
    pub added_at: String,
}
