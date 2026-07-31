//! The `device-name` request header: the OS device display name, read live and
//! never persisted, reported as raw un-encoded UTF-8.

use std::sync::OnceLock;

/// Reported when the OS name normalizes to nothing.
const UNKNOWN_DEVICE: &str = "unknown-device";

/// Upper bound on the reported name, in bytes.
const DEVICE_NAME_MAX_BYTES: usize = 128;

static DEVICE_NAME: OnceLock<String> = OnceLock::new();

/// Returns the normalized device name, reading it from the OS on the first call
/// and serving a memoized value afterwards.
///
/// Always non-empty and always valid as a header value.
pub fn get_cached_device_name() -> &'static str {
    DEVICE_NAME.get_or_init(|| {
        let name = normalize_device_name(&whoami::devicename());
        if cfg!(feature = "debug-log") {
            eprintln!("[DEBUG] device-name: resolved ({} bytes)", name.len());
        }
        name
    })
}

/// Truncates `s` to at most `max_bytes`, cutting on a UTF-8 character boundary
/// so a multi-byte sequence is never split.
fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Normalizes a raw OS device name: trim, strip control characters, fall back to
/// [`UNKNOWN_DEVICE`] if nothing survives, then cap at [`DEVICE_NAME_MAX_BYTES`].
///
/// The strip covers the whole Unicode `Cc` category, including C1
/// (`U+0080–U+009F`). C1 encodes to bytes `>= 0x80`, which `HeaderValue` accepts,
/// so a C0-only strip would let `U+0085` NEL through to the header.
fn normalize_device_name(raw: &str) -> String {
    let cleaned: String = raw.trim().chars().filter(|c| !c.is_control()).collect();
    if cleaned.is_empty() {
        return UNKNOWN_DEVICE.to_string();
    }
    truncate_on_char_boundary(&cleaned, DEVICE_NAME_MAX_BYTES).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_non_ascii_raw_unencoded() {
        let name = normalize_device_name("张三的电脑");
        assert_eq!(name, "张三的电脑");
        assert!(!name.contains('%'));
    }

    #[test]
    fn preserves_emoji_and_spaces_raw() {
        let name = normalize_device_name("💻 Zhang MacBook Pro");
        assert_eq!(name, "💻 Zhang MacBook Pro");
        assert!(!name.contains('%') && !name.contains('+'));
    }

    #[test]
    fn strips_control_chars_no_injection() {
        let name = normalize_device_name("host\r\n\tX-Injected:\u{7f} evil");
        assert_eq!(name, "hostX-Injected: evil");
        assert!(!name.chars().any(|c| c.is_control()));
        assert!(!name.contains("%0D") && !name.contains("%0A"));
    }

    #[test]
    fn strips_c1_control_chars() {
        let name = normalize_device_name("host\u{85}\u{80}\u{9f}name");
        assert_eq!(name, "hostname");
    }

    #[test]
    fn all_control_falls_back_to_unknown_device() {
        assert_eq!(normalize_device_name("\r\n\t"), UNKNOWN_DEVICE);
        assert_eq!(normalize_device_name("\u{1}\u{2}\u{7f}"), UNKNOWN_DEVICE);
        assert_eq!(normalize_device_name("\u{85}\u{9f}"), UNKNOWN_DEVICE);
    }

    #[test]
    fn empty_falls_back_to_unknown_device() {
        assert_eq!(normalize_device_name(""), UNKNOWN_DEVICE);
        assert_eq!(normalize_device_name("   "), UNKNOWN_DEVICE);
    }

    #[test]
    fn literal_percent_is_left_raw() {
        assert_eq!(normalize_device_name("50%-battery"), "50%-battery");
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(normalize_device_name("  Jose-MBP  "), "Jose-MBP");
    }

    #[test]
    fn truncates_at_utf8_boundary_within_byte_cap() {
        // '电' is 3 bytes: 42 fit in 126, the 43rd would exceed the 128 cap.
        let name = normalize_device_name(&"电".repeat(500));
        assert!(name.len() <= DEVICE_NAME_MAX_BYTES);
        assert!(name.chars().all(|c| c == '电'));
        assert_eq!(name.chars().count(), 42);

        let at_cap = "a".repeat(DEVICE_NAME_MAX_BYTES);
        assert_eq!(normalize_device_name(&at_cap).len(), DEVICE_NAME_MAX_BYTES);
        assert_eq!(normalize_device_name("abc"), "abc");
    }

    #[test]
    fn normalized_value_is_always_a_valid_header_value() {
        for raw in [
            "张三的电脑",
            "💻 Zhang MacBook Pro",
            "host\r\n\tevil",
            "\u{85}\u{9f}",
            "",
            "50%-battery",
            &"电".repeat(500),
        ] {
            let name = normalize_device_name(raw);
            assert!(
                reqwest::header::HeaderValue::from_bytes(name.as_bytes()).is_ok(),
                "invalid header value: {name:?}"
            );
        }
    }

    #[test]
    fn cached_value_is_stable_and_usable() {
        let first = get_cached_device_name();
        assert!(!first.is_empty());
        assert!(first.len() <= DEVICE_NAME_MAX_BYTES);
        assert!(!first.chars().any(|c| c.is_control()));
        assert!(reqwest::header::HeaderValue::from_bytes(first.as_bytes()).is_ok());
        assert_eq!(first, get_cached_device_name());
    }
}
