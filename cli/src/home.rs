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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn onchainos_home_respects_env_override() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("ONCHAINOS_HOME", "/tmp/test_onchainos");
        let path = onchainos_home().unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test_onchainos"));
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn onchainos_home_falls_back_to_home_dir() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::remove_var("ONCHAINOS_HOME");
        let path = onchainos_home().unwrap();
        assert!(path.ends_with(".onchainos"));
    }
}
