use crate::config::state_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Popup panes carry no pane id; the open marker stores this instead.
pub const POPUP: &str = "popup";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    #[serde(default)]
    pub preview: bool,
}

fn state_path() -> PathBuf {
    state_dir().join("state.json")
}

fn open_path() -> PathBuf {
    state_dir().join("open.json")
}

pub fn load() -> State {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(state: &State) {
    let _ = std::fs::create_dir_all(state_dir());
    if let Ok(s) = serde_json::to_string(state) {
        let _ = std::fs::write(state_path(), s);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPalette {
    pub pane_id: String,
}

pub fn is_open() -> bool {
    open_path().exists()
}

pub fn open_palette() -> Option<OpenPalette> {
    std::fs::read_to_string(open_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn mark_open(pane_id: &str) {
    let _ = std::fs::create_dir_all(state_dir());
    let _ = std::fs::write(
        open_path(),
        serde_json::to_string(&OpenPalette { pane_id: pane_id.to_string() }).unwrap_or_default(),
    );
}

pub fn clear_open() {
    let _ = std::fs::remove_file(open_path());
}
