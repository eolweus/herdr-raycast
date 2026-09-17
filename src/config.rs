use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub width: String,
    pub height: String,
    pub min_width: u16,
    pub min_height: u16,
    pub editor: String,
    pub poll_ms: u64,
    pub preview_lines: u32,
    pub agent_kind: String,
    pub debug_keys: bool,
    pub machines_dir: String,
    pub remote_poll_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: "60%".into(),
            height: "60%".into(),
            min_width: 100,
            min_height: 24,
            editor: "auto".into(),
            poll_ms: 500,
            preview_lines: 40,
            agent_kind: "claude".into(),
            debug_keys: false,
            machines_dir: "~/.herdr/machines".into(),
            remote_poll_ms: 2000,
        }
    }
}

impl Config {
    pub fn machines_dir(&self) -> PathBuf {
        PathBuf::from(crate::commands::files::expand_home(&self.machines_dir))
    }
}

// Herdr popups take cells or a percent, never both, so the floor is applied here.
pub fn popup_size(spec: &str, min: u16, available: Option<u16>) -> serde_json::Value {
    let percent = spec.strip_suffix('%').and_then(|p| p.trim().parse::<u32>().ok());
    match (percent, available) {
        (Some(p), Some(avail)) => {
            let cells = (avail as u32 * p / 100) as u16;
            if cells >= min { spec.into() } else { min.min(avail).into() }
        }
        (Some(_), None) => spec.into(),
        (None, _) => spec.trim().parse::<u16>().map(|c| c.max(min).into()).unwrap_or_else(|_| spec.into()),
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

#[cfg(test)]
mod tests {
    use super::popup_size;
    use serde_json::json;

    #[test]
    fn percent_is_kept_when_it_clears_the_floor() {
        assert_eq!(popup_size("60%", 100, Some(200)), json!("60%"));
    }

    #[test]
    fn floor_replaces_a_small_percent_and_never_exceeds_the_area() {
        assert_eq!(popup_size("60%", 100, Some(120)), json!(100));
        assert_eq!(popup_size("60%", 100, Some(80)), json!(80));
    }

    #[test]
    fn unknown_area_and_cell_specs() {
        assert_eq!(popup_size("60%", 100, None), json!("60%"));
        assert_eq!(popup_size("90", 100, Some(200)), json!(100));
        assert_eq!(popup_size("140", 100, None), json!(140));
    }
}
