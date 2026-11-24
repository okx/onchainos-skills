use serde_json::Value;

const MAX_CHARS: usize = 80;

/// Limit untrusted token metadata before it reaches CLI, MCP, or workflow output.
///
/// DEX APIs expose token metadata as both `name` / `symbol` and
/// `tokenName` / `tokenSymbol`. A bare `name` is truncated only when its object
/// also has `symbol`; other DEX responses use `name` for chains, liquidity
/// sources, and EIP-712 signing data. Walk the whole value because token fields
/// can appear inside nested lists and token objects.
pub(crate) fn truncate(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                truncate(item);
            }
        }
        Value::Object(fields) => {
            let has_generic_token_pair =
                fields.contains_key("name") && fields.contains_key("symbol");
            for (key, field) in fields {
                if matches!(key.as_str(), "symbol" | "tokenName" | "tokenSymbol")
                    || (key == "name" && has_generic_token_pair)
                {
                    truncate_string(field);
                }
                truncate(field);
            }
        }
        _ => {}
    }
}

fn truncate_string(value: &mut Value) {
    let Value::String(text) = value else {
        return;
    };
    if text.chars().count() <= MAX_CHARS {
        return;
    }

    let mut truncated: String = text.chars().take(MAX_CHARS).collect();
    truncated.push_str("...");
    *text = truncated;
}

#[cfg(test)]
mod tests {
    use super::truncate;
    use serde_json::json;

    #[test]
    fn keeps_values_at_the_80_character_limit() {
        let value = "x".repeat(80);
        let mut data = json!({"tokenName": value, "tokenSymbol": value});

        truncate(&mut data);

        assert_eq!(data["tokenName"].as_str().unwrap().chars().count(), 80);
        assert_eq!(data["tokenSymbol"].as_str().unwrap().chars().count(), 80);
        assert!(!data["tokenName"].as_str().unwrap().ends_with("..."));
    }

    #[test]
    fn truncates_nested_token_name_and_symbol_fields_by_unicode_characters() {
        let mut data = json!({
            "list": [{
                "tokenName": "a".repeat(81),
                "tokenSymbol": "é".repeat(81),
                "nested": {"name": "b".repeat(81), "symbol": "c".repeat(81)},
                "description": "d".repeat(81)
            }]
        });

        truncate(&mut data);

        assert_eq!(
            data["list"][0]["tokenName"],
            format!("{}...", "a".repeat(80))
        );
        assert_eq!(
            data["list"][0]["tokenSymbol"],
            format!("{}...", "é".repeat(80))
        );
        assert_eq!(
            data["list"][0]["nested"]["name"],
            format!("{}...", "b".repeat(80))
        );
        assert_eq!(
            data["list"][0]["nested"]["symbol"],
            format!("{}...", "c".repeat(80))
        );
        assert_eq!(data["list"][0]["description"], "d".repeat(81));
    }

    #[test]
    fn leaves_non_token_name_fields_unchanged() {
        let long_name = "x".repeat(81);
        let mut data = json!({
            "chain": {"chainIndex": "1", "name": long_name},
            "liquidity": {"id": "34", "name": long_name},
            "signData": {
                "domain": {"name": long_name, "version": "1"},
                "types": [{"name": long_name, "type": "address"}]
            }
        });

        truncate(&mut data);

        assert_eq!(data["chain"]["name"], long_name);
        assert_eq!(data["liquidity"]["name"], long_name);
        assert_eq!(data["signData"]["domain"]["name"], long_name);
        assert_eq!(data["signData"]["types"][0]["name"], long_name);
    }
}
