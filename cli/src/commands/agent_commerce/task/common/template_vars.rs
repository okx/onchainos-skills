//! Template-variable decode / validate / render — the in-process substitution
//! that severs the untrusted-title → shell-source data flow.
//!
//! `next-action` emitters no longer interpolate a raw task title into the emitted
//! `--user-content` / `--list-label`. Instead they emit the fixed placeholder
//! [`TITLE_PLACEHOLDER`] plus a `--template-vars-b64 "<Base64 JSON>"` line. This
//! module decodes + validates that Base64 payload and performs a single-pass,
//! non-recursive, literal substitution of each `{{KEY}}` placeholder — entirely
//! in-process, after clap parse and before any card is pushed — so the shell never
//! sees the raw title.
//!
//! Fail-closed / default-deny: any malformed payload, whitelist violation,
//! or placeholder/var mismatch aborts with a stable `errorCode` and no card push.
//! Error `Display` strings are intentionally value-free — the decoded title is
//! never logged or embedded in an error message.
//!
//! Wired into `request_prompt_inner` (decode + render) and the `next-action`
//! emitters (`TITLE_PLACEHOLDER`) in Phase 3.

use std::collections::BTreeMap;

use base64::Engine;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::Deserialize;

/// Compile-time whitelist of permitted template-var keys. Adding a key is a
/// deliberate, reviewed change.
///
/// Two keys, because the `sub_user_reject` renderer resolves the visible title
/// from two independent base sources that can legitimately differ:
///   * `__OKX_TASK_TITLE__`       — the decision-copy title
///     (`message.jobTitle` → `message.title` → `title_display`).
///   * `__OKX_TASK_LABEL_TITLE__` — the list-label title (`title_display`, which
///     itself falls back to the literal `<title>`).
///
/// They are carried as separate whitelisted variables so the two base values are
/// never collapsed into one `resolved_title`.
pub const TEMPLATE_VAR_WHITELIST: &[&str] = &["__OKX_TASK_TITLE__", "__OKX_TASK_LABEL_TITLE__"];

/// Placeholder delimiters. A placeholder for key `K` is the literal `{{K}}`.
pub const PLACEHOLDER_OPEN: &str = "{{";
pub const PLACEHOLDER_CLOSE: &str = "}}";

/// The single reserved placeholder token for the untrusted task title. Emitter
/// sites put this literal in `--user-content` / `--list-label`; this module
/// decodes the matching `--template-vars-b64` var and substitutes it in-process.
/// Exposed as a const so emitter call sites don't have to escape `{{`/`}}` inside
/// a Rust `format!` string. Equals `placeholder_for("__OKX_TASK_TITLE__")`.
pub const TITLE_PLACEHOLDER: &str = "{{__OKX_TASK_TITLE__}}";

/// The reserved placeholder for the list-label title (`title_display`). Kept
/// distinct from [`TITLE_PLACEHOLDER`] so the label title and the decision-copy
/// title stay independent. Equals
/// `placeholder_for("__OKX_TASK_LABEL_TITLE__")`.
pub const LABEL_TITLE_PLACEHOLDER: &str = "{{__OKX_TASK_LABEL_TITLE__}}";

/// Per-value cap on a decoded template value, in bytes.
///
/// Design note: there is no single shared `MAX_TITLE_LEN` constant in the tree —
/// the two existing title
/// caps are the private, char-based `MAX_TITLE_CHARS` (30 in `task/user/create.rs`,
/// 64 in `task/user/create_subscribe.rs`). A byte cap of 256 comfortably covers
/// the largest (a 64-char title is ≤ 256 bytes even in 4-byte UTF-8), so 256 is
/// used rather than importing a divergent private char count. This is
/// defense-in-depth; the real bound is
/// [`MAX_TEMPLATE_PAYLOAD_BYTES`].
pub const MAX_TEMPLATE_VALUE_LEN: usize = 256;

/// Hard cap on the total decoded Base64 payload (bytes), independent of key count.
pub const MAX_TEMPLATE_PAYLOAD_BYTES: usize = 8 * 1024;

