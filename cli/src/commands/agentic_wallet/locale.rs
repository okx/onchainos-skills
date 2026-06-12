//! Locale normalization + system-locale detection.
//!
//! Pure, shared helpers consumed by `cmd_login` (email path) and
//! `cmd_login_ak` (AK path) to assemble both the wire-level `locale`
//! field and the new camelCase `sysLocale` field on the two auth
//! endpoints. No I/O except the single `sys_locale::get_locale()` call
//! inside `detect_system_locale`; the `normalize_from_raw` seam keeps
//! both branches unit-testable without depending on the host OS.
//!
//! See `oli-docs/.../spec.md` Appendix A/B/C and `fe-design-doc.md` §3.

/// Canonical wire value for any input that resolves to "Chinese" via
/// the §3 normalization rules.
pub const CANONICAL_ZH_CN: &str = "zh_CN";

/// Canonical wire value for any input that resolves to "English" via
/// the §3 normalization rules.
pub const CANONICAL_EN_US: &str = "en_US";

/// Maximum permitted length for a cleaned locale string. Inputs that
/// exceed this length after the `trim` / encoding-suffix / `-`→`_`
/// cleanup phase are rejected by `normalize_locale` (returns `None`).
const MAX_LOCALE_LEN: usize = 35;

/// Chinese / Sinitic primary-subtag aliases that map to `zh_CN`.
/// Region / script aliases plus ISO 639 zh-macrolanguage members.
const CHINESE_ALIASES: &[&str] = &[
    // Region / script aliases
    "cn", "tw", "hk", "mo", "hans", "hant", "chs", "cht", "chinese",
    // Sinitic language codes (zh macrolanguage members)
    "cmn", "yue", "wuu", "nan", "hak", "gan", "hsn",
];

/// English / English-region primary-subtag aliases that map to `en_US`.
/// Language aliases plus English-region bare codes.
///
/// Code-collision note: `ca` (Catalan), `sg` (Sango), `in` (old
/// Indonesian) are both language and region codes. Per FE-TDD §3.1 we
/// treat them as English-region. Low risk because `sys-locale` returns
/// BCP-47 tags with an explicit primary subtag.
const ENGLISH_ALIASES: &[&str] = &[
    "english", "eng", "us", "uk", "gb", "au", "ca", "nz", "ie", "in", "ph", "sg", "za",
];

/// POSIX placeholder locales — treated as "no readable locale" so the
/// caller can omit the wire field (or fall back, on the AK path).
const POSIX_PLACEHOLDERS: &[&str] = &["C", "POSIX"];

/// Normalize a free-form locale string to one of:
/// - `Some("zh_CN")` for any Chinese/Sinitic variant,
/// - `Some("en_US")` for any English/English-region variant,
/// - `Some(<cleaned>)` (pass-through) for other valid locales
///   (e.g. `ja-JP` → `ja_JP`, `ko_KR` → `ko_KR`, `fr-FR` → `fr_FR`),
/// - `None` for empty / overlong / illegal-char / POSIX-placeholder
///   inputs.
///
/// Steps (FE-TDD §3.1):
/// 1. `trim`; take substring before first `.` (strip encoding suffix
///    such as `.UTF-8`); replace `-` with `_`.
/// 2. Sanitize: if cleaned len > 35 or contains chars outside
///    `[A-Za-z0-9_]` → `None`. `C` / `POSIX` placeholders → `None`.
/// 3. Primary subtag = substring before first `_`, lowercased.
/// 4. Primary starts with `"zh"` OR in Chinese table → `Some("zh_CN")`.
/// 5. Primary starts with `"en"` OR in English table → `Some("en_US")`.
/// 6. Otherwise → `Some(<cleaned>)` (pass-through).
///
/// Pure function: no I/O, no allocations beyond the returned `String`.
pub(crate) fn normalize_locale(input: &str) -> Option<String> {
    // Step 1: trim, strip encoding suffix, replace `-` with `_`.
    let trimmed = input.trim();
    let before_dot = match trimmed.find('.') {
        Some(idx) => &trimmed[..idx],
        None => trimmed,
    };
    let cleaned: String = before_dot.replace('-', "_");

    let result = normalize_cleaned(&cleaned);

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] normalize_locale: input={input}, result={result:?}");
    }
    result
}

