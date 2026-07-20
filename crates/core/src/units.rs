use crate::model::UnitKind;
use anyhow::{bail, Result};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

/// A parsed amount, reduced to base units (pcs / ml / g).
pub struct Amount {
    pub kind: UnitKind,
    pub base: i64,
}

/// Parse an amount string like `1.5l`, `200ml`, `250g`, `0.25kg`, `10`, `10pcs`.
///
/// Everything goes through `rust_decimal` — no floats — because `2.675 kg`
/// through f64 yields 2674 g. A fractional remainder after converting to base
/// units is an error: we store neither half an egg nor 0.1 ml.
pub fn parse_amount(input: &str) -> Result<Amount> {
    let s = input.trim();
    if s.is_empty() {
        bail!("empty amount");
    }

    // Split leading number (digits, '.', ',') from a trailing alphabetic unit.
    let split = s
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    let (num_part, unit_part) = s.split_at(split);
    let num_part = num_part.trim();
    let unit = unit_part.trim().to_ascii_lowercase();

    if num_part.is_empty() {
        bail!("{input:?} has no numeric amount");
    }

    let value = Decimal::from_str_exact(num_part)
        .map_err(|_| anyhow::anyhow!("{num_part:?} is not a valid number"))?;
    if value.is_sign_negative() || value.is_zero() {
        bail!("amount must be greater than zero");
    }

    // (kind, factor to base units)
    let (kind, factor): (UnitKind, i64) = match unit.as_str() {
        "" | "pcs" | "pc" => (UnitKind::Count, 1),
        "ml" => (UnitKind::Volume, 1),
        "l" => (UnitKind::Volume, 1000),
        "g" => (UnitKind::Mass, 1),
        "kg" => (UnitKind::Mass, 1000),
        other => bail!("unknown unit {other:?} (this system is metric: use pcs, ml, l, g, kg)"),
    };

    let base = value * Decimal::from(factor);
    if !base.fract().is_zero() {
        bail!("{input} does not divide into whole base units");
    }
    let base = base
        .to_i64()
        .ok_or_else(|| anyhow::anyhow!("{input} is too large"))?;
    if base <= 0 {
        bail!("amount must be greater than zero");
    }

    Ok(Amount { kind, base })
}

/// Render a base-unit amount for display. Parentheses always, except COUNT:
///   COUNT  -> `10 pcs`
///   VOLUME -> `2000 ml (2 l)`
///   MASS   -> `250 g (0.25 kg)`
pub fn display(kind: UnitKind, base: i64) -> String {
    match kind {
        UnitKind::Count => format!("{base} pcs"),
        UnitKind::Volume => format!("{base} ml ({} l)", scaled(base, 1000)),
        UnitKind::Mass => format!("{base} g ({} kg)", scaled(base, 1000)),
    }
}

/// base / divisor, as a trimmed decimal string (`2000/1000` -> `2`, `250/1000`
/// -> `0.25`).
fn scaled(base: i64, divisor: i64) -> String {
    let d = Decimal::from(base) / Decimal::from(divisor);
    d.normalize().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_never_goes_through_a_float() {
        // 2.675 kg through f64 yields 2674 g; rust_decimal keeps it exact.
        let a = parse_amount("2.675kg").unwrap();
        assert_eq!(a.kind, UnitKind::Mass);
        assert_eq!(a.base, 2675);
    }

    #[test]
    fn litres_and_millilitres_share_a_base() {
        assert_eq!(parse_amount("1.5l").unwrap().base, 1500);
        assert_eq!(parse_amount("200ml").unwrap().base, 200);
    }

    #[test]
    fn bare_number_is_a_count() {
        let a = parse_amount("10").unwrap();
        assert_eq!(a.kind, UnitKind::Count);
        assert_eq!(a.base, 10);
    }

    #[test]
    fn fractional_base_units_are_rejected() {
        assert!(parse_amount("0.1ml").is_err());
        assert!(parse_amount("1.5").is_err()); // half an egg
    }

    #[test]
    fn non_metric_and_nonsense_are_rejected() {
        assert!(parse_amount("5oz").is_err());
        assert!(parse_amount("0ml").is_err());
        assert!(parse_amount("-3l").is_err());
        assert!(parse_amount("").is_err());
    }

    #[test]
    fn display_uses_parens_except_for_count() {
        assert_eq!(display(UnitKind::Count, 10), "10 pcs");
        assert_eq!(display(UnitKind::Volume, 2000), "2000 ml (2 l)");
        assert_eq!(display(UnitKind::Mass, 250), "250 g (0.25 kg)");
    }
}
