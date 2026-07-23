//! Device-ID derivation, validation, and process-lifetime caching for
//! onchainos.
//!
//! The stable per-device identifier sent as a request header. Two layers:
//!
//! Pure, I/O-free building blocks:
//! - `derive_device_id` — spec §A.3 recipe:
//!   `hex_lower(sha256(utf8(machine_id) || utf8("onchainos")))`.
//! - `generate_fallback_uuid` — spec §A.4 fallback: a random UUIDv4 string.
//! - `is_valid_device_id` — spec §A.5 / §11.3 L1 read-validation guard.
//!
//! Stateful / I/O layer (spec §9.1 / §A.1 / §A.2):
//! - `get_cached_device_id` — public entry point; memoized via `OnceLock`.
//! - `ensure_device_id` — get-or-create pipeline (keyring read + validate,
//!   else generate).
//! - `generate_device_id` — `machine_uid`/sha256 or UUID fallback, then
//!   best-effort keyring persist.
//!
//! Value format (spec §4.3): a 64-char lowercase-hex SHA-256 digest OR a
//! 36-char UUIDv4, always pure ASCII.

use std::sync::OnceLock;

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Fixed namespace suffix concatenated after the machine id before hashing
/// (spec §5.2 — never changed).
const NAMESPACE_SUFFIX: &[u8] = b"onchainos";

/// Keyring key under which the derived device id is persisted (spec §8.3).
///
/// Lives in the existing unified blob (`SERVICE="onchainos"`,
/// `UNIFIED_KEY="agentic-wallet"`) alongside the auth credentials; the
/// device id is explicitly non-sensitive (forgeable, non-credential).
const KEYRING_KEY: &str = "device_id";

/// Process-lifetime memoization cell for the device id (spec §9.1 / §A.6).
///
/// `Some(id)` on successful computation, `None` on total failure. Failure is
/// memoized — there is no retry within the process. `OnceLock` is `Sync` and
/// guarantees exactly-once initialization even under concurrent access from
/// multiple tokio tasks.
static DEVICE_ID: OnceLock<Option<String>> = OnceLock::new();

/// Returns the cached device id, computing it on the first call (spec §A.1).
///
/// The first call runs the get-or-create pipeline exactly once (keyring read,
/// optional `machine_uid::get()`, optional keyring write); every subsequent
/// call is a pure memory read from the `OnceLock`. Returns `None` when
/// computation failed, in which case the caller skips the `device-id` header.
pub fn get_cached_device_id() -> Option<&'static str> {
    DEVICE_ID.get_or_init(ensure_device_id).as_deref()
}

/// Get-or-create pipeline for the device id (spec §A.2).
///
/// Reads the persisted value from the keyring: a valid value is returned as-is
/// (no regeneration); a missing or invalid value falls through to
/// `generate_device_id`. Never panics — keyring failures are absorbed
/// (best-effort contract §3.3).
fn ensure_device_id() -> Option<String> {
    if let Some(existing) = crate::keyring_store::get_opt(KEYRING_KEY) {
        if is_valid_device_id(&existing) {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG] device-id: loaded from keyring cache");
            }
            return Some(existing);
        }
    }
    generate_device_id()
}

/// Generate a fresh device id and best-effort persist it (spec §A.2 / §A.3).
///
/// Prefers the sha256 derivation over the raw `machine_uid::get()` value; on
/// any `machine_uid` error falls back to a random UUIDv4. The raw machine id is
/// used only as sha256 input — never logged, stored, or transmitted (spec
/// §5.2). The generated value is returned even when persistence fails (still
/// usable for this process); the only `None` case is a generation failure,
/// which cannot happen because UUIDv4 generation is infallible.
fn generate_device_id() -> Option<String> {
    let value = match machine_uid::get() {
        Ok(machine_id) => {
            let derived = derive_device_id(&machine_id);
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG] device-id: generated via sha256 (64 chars)");
            }
            derived
        }
        Err(_) => {
            let fallback = generate_fallback_uuid();
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG] device-id: generated via uuid fallback (36 chars)");
            }
            fallback
        }
    };

    match crate::keyring_store::set(KEYRING_KEY, &value) {
        Ok(()) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG] device-id: persisted to keyring");
            }
        }
        Err(_) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG] device-id: persist failed, value usable this process only");
            }
        }
    }

    Some(value)
}

/// Derive a stable device id from a machine identifier (spec §A.3).
///
/// Feeds `machine_id`'s UTF-8 bytes directly followed by `b"onchainos"` (no
/// separator) into SHA-256 and returns the lowercase-hex digest — exactly 64
/// ASCII chars in `[0-9a-f]`.
fn derive_device_id(machine_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(machine_id.as_bytes());
    hasher.update(NAMESPACE_SUFFIX);
    hex::encode(hasher.finalize())
}

/// Generate a random UUIDv4 fallback device id (spec §A.4).
///
/// Returns a 36-char hyphenated lowercase UUID string, used when no stable
/// machine identifier is available.
fn generate_fallback_uuid() -> String {
    Uuid::new_v4().to_string()
}

