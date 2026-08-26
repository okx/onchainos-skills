//! Maps Bitcoin Agentic Wallet API errors to stable CLI errors.

use anyhow::Error;
use serde_json::json;

use crate::commands::sink::CodedError;
use crate::wallet_api::ApiCodeError;

use super::models::{next_steps, ReadOnlyNextStep};

/// Maps documented Bitcoin service codes to CLI errors while preserving service messages.
pub fn map_api_error(error: Error) -> Error {
    match error.downcast::<ApiCodeError>() {
        Ok(api_error) => {
            let mut coded = CodedError::new(&api_error.code, None, api_error.msg);
            match api_error.code.as_str() {
                "44001" => {
                    coded = coded
                        .with_data(json!({"state": "INSUFFICIENT_UTXO"}))
                        .with_next_steps(
                            next_steps([ReadOnlyNextStep::QueryUnavailableUtxos])
                                .unwrap_or_else(|_| json!({})),
                        );
                }
                "44002" => {
                    coded = coded
                        .with_data(json!({"state": "INSUFFICIENT_BTC_FOR_INSCRIPTION"}))
                        .with_next_steps(
                            next_steps([
                                ReadOnlyNextStep::ShowBitcoinAddress,
                                ReadOnlyNextStep::RefreshBtcBalance,
                            ])
                            .unwrap_or_else(|_| json!({})),
                        );
                }
                "44003" => {
                    coded = coded.with_data(json!({"state": "NEED_INSCRIBE"}));
                }
                "82001" => {
                    coded = coded.with_data(json!({"state": "UTXO_PERMISSION_DENIED"}));
                }
                "82002" => {
                    coded = coded
                        .with_data(json!({"state": "UTXO_NOT_FOUND"}))
                        .with_next_steps(
                            next_steps([ReadOnlyNextStep::QueryUnavailableUtxos])
                                .unwrap_or_else(|_| json!({})),
                        );
                }
                "82003" => {
                    coded = coded.with_data(json!({"state": "INVALID_UTXO_REQUEST"}));
                }
                "82005" => {
                    coded = coded
                        .with_data(json!({"state": "UTXO_ALREADY_SPENT"}))
                        .with_next_steps(
                            next_steps([ReadOnlyNextStep::QueryUnavailableUtxos])
                                .unwrap_or_else(|_| json!({})),
                        );
                }
                _ => {}
            }
            coded.into()
        }
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(code: &str) -> CodedError {
        map_api_error(
            ApiCodeError {
                code: code.to_string(),
                msg: "service message".to_string(),
                http_status: 200,
            }
            .into(),
        )
        .downcast::<CodedError>()
        .unwrap()
    }

    #[test]
    fn maps_latest_utxo_error_codes_without_rewriting_service_message() {
        let insufficient = map("44001");
        assert_eq!(insufficient.data.unwrap()["state"], "INSUFFICIENT_UTXO");
        assert!(insufficient.next_steps.unwrap()["queryUnavailableUtxos"]
            .as_str()
            .unwrap()
            .contains("utxo unavailable --chain bitcoin"));

        let permission = map("82001");
        assert_eq!(permission.message, "service message");
        assert_eq!(permission.data.unwrap()["state"], "UTXO_PERMISSION_DENIED");

        let missing = map("82002");
        assert_eq!(missing.data.unwrap()["state"], "UTXO_NOT_FOUND");
        assert!(missing.next_steps.unwrap()["queryUnavailableUtxos"]
            .as_str()
            .unwrap()
            .contains("utxo unavailable --chain bitcoin"));

        assert_eq!(map("82003").data.unwrap()["state"], "INVALID_UTXO_REQUEST");
        assert_eq!(map("82005").data.unwrap()["state"], "UTXO_ALREADY_SPENT");
    }
}
