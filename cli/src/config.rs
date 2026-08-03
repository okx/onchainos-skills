use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub session_token: String,
    #[serde(default)]
    pub active_wallet: String,
    #[serde(default)]
    pub default_chain: String,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        // Cwd migration (spec §4, AC#9): if the home config does not exist but
        // a stale `./.onchainos/config.json` in the cwd does, auto-copy it to
        // ONCHAINOS_HOME and prompt the user to delete the stale file.
        if !path.exists() {
            if let Ok(cwd) = std::env::current_dir() {
                let stale = cwd.join(".onchainos").join("config.json");
                if stale.exists() {
                    if let Ok(bytes) = fs::read(&stale) {
                        // atomic_write(path, bytes, sensitive=false) — config.json
                        // contains no sensitive data after dead-field deletion.
                        let _ = crate::home::atomic_write(&path, &bytes, false);
                        eprintln!(
                            "Migrated config from {stale_dir}/config.json to {home}. \
                             You can safely delete the stale .onchainos directory at {stale_dir}.",
                            stale_dir = stale.parent().unwrap_or(&cwd).display(),
                            home = path.display(),
                        );
                    }
                }
            }
        }
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path).context("failed to read config file")?;
        let cfg: AppConfig = serde_json::from_str(&data).context("failed to parse config")?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("failed to create config directory")?;
        }
        let data = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &data)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }
}

fn config_path() -> Result<PathBuf> {
    Ok(crate::home::onchainos_home()?.join("config.json"))
}
