//! FR-2.2…FR-2.6 dispatch: route the positional `|` fields of a signal (already
//! split + language-classified) to the per-class parser.
//!
//! Each per-class parser owns the WHOLE positional layout for its class —
//! including where `position` and `ttl` sit (Prediction places `position` before
//! its settle-date field, so the common fields are NOT at a class-independent
//! index). A parser therefore returns the class params PLUS the parsed
//! `positionPct` / `ttlSec`, which the caller assembles into [`ParsedSignal`].

pub mod defi;
pub mod option;
pub mod perp;
pub mod prediction;
pub mod spot;

use crate::asset_class::AssetClass;

use super::error::ParseError;
use super::{Language, SignalParams};

/// The per-class parse output: the class params + the two common scalar fields.
pub type ClassParse = (SignalParams, String, u64);

/// Build the class-specific params (+ position/ttl) from the positional fields.
pub fn dispatch(
    class: AssetClass,
    lang: Language,
    fields: &[String],
) -> Result<ClassParse, ParseError> {
    match class {
        AssetClass::Spot => spot::parse(fields, lang),
        AssetClass::Perp => perp::parse(fields, lang),
        AssetClass::Prediction => prediction::parse(fields, lang),
        AssetClass::Option => option::parse(fields, lang),
        AssetClass::Defi => defi::parse(fields, lang),
    }
}