/// Inner step-2..6 helper; takes the already-cleaned string (after
/// trim / encoding-strip / `-`→`_`). Split out so the debug-log site
/// stays at one place in `normalize_locale`.
fn normalize_cleaned(cleaned: &str) -> Option<String> {
    // Step 2a: POSIX placeholder rejection (case-sensitive — only
    // exact `C` / `POSIX` are placeholders; lowercase `c` passes the
    // alphabet check and falls through as `Some("c")` at step 6).
    if POSIX_PLACEHOLDERS.contains(&cleaned) {
        return None;
    }

    // Step 2b: sanitize length + alphabet.
    if cleaned.is_empty() || cleaned.len() > MAX_LOCALE_LEN {
        return None;
    }
    if !cleaned
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }

    // Step 3: primary subtag = substring before first `_`, lowercased.
    let primary = match cleaned.find('_') {
        Some(idx) => &cleaned[..idx],
        None => cleaned,
    };
    let primary_lower = primary.to_ascii_lowercase();

    // Step 4: Chinese / Sinitic.
    if primary_lower.starts_with("zh") || CHINESE_ALIASES.contains(&primary_lower.as_str()) {
        return Some(CANONICAL_ZH_CN.to_string());
    }

    // Step 5: English / English-region.
    if primary_lower.starts_with("en") || ENGLISH_ALIASES.contains(&primary_lower.as_str()) {
        return Some(CANONICAL_EN_US.to_string());
    }

    // Step 6: pass-through with the cleaned form.
    Some(cleaned.to_string())
}

/// Detect the operating-system locale via `sys_locale::get_locale()`
/// and run the result through `normalize_locale`.
///
/// Returns `None` when the OS locale is unreadable or normalizes to
/// `None`. The caller decides how to handle `None` — `cmd_login`
/// silently omits the `sysLocale` body field; the `--locale` AK path
/// falls back to `CANONICAL_EN_US`.
pub(crate) fn detect_system_locale() -> Option<String> {
    let raw = sys_locale::get_locale();
    let result = normalize_from_raw(raw.as_deref());

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] detect_system_locale: raw={raw:?}, normalized={result:?}");
    }
    result
}

