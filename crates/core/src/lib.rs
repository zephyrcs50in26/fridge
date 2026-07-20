//! Pure fridge domain logic: the data model, unit parsing/formatting, name
//! rules, and date handling. No database, no terminal IO — everything here is
//! deterministic and unit-testable.

pub mod dates;
pub mod model;
pub mod names;
pub mod units;
