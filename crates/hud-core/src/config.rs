//! Persisted configuration: window states, mini bar, profiles, thresholds, hotkeys.

use crate::appearance::MIN_OPACITY;
use crate::position::{first_run_position, Size, MARGIN};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

/// Single source of truth for the HUD window size (the Slint literal in hud-app must match).
/// HUD height without the Codex section; `HUD_SIZE` is the tallest layout.
pub const HUD_BASE_HEIGHT: i32 = 500;
pub const HUD_SIZE: Size = Size {
    width: 320,
    height: 566,
};

const DEFAULT_OPACITY: f32 = 0.88;
const DEFAULT_CPU: Threshold = Threshold {
    warn: 70.0,
    crit: 90.0,
};
const DEFAULT_RAM: Threshold = Threshold {
    warn: 75.0,
    crit: 90.0,
};
const DEFAULT_TEMP: Threshold = Threshold {
    warn: 75.0,
    crit: 90.0,
};
const DEFAULT_PING: Threshold = Threshold {
    warn: 80.0,
    crit: 200.0,
};
const DEFAULT_CLAUDE_5H: Threshold = Threshold {
    warn: 70.0,
    crit: 90.0,
};
const DEFAULT_TOGGLE_HUD: &str = "Ctrl+Alt+H";
const DEFAULT_TOGGLE_MINIBAR: &str = "Ctrl+Alt+M";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config file not found")]
    NotFound,
    #[error("invalid config field `{field}`")]
    Invalid { field: &'static str },
    #[error("config is not valid TOML: {0}")]
    Parse(String),
    #[error("config io: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MiniBarItem {
    Cpu,
    Ram,
    Net,
    Temp,
    Ping,
    Gpu,
    Disk,
    Claude5h,
    Claude7d,
    Codex5h,
    Codex7d,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub opacity: f32,
    pub visible: bool,
    #[serde(default)]
    pub acrylic: bool,
    #[serde(default)]
    pub click_through: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MiniBar {
    pub window: WindowState,
    pub items: Vec<MiniBarItem>,
    pub orientation: Orientation,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Threshold {
    pub warn: f32,
    pub crit: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    pub cpu_percent: Threshold,
    pub ram_percent: Threshold,
    pub temp_c: Threshold,
    pub ping_ms: Threshold,
    #[serde(default = "default_claude_5h")]
    pub claude_5h: Threshold,
}

fn default_claude_5h() -> Threshold {
    DEFAULT_CLAUDE_5H
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hotkeys {
    pub toggle_hud: String,
    pub toggle_minibar: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub items: Vec<MiniBarItem>,
    pub orientation: Orientation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub hud: WindowState,
    pub minibar: MiniBar,
    pub profiles: Vec<Profile>,
    pub active_profile: Option<String>,
    pub thresholds: Thresholds,
    pub hotkeys: Hotkeys,
    pub autostart: bool,
}

fn invalid(field: &'static str) -> ConfigError {
    ConfigError::Invalid { field }
}

fn check_window(w: &WindowState, field: &'static str) -> Result<(), ConfigError> {
    if (MIN_OPACITY..=1.0).contains(&w.opacity) {
        Ok(())
    } else {
        Err(invalid(field))
    }
}

fn check_threshold(t: Threshold, field: &'static str) -> Result<(), ConfigError> {
    if t.warn.is_finite() && t.crit.is_finite() && t.warn < t.crit {
        Ok(())
    } else {
        Err(invalid(field))
    }
}

const MODIFIERS: [&str; 4] = ["Ctrl", "Alt", "Shift", "Win"];
const NAMED_KEYS: [&str; 12] = [
    "Space", "Tab", "Enter", "Esc", "Home", "End", "PageUp", "PageDown", "Insert", "Delete", "Up",
    "Down",
];

fn is_key(key: &str) -> bool {
    let b = key.as_bytes();
    let fn_key = key
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n) && !key.starts_with("F0"));
    (b.len() == 1 && b[0].is_ascii_alphanumeric() && !b[0].is_ascii_lowercase())
        || fn_key
        || NAMED_KEYS.contains(&key)
        || key == "Left"
        || key == "Right"
}

/// `Mod+...+Key`: 1+ distinct modifiers from MODIFIERS, then exactly one known key.
fn is_hotkey(text: &str) -> bool {
    let parts: Vec<&str> = text.split('+').collect();
    let Some((key, mods)) = parts.split_last() else {
        return false;
    };
    !mods.is_empty()
        && is_key(key)
        && mods
            .iter()
            .enumerate()
            .all(|(i, m)| MODIFIERS.contains(m) && !mods[..i].contains(m))
}

impl Config {
    /// Explicit initial config: HUD top-right, mini bar just below it (hidden).
    pub fn first_run(screen: Size) -> Config {
        let hud_pos = first_run_position(screen, HUD_SIZE);
        Config {
            hud: WindowState {
                x: hud_pos.x,
                y: hud_pos.y,
                opacity: DEFAULT_OPACITY,
                visible: true,
                acrylic: false,
                click_through: false,
            },
            minibar: MiniBar {
                window: WindowState {
                    x: hud_pos.x,
                    y: hud_pos.y + HUD_SIZE.height + MARGIN,
                    opacity: DEFAULT_OPACITY,
                    visible: false,
                    acrylic: false,
                    click_through: false,
                },
                items: vec![
                    MiniBarItem::Cpu,
                    MiniBarItem::Ram,
                    MiniBarItem::Net,
                    MiniBarItem::Temp,
                ],
                orientation: Orientation::Horizontal,
            },
            profiles: Vec::new(),
            active_profile: None,
            thresholds: Thresholds {
                cpu_percent: DEFAULT_CPU,
                ram_percent: DEFAULT_RAM,
                temp_c: DEFAULT_TEMP,
                ping_ms: DEFAULT_PING,
                claude_5h: DEFAULT_CLAUDE_5H,
            },
            hotkeys: Hotkeys {
                toggle_hud: DEFAULT_TOGGLE_HUD.to_owned(),
                toggle_minibar: DEFAULT_TOGGLE_MINIBAR.to_owned(),
            },
            autostart: false,
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        check_window(&self.hud, "opacity")?;
        check_window(&self.minibar.window, "minibar.opacity")?;
        if self.minibar.items.is_empty() {
            return Err(invalid("minibar.items"));
        }
        check_threshold(self.thresholds.cpu_percent, "thresholds.cpu_percent")?;
        check_threshold(self.thresholds.ram_percent, "thresholds.ram_percent")?;
        check_threshold(self.thresholds.temp_c, "thresholds.temp_c")?;
        check_threshold(self.thresholds.ping_ms, "thresholds.ping_ms")?;
        check_threshold(self.thresholds.claude_5h, "thresholds.claude_5h")?;
        if !is_hotkey(&self.hotkeys.toggle_hud) {
            return Err(invalid("hotkeys.toggle_hud"));
        }
        if !is_hotkey(&self.hotkeys.toggle_minibar) {
            return Err(invalid("hotkeys.toggle_minibar"));
        }
        self.validate_profiles()
    }

    fn validate_profiles(&self) -> Result<(), ConfigError> {
        for (i, p) in self.profiles.iter().enumerate() {
            if p.name.trim().is_empty() {
                return Err(invalid("profiles.name"));
            }
            if p.items.is_empty() {
                return Err(invalid("profiles.items"));
            }
            if self.profiles[..i].iter().any(|q| q.name == p.name) {
                return Err(invalid("profiles.duplicate"));
            }
        }
        match &self.active_profile {
            Some(name) if !self.profiles.iter().any(|p| &p.name == name) => {
                Err(invalid("active_profile"))
            }
            _ => Ok(()),
        }
    }

    /// Hotkey for the HUD: hides it when shown, otherwise shows it in place of the mini bar.
    pub fn toggle_hud(&mut self) {
        self.hud.visible = !self.hud.visible;
        if self.hud.visible {
            self.minibar.window.visible = false;
        }
    }

    /// Hotkey for the mini bar: hides it when shown, otherwise shows it in place of the HUD.
    pub fn toggle_minibar(&mut self) {
        self.minibar.window.visible = !self.minibar.window.visible;
        if self.minibar.window.visible {
            self.hud.visible = false;
        }
    }

    /// Alert escalation: when both windows are hidden, shows the mini bar. Returns whether it changed.
    pub fn reveal_minibar_if_hidden(&mut self) -> bool {
        let both_hidden = !self.hud.visible && !self.minibar.window.visible;
        if both_hidden {
            self.minibar.window.visible = true;
        }
        both_hidden
    }

    /// Applies the named profile's mini bar items and orientation.
    pub fn switch_profile(&mut self, name: &str) -> Result<(), ConfigError> {
        let profile = self
            .profiles
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| invalid("profile"))?;
        self.minibar.items = profile.items.clone();
        self.minibar.orientation = profile.orientation;
        self.active_profile = Some(name.to_owned());
        Ok(())
    }

    pub fn from_toml(text: &str) -> Result<Config, ConfigError> {
        let config: Config = toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn to_toml(&self) -> Result<String, ConfigError> {
        self.validate()?;
        toml::to_string_pretty(self).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        match fs::read_to_string(path) {
            Ok(text) => Config::from_toml(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(ConfigError::NotFound),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = self.to_toml()?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    const SCREEN: Size = Size {
        width: 1920,
        height: 1080,
    };

    fn temp(name: &str) -> std::path::PathBuf {
        env::temp_dir().join(format!("hud-core-test-{}-{name}", std::process::id()))
    }

    fn with_profiles() -> Config {
        let mut c = Config::first_run(SCREEN);
        c.profiles = vec![
            Profile {
                name: "Trabalho".into(),
                items: vec![MiniBarItem::Cpu, MiniBarItem::Ram],
                orientation: Orientation::Horizontal,
            },
            Profile {
                name: "Jogo".into(),
                items: vec![MiniBarItem::Temp, MiniBarItem::Ping, MiniBarItem::Cpu],
                orientation: Orientation::Vertical,
            },
        ];
        c
    }

    #[test]
    fn load_missing_file_is_not_found() {
        assert!(matches!(
            Config::load(&temp("missing")),
            Err(ConfigError::NotFound)
        ));
    }

    #[test]
    fn first_run_is_valid_and_top_right() {
        let c = Config::first_run(SCREEN);
        assert!(c.validate().is_ok());
        assert_eq!((c.hud.x, c.hud.y), (1920 - 320 - 12, 12));
    }

    #[test]
    fn opacity_out_of_range_is_invalid() {
        let toml = Config::first_run(SCREEN).to_toml().unwrap();
        let text: Vec<String> = toml
            .lines()
            .map(|l| match l.starts_with("opacity = ") {
                true => "opacity = 1.5".to_owned(),
                false => l.to_owned(),
            })
            .collect();
        let text = text.join(
            "
",
        );
        assert!(matches!(
            Config::from_toml(&text),
            Err(ConfigError::Invalid { field: "opacity" })
        ));
    }

    #[test]
    fn save_and_reload_is_structurally_equal() {
        let path = temp("roundtrip");
        let c = with_profiles();
        c.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(loaded, c);
    }

    #[test]
    fn switching_profile_applies_saved_items() {
        let mut c = Config::from_toml(&with_profiles().to_toml().unwrap()).unwrap();
        c.switch_profile("Jogo").unwrap();
        assert_eq!(
            c.minibar.items,
            [MiniBarItem::Temp, MiniBarItem::Ping, MiniBarItem::Cpu]
        );
        assert_eq!(c.minibar.orientation, Orientation::Vertical);
        assert_eq!(c.active_profile.as_deref(), Some("Jogo"));
    }

    #[test]
    fn unknown_profile_is_invalid() {
        let mut c = with_profiles();
        assert!(matches!(
            c.switch_profile("Nope"),
            Err(ConfigError::Invalid { field: "profile" })
        ));
    }

    #[test]
    fn empty_minibar_items_is_invalid() {
        let mut c = Config::first_run(SCREEN);
        c.minibar.items.clear();
        assert!(matches!(
            c.to_toml(),
            Err(ConfigError::Invalid {
                field: "minibar.items"
            })
        ));
    }

    #[test]
    fn inverted_threshold_is_invalid() {
        let mut c = Config::first_run(SCREEN);
        c.thresholds.cpu_percent = Threshold {
            warn: 90.0,
            crit: 70.0,
        };
        assert!(matches!(
            c.validate(),
            Err(ConfigError::Invalid {
                field: "thresholds.cpu_percent"
            })
        ));
    }

    #[test]
    fn duplicate_profile_names_are_invalid() {
        let mut c = with_profiles();
        c.profiles[1].name = "Trabalho".into();
        assert!(matches!(
            c.validate(),
            Err(ConfigError::Invalid {
                field: "profiles.duplicate"
            })
        ));
    }

    #[test]
    fn malformed_hotkey_is_invalid() {
        let mut c = Config::first_run(SCREEN);
        c.hotkeys.toggle_hud = "Ctrl+".into();
        assert!(matches!(
            c.validate(),
            Err(ConfigError::Invalid {
                field: "hotkeys.toggle_hud"
            })
        ));
    }

    #[test]
    fn hotkey_grammar() {
        for ok in [
            "Ctrl+Alt+M",
            "Win+F24",
            "Shift+Ctrl+Alt+Win+0",
            "Alt+PageUp",
            "Ctrl+Left",
        ] {
            assert!(is_hotkey(ok), "{ok}");
        }
        for bad in [
            "Foo+Bar+Baz",
            "M",
            "Ctrl+Ctrl+M",
            "Ctrl+",
            "",
            "Ctrl+m",
            "Ctrl+F25",
            "Ctrl+F0",
            "Ctrl+Alt",
            "Ctrl+M+Alt",
            "Ctrl+MM",
            "Ctrl+F01",
        ] {
            assert!(!is_hotkey(bad), "{bad}");
        }
    }

    #[test]
    fn invalid_minibar_hotkey_names_its_field() {
        let mut c = Config::first_run(SCREEN);
        c.hotkeys.toggle_minibar = "Foo+Bar+Baz".into();
        assert!(matches!(
            c.validate(),
            Err(ConfigError::Invalid {
                field: "hotkeys.toggle_minibar"
            })
        ));
    }

    #[test]
    fn garbage_is_a_parse_error() {
        assert!(matches!(
            Config::from_toml("not = [toml"),
            Err(ConfigError::Parse(_))
        ));
    }
}

#[cfg(test)]
mod appearance_tests {
    use super::*;

    const SCREEN: Size = Size {
        width: 1920,
        height: 1080,
    };

    #[test]
    fn toggling_a_window_hides_it_or_swaps_it_for_the_other() {
        let mut c = Config::first_run(SCREEN);
        c.toggle_hud();
        assert!(!c.hud.visible && !c.minibar.window.visible);
        c.toggle_minibar();
        assert!(c.minibar.window.visible && !c.hud.visible);
        c.toggle_hud();
        assert!(c.hud.visible && !c.minibar.window.visible);
    }

    #[test]
    fn crit_with_both_windows_hidden_shows_the_minibar() {
        let mut c = Config::first_run(Size {
            width: 1920,
            height: 1080,
        });
        c.hud.visible = false;
        c.minibar.window.visible = false;
        assert!(c.reveal_minibar_if_hidden());
        assert!(c.minibar.window.visible && !c.hud.visible);
        assert!(!c.reveal_minibar_if_hidden());
    }

    #[test]
    fn crit_leaves_a_visible_hud_alone() {
        let mut c = Config::first_run(Size {
            width: 1920,
            height: 1080,
        });
        c.hud.visible = true;
        assert!(!c.reveal_minibar_if_hidden());
        assert!(c.hud.visible && !c.minibar.window.visible);
    }

    #[test]
    fn opacity_below_twenty_percent_is_invalid() {
        let mut c = Config::first_run(SCREEN);
        c.hud.opacity = 0.1;
        assert!(c.validate().is_err());
        c.hud.opacity = 0.2;
        assert!(c.validate().is_ok());
    }

    #[test]
    fn config_saved_before_acrylic_and_click_through_still_loads() {
        let c = Config::first_run(SCREEN);
        let old: String = c
            .to_toml()
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with("acrylic") && !l.starts_with("click_through"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_eq!(Config::from_toml(&old).unwrap(), c);
    }

    #[test]
    fn config_saved_before_claude_threshold_gets_warn_70_crit_90() {
        let c = Config::first_run(SCREEN);
        let old: String = c
            .to_toml()
            .unwrap()
            .lines()
            .scan(false, |skip, l| {
                if l.starts_with("[thresholds.claude_5h]") {
                    *skip = true;
                } else if l.starts_with('[') {
                    *skip = false;
                }
                Some((*skip, l))
            })
            .filter(|(skip, _)| !skip)
            .map(|(_, l)| {
                format!(
                    "{l}
"
                )
            })
            .collect();
        assert!(!old.contains("claude_5h"));
        let loaded = Config::from_toml(&old).unwrap();
        assert_eq!(
            loaded.thresholds.claude_5h,
            Threshold {
                warn: 70.0,
                crit: 90.0
            }
        );
    }
}
