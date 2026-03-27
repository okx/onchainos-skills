//! Encrypted-file fallback for OS keyring.
//!
//! When the OS keyring (gnome-keyring, kwallet, etc.) is unavailable — common
//! on headless Linux, Docker, and minimal distros — credentials are stored in
//! `~/.onchainos/keyring.enc` encrypted with AES-256-GCM.
//!
//! Key derivation: `scrypt(machine_id + username, random_salt) → 32-byte key`
//!
//! Machine ID priority:
//!   1. /etc/machine-id
//!   2. /var/lib/dbus/machine-id
//!   3. hostname

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result};
use rand::RngCore;
use zeroize::Zeroizing;

use crate::home::onchainos_home;

const KEYRING_FILE: &str = "keyring.enc";
const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;

// scrypt parameters: log_n=15, r=8, p=1 (~32 MB, ~100ms)
const SCRYPT_LOG_N: u8 = 15;
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;

/// Build a deterministic machine identity string.
fn machine_identity() -> String {
    let machine_id = read_machine_id();
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "onchainos-user".to_string());
    format!("{machine_id}:{username}")
}

/// Read machine ID: /etc/machine-id → /var/lib/dbus/machine-id → hostname
fn read_machine_id() -> String {
    if let Ok(id) = fs::read_to_string("/etc/machine-id") {
        let trimmed = id.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    if let Ok(id) = fs::read_to_string("/var/lib/dbus/machine-id") {
        let trimmed = id.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    hostname()
}

fn hostname() -> String {
    // Read hostname directly from /proc (Linux) or via gethostname (other Unix).
    // Avoids spawning a child process and PATH hijacking risk.
    if let Ok(h) = fs::read_to_string("/proc/sys/kernel/hostname") {
        let trimmed = h.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    // Fallback: gethostname syscall via libc
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: buf is a valid mutable [u8; 256] on the stack. gethostname writes
        // at most buf.len() bytes including the NUL terminator and does not read buf.
        let ret = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
        if ret == 0 {
            let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            if let Ok(s) = std::str::from_utf8(&buf[..len]) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }
    "unknown-host".to_string()
}

/// Derive a 32-byte AES-256 key from identity and salt via scrypt.
/// Returns `Zeroizing<Vec<u8>>` so the key is automatically zeroed on drop.
fn derive_key(identity: &str, salt: &[u8]) -> Zeroizing<Vec<u8>> {
    let params =
        scrypt::Params::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P, 32).expect("valid scrypt params");
    let mut key = Zeroizing::new(vec![0u8; 32]);
    scrypt::scrypt(identity.as_bytes(), salt, &params, &mut key)
        .expect("scrypt output length is valid");
    key
}

// ── File permission enforcement ────────────────────────────────────

/// Ensure directory exists with mode 0700. Create if missing; fix if wrong.
#[cfg(unix)]
fn ensure_dir_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if !path.exists() {
        fs::create_dir_all(path).context("failed to create directory")?;
    }
    let meta = fs::metadata(path).context("failed to read directory metadata")?;
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o700 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .context("failed to set directory permissions to 0700")?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_dir_permissions(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path).context("failed to create directory")?;
    }
    Ok(())
}

/// Ensure file has mode 0600. Fix silently if wrong.
#[cfg(unix)]
fn ensure_file_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::metadata(path).context("failed to read file metadata")?;
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o600 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .context("failed to set file permissions to 0600")?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_file_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

/// Check file permissions before reading; auto-fix if possible.
fn check_and_fix_permissions(path: &std::path::Path) -> Result<()> {
    ensure_file_permissions(path)
}

// ── Public API (same shape as keyring_store) ───────────────────────

/// Read the credential blob from the encrypted file.
/// Returns an empty map if the file does not exist.
pub fn read_blob() -> Result<HashMap<String, String>> {
    let path = keyring_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    check_and_fix_permissions(&path)?;
    let data = fs::read(&path).context("failed to read keyring.enc")?;

    if data.len() < SALT_LEN + NONCE_LEN + 1 {
        anyhow::bail!("keyring.enc is corrupted (too short)");
    }
    let (salt, rest) = data.split_at(SALT_LEN);
    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_LEN);

    let identity = machine_identity();
    let key = derive_key(&identity, salt);
    let cipher = Aes256Gcm::new_from_slice(&key).context("failed to create AES cipher")?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| {
                anyhow::anyhow!(
                    "failed to decrypt keyring.enc (wrong machine or corrupted file)"
                )
            })?,
    );

    let map: HashMap<String, String> =
        serde_json::from_slice(&plaintext).context("failed to parse decrypted keyring blob")?;
    Ok(map)
}

/// Write the credential blob to the encrypted file.
pub fn write_blob(map: &HashMap<String, String>) -> Result<()> {
    let home = onchainos_home()?;
    ensure_dir_permissions(&home)?;
    let path = home.join(KEYRING_FILE);
    let json = Zeroizing::new(serde_json::to_string(map).context("failed to serialize keyring blob")?);

    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);

    let identity = machine_identity();
    let key = derive_key(&identity, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key).context("failed to create AES cipher")?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, json.as_bytes())
        .map_err(|_| anyhow::anyhow!("failed to encrypt keyring blob"))?;

    let mut out = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);

    let tmp = path.with_extension("enc.tmp");
    fs::write(&tmp, &out).context("failed to write keyring.enc.tmp")?;
    ensure_file_permissions(&tmp)?;
    fs::rename(&tmp, &path).context("failed to rename keyring.enc.tmp")?;
    Ok(())
}

