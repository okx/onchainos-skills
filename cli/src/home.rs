use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Returns the path to `~/.onchainos` (or `%USERPROFILE%\.onchainos` on Windows).
///
/// Can be overridden via the `ONCHAINOS_HOME` environment variable.
pub fn onchainos_home() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("ONCHAINOS_HOME") {
        return Ok(PathBuf::from(p));
    }

    let home = dirs::home_dir().context("cannot determine home directory")?;
    Ok(home.join(".onchainos"))
}

/// Shared mutex for tests that manipulate the `ONCHAINOS_HOME` environment variable.
/// All test modules (wallet_store, file_keyring, home) must lock this before
/// setting/unsetting `ONCHAINOS_HOME` to prevent race conditions.
#[cfg(test)]
pub static TEST_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Ensure a directory exists with mode 0700 on Unix (create if missing; fix if wrong).
///
/// Extracted from the inline logic that used to live in `ensure_onchainos_home()`
/// (§8.6 directory-permission deduplication). `file_keyring.rs` also points at this fn.
/// On non-Unix platforms the create-if-missing step runs but permissions rely on the
/// user-dir ACL (no `set_permissions`).
pub fn ensure_dir_0700(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)
            .with_context(|| format!("failed to create directory {}", path.display()))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::metadata(path)
            .with_context(|| format!("failed to read metadata for {}", path.display()))?;
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o700 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("failed to set 0700 on {}", path.display()))?;
        }
    }
    Ok(())
}

/// Ensure `~/.onchainos` exists with correct permissions (0700 on Unix).
pub fn ensure_onchainos_home() -> Result<PathBuf> {
    let home = onchainos_home()?;
    ensure_dir_0700(&home).context("failed to prepare ~/.onchainos")?;
    Ok(home)
}

