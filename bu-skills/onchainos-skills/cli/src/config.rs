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
    // Priority: ONCHAINOS_HOME env > ./.onchainos/ (project-local)
    let base = match std::env::var("ONCHAINOS_HOME") {
        Ok(p) => PathBuf::from(p),
        Err(_) => std::env::current_dir()
            .context("cannot get cwd")?
            .join(".onchainos"),
    };
    Ok(base.join("config.json"))
}
