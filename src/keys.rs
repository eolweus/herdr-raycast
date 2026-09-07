use crate::app::View;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

const MODS: KeyModifiers = KeyModifiers::CONTROL
    .union(KeyModifiers::ALT)
    .union(KeyModifiers::SHIFT)
    .union(KeyModifiers::SUPER);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub mods: KeyModifiers,
    pub code: KeyCode,
}

impl Binding {
    pub fn parse(s: &str) -> Option<Binding> {
        let s = s.trim();
        let (prefix, key) = match s.rfind('+') {
            Some(i) if i + 1 < s.len() => (&s[..i], &s[i + 1..]),
            Some(i) if i > 0 => (&s[..i - 1], "+"),
            _ => ("", s),
        };
        let mut mods = KeyModifiers::NONE;
        for m in prefix.split('+').filter(|m| !m.is_empty()) {
            match m.trim().to_lowercase().as_str() {
                "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
                "alt" | "opt" | "option" | "meta" => mods |= KeyModifiers::ALT,
                "shift" => mods |= KeyModifiers::SHIFT,
                "cmd" | "super" | "win" => mods |= KeyModifiers::SUPER,
                _ => return None,
            }
        }
        let lower = key.to_lowercase();
        let code = match lower.as_str() {
            "space" => KeyCode::Char(' '),
            "enter" | "return" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "esc" | "escape" => KeyCode::Esc,
            "backspace" => KeyCode::Backspace,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            k if k.len() > 1 && k.starts_with('f') && k[1..].chars().all(|c| c.is_ascii_digit()) => {
                KeyCode::F(k[1..].parse().ok()?)
            }
            _ => {
                let mut chars = key.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                if c.is_uppercase() {
                    mods |= KeyModifiers::SHIFT;
                }
                KeyCode::Char(c.to_lowercase().next().unwrap_or(c))
            }
        };
        Some(Binding { mods, code })
    }

    pub fn matches(&self, key: &KeyEvent) -> bool {
        let mut mods = key.modifiers & MODS;
        let code = match key.code {
            KeyCode::Char(c) => {
                if c.is_uppercase() {
                    mods |= KeyModifiers::SHIFT;
                }
                KeyCode::Char(c.to_lowercase().next().unwrap_or(c))
            }
            KeyCode::BackTab => {
                mods |= KeyModifiers::SHIFT;
                KeyCode::Tab
            }
            other => other,
        };
        code == self.code && mods == self.mods
    }
}

pub struct PaletteKey {
    pub binding: Binding,
    pub view: Option<View>,
}

// Keys the user bound to this plugin's open actions in the Herdr config, so
// the same chord closes the palette (or switches View) while it is open:
// Herdr does not dispatch custom bindings while a popup has focus.
pub fn palette_keys(plugin_id: &str) -> Vec<PaletteKey> {
    let Ok(text) = std::fs::read_to_string(herdr_config_path()) else { return vec![] };
    let Ok(doc) = text.parse::<toml::Value>() else { return vec![] };
    let entries = doc.get("keys").and_then(|k| k.get("command")).and_then(|c| c.as_array());
    let mut out = vec![];
    for e in entries.into_iter().flatten() {
        if e.get("type").and_then(|t| t.as_str()) != Some("plugin_action") {
            continue;
        }
        let Some(cmd) = e.get("command").and_then(|c| c.as_str()) else { continue };
        let Some(action) = cmd.strip_prefix(plugin_id).and_then(|r| r.strip_prefix('.')) else { continue };
        let view = match action {
            "open" => None,
            a => match a.strip_prefix("open-").and_then(View::parse) {
                Some(v) => Some(v),
                None => continue,
            },
        };
        let Some(binding) = e.get("key").and_then(|k| k.as_str()).and_then(Binding::parse) else { continue };
        out.push(PaletteKey { binding, view });
    }
    out
}

fn herdr_config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("HERDR_CONFIG_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/herdr/config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent { code, modifiers: mods, kind: KeyEventKind::Press, state: KeyEventState::NONE }
    }

    #[test]
    fn parses_chords() {
        let b = Binding::parse("cmd+ø").unwrap();
        assert_eq!(b, Binding { mods: KeyModifiers::SUPER, code: KeyCode::Char('ø') });
        let b = Binding::parse("ctrl+shift+P").unwrap();
        assert_eq!(b.mods, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        assert_eq!(b.code, KeyCode::Char('p'));
        assert_eq!(Binding::parse("f9").unwrap().code, KeyCode::F(9));
        assert_eq!(Binding::parse("cmd+=").unwrap().code, KeyCode::Char('='));
        assert_eq!(Binding::parse("cmd++").unwrap().code, KeyCode::Char('+'));
        assert_eq!(Binding::parse("cmd+shift+enter").unwrap().code, KeyCode::Enter);
        assert!(Binding::parse("hyper+x").is_none());
    }

    #[test]
    fn matches_kitty_events() {
        let b = Binding::parse("cmd+ø").unwrap();
        assert!(b.matches(&ev(KeyCode::Char('ø'), KeyModifiers::SUPER)));
        assert!(!b.matches(&ev(KeyCode::Char('ø'), KeyModifiers::NONE)));
        let b = Binding::parse("cmd+shift+p").unwrap();
        assert!(b.matches(&ev(KeyCode::Char('P'), KeyModifiers::SUPER)));
        assert!(b.matches(&ev(KeyCode::Char('p'), KeyModifiers::SUPER | KeyModifiers::SHIFT)));
        let b = Binding::parse("shift+tab").unwrap();
        assert!(b.matches(&ev(KeyCode::BackTab, KeyModifiers::NONE)));
    }
}
