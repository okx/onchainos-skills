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

/// Atomically write `contents` to `path` with owner-only permissions.
///
/// Creates the parent dir (0700 on Unix), writes to a same-dir temp file opened 0600
/// on Unix, then renames it over `path`. A crash mid-write leaves either the old file
/// or nothing — never a torn file. Used for autotrade authorization records
/// (consent / grant / pending / plugin-approved) so they are neither world-readable
/// nor half-written. Non-Unix falls back to default perms; the atomic rename still holds.
pub fn write_secure(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }
    let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("f");
    let tmp = parent.join(format!(".{fname}.{}.tmp", std::process::id()));
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(contents)?;
        f.flush()?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
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

    #[cfg(unix)]
    #[test]
    fn write_secure_is_0600_atomic_and_overwrites() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("write_secure");
        fs::remove_dir_all(&dir).ok();
        let path = dir.join("sub").join("rec.json");
        write_secure(&path, b"{\"cap\":\"100\"}").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"cap\":\"100\"}");
        // file is 0600, its parent dir is 0700
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        // overwrite replaces atomically + leaves no stray temp file
        write_secure(&path, b"{\"cap\":\"200\"}").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"cap\":\"200\"}");
        let strays: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(strays.is_empty(), "temp file left behind");
        fs::remove_dir_all(&dir).ok();
    }
}