/// Startup permission repair: scan known sensitive files and chmod any that are
/// not 0600 to 0600. Repairs files written by older CLI versions that did not
/// enforce permissions.
///
/// **Scanned file set** (spec section 8.3): `session.json`, `wallets.json`,
/// `keyring.enc`, `machine-identity`, `audit.jsonl`, `watch/*/daemon.log`.
/// `config.json` is deliberately EXCLUDED (non-sensitive after dead-field deletion).
///
/// On chmod failure (EPERM): prints a warning per file. **Never aborts startup.**
/// Idempotent: no-op if all files are already 0600.
pub fn self_heal_permissions() -> Result<()> {
    #[cfg(unix)]
    {
        let home = onchainos_home()?;
        if !home.exists() {
            return Ok(());
        }

        // Fixed set of sensitive files at the root of onchainos_home.
        let root_files = [
            "session.json",
            "wallets.json",
            "keyring.enc",
            "machine-identity",
            "audit.jsonl",
        ];

        for name in &root_files {
            let path = home.join(name);
            if path.exists() {
                heal_file_to_0600(&path);
            }
        }

        // watch/*/daemon.log
        let watch_dir = home.join("watch");
        if watch_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&watch_dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let log = entry.path().join("daemon.log");
                        if log.exists() {
                            heal_file_to_0600(&log);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Chmod a single file to 0600 if it is not already. Prints a warning on
/// failure — never panics or aborts.
#[cfg(unix)]
fn heal_file_to_0600(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    match fs::metadata(path) {
        Ok(meta) => {
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o600 {
                if let Err(e) =
                    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                {
                    eprintln!(
                        "Warning: cannot fix permissions on {}: {} — file may have been created by a different user",
                        path.display(),
                        e
                    );
                }
            }
        }
        Err(e) => {
            eprintln!(
                "Warning: cannot read metadata for {}: {}",
                path.display(),
                e
            );
        }
    }
}

/// Atomic, permission-safe file write.
///
/// 1. Write `bytes` to `<full-filename>.tmp` in the same directory (guarantees a
///    same-filesystem rename).
/// 2. If `sensitive`, set permissions to 0600 on the `.tmp` file BEFORE rename
///    (`#[cfg(unix)]`); if not sensitive, set 0644 explicitly (`#[cfg(unix)]`).
/// 3. Rename `.tmp` to the final path (atomic on POSIX; atomic on Windows for
///    same-volume).
///
/// The temp file name appends `.tmp` to the **full existing file name** via an
/// `OsString` push — `keyring.enc` → `keyring.enc.tmp`, `session.json` →
/// `session.json.tmp`, `machine-identity` → `machine-identity.tmp`. Do NOT use
/// `Path::with_extension("tmp")`, which *replaces* the extension and would corrupt
/// the name (mirrors `cli/src/file_keyring.rs:302-305`).
///
/// Downloaded executables (e.g. `bin/okx-pilot`) use a dedicated 0755 path — they do
/// NOT go through this function.
pub fn atomic_write(path: &Path, bytes: &[u8], sensitive: bool) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("path {} has no parent directory", path.display()))?;

    // Ensure the destination directory exists with 0700. If it is the onchainos root,
    // route through ensure_onchainos_home(); otherwise create + enforce 0700 directly.
    let root = onchainos_home()?;
    if parent == root {
        ensure_onchainos_home()?;
    } else {
        ensure_dir_0700(parent)?;
    }

    // Build "<full-filename>.tmp" without replacing the extension.
    let mut tmp_name = path
        .file_name()
        .with_context(|| format!("path {} has no file name", path.display()))?
        .to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);

    fs::write(&tmp, bytes)
        .with_context(|| format!("failed to write temp file {}", tmp.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if sensitive { 0o600 } else { 0o644 };
        fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))
            .with_context(|| format!("failed to set {mode:o} on {}", tmp.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = sensitive; // permissions rely on user-dir ACL on non-Unix
    }

    fs::rename(&tmp, path)
        .with_context(|| format!("failed to rename {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onchainos_home_respects_env_override() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        std::env::set_var("ONCHAINOS_HOME", "/tmp/test_onchainos");
        let path = onchainos_home().unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test_onchainos"));
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn onchainos_home_falls_back_to_home_dir() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        std::env::remove_var("ONCHAINOS_HOME");
        let path = onchainos_home().unwrap();
        assert!(path.ends_with(".onchainos"));
    }

    #[cfg(unix)]
    #[test]
    fn ensure_onchainos_home_creates_with_0700() {
        use std::os::unix::fs::PermissionsExt;
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("ensure_home_0700");
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        std::env::set_var("ONCHAINOS_HOME", &dir);
        let result = ensure_onchainos_home().unwrap();
        assert_eq!(result, dir);
        let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }

    /// Set up a sandbox `ONCHAINOS_HOME` under `target/test_tmp/<name>`, run `f`
    /// with the sandbox path, then tear it down. Mirrors the `file_keyring.rs`
    /// `with_temp_home` convention and holds `TEST_ENV_MUTEX` for the duration.
    fn with_temp_home<F: FnOnce(&std::path::Path)>(name: &str, f: F) {
        let _lock = TEST_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(format!("home_{name}"));
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &dir);
        f(&dir);
        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn atomic_write_non_sensitive_writes_content_no_leftover_tmp() {
        with_temp_home("aw_plain", |home| {
            let path = home.join("config.json");
            let bytes = b"{\"k\":1}";
            atomic_write(&path, bytes, false).unwrap();
            assert_eq!(fs::read(&path).unwrap(), bytes);
            // No leftover .tmp sibling.
            let tmp = home.join("config.json.tmp");
            assert!(!tmp.exists(), "leftover .tmp file should not exist");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o644);
            }
        });
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_sensitive_sets_0600() {
        use std::os::unix::fs::PermissionsExt;
        with_temp_home("aw_sensitive", |home| {
            let path = home.join("session.json");
            atomic_write(&path, b"secret", true).unwrap();
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        });
    }

    #[test]
    fn atomic_write_preserves_full_filename_with_extension() {
        with_temp_home("aw_ext", |home| {
            let path = home.join("keyring.enc");
            atomic_write(&path, b"blob", true).unwrap();
            // Extension intact: keyring.enc (NOT keyring.tmp / keyring).
            assert!(home.join("keyring.enc").exists());
            assert!(!home.join("keyring.tmp").exists());
            assert!(!home.join("keyring.enc.tmp").exists());
        });
    }

    #[test]
    fn atomic_write_preserves_extensionless_filename() {
        with_temp_home("aw_noext", |home| {
            let path = home.join("machine-identity");
            atomic_write(&path, b"id", true).unwrap();
            assert!(home.join("machine-identity").exists());
            assert!(!home.join("machine-identity.tmp").exists());
        });
    }

    #[test]
    fn atomic_write_errors_when_parent_cannot_be_created() {
        with_temp_home("aw_err", |home| {
            // Create a regular file, then attempt to write "under" it as if it were
            // a directory — create_dir_all on the parent must fail.
            let blocker = home.join("blocker");
            fs::write(&blocker, b"x").unwrap();
            let path = blocker.join("child").join("target.json");
            let err = atomic_write(&path, b"data", false).unwrap_err();
            let msg = format!("{err:#}");
            assert!(
                msg.contains("failed to create directory"),
                "expected .context() substring, got: {msg}"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_0700_creates_new_dir_at_0700() {
        use std::os::unix::fs::PermissionsExt;
        with_temp_home("ed_create", |home| {
            let sub = home.join("sub").join("nested");
            ensure_dir_0700(&sub).unwrap();
            assert!(sub.is_dir());
            let mode = fs::metadata(&sub).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        });
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_0700_corrects_wrong_mode() {
        use std::os::unix::fs::PermissionsExt;
        with_temp_home("ed_fix", |home| {
            let sub = home.join("loose");
            fs::create_dir_all(&sub).unwrap();
            fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
            ensure_dir_0700(&sub).unwrap();
            let mode = fs::metadata(&sub).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        });
    }
}
