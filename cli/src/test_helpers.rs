//! Test-only shared helpers. Not compiled in release builds.
//!
//! Put re-usable fixtures (especially for Gas Station tests that need to build
//! `GasStationToken` / `UnsignedInfoResponse` mocks) here so multiple test modules can
//! share a single definition instead of hand-rolling their own.
#![cfg(test)]

pub mod gas_station {
    use crate::wallet_api::{GasStationToken, UnsignedInfoResponse};

    /// Minimal token mock — symbol, fee-token address, sufficient flag. Other fields default.
    pub fn make_token(symbol: &str, fee_token_address: &str, sufficient: bool) -> GasStationToken {
        let json = format!(
            r#"{{"symbol":"{}","feeTokenAddress":"{}","sufficient":{}}}"#,
            symbol, fee_token_address, sufficient
        );
        serde_json::from_str(&json).unwrap()
    }

    /// Full token mock including balance + service charge (used by prompt-formatter tests).
    pub fn make_token_full(
        symbol: &str,
        fee_token_address: &str,
        balance: &str,
        service_charge: &str,
        sufficient: bool,
    ) -> GasStationToken {
        let json = format!(
            r#"{{"symbol":"{}","feeTokenAddress":"{}","balance":"{}","serviceCharge":"{}","sufficient":{}}}"#,
            symbol, fee_token_address, balance, service_charge, sufficient
        );
        serde_json::from_str(&json).unwrap()
    }

    /// Build an `UnsignedInfoResponse` with the two Gas-Station routing fields populated.
    /// All other fields default to empty / false / Null.
    pub fn make_unsigned_with_tokens(
        default_gas_token_address: &str,
        gas_station_token_list: Vec<GasStationToken>,
    ) -> UnsignedInfoResponse {
        let mut resp: UnsignedInfoResponse = serde_json::from_str("{}").unwrap();
        resp.default_gas_token_address = default_gas_token_address.to_string();
        resp.gas_station_token_list = gas_station_token_list;
        resp
    }
}
