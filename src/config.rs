use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub hooks_enabled: bool,
    pub autostart_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hooks_enabled: true,
            autostart_enabled: true,
        }
    }
}

pub fn load() -> Result<AppConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed reading config file at {}", path.display()))?;
    let parsed = serde_json::from_str::<AppConfig>(&content)
        .with_context(|| format!("failed parsing config file at {}", path.display()))?;
    Ok(parsed)
}

pub fn save(config: &AppConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating config dir at {}", parent.display()))?;
    }

    let content = serde_json::to_string_pretty(config)?;
    fs::write(&path, content)
        .with_context(|| format!("failed writing config file at {}", path.display()))?;

    Ok(())
}

fn config_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "tesmo", "vdesk")
        .context("failed resolving per-user config directory")?;
    Ok(dirs.config_dir().join("config.json"))
}
