use anyhow::Result;
use serde_json::Value;

use super::amount::value_as_decimal_string;

/// Unwraps a single-item data array while preserving all other JSON shapes.
pub fn first_data_item(value: Value) -> Value {
    match value {
        Value::Array(mut values) if values.len() == 1 => values.remove(0),
        other => other,
    }
}

/// Quotes one value for safe reuse in a shell continuation command.
pub fn shell_arg(value: &str) -> String {
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
    }) {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

/// Recursively returns the first string-like value matching one of `keys`.
pub fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(found) = map.get(*key).and_then(value_as_decimal_string) {
                    return Some(found);
                }
            }
            map.values().find_map(|nested| find_string(nested, keys))
        }
        Value::Array(values) => values.iter().find_map(|nested| find_string(nested, keys)),
        _ => None,
    }
}

/// Reads a required non-empty string field and reports its response source on failure.
pub fn required_string<'a>(value: &'a Value, key: &str, source: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{source} is missing {key}"))
}