/// Delete the encrypted keyring file.
pub fn clear_all() -> Result<()> {
    let path = keyring_path()?;
    if path.exists() {
        fs::remove_file(&path).context("failed to delete keyring.enc")?;
    }
    Ok(())
}

fn keyring_path() -> Result<PathBuf> {
    Ok(onchainos_home()?.join(KEYRING_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_identity_returns_non_empty() {
        let id = machine_identity();
        assert!(!id.is_empty());
    }

    #[test]
    fn machine_identity_is_deterministic() {
        let id1 = machine_identity();
        let id2 = machine_identity();
        assert_eq!(id1, id2);
    }

    #[test]
    fn derive_key_returns_32_bytes() {
        let salt = [0u8; 32];
        let key = derive_key("test-identity", &salt);
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn derive_key_different_salts_yield_different_keys() {
        let salt1 = [1u8; 32];
        let salt2 = [2u8; 32];
        let key1 = derive_key("same-identity", &salt1);
        let key2 = derive_key("same-identity", &salt2);
        assert_ne!(key1, key2);
    }

    #[test]
    fn derive_key_different_identities_yield_different_keys() {
        let salt = [0u8; 32];
        let key1 = derive_key("identity-a", &salt);
        let key2 = derive_key("identity-b", &salt);
        assert_ne!(key1, key2);
    }

    #[cfg(unix)]
    mod perm_tests {
        use super::super::*;
        use std::os::unix::fs::PermissionsExt;

        fn perm_test_dir(name: &str) -> std::path::PathBuf {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("test_tmp")
                .join(format!("perm_{name}"));
            if dir.exists() {
                fs::remove_dir_all(&dir).ok();
            }
            fs::create_dir_all(&dir).unwrap();
            dir
        }

        #[test]
        fn ensure_dir_permissions_creates_with_0700() {
            let dir = perm_test_dir("create_0700");
            let target = dir.join("sub");
            ensure_dir_permissions(&target).unwrap();
            let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
            fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn ensure_dir_permissions_fixes_wrong_mode() {
            let dir = perm_test_dir("fix_dir_mode");
            let target = dir.join("sub");
            fs::create_dir(&target).unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
            ensure_dir_permissions(&target).unwrap();
            let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
            fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn ensure_file_permissions_fixes_wrong_mode() {
            let dir = perm_test_dir("fix_file_mode");
            let file = dir.join("test.enc");
            fs::write(&file, b"data").unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
            ensure_file_permissions(&file).unwrap();
            let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn check_file_permissions_passes_for_0600() {
            let dir = perm_test_dir("check_0600");
            let file = dir.join("ok.enc");
            fs::write(&file, b"data").unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(check_and_fix_permissions(&file).is_ok());
            fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn check_file_permissions_auto_fixes_0644() {
            let dir = perm_test_dir("autofix_0644");
            let file = dir.join("bad.enc");
            fs::write(&file, b"data").unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
            check_and_fix_permissions(&file).unwrap();
            let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            fs::remove_dir_all(&dir).ok();
        }
    }

    fn with_temp_home<F: FnOnce()>(name: &str, f: F) {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(format!("fk_{name}"));
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
    fn write_and_read_blob_roundtrip() {
        with_temp_home("roundtrip", || {
            let mut map = HashMap::new();
            map.insert("access_token".to_string(), "tok-123".to_string());
            map.insert("refresh_token".to_string(), "ref-456".to_string());
            write_blob(&map).unwrap();
            let loaded = read_blob().unwrap();
            assert_eq!(loaded.get("access_token").unwrap(), "tok-123");
            assert_eq!(loaded.get("refresh_token").unwrap(), "ref-456");
        });
    }

    #[test]
    fn read_blob_returns_empty_when_no_file() {
        with_temp_home("no_file", || {
            let map = read_blob().unwrap();
            assert!(map.is_empty());
        });
    }

    #[test]
    fn write_blob_overwrites_previous() {
        with_temp_home("overwrite", || {
            let mut map1 = HashMap::new();
            map1.insert("key".to_string(), "value1".to_string());
            write_blob(&map1).unwrap();
            let mut map2 = HashMap::new();
            map2.insert("key".to_string(), "value2".to_string());
            write_blob(&map2).unwrap();
            let loaded = read_blob().unwrap();
            assert_eq!(loaded.get("key").unwrap(), "value2");
        });
    }

    #[test]
    fn clear_all_removes_file() {
        with_temp_home("clear", || {
            let mut map = HashMap::new();
            map.insert("k".to_string(), "v".to_string());
            write_blob(&map).unwrap();
            clear_all().unwrap();
            let loaded = read_blob().unwrap();
            assert!(loaded.is_empty());
        });
    }

    #[test]
    fn clear_all_noop_when_no_file() {
        with_temp_home("clear_noop", || {
            clear_all().unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn write_blob_sets_file_permissions_0600() {
        use std::os::unix::fs::PermissionsExt;
        with_temp_home("perms", || {
            let mut map = HashMap::new();
            map.insert("k".to_string(), "v".to_string());
            write_blob(&map).unwrap();
            let path = onchainos_home().unwrap().join(KEYRING_FILE);
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        });
    }
}
