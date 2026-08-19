//! The single, repo-wide asset-class taxonomy (FR-1.4 / NFR-6).
//!
//! Exactly ONE `AssetClass` definition may exist in the crate. It lives at crate
//! root so the subscription `serviceDescription` pre-check, bounded model-route
//! cache, and CLI arguments all share the same wire taxonomy.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// The five bounded asset classes a trading signal can describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetClass {
    Spot,
    Perp,
    Prediction,
    Option,
    Defi,
}

impl FromStr for AssetClass {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "spot" => Ok(Self::Spot),
            "perp" | "futures" => Ok(Self::Perp),
            "prediction" => Ok(Self::Prediction),
            "option" | "options" => Ok(Self::Option),
            "defi" => Ok(Self::Defi),
            _ => Err("asset class must be spot, perp, prediction, option, or defi"),
        }
    }
}

impl AssetClass {
    /// Canonical stable output order (FR-2: stable, de-duplicated). Sibling features
    /// iterate this to emit a stable, de-duplicated classification (e.g. the
    /// subscription `serviceDescription` pre-check in `autotrade::tooling`). The five
    /// entries match the declaration order of the variants above.
    pub const ORDER: [AssetClass; 5] = [
        AssetClass::Spot,
        AssetClass::Perp,
        AssetClass::Prediction,
        AssetClass::Option,
        AssetClass::Defi,
    ];

    /// The stable wire string (`"spot" | "perp" | "prediction" | "option" | "defi"`).
    /// Matches the serde `lowercase` rename and route-cache CLI values.
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetClass::Spot => "spot",
            AssetClass::Perp => "perp",
            AssetClass::Prediction => "prediction",
            AssetClass::Option => "option",
            AssetClass::Defi => "defi",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_maps_every_variant() {
        assert_eq!(AssetClass::Spot.as_str(), "spot");
        assert_eq!(AssetClass::Perp.as_str(), "perp");
        assert_eq!(AssetClass::Prediction.as_str(), "prediction");
        assert_eq!(AssetClass::Option.as_str(), "option");
        assert_eq!(AssetClass::Defi.as_str(), "defi");
    }

    #[test]
    fn order_covers_all_five_variants_in_declaration_order() {
        assert_eq!(
            AssetClass::ORDER,
            [
                AssetClass::Spot,
                AssetClass::Perp,
                AssetClass::Prediction,
                AssetClass::Option,
                AssetClass::Defi,
            ]
        );
        // The stable order maps to the stable wire strings in the same sequence.
        let wire: Vec<&str> = AssetClass::ORDER.iter().map(|c| c.as_str()).collect();
        assert_eq!(wire, ["spot", "perp", "prediction", "option", "defi"]);
    }

    #[test]
    fn serde_round_trips_to_lowercase_token() {
        for (variant, token) in [
            (AssetClass::Spot, "\"spot\""),
            (AssetClass::Perp, "\"perp\""),
            (AssetClass::Prediction, "\"prediction\""),
            (AssetClass::Option, "\"option\""),
            (AssetClass::Defi, "\"defi\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, token);
            let back: AssetClass = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn parses_cli_tokens_and_aliases() {
        assert_eq!("spot".parse(), Ok(AssetClass::Spot));
        assert_eq!("FUTURES".parse(), Ok(AssetClass::Perp));
        assert_eq!("options".parse(), Ok(AssetClass::Option));
        assert!("unknown".parse::<AssetClass>().is_err());
    }
}
