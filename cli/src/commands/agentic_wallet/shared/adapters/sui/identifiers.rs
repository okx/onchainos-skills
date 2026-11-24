//! SUI account-address and `Coin<T>` identifier normalization.

use anyhow::{bail, Result};

pub const NATIVE_COIN_TYPE: &str = "0x2::sui::SUI";

/// Normalizes a SUI address to a lowercase, 32-byte hexadecimal representation.
pub fn normalize_address(value: &str) -> Result<String> {
    let value = value.trim();
    let hex = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    if hex.is_empty() || hex.len() > 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("SUI address must contain 1 to 64 hexadecimal characters");
    }
    Ok(format!("0x{:0>64}", hex.to_ascii_lowercase()))
}

/// Compares two SUI addresses after canonical normalization.
pub fn same_address(left: &str, right: &str) -> Result<bool> {
    Ok(normalize_address(left)? == normalize_address(right)?)
}

/// Normalizes a SUI `Coin<T>` type while preserving its module and type arguments.
pub fn normalize_coin_type(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
        bail!("SUI Coin Type must be a complete <package>::<module>::<type> value");
    }
    let mut parts = value.splitn(3, "::");
    let package = parts.next().unwrap_or_default();
    let module = parts.next().unwrap_or_default();
    let type_name = parts.next().unwrap_or_default();
    if package.is_empty() || !is_valid_identifier(module) || !is_valid_type_name(type_name) {
        bail!("SUI Coin Type must be a complete <package>::<module>::<type> value");
    }
    let full_package = normalize_address(package)?;
    let package_hex = full_package
        .trim_start_matches("0x")
        .trim_start_matches('0');
    let package_hex = if package_hex.is_empty() {
        "0"
    } else {
        package_hex
    };
    Ok(format!("0x{package_hex}::{module}::{type_name}"))
}

/// Compares two SUI coin types after canonical normalization.
pub fn same_coin_type(left: &str, right: &str) -> Result<bool> {
    Ok(normalize_coin_type(left)? == normalize_coin_type(right)?)
}

/// Checks whether a Move module or type identifier uses supported characters.
fn is_valid_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Validates a possibly generic Move type name used inside a SUI coin type.
fn is_valid_type_name(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let mut depth = 0i32;
    for byte in value.bytes() {
        match byte {
            b'<' => depth += 1,
            b'>' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            b':' | b',' | b'_' => {}
            byte if byte.is_ascii_alphanumeric() => {}
            _ => return false,
        }
    }
    depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_is_padded_and_lowercased() {
        assert_eq!(
            normalize_address("0xAbC").unwrap(),
            format!("0x{}abc", "0".repeat(61))
        );
        assert!(normalize_address("0x").is_err());
        assert!(normalize_address(&format!("0x{}", "1".repeat(65))).is_err());
    }

    #[test]
    fn coin_type_uses_compact_package_address() {
        assert_eq!(
            normalize_coin_type("0x0002::sui::SUI").unwrap(),
            NATIVE_COIN_TYPE
        );
        assert!(same_coin_type("0x2::sui::SUI", "2::sui::SUI").unwrap());
        assert!(normalize_coin_type("SUI").is_err());
    }
}