/// Stable, machine-readable error codes surfaced via `output::error_coded`.
pub const CODE_VARS_INVALID: &str = "TEMPLATE_VARS_INVALID";
pub const CODE_VALUE_MISSING: &str = "TEMPLATE_VALUE_MISSING";
pub const CODE_PLACEHOLDER_MISSING: &str = "TEMPLATE_PLACEHOLDER_MISSING";

/// Errors from decode/validate/render. `Display` strings are intentionally
/// value-free — they NEVER embed the decoded title.
///
/// Hand-rolled `Display` + `Error` (rather than a `thiserror` derive) because
/// `thiserror` is NOT a dependency of `onchainos-cli`; this mirrors the in-tree
/// convention (`commands::sink::CodedError`,
/// `output::CliConfirming`).
#[derive(Debug, PartialEq, Eq)]
pub enum TemplateVarError {
    /// Bad Base64 / non-UTF-8 / non-object JSON / unknown key / non-string value /
    /// over-length value / over-cap payload / duplicate key. → `TEMPLATE_VARS_INVALID`.
    Invalid,
    /// Content declares `{{KEY}}` but no matching var was supplied. → `TEMPLATE_VALUE_MISSING`.
    ValueMissing,
    /// A var was supplied but its `{{KEY}}` placeholder is absent from the content.
    /// → `TEMPLATE_PLACEHOLDER_MISSING`.
    PlaceholderMissing,
}

impl TemplateVarError {
    /// Stable machine-readable code for the `errorCode` envelope field.
    pub fn code(&self) -> &'static str {
        match self {
            TemplateVarError::Invalid => CODE_VARS_INVALID,
            TemplateVarError::ValueMissing => CODE_VALUE_MISSING,
            TemplateVarError::PlaceholderMissing => CODE_PLACEHOLDER_MISSING,
        }
    }
}

impl std::fmt::Display for TemplateVarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Generic, value-free messages — never embed the decoded title.
        let msg = match self {
            TemplateVarError::Invalid => "template variables payload is invalid",
            TemplateVarError::ValueMissing => {
                "a declared placeholder has no matching template variable"
            }
            TemplateVarError::PlaceholderMissing => {
                "a supplied template variable has no matching placeholder"
            }
        };
        f.write_str(msg)
    }
}

impl std::error::Error for TemplateVarError {}

/// The literal placeholder string for a key: `{{KEY}}`.
fn placeholder_for(key: &str) -> String {
    format!("{PLACEHOLDER_OPEN}{key}{PLACEHOLDER_CLOSE}")
}

/// Newtype whose `Deserialize` rejects a non-object payload AND duplicate keys.
/// serde_json's default `Map` silently keeps the last value on duplicate keys, so
/// duplicate detection is done here via a `MapAccess` visitor.
struct RawVars(BTreeMap<String, serde_json::Value>);

impl<'de> Deserialize<'de> for RawVars {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = BTreeMap<String, serde_json::Value>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON object of template variables")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut out: BTreeMap<String, serde_json::Value> = BTreeMap::new();
                while let Some((k, v)) = map.next_entry::<String, serde_json::Value>()? {
                    if out.insert(k, v).is_some() {
                        return Err(de::Error::custom("duplicate template-var key"));
                    }
                }
                Ok(out)
            }
        }
        // `deserialize_map` fails on any non-object JSON (array / scalar / null).
        deserializer.deserialize_map(V).map(RawVars)
    }
}

