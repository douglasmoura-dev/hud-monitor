//! Desktop shell integration: tray icon, global hotkeys and native window look.

use crate::winffi;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use hud_core::appearance::{click_through_style, passes_clicks, MIN_OPACITY};
use hud_core::config::{Profile, WindowState};
use hud_core::integration::{apply_backdrop, HotkeyApi, IntegrationError};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::Cell;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const ICON_SIZE: u32 = 16;
const ICON_RGB: [u8; 3] = [0x1D, 0x9E, 0x75];

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("tray icon: {0}")]
    Tray(String),
}

fn tray_err(e: impl std::fmt::Display) -> ShellError {
    ShellError::Tray(e.to_string())
}

/// What the user asked for through the tray menu.
#[derive(Debug, PartialEq, Eq)]
pub enum TrayAction {
    ShowHud,
    ShowMinibar,
    Profile(String),
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    show_hud: MenuItem,
    show_minibar: MenuItem,
    quit: MenuItem,
    profiles: Vec<(MenuItem, String)>,
}

/// Filled circle in the accent color on a transparent square.
fn circle_icon() -> Result<Icon, ShellError> {
    let mid = ICON_SIZE as f32 / 2.0;
    let mut rgba = Vec::with_capacity((ICON_SIZE * ICON_SIZE * 4) as usize);
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let (dx, dy) = (x as f32 + 0.5 - mid, y as f32 + 0.5 - mid);
            let inside = dx * dx + dy * dy <= (mid - 1.0) * (mid - 1.0);
            let [r, g, b] = ICON_RGB;
            rgba.extend_from_slice(&[r, g, b, if inside { 255 } else { 0 }]);
        }
    }
    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).map_err(tray_err)
}

impl Tray {
    pub fn new(profiles: &[Profile]) -> Result<Self, ShellError> {
        let show_hud = MenuItem::new("Mostrar HUD", true, None);
        let show_minibar = MenuItem::new("Mostrar barrinha", true, None);
        let quit = MenuItem::new("Sair", true, None);
        let submenu = Submenu::new("Perfis", !profiles.is_empty());
        let items: Vec<(MenuItem, String)> = profiles
            .iter()
            .map(|p| (MenuItem::new(&p.name, true, None), p.name.clone()))
            .collect();
        for (item, _) in &items {
            submenu.append(item).map_err(tray_err)?;
        }
        let menu = Menu::new();
        menu.append_items(&[
            &show_hud,
            &show_minibar,
            &submenu,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(tray_err)?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("HUD Monitor")
            .with_icon(circle_icon()?)
            .build()
            .map_err(tray_err)?;
        Ok(Self {
            icon,
            show_hud,
            show_minibar,
            quit,
            profiles: items,
        })
    }

    pub fn set_tooltip(&self, text: &str) {
        if let Err(e) = self.icon.set_tooltip(Some(text)) {
            eprintln!("hud-monitor: could not update tray tooltip: {e}");
        }
    }

    /// Menu clicks received since the last call.
    pub fn poll(&self) -> Vec<TrayAction> {
        MenuEvent::receiver()
            .try_iter()
            .filter_map(|e| self.action(&e.id))
            .collect()
    }

    fn action(&self, id: &tray_icon::menu::MenuId) -> Option<TrayAction> {
        if *id == *self.show_hud.id() {
            Some(TrayAction::ShowHud)
        } else if *id == *self.show_minibar.id() {
            Some(TrayAction::ShowMinibar)
        } else if *id == *self.quit.id() {
            Some(TrayAction::Quit)
        } else {
            self.profiles
                .iter()
                .find(|(item, _)| *item.id() == *id)
                .map(|(_, name)| TrayAction::Profile(name.clone()))
        }
    }
}

/// Global hotkeys registered with the OS; `pressed` maps events back to the combo text.
pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    registered: Vec<(u32, String)>,
}

impl Hotkeys {
    pub fn new() -> Result<Self, IntegrationError> {
        let manager = GlobalHotKeyManager::new().map_err(|e| IntegrationError(e.to_string()))?;
        Ok(Self {
            manager,
            registered: Vec::new(),
        })
    }

    /// Combos pressed since the last call.
    pub fn pressed(&self) -> Vec<&str> {
        GlobalHotKeyEvent::receiver()
            .try_iter()
            .filter(|e| e.state == HotKeyState::Pressed)
            .filter_map(|e| {
                self.registered
                    .iter()
                    .find(|(id, _)| *id == e.id)
                    .map(|(_, combo)| combo.as_str())
            })
            .collect()
    }
}

impl HotkeyApi for Hotkeys {
    fn register(&mut self, combo: &str) -> Result<(), IntegrationError> {
        let hotkey: HotKey =
            combo
                .parse()
                .map_err(|e: global_hotkey::hotkey::HotKeyParseError| {
                    IntegrationError(e.to_string())
                })?;
        self.manager
            .register(hotkey)
            .map_err(|e| IntegrationError(e.to_string()))?;
        self.registered.push((hotkey.id(), combo.to_owned()));
        Ok(())
    }
}

/// Win32 handle of a shown Slint window, `None` while it is hidden.
fn hwnd_of(window: &slint::Window) -> Option<isize> {
    match window.window_handle().window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

/// Opacity actually drawn: config values are clamped to 20-100 %.
pub fn effective_opacity(opacity: f32) -> f32 {
    opacity.clamp(MIN_OPACITY, 1.0)
}

/// Keeps one window's native look (acrylic, click-through) in sync with its config.
#[derive(Default)]
pub struct NativeLook {
    hwnd: Cell<isize>,
}

impl NativeLook {
    /// Cheap enough to run on a short timer: acrylic is applied once per native window,
    /// click-through only changes the style when Ctrl is pressed or released.
    pub fn refresh(
        &self,
        window: &slint::Window,
        state: &WindowState,
        ctrl_down: bool,
        set_alpha: impl Fn(f32),
    ) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        if self.hwnd.replace(hwnd) != hwnd {
            let opacity = effective_opacity(state.opacity);
            let out = apply_backdrop(&mut winffi::WindowBackdrop(hwnd), state.acrylic, opacity);
            set_alpha(out.slint_alpha);
            if let Some(warning) = out.warning {
                eprintln!("hud-monitor: {warning}");
            }
        }
        let current = winffi::ex_style(hwnd);
        let wanted = click_through_style(current, passes_clicks(state.click_through, ctrl_down));
        if wanted != current {
            winffi::set_ex_style(hwnd, wanted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacity_is_clamped_to_twenty_through_hundred_percent() {
        assert_eq!(effective_opacity(0.05), 0.2);
        assert_eq!(effective_opacity(0.55), 0.55);
        assert_eq!(effective_opacity(3.0), 1.0);
    }

    #[test]
    fn circle_icon_builds() {
        assert!(circle_icon().is_ok());
    }
}
