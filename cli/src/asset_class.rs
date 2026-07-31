//! The single, repo-wide asset-class taxonomy (FR-1.4 / NFR-6).
//!
//! Exactly ONE `AssetClass` definition may exist in the crate. It lives at crate
//! root (not inside `trade_signal`) so sibling features (e.g. the subscription
//! `serviceDescription` pre-check) can `use crate::asset_class::AssetClass`
//! without depending on the parser module. On merge, any duplicate `AssetClass`
//! from another task MUST be deleted and repointed here (AC-23).

use serde::{Deserialize, Serialize};

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

impl AssetClass {
    /// The stable wire string (`"spot" | "perp" | "prediction" | "option" | "defi"`).
    /// Matches the serde `lowercase` rename and the `params.kind` tag.
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
}
