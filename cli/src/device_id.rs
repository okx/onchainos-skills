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
//! - `ensure_device_id` — get-or-create pipeline (session.json read +
//!   validate, else generate).
//! - `generate_device_id` — `machine_uid`/sha256 or UUID fallback, then
//!   best-effort persist to `session.json`.
//!
//! Storage: the persisted value lives in the non-sensitive on-disk session
//! metadata (`wallet_store::SessionJson.device_id`, at
//! `$ONCHAINOS_HOME/session.json`), NOT the OS keyring. The device id is a
//! forgeable, non-credential identifier that is also reported in plaintext
//! via the header, so it belongs beside other session metadata rather than
//! next to tokens / private keys. A missing file or empty field is a cache
//! miss: the value is deterministically re-derived
//! (`sha256(machine_id + "onchainos")`) and re-persisted, so clearing
//! `session.json` yields the identical id on any machine with a readable
//! hardware id.
//!
//! Value format (spec §4.3): a 64-char lowercase-hex SHA-256 digest OR a
//! 36-char UUIDv4, always pure ASCII.

use std::sync::OnceLock;

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Fixed namespace suffix concatenated after the machine id before hashing
/// (spec §5.2 — never changed).
const NAMESPACE_SUFFIX: &[u8] = b"onchainos";

/// Process-lifetime memoization cell for the device id (spec §9.1 / §A.6).
///
/// `Some(id)` on successful computation, `None` on total failure. Failure is
/// memoized — there is no retry within the process. `OnceLock` is `Sync` and
/// guarantees exactly-once initialization even under concurrent access from
/// multiple tokio tasks.
static DEVICE_ID: OnceLock<Option<String>> = OnceLock::new();

/// Returns the cached device id, computing it on the first call (spec §A.1).
///
/// The first call runs the get-or-create pipeline exactly once (session.json
/// read, optional `machine_uid::get()`, optional session.json write); every subsequent
/// call is a pure memory read from the `OnceLock`. Returns `None` when
/// computation failed, in which case the caller skips the `device-id` header.
pub fn get_cached_device_id() -> Option<&'static str> {
    DEVICE_ID.get_or_init(ensure_device_id).as_deref()
}

/// Get-or-create pipeline for the device id (spec §A.2).
///
/// Reads the persisted value from `session.json`: a valid value is returned
/// as-is (no regeneration); a missing/empty or invalid value falls through to
/// `generate_device_id`. Never panics — session I/O failures are absorbed
/// (best-effort contract §3.3).
fn ensure_device_id() -> Option<String> {
    if let Some(existing) = read_persisted_device_id() {
        if is_valid_device_id(&existing) {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG] device-id: loaded from session.json cache");
            }
            return Some(existing);
        }
    }
    generate_device_id()
}

/// Absent-tolerant read of the persisted device id from `session.json`
/// (spec §A.2). A missing file, a missing/empty `device_id` field, or any read
/// error all collapse to `None` (a cache miss → regenerate) — the device id is
/// non-sensitive session metadata, never a hard dependency.
fn read_persisted_device_id() -> Option<String> {
    match crate::wallet_store::load_session() {
        Ok(Some(session)) if !session.device_id.is_empty() => Some(session.device_id),
        _ => None,
    }
}

/// Best-effort persist of the device id into `session.json`, preserving every
/// other session field (spec §A.2 / §3.3). Loads the current session (or a
/// default when absent), sets `device_id`, and writes it back. A load or write
/// failure is swallowed — the value is still usable for this process and is
/// re-derived deterministically next run; the error never propagates.
fn persist_device_id(value: &str) {
    let mut session = crate::wallet_store::load_session()
        .ok()
        .flatten()
        .unwrap_or_default();
    session.device_id = value.to_string();
    match crate::wallet_store::save_session(&session) {
        Ok(()) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG] device-id: persisted to session.json");
            }
        }
        Err(_) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG] device-id: persist failed, value usable this process only");
            }
        }
    }
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

    persist_device_id(&value);

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
            // `session.json` lives under `ONCHAINOS_HOME`, which `with_temp_home`
            // points at a fresh dir — so (unlike the OS keyring) there is
            // genuinely no persisted value yet.
            assert!(read_persisted_device_id().is_none());

            let first = ensure_device_id().expect("first ensure returns Some");
            assert!(is_valid_device_id(&first));

            // The generated value is now persisted in session.json.
            let persisted = crate::wallet_store::load_session()
                .expect("session loads")
                .expect("session.json written")
                .device_id;
            assert_eq!(persisted, first);

            // A second ensure reads the identical value back (no regeneration).
            let second = ensure_device_id().expect("second ensure returns Some");
            assert_eq!(second, first);
        });
    }

    #[test]
    fn invalid_persisted_value_regenerates() {
        // Empty field: treated as absent (cache miss) → regenerate.
        with_temp_home("invalid_empty", || {
            persist_device_id("");
            let regenerated = ensure_device_id().expect("regenerates over empty value");
            assert!(is_valid_device_id(&regenerated));
            assert!(regenerated.len() == 64 || regenerated.len() == 36);
        });

        // 100-char field: present but invalid length → regenerate.
        with_temp_home("invalid_100_chars", || {
            persist_device_id(&"a".repeat(100));
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
        // Best-effort contract §3.3: `ensure_device_id()` never panics and
        // returns a usable value even when the session.json persist is a no-op
        // or fails — the value stays valid for this process regardless.
        with_temp_home("failure_absorbed_no_panic", || {
            let value = ensure_device_id().expect("returns Some even if persist is best-effort");
            assert!(is_valid_device_id(&value));
        });
    }
}