/// Injectable seam for unit-testing `detect_system_locale` without
/// depending on the host OS locale. Takes the raw `Option<&str>`
/// the OS layer would have returned and pipes it through
/// `normalize_locale`. `None` propagates as `None`.
pub(crate) fn normalize_from_raw(raw: Option<&str>) -> Option<String> {
    raw.and_then(normalize_locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_locale → Some("zh_CN") ────────────────────────────

    #[test]
    fn normalize_locale_zh_bare_primary() {
        assert_eq!(normalize_locale("zh"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_zh_hans_script() {
        assert_eq!(normalize_locale("zh-Hans"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_zh_hant_tw_script_region() {
        assert_eq!(normalize_locale("zh-Hant-TW"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_zh_cn_with_utf8_encoding_suffix() {
        // POSIX form with `.UTF-8` encoding suffix is stripped at step 1.
        assert_eq!(normalize_locale("zh_CN.UTF-8"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_apple_cflocale_zh_hans_cn() {
        assert_eq!(normalize_locale("zh_Hans_CN"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_windows_legacy_zh_chs() {
        assert_eq!(normalize_locale("zh-CHS"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_windows_legacy_zh_cht() {
        assert_eq!(normalize_locale("zh-CHT"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_bare_cn_alias() {
        assert_eq!(normalize_locale("cn"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_chinese_word_alias() {
        assert_eq!(normalize_locale("chinese"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_sinitic_yue_hant_hk() {
        // Sinitic language code with script + region: still → zh_CN.
        assert_eq!(normalize_locale("yue-Hant-HK"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_sinitic_yue_bare() {
        assert_eq!(normalize_locale("yue"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_sinitic_wuu() {
        assert_eq!(normalize_locale("wuu"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_sinitic_nan() {
        assert_eq!(normalize_locale("nan"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_sinitic_hak() {
        assert_eq!(normalize_locale("hak"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_sinitic_gan() {
        assert_eq!(normalize_locale("gan"), Some("zh_CN".to_string()));
    }

    #[test]
    fn normalize_locale_sinitic_hsn() {
        assert_eq!(normalize_locale("hsn"), Some("zh_CN".to_string()));
    }

    // ── normalize_locale → Some("en_US") ────────────────────────────

    #[test]
    fn normalize_locale_en_bare_primary() {
        assert_eq!(normalize_locale("en"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_en_us_bcp47() {
        assert_eq!(normalize_locale("en-US"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_en_gb_posix() {
        assert_eq!(normalize_locale("en_GB"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_en_029_caribbean() {
        // Numeric region subtag — still maps to en_US.
        assert_eq!(normalize_locale("en-029"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_english_word_alias() {
        assert_eq!(normalize_locale("english"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_eng_alias() {
        assert_eq!(normalize_locale("eng"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_bare_us_region() {
        assert_eq!(normalize_locale("us"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_bare_uk_region() {
        assert_eq!(normalize_locale("uk"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_bare_gb_region() {
        assert_eq!(normalize_locale("gb"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_bare_au_region() {
        assert_eq!(normalize_locale("au"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_bare_nz_region() {
        assert_eq!(normalize_locale("nz"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_bare_ie_region() {
        assert_eq!(normalize_locale("ie"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_bare_ph_region() {
        assert_eq!(normalize_locale("ph"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_bare_za_region() {
        assert_eq!(normalize_locale("za"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_bare_sg_alias() {
        // "sg" is both Singapore (English-region, in ENGLISH_ALIASES) and Sango (ISO 639);
        // the alias table treats it as English-region per FE-TDD §3.1 code-collision policy.
        assert_eq!(normalize_locale("sg"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_bare_ca_alias() {
        // "ca" is both Canada (English-region, in ENGLISH_ALIASES) and Catalan (ISO 639);
        // the alias table treats it as English-region per FE-TDD §3.1 code-collision policy.
        assert_eq!(normalize_locale("ca"), Some("en_US".to_string()));
    }

    #[test]
    fn normalize_locale_bare_in_alias() {
        // "in" is the legacy ISO 639 code for Indonesian (superseded by "id"),
        // also used as a region alias in ENGLISH_ALIASES;
        // the alias table treats it as English-region per FE-TDD §3.1 code-collision policy.
        assert_eq!(normalize_locale("in"), Some("en_US".to_string()));
    }

    // ── normalize_locale pass-through (other valid locales) ────────

    #[test]
    fn normalize_locale_passthrough_ja_jp_bcp47() {
        assert_eq!(normalize_locale("ja-JP"), Some("ja_JP".to_string()));
    }

    #[test]
    fn normalize_locale_passthrough_ko_kr_posix() {
        assert_eq!(normalize_locale("ko_KR"), Some("ko_KR".to_string()));
    }

    #[test]
    fn normalize_locale_passthrough_fr_fr_bcp47() {
        assert_eq!(normalize_locale("fr-FR"), Some("fr_FR".to_string()));
    }

    #[test]
    fn normalize_locale_passthrough_unknown_uuu111() {
        // Non-zh, non-en primary subtag with arbitrary alnum content
        // passes through unchanged (cleaning is a no-op here).
        assert_eq!(normalize_locale("uuu111"), Some("uuu111".to_string()));
    }

    // ── normalize_locale → None ────────────────────────────────────

    #[test]
    fn normalize_locale_empty_string_returns_none() {
        assert_eq!(normalize_locale(""), None);
    }

    #[test]
    fn normalize_locale_whitespace_only_returns_none() {
        // After trim, "   " is empty → None.
        assert_eq!(normalize_locale("   "), None);
    }

    #[test]
    fn normalize_locale_overlong_string_returns_none() {
        // 36 chars after cleaning (>35 limit) → None. Pure ASCII alpha
        // so the alphabet check would pass; only the length rule fires.
        let overlong = "a".repeat(36);
        assert_eq!(normalize_locale(&overlong), None);
    }

    #[test]
    fn normalize_locale_sql_injection_attempt_returns_none() {
        // `; ` introduces non-`[A-Za-z0-9_]` characters → None.
        assert_eq!(normalize_locale("zh_CN; DROP"), None);
    }

    #[test]
    fn normalize_locale_html_tag_injection_returns_none() {
        // `<script>` contains `<` and `>` → outside alphabet → None.
        assert_eq!(normalize_locale("en<script>"), None);
    }

    #[test]
    fn normalize_locale_posix_placeholder_c_returns_none() {
        assert_eq!(normalize_locale("C"), None);
    }

    #[test]
    fn normalize_locale_posix_placeholder_posix_returns_none() {
        assert_eq!(normalize_locale("POSIX"), None);
    }

    // ── detect_system_locale seam coverage ─────────────────────────

    #[test]
    fn normalize_from_raw_none_propagates_as_none() {
        // Branch A: OS layer returned None (locale unreadable).
        assert_eq!(normalize_from_raw(None), None);
    }

    #[test]
    fn normalize_from_raw_some_pipes_through_normalize_locale() {
        // Branch B: OS layer returned Some(...) — verify it is piped
        // through normalize_locale (Chinese-mapping case proves the
        // wiring works, not just an identity pass-through).
        assert_eq!(
            normalize_from_raw(Some("zh-Hans")),
            Some("zh_CN".to_string()),
        );
    }

    #[test]
    fn normalize_from_raw_some_passthrough_branch() {
        assert_eq!(normalize_from_raw(Some("ja-JP")), Some("ja_JP".to_string()));
    }

    #[test]
    fn normalize_from_raw_some_with_normalize_none_returns_none() {
        // Branch C: OS layer returned an unusable string (POSIX
        // placeholder) — normalize_locale returns None, and
        // normalize_from_raw propagates that.
        assert_eq!(normalize_from_raw(Some("C")), None);
    }

    // ── constants ──────────────────────────────────────────────────

    #[test]
    fn canonical_constants_match_wire_contract() {
        // Spec App. C: wire contract is underscore form `zh_CN`/`en_US`.
        assert_eq!(CANONICAL_ZH_CN, "zh_CN");
        assert_eq!(CANONICAL_EN_US, "en_US");
    }
}
