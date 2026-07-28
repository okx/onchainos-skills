//! Per-job user-language marker for CLI-rendered copy (autotrade decision
//! cards + swap self-notify).
//!
//! Problem: those artifacts are pushed **deterministically by the CLI** (no
//! LLM in between — see `pending_v2::push_decision_direct` and
//! `autotrade::notify`), so nobody can translate them at delivery time.
//! Solution: capture the user's language **earlier, deterministically** — every
//! decision reply flows through `pending-decisions-v2 resolve*` carrying the
//! user's verbatim words, and a CJK sniff on that text needs no LLM. Every
//! replay/execution direct-push happens after at least one reply, so the
//! marker is in place for those. Known phase-1 gap: the FIRST consent card of
//! a brand-new machine renders before any reply exists and falls back
//! global-default → English (subscribe-time capture is the phase-2 fix); the
//! global marker means any earlier job on the machine already fixes this.
//!
//! Resolution order: per-job marker → machine-global `_default` (last language
//! seen in ANY job) → English.
//!
//! Storage: `<onchainos_home>/autotrade/lang/<jobId>` and `.../lang/_default`,
//! each holding the literal string `zh` or `en`. Phase-1 scope is zh/en only;
//! arbitrary-language support is the pre-translated-template follow-up
//! (`prefilled_notify` pattern).

use std::path::PathBuf;

/// The two copy variants shipped in the binary (phase 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

/// `<onchainos_home>/autotrade/lang/<name>`. `name` is either a
/// charset-checked jobId or the literal `_default`.
fn marker_path(name: &str) -> Option<PathBuf> {
    let home = crate::home::onchainos_home().ok()?;
    Some(home.join("autotrade").join("lang").join(name))
}

/// Path-traversal defense, same charset rule as the other autotrade per-job
/// stores: a jobId may only contain `[A-Za-z0-9_-]`.
fn job_id_ok(job_id: &str) -> bool {
    !job_id.is_empty()
        && job_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Deterministic language sniff on the user's verbatim reply.
///
/// - Any CJK ideograph → `Zh`.
/// - Otherwise ≥3 LOWERCASE ASCII letters in language-bearing tokens → `En`
///   ("install please", "yes", "skip it").
/// - Otherwise `None` — language-neutral replies must NOT flip the marker.
///   Neutral covers option letters ("A"), amounts ("100u"), AND
///   identifier-ish tokens that carry no language signal: 0x addresses / tx
///   hashes (hex a–f would otherwise count as letters and flip a Chinese
///   user to English globally), pure-hex words, URLs/emails, and
///   ticker-style all-caps tokens ("100USDT", "SKIP").
pub fn detect(text: &str) -> Option<Lang> {
    let mut lowercase_letters = 0usize;
    for token in text.split_whitespace() {
        if token
            .chars()
            .any(|c| matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}'))
        {
            return Some(Lang::Zh);
        }
        let t = token.trim_matches(|c: char| c.is_ascii_punctuation());
        if t.is_empty() {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("0x")
            || t.contains("://")
            || t.contains('@')
            || t.chars().all(|c| c.is_ascii_hexdigit())
        {
            continue;
        }
        lowercase_letters += t.chars().filter(|c| c.is_ascii_lowercase()).count();
    }
    (lowercase_letters >= 3).then_some(Lang::En)
}

/// Sniff `text` and persist the result for `job_id` (+ refresh the global
/// fallback). Best-effort: a marker write must never fail the caller's flow.
pub fn record_from_user_text(job_id: &str, text: &str) {
    let Some(lang) = detect(text) else { return };
    let tag = match lang {
        Lang::Zh => "zh",
        Lang::En => "en",
    };
    if job_id_ok(job_id) {
        if let Some(p) = marker_path(job_id) {
            let _ = crate::home::write_secure(&p, tag.as_bytes());
        }
    }
    if let Some(p) = marker_path("_default") {
        let _ = crate::home::write_secure(&p, tag.as_bytes());
    }
}

/// The language to render CLI-pushed copy in: per-job → global → `En`.
pub fn resolve(job_id: &str) -> Lang {
    let read = |name: &str| -> Option<Lang> {
        let p = marker_path(name)?;
        match std::fs::read_to_string(p).ok()?.trim() {
            "zh" => Some(Lang::Zh),
            "en" => Some(Lang::En),
            _ => None,
        }
    };
    if job_id_ok(job_id) {
        if let Some(l) = read(job_id) {
            return l;
        }
    }
    read("_default").unwrap_or(Lang::En)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_cjk_wins_and_neutral_replies_are_none() {
        assert_eq!(detect("跳过本次"), Some(Lang::Zh));
        assert_eq!(detect("A 自动,每笔100"), Some(Lang::Zh));
        assert_eq!(detect("install and execute"), Some(Lang::En));
        assert_eq!(detect("yes"), Some(Lang::En));
        // Language-neutral: single option letters / amounts must not flip the marker.
        assert_eq!(detect("A"), None);
        assert_eq!(detect("b"), None);
        assert_eq!(detect("100u"), None);
        assert_eq!(detect("100"), None);
        assert_eq!(detect(""), None);
        // Identifier-ish replies carry no language signal: a Chinese user pasting
        // a token address / hash / URL / ticker must NOT flip to English.
        assert_eq!(detect("0x8f3A9bDeadBeef00112233445566778899aabbcc"), None);
        assert_eq!(detect("deadbeef"), None); // pure hex word
        assert_eq!(detect("100USDT"), None);
        assert_eq!(detect("SKIP"), None); // all-caps ticker-style
        assert_eq!(detect("https://www.okx.com/trade"), None);
        assert_eq!(detect("nico@okx.com"), None);
        // …but real words around identifiers still detect.
        assert_eq!(detect("buy 100USDT now please"), Some(Lang::En));
        assert_eq!(detect("买 0x8f3A9bDeadBeef"), Some(Lang::Zh));
    }

    #[test]
    fn resolve_layers_job_then_default_then_en() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("user_lang_resolve");
        std::fs::remove_dir_all(&dir).ok();
        std::env::set_var("ONCHAINOS_HOME", &dir);

        // Nothing recorded → En.
        assert_eq!(resolve("job1"), Lang::En);
        // Neutral reply records nothing.
        record_from_user_text("job1", "A");
        assert_eq!(resolve("job1"), Lang::En);
        // Chinese reply → per-job zh + global default zh.
        record_from_user_text("job1", "A 自动,每笔100");
        assert_eq!(resolve("job1"), Lang::Zh);
        // Unseen job falls back to the refreshed global default.
        assert_eq!(resolve("job2"), Lang::Zh);
        // English reply on job2 flips job2 AND the default; job1 keeps zh.
        record_from_user_text("job2", "skip this trade");
        assert_eq!(resolve("job2"), Lang::En);
        assert_eq!(resolve("job1"), Lang::Zh);
        assert_eq!(resolve("job3"), Lang::En);
        // Path-traversal-ish job ids never touch per-job files.
        record_from_user_text("../evil", "跳过");
        assert_eq!(resolve("../evil"), Lang::Zh); // global default only

        std::env::remove_var("ONCHAINOS_HOME");
        std::fs::remove_dir_all(&dir).ok();
    }
}
