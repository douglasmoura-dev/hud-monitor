//! Persisted configuration: window states, mini bar, profiles, thresholds, hotkeys.

use crate::position::{first_run_position, Size, MARGIN};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

pub const HUD_SIZE: Size = Size {
    width: 320,
    height: 200,
};

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
    if (0.0..=1.0).contains(&w.opacity) {
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

/// `Mod+Mod+Key` shape: at least one `+`-separated part and none empty.
fn is_hotkey(text: &str) -> bool {
    text.split('+').all(|part| !part.trim().is_empty())
}

impl Config {
    /// Explicit initial config: HUD top-right, mini bar just below it (hidden).
    pub fn first_run(screen: Size) -> Config {
        let hud_pos = first_run_position(screen, HUD_SIZE);
        Config {
            hud: WindowState {
                x: hud_pos.x,
                y: hud_pos.y,
                opacity: 0.9,
                visible: true,
            },
            minibar: MiniBar {
                window: WindowState {
                    x: hud_pos.x,
                    y: hud_pos.y + HUD_SIZE.height + MARGIN,
                    opacity: 0.9,
                    visible: false,
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
                cpu_percent: Threshold {
                    warn: 70.0,
                    crit: 90.0,
                },
                ram_percent: Threshold {
                    warn: 75.0,
                    crit: 90.0,
                },
                temp_c: Threshold {
                    warn: 75.0,
                    crit: 90.0,
                },
                ping_ms: Threshold {
                    warn: 80.0,
                    crit: 200.0,
                },
            },
            hotkeys: Hotkeys {
                toggle_hud: "Ctrl+Alt+H".to_owned(),
                toggle_minibar: "Ctrl+Alt+M".to_owned(),
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
        if !is_hotkey(&self.hotkeys.toggle_hud) || !is_hotkey(&self.hotkeys.toggle_minibar) {
            return Err(invalid("hotkeys"));
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
            Err(ConfigError::Invalid { field: "hotkeys" })
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