/// Decode + validate the standard-charset Base64(JSON object) payload against the
/// whitelist and caps. Returns an ordered key→value map. Never logs or
/// embeds the decoded values. Every failure mode maps to
/// [`TemplateVarError::Invalid`] (`TEMPLATE_VARS_INVALID`).
pub fn decode_and_validate(b64: &str) -> Result<BTreeMap<String, String>, TemplateVarError> {
    // 1. Base64 (standard charset) → bytes.
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| TemplateVarError::Invalid)?;

    // 2. Total decoded payload cap (bytes), independent of key count.
    if bytes.len() > MAX_TEMPLATE_PAYLOAD_BYTES {
        return Err(TemplateVarError::Invalid);
    }

    // 3. Valid UTF-8.
    let text = std::str::from_utf8(&bytes).map_err(|_| TemplateVarError::Invalid)?;

    // 4. Parse as a JSON object; reject non-object and duplicate keys.
    let RawVars(raw) =
        serde_json::from_str::<RawVars>(text).map_err(|_| TemplateVarError::Invalid)?;

    // 5. Per-entry validation: whitelist keys + string values + per-value cap.
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in raw {
        if !TEMPLATE_VAR_WHITELIST.contains(&key.as_str()) {
            return Err(TemplateVarError::Invalid);
        }
        let s = match value {
            serde_json::Value::String(s) => s,
            _ => return Err(TemplateVarError::Invalid),
        };
        if s.len() > MAX_TEMPLATE_VALUE_LEN {
            return Err(TemplateVarError::Invalid);
        }
        out.insert(key, s);
    }

    Ok(out)
}

/// Single-pass, non-recursive, literal replacement of each `{{KEY}}` in every
/// `content` with its value, PLUS the bidirectional placeholder/var consistency
/// check across the combined content set.
///
/// `contents` is every string that may carry placeholders (`user_content` +
/// `list_label`) so a var with no placeholder in EITHER string is caught. The
/// bijection check runs FIRST; only then are the strings rendered. Returns the
/// rendered strings in the same order as `contents`, or a typed error.
///
/// The render pass scans each original string once left-to-right and appends the
/// replacement without rescanning it: a title whose own text is
/// `{{__OKX_TASK_TITLE__}}` is therefore substituted exactly once, never
/// re-expanded. `vars` keys are already ⊆ [`TEMPLATE_VAR_WHITELIST`] (guaranteed
/// by [`decode_and_validate`]).
pub fn render_all(
    contents: &[&str],
    vars: &BTreeMap<String, String>,
) -> Result<Vec<String>, TemplateVarError> {
    // ── Bijection check across the combined content set ─────────────────────
    for key in TEMPLATE_VAR_WHITELIST {
        let ph = placeholder_for(key);
        let present_in_content = contents.iter().any(|c| c.contains(&ph));
        let supplied = vars.contains_key(*key);
        match (supplied, present_in_content) {
            // Var supplied but no matching placeholder anywhere → placeholder missing.
            (true, false) => return Err(TemplateVarError::PlaceholderMissing),
            // Placeholder declared but no matching var supplied → value missing.
            (false, true) => return Err(TemplateVarError::ValueMissing),
            // Both present → will render; neither → nothing to do for this key.
            _ => {}
        }
    }

    Ok(contents.iter().map(|c| render_one(c, vars)).collect())
}

