use std::fs;
use std::path::PathBuf;

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

/// Ensure `~/.onchainos` exists with correct permissions (0700 on Unix).
pub fn ensure_onchainos_home() -> Result<PathBuf> {
    let home = onchainos_home()?;
    if !home.exists() {
        fs::create_dir_all(&home).context("failed to create ~/.onchainos")?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::metadata(&home).context("failed to read ~/.onchainos metadata")?;
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o700 {
            fs::set_permissions(&home, fs::Permissions::from_mode(0o700))
                .context("failed to set ~/.onchainos permissions to 0700")?;
        }
    }
    Ok(home)
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
}
