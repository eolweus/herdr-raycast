use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub width: String,
    pub height: String,
    pub editor: String,
    pub poll_ms: u64,
    pub preview_lines: u32,
    pub agent_kind: String,
    pub debug_keys: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: "60%".into(),
            height: "60%".into(),
            editor: "auto".into(),
            poll_ms: 500,
            preview_lines: 40,
            agent_kind: "claude".into(),
            debug_keys: false,
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("HERDR_PLUGIN_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/herdr/plugins/config/erling.command-palette")
}

pub fn commands_dir() -> PathBuf {
    config_dir().join("commands")
}

pub fn state_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("HERDR_PLUGIN_STATE_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".local/state/herdr/plugins/erling.command-palette")
}

pub fn load() -> Config {
    let path = config_dir().join("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
            eprintln!("herdr-raycast: {}: {e}; using defaults", path.display());
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}
