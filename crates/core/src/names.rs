use anyhow::{bail, Result};

/// Trim and collapse internal whitespace runs to a single space, so that
/// "Milk  Korovka" does not diverge from "Milk Korovka" past the UNIQUE index.
pub fn normalize(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// users, fridges:  ^[A-Za-z0-9 ]{1,64}$
pub fn validate_account(raw: &str) -> Result<String> {
    let name = normalize(raw);
    if name.is_empty() || name.len() > 64 {
        bail!("name must be 1-64 characters");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ') {
        bail!("name may contain only ASCII letters, digits and spaces");
    }
    Ok(name)
}

/// products:  ^[A-Za-z0-9][A-Za-z0-9 .%&'-]{0,63}$
///
/// The first character is separate — otherwise `-Milk` goes into the argument
/// parser as a flag. ASCII-only keeps COLLATE NOCASE correct and removes the
/// need for NFC normalization.
pub fn validate_product(raw: &str) -> Result<String> {
    let name = normalize(raw);
    let mut chars = name.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => bail!("product name must not be empty"),
    };
    if name.len() > 64 {
        bail!("product name must be at most 64 characters");
    }
    if !first.is_ascii_alphanumeric() {
        bail!("product name must start with an ASCII letter or digit");
    }
    let allowed = |c: char| c.is_ascii_alphanumeric() || " .%&'-".contains(c);
    if !name.chars().all(allowed) {
        bail!("product name may contain only letters, digits, spaces and . % & ' -");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_is_collapsed() {
        assert_eq!(normalize("  Milk   Korovka  "), "Milk Korovka");
    }

    #[test]
    fn products_allow_punctuation_but_not_a_leading_dash() {
        assert!(validate_product("Coca-Cola").is_ok());
        assert!(validate_product("Milk 3.2%").is_ok());
        assert!(validate_product("-Milk").is_err()); // would look like a flag
        assert!(validate_product("").is_err());
    }

    #[test]
    fn accounts_are_alnum_and_spaces_only() {
        assert!(validate_account("anton 2").is_ok());
        assert!(validate_account("anton.k").is_err());
        assert!(validate_account("").is_err());
    }
}