/// L1 read-validation guard for a persisted device id (spec §A.5 / §11.3).
///
/// A value is valid iff it is (1) non-empty, (2) exactly 64 chars (sha256 hex)
/// or exactly 36 chars (UUIDv4), and (3) composed only of ASCII alphanumerics
/// or `-` (UUID hyphens). Anything else is treated as absent → regenerate.
fn is_valid_device_id(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let len = s.len();
    if len != 64 && len != 36 {
        return false;
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn sha256_derivation_recipe() {
        // Known digest of sha256("test-machine-id" + "onchainos").
        let expected = "d0f3de61e3704af433758f3ea65c56723b37354844eceaffdd6a229dd76f2289";
        let actual = derive_device_id("test-machine-id");
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 64);
        assert!(actual
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    }

    #[test]
    fn uuid_fallback_format() {
        let uuid = generate_fallback_uuid();
        assert_eq!(uuid.len(), 36);
        assert!(uuid
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c) || c == '-'));
        assert_eq!(uuid.chars().filter(|&c| c == '-').count(), 4);
    }

    #[test]
    fn header_value_is_valid_ascii() {
        let derived = derive_device_id("some-machine");
        let fallback = generate_fallback_uuid();
        assert!(reqwest::header::HeaderValue::from_str(&derived).is_ok());
        assert!(reqwest::header::HeaderValue::from_str(&fallback).is_ok());
    }

    #[test]
    fn validation_accepts_64_and_36() {
        assert!(is_valid_device_id(&"a".repeat(64)));
        assert!(is_valid_device_id(&generate_fallback_uuid()));
    }

    #[test]
    fn validation_rejects_bad() {
        assert!(!is_valid_device_id(""));
        assert!(!is_valid_device_id(&"a".repeat(63)));
        assert!(!is_valid_device_id(&"a".repeat(65)));
        assert!(!is_valid_device_id(&"a".repeat(100)));
        // 64 chars but contains a non-alphanumeric, non-hyphen char ('_').
        assert!(!is_valid_device_id(&format!("{}_", "a".repeat(63))));
        // 36 chars but contains a non-ASCII char.
        assert!(!is_valid_device_id(&format!("{}é", "a".repeat(34))));
    }

    /// §B.3 sandbox: lock `TEST_ENV_MUTEX`, point `ONCHAINOS_HOME` at a fresh
    /// per-test temp dir under `target/test_tmp/`, run `f`, then clean up. The
    /// lock survives a poisoned mutex (`into_inner`) so one failing sibling
    /// test does not cascade-poison the rest.
    fn with_temp_home<F: FnOnce()>(name: &str, f: F) {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir: PathBuf = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(format!("device_id_{name}"));
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &dir);
        f();
        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn get_or_create_persists() {
        with_temp_home("get_or_create_persists", || {
            // NOTE: no fresh-keyring precondition here. `with_temp_home` isolates
            // only `ONCHAINOS_HOME` (which scopes the `file_keyring` `keyring.enc`),
            // but on Linux the `keyring` crate's `linux-native` keyutils backend is
            // a session-global kernel store NOT scoped by `ONCHAINOS_HOME`, so a
            // `device_id` persisted by an earlier test-run process can survive there.
            // Asserting an empty keyring purely from a fresh home is therefore
            // invalid on Linux CI. We instead assert the behavior actually under
            // test: get-or-create returns a valid value, persists it, and is stable
            // across calls — which holds regardless of pre-existing OS-keyring state.
            let first = ensure_device_id().expect("first ensure returns Some");
            assert!(is_valid_device_id(&first));

            // The generated value is now persisted in the keyring.
            let persisted =
                crate::keyring_store::get_opt("device_id").expect("value persisted to keyring");
            assert_eq!(persisted, first);

            // A second ensure reads the identical value back (no regeneration).
            let second = ensure_device_id().expect("second ensure returns Some");
            assert_eq!(second, first);
        });
    }

    #[test]
    fn invalid_persisted_value_regenerates() {
        // Empty string: invalid → regenerate.
        with_temp_home("invalid_empty", || {
            crate::keyring_store::set("device_id", "").unwrap();
            let regenerated = ensure_device_id().expect("regenerates over empty value");
            assert!(is_valid_device_id(&regenerated));
            assert!(regenerated.len() == 64 || regenerated.len() == 36);
        });

        // 100-char string: invalid length → regenerate.
        with_temp_home("invalid_100_chars", || {
            crate::keyring_store::set("device_id", &"a".repeat(100)).unwrap();
            let regenerated = ensure_device_id().expect("regenerates over 100-char value");
            assert!(is_valid_device_id(&regenerated));
            assert!(regenerated.len() == 64 || regenerated.len() == 36);
        });
    }

    #[test]
    fn generated_value_valid() {
        with_temp_home("generated_value_valid", || {
            let value = generate_device_id().expect("generation is infallible");
            assert!(is_valid_device_id(&value));
            assert!(reqwest::header::HeaderValue::from_str(&value).is_ok());
        });
    }

    #[test]
    fn failure_absorbed_no_panic() {
        // Best-effort contract §3.3: `ensure_device_id()` never panics, even
        // when persistence goes through the volatile-only keyring fallback.
        with_temp_home("failure_absorbed_no_panic", || {
            let value = ensure_device_id().expect("returns Some even if persist is best-effort");
            assert!(is_valid_device_id(&value));
        });
    }
}
