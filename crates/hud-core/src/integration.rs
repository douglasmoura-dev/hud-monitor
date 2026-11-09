//! OS integration rules behind traits (acrylic, global hotkeys, autostart) so they test with fakes.

use crate::appearance::acrylic_tint;
use crate::config::Hotkeys;
use crate::theme::Color;
use crate::ui_text::{hotkeys_unavailable, tray_unavailable};
use std::path::Path;

pub const AUTOSTART_VALUE: &str = "hud-monitor";

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct IntegrationError(pub String);

/// Enables the blurred backdrop on a window; the real one calls the Windows API.
pub trait Backdrop {
    fn enable_acrylic(&mut self, tint_abgr: u32) -> Result<(), IntegrationError>;
}

pub struct BackdropOutcome {
    /// Opacity the Slint background must draw (0 when the OS draws the tinted backdrop).
    pub slint_alpha: f32,
    pub warning: Option<String>,
}

/// Tries acrylic when wanted; on failure keeps the solid background at the same opacity.
pub fn apply_backdrop(
    api: &mut dyn Backdrop,
    acrylic: bool,
    opacity: f32,
    background: Color,
) -> BackdropOutcome {
    if !acrylic {
        return BackdropOutcome {
            slint_alpha: opacity,
            warning: None,
        };
    }
    match api.enable_acrylic(acrylic_tint(background, opacity)) {
        Ok(()) => BackdropOutcome {
            slint_alpha: 0.0,
            warning: None,
        },
        Err(e) => BackdropOutcome {
            slint_alpha: opacity,
            warning: Some(format!("acrylic unavailable, using solid background: {e}")),
        },
    }
}

/// Registers one global hotkey; the real one wraps the `global-hotkey` crate.
pub trait HotkeyApi {
    fn register(&mut self, combo: &str) -> Result<(), IntegrationError>;
}

/// Registers both hotkeys; a conflict is a warning, never an error.
pub fn register_hotkeys(api: &mut dyn HotkeyApi, hotkeys: &Hotkeys) -> Vec<String> {
    [&hotkeys.toggle_hud, &hotkeys.toggle_minibar]
        .into_iter()
        .filter_map(|combo| {
            api.register(combo)
                .err()
                .map(|e| format!("hotkey {combo} not registered: {e}"))
        })
        .collect()
}

/// One-line notice for the HUD tip bar about shell features that could not start.
pub fn shell_notice(
    tray_error: Option<&str>,
    hotkeys_error: Option<&str>,
    hotkey_warnings: &[String],
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(e) = tray_error {
        parts.push(tray_unavailable(e));
    }
    if let Some(e) = hotkeys_error {
        parts.push(hotkeys_unavailable(e));
    }
    parts.extend(hotkey_warnings.iter().cloned());
    parts.join(" · ")
}

/// The `Run` registry key of the current user.
pub trait RunKey {
    fn set(&mut self, name: &str, command: &str) -> Result<(), IntegrationError>;
    fn remove(&mut self, name: &str) -> Result<(), IntegrationError>;
}

/// Adds the quoted exe path to the Run key, or removes the value.
pub fn apply_autostart(
    key: &mut dyn RunKey,
    enabled: bool,
    exe: &Path,
) -> Result<(), IntegrationError> {
    if enabled {
        key.set(AUTOSTART_VALUE, &format!("\"{}\"", exe.display()))
    } else {
        key.remove(AUTOSTART_VALUE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    struct FakeBackdrop(Option<&'static str>, Vec<u32>);
    impl Backdrop for FakeBackdrop {
        fn enable_acrylic(&mut self, tint: u32) -> Result<(), IntegrationError> {
            self.1.push(tint);
            self.0.map_or(Ok(()), |e| Err(IntegrationError(e.into())))
        }
    }

    #[test]
    fn acrylic_success_hands_the_tint_to_the_os() {
        let mut api = FakeBackdrop(None, vec![]);
        let out = apply_backdrop(&mut api, true, 0.55, BG);
        assert_eq!(api.1, vec![acrylic_tint(BG, 0.55)]);
        assert!(out.warning.is_none() && out.slint_alpha == 0.0);
    }

    #[test]
    fn acrylic_failure_keeps_solid_background_and_warns() {
        let mut api = FakeBackdrop(Some("denied"), vec![]);
        let out = apply_backdrop(&mut api, true, 0.55, BG);
        assert_eq!(out.slint_alpha, 0.55);
        assert!(out.warning.unwrap().contains("denied"));
    }

    #[test]
    fn acrylic_off_never_calls_the_os() {
        let mut api = FakeBackdrop(None, vec![]);
        let out = apply_backdrop(&mut api, false, 0.7, BG);
        assert!(api.1.is_empty() && out.slint_alpha == 0.7 && out.warning.is_none());
    }

    const BG: Color = Color::rgb(0x0A0E14);

    struct FakeHotkeys(&'static str);
    impl HotkeyApi for FakeHotkeys {
        fn register(&mut self, combo: &str) -> Result<(), IntegrationError> {
            if combo == self.0 {
                Err(IntegrationError("already registered".into()))
            } else {
                Ok(())
            }
        }
    }

    fn hotkeys() -> Hotkeys {
        Hotkeys {
            toggle_hud: "Ctrl+Alt+H".into(),
            toggle_minibar: "Ctrl+Alt+M".into(),
        }
    }

    #[test]
    fn hotkey_conflict_is_a_warning_and_the_other_still_registers() {
        let warnings = register_hotkeys(&mut FakeHotkeys("Ctrl+Alt+M"), &hotkeys());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Ctrl+Alt+M"));
        assert!(register_hotkeys(&mut FakeHotkeys("none"), &hotkeys()).is_empty());
    }

    #[test]
    fn notice_names_each_failed_feature_and_is_empty_when_all_works() {
        assert_eq!(shell_notice(None, None, &[]), "");
        let warnings = register_hotkeys(&mut FakeHotkeys("Ctrl+Alt+H"), &hotkeys());
        let n = shell_notice(Some("no tray"), Some("already taken"), &warnings);
        assert!(n.starts_with(&format!(
            "{} · {}",
            tray_unavailable("no tray"),
            hotkeys_unavailable("already taken")
        )));
        assert!(n.contains("Ctrl+Alt+H"));
    }

    #[derive(Default)]
    struct FakeRun(HashMap<String, String>);
    impl RunKey for FakeRun {
        fn set(&mut self, name: &str, command: &str) -> Result<(), IntegrationError> {
            self.0.insert(name.into(), command.into());
            Ok(())
        }
        fn remove(&mut self, name: &str) -> Result<(), IntegrationError> {
            self.0.remove(name);
            Ok(())
        }
    }

    #[test]
    fn autostart_on_writes_quoted_exe_and_off_removes_it() {
        let mut key = FakeRun::default();
        let exe = PathBuf::from(r"C:\Apps\hud app\hud-app.exe");
        apply_autostart(&mut key, true, &exe).unwrap();
        assert_eq!(key.0["hud-monitor"], r#""C:\Apps\hud app\hud-app.exe""#);
        apply_autostart(&mut key, false, &exe).unwrap();
        assert!(key.0.is_empty());
    }
}
