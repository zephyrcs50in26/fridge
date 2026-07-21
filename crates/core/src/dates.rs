use anyhow::{Context, Result};
use jiff::civil::Date;

/// The sentinel written for `--no-expiry`: an ordinary date that simply never
/// arrives. There is no `if is_forever` branch anywhere else in the code.
pub const NEVER: &str = "9999-12-31";

/// Today, captured from the system clock in the local time zone. Called once
/// per `open` and then frozen into the session state.
pub fn today() -> Result<Date> {
    Ok(jiff::Zoned::now().date())
}

/// Parse an ISO `YYYY-MM-DD` date (the on-disk representation and the format
/// accepted from `--use-by`).
pub fn parse(s: &str) -> Result<Date> {
    s.parse::<Date>()
        .with_context(|| format!("{s:?} is not a valid date (expected YYYY-MM-DD)"))
}

/// Store form: exactly what SQLite holds, ISO 8601.
pub fn to_storage(d: Date) -> String {
    d.to_string()
}

/// Human display: `24 Jul`, or `never` for the sentinel.
pub fn display(iso: &str) -> String {
    if iso == NEVER {
        return "never".to_string();
    }
    match parse(iso) {
        Ok(d) => d.strftime("%d %b").to_string(),
        Err(_) => iso.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_displays_as_never() {
        assert_eq!(display(NEVER), "never");
    }

    #[test]
    fn ordinary_dates_display_day_month() {
        assert_eq!(display("2026-07-24"), "24 Jul");
        assert_eq!(display("2026-08-02"), "02 Aug");
    }

    #[test]
    fn round_trips_iso() {
        let d = parse("2026-07-24").unwrap();
        assert_eq!(to_storage(d), "2026-07-24");
        // The sentinel is an ordinary, parseable date that simply never arrives.
        assert!(parse(NEVER).unwrap() > d);
    }
}