/// Single left-to-right pass over `content`, replacing each whitelisted `{{KEY}}`
/// with its value. Substituted text is never rescanned. Non-matching
/// `{{...}}` sequences are copied through verbatim.
fn render_one(content: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(content.len());
    let mut i = 0usize;
    while i < content.len() {
        let rest = &content[i..];
        if let Some(after_open) = rest.strip_prefix(PLACEHOLDER_OPEN) {
            if let Some(close_rel) = after_open.find(PLACEHOLDER_CLOSE) {
                let key = &after_open[..close_rel];
                if let Some(value) = vars.get(key) {
                    out.push_str(value);
                    // Advance past the whole `{{key}}`. `{{` and `}}` are ASCII and
                    // matched keys are ASCII whitelist entries, so this lands on a
                    // char boundary.
                    i += PLACEHOLDER_OPEN.len() + close_rel + PLACEHOLDER_CLOSE.len();
                    continue;
                }
            }
        }
        // Not a resolvable placeholder — copy one char, respecting UTF-8 boundaries.
        let ch = rest.chars().next().expect("i < len ⇒ at least one char");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Test-only helper: given a next-action block that carries a
/// `--template-vars-b64 "<b64>"` line, extract the Base64 value, decode + validate
/// it, and return the full decoded variable map. Panics if the flag is absent. The
/// Base64 (STANDARD alphabet) never contains `"`, so quote-delimited extraction is
/// unambiguous.
#[cfg(test)]
pub(crate) fn decode_emitted_vars(block: &str) -> BTreeMap<String, String> {
    const MARKER: &str = "--template-vars-b64 \"";
    let start = block
        .find(MARKER)
        .expect("emitted block must carry --template-vars-b64")
        + MARKER.len();
    let rest = &block[start..];
    let end = rest
        .find('"')
        .expect("emitted --template-vars-b64 value must be closed by a double quote");
    let b64 = &rest[..end];
    decode_and_validate(b64).expect("emitted Base64 title payload must decode + validate")
}

/// Test-only helper: the decoded decision-copy title (`__OKX_TASK_TITLE__`) from
/// an emitted block. Panics if the key is absent.
#[cfg(test)]
pub(crate) fn extract_emitted_title(block: &str) -> String {
    decode_emitted_vars(block)
        .get("__OKX_TASK_TITLE__")
        .cloned()
        .expect("emitted payload must carry the __OKX_TASK_TITLE__ var")
}

/// Test-only helper: the decoded list-label title (`__OKX_TASK_LABEL_TITLE__`)
/// from an emitted block. Panics if the key is absent.
#[cfg(test)]
pub(crate) fn extract_emitted_label_title(block: &str) -> String {
    decode_emitted_vars(block)
        .get("__OKX_TASK_LABEL_TITLE__")
        .cloned()
        .expect("emitted payload must carry the __OKX_TASK_LABEL_TITLE__ var")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Standard Base64 of a single-key title object — the emitter's wire format.
    fn b64_title(title: &str) -> String {
        let obj = serde_json::json!({ "__OKX_TASK_TITLE__": title });
        base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&obj).unwrap())
    }

    fn b64_of(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
    }

    // ── decode_and_validate: happy path ─────────────────────────────────────
    #[test]
    fn decode_happy_path() {
        let vars = decode_and_validate(&b64_title("Weekly Report")).unwrap();
        assert_eq!(
            vars.get("__OKX_TASK_TITLE__").map(String::as_str),
            Some("Weekly Report")
        );
        assert_eq!(vars.len(), 1);
    }

    // ── decode_and_validate: every TEMPLATE_VARS_INVALID trigger ─────────────
    #[test]
    fn decode_rejects_bad_base64() {
        assert_eq!(
            decode_and_validate("not_base64!!!"),
            Err(TemplateVarError::Invalid)
        );
    }

    #[test]
    fn decode_rejects_non_utf8() {
        // 0xFF 0xFE is not valid UTF-8.
        let b64 = base64::engine::general_purpose::STANDARD.encode([0xFFu8, 0xFE]);
        assert_eq!(decode_and_validate(&b64), Err(TemplateVarError::Invalid));
    }

    #[test]
    fn decode_rejects_non_object_json() {
        assert_eq!(
            decode_and_validate(&b64_of("[1,2,3]")),
            Err(TemplateVarError::Invalid)
        );
        assert_eq!(
            decode_and_validate(&b64_of("\"a string\"")),
            Err(TemplateVarError::Invalid)
        );
        assert_eq!(
            decode_and_validate(&b64_of("42")),
            Err(TemplateVarError::Invalid)
        );
        assert_eq!(
            decode_and_validate(&b64_of("null")),
            Err(TemplateVarError::Invalid)
        );
    }

    #[test]
    fn decode_rejects_unknown_key() {
        assert_eq!(
            decode_and_validate(&b64_of(r#"{"__EVIL__":"x"}"#)),
            Err(TemplateVarError::Invalid)
        );
    }

    #[test]
    fn decode_rejects_non_string_value() {
        assert_eq!(
            decode_and_validate(&b64_of(r#"{"__OKX_TASK_TITLE__":123}"#)),
            Err(TemplateVarError::Invalid)
        );
        assert_eq!(
            decode_and_validate(&b64_of(r#"{"__OKX_TASK_TITLE__":{"nested":1}}"#)),
            Err(TemplateVarError::Invalid)
        );
        assert_eq!(
            decode_and_validate(&b64_of(r#"{"__OKX_TASK_TITLE__":["a"]}"#)),
            Err(TemplateVarError::Invalid)
        );
        assert_eq!(
            decode_and_validate(&b64_of(r#"{"__OKX_TASK_TITLE__":true}"#)),
            Err(TemplateVarError::Invalid)
        );
        assert_eq!(
            decode_and_validate(&b64_of(r#"{"__OKX_TASK_TITLE__":null}"#)),
            Err(TemplateVarError::Invalid)
        );
    }

    #[test]
    fn decode_rejects_over_length_value() {
        let long = "a".repeat(MAX_TEMPLATE_VALUE_LEN + 1);
        assert_eq!(
            decode_and_validate(&b64_title(&long)),
            Err(TemplateVarError::Invalid)
        );
        // Exactly at the cap is accepted.
        let at_cap = "a".repeat(MAX_TEMPLATE_VALUE_LEN);
        assert!(decode_and_validate(&b64_title(&at_cap)).is_ok());
    }

    #[test]
    fn decode_rejects_over_cap_payload() {
        // A payload whose decoded bytes exceed MAX_TEMPLATE_PAYLOAD_BYTES. Use a
        // whitespace-padded valid JSON object so it is well-formed but oversized.
        let padding = " ".repeat(MAX_TEMPLATE_PAYLOAD_BYTES);
        let json = format!("{{\"__OKX_TASK_TITLE__\":\"x\"}}{padding}");
        assert!(json.len() > MAX_TEMPLATE_PAYLOAD_BYTES);
        assert_eq!(
            decode_and_validate(&b64_of(&json)),
            Err(TemplateVarError::Invalid)
        );
    }

    #[test]
    fn decode_rejects_duplicate_key() {
        // serde_json would silently keep the last value; RawVars must reject it.
        let dup = r#"{"__OKX_TASK_TITLE__":"a","__OKX_TASK_TITLE__":"b"}"#;
        assert_eq!(
            decode_and_validate(&b64_of(dup)),
            Err(TemplateVarError::Invalid)
        );
    }

    // ── render_all: bijection errors ─────────────────────────────────────────
    #[test]
    fn render_value_missing_when_placeholder_has_no_var() {
        let vars = BTreeMap::new(); // no var supplied
        let contents = ["title is {{__OKX_TASK_TITLE__}} here"];
        assert_eq!(
            render_all(&contents, &vars),
            Err(TemplateVarError::ValueMissing)
        );
    }

    #[test]
    fn render_placeholder_missing_when_var_has_no_placeholder() {
        let mut vars = BTreeMap::new();
        vars.insert("__OKX_TASK_TITLE__".to_string(), "Report".to_string());
        let contents = ["no placeholder here", "still none"];
        assert_eq!(
            render_all(&contents, &vars),
            Err(TemplateVarError::PlaceholderMissing)
        );
    }

    #[test]
    fn render_ok_when_placeholder_in_either_content() {
        let mut vars = BTreeMap::new();
        vars.insert("__OKX_TASK_TITLE__".to_string(), "Report".to_string());
        // Placeholder appears only in the SECOND content (list_label); still valid.
        let contents = [
            "plain user content",
            "[Decision 0xabc] {{__OKX_TASK_TITLE__}} decision",
        ];
        let out = render_all(&contents, &vars).unwrap();
        assert_eq!(out[0], "plain user content");
        assert_eq!(out[1], "[Decision 0xabc] Report decision");
    }

    #[test]
    fn render_no_vars_no_placeholders_is_noop() {
        let vars = BTreeMap::new();
        let contents = ["a", "b"];
        assert_eq!(
            render_all(&contents, &vars).unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    // ── Single-pass, non-recursive (value that IS a placeholder) ─────────────
    #[test]
    fn render_value_equal_to_placeholder_is_substituted_once() {
        let mut vars = BTreeMap::new();
        vars.insert(
            "__OKX_TASK_TITLE__".to_string(),
            "{{__OKX_TASK_TITLE__}}".to_string(),
        );
        let contents = ["X {{__OKX_TASK_TITLE__}} Y"];
        let out = render_all(&contents, &vars).unwrap();
        // Substituted exactly once: the injected placeholder text is NOT re-expanded.
        assert_eq!(out[0], "X {{__OKX_TASK_TITLE__}} Y");
    }

    #[test]
    fn render_multiple_occurrences_all_replaced_single_pass() {
        let mut vars = BTreeMap::new();
        vars.insert("__OKX_TASK_TITLE__".to_string(), "T".to_string());
        let contents = ["{{__OKX_TASK_TITLE__}} and {{__OKX_TASK_TITLE__}}"];
        assert_eq!(render_all(&contents, &vars).unwrap()[0], "T and T");
    }

    // ── i18n / metacharacter round-trip ──────────────────────────────────────
    #[test]
    fn i18n_and_metacharacter_round_trip() {
        for title in [
            // CJK title (U+4E2D U+6587 U+6807 U+9898) + rocket emoji, as \u escapes:
            // no raw CJK bytes in source (onchainos_check CJK lint), identical String
            // at runtime.
            "\u{4e2d}\u{6587}\u{6807}\u{9898}\u{1f680}",
            "Oli's task",
            "line1\nline2",
            "`id`",
            "$(touch /tmp/x)",
            "\"; id; #",
            "a\\b",
            // grinning emoji + mixed ASCII/CJK (U+4E2D U+6587), \u escapes
            "emoji \u{1f600} mix \u{4e2d}\u{6587}",
        ] {
            let vars = decode_and_validate(&b64_title(title)).unwrap();
            assert_eq!(
                vars.get("__OKX_TASK_TITLE__").map(String::as_str),
                Some(title)
            );
            let contents = ["[Decision 0x1] {{__OKX_TASK_TITLE__}} decision"];
            let out = render_all(&contents, &vars).unwrap();
            assert_eq!(out[0], format!("[Decision 0x1] {title} decision"));
        }
    }

    // ── code() mapping ───────────────────────────────────────────────────────
    #[test]
    fn code_mapping_is_stable() {
        assert_eq!(TemplateVarError::Invalid.code(), "TEMPLATE_VARS_INVALID");
        assert_eq!(
            TemplateVarError::ValueMissing.code(),
            "TEMPLATE_VALUE_MISSING"
        );
        assert_eq!(
            TemplateVarError::PlaceholderMissing.code(),
            "TEMPLATE_PLACEHOLDER_MISSING"
        );
    }

    #[test]
    fn title_placeholder_const_matches_helper() {
        assert_eq!(TITLE_PLACEHOLDER, placeholder_for("__OKX_TASK_TITLE__"));
        assert_eq!(
            LABEL_TITLE_PLACEHOLDER,
            placeholder_for("__OKX_TASK_LABEL_TITLE__")
        );
    }

    // Both whitelisted keys render independently when both placeholders + vars are
    // present (the sub_user_reject label-vs-copy split). A single-pass render must
    // substitute each key with its own value even when the values differ.
    #[test]
    fn render_two_keys_independently() {
        let mut vars = BTreeMap::new();
        vars.insert("__OKX_TASK_TITLE__".to_string(), "Copy Title".to_string());
        vars.insert(
            "__OKX_TASK_LABEL_TITLE__".to_string(),
            "<title>".to_string(),
        );
        let contents = [
            "copy: {{__OKX_TASK_TITLE__}}",
            "label: {{__OKX_TASK_LABEL_TITLE__}}",
        ];
        let out = render_all(&contents, &vars).unwrap();
        assert_eq!(out[0], "copy: Copy Title");
        assert_eq!(out[1], "label: <title>");
    }

    #[test]
    fn error_display_never_embeds_a_value() {
        // Display strings must be generic — no decoded value leaks.
        assert_eq!(
            format!("{}", TemplateVarError::Invalid),
            "template variables payload is invalid"
        );
        assert!(!format!("{}", TemplateVarError::Invalid).contains("__OKX"));
    }
}
