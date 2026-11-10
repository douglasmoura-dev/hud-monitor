//! The two Slint windows as seen by the window use case: the `WindowHost` port, dragging,
//! placement on screen and the native look refresh.

use super::{paint_hud, paint_minibar, Hud, MiniBar, Theme};
use crate::scanner;
use crate::shell::{effective_opacity, NativeLook};
use crate::store::FileConfigStore;
use crate::winffi;
use hud_core::config::{Config, Orientation};
use hud_core::position::{clamp_to_screen, Point, Size};
use hud_core::scale::scaled_size;
use hud_core::theme::Palette;
use hud_core::ui_text::PIN_TIP;
use hud_core::view::minibar_size;
use hud_core::windowing::{WindowController, WindowHost, WindowId};
use slint::{ComponentHandle, PhysicalPosition};
use std::rc::Rc;

pub type Controller = WindowController<SlintWindows, FileConfigStore>;

/// Weak handles to both windows; they stop working once the windows are gone.
#[derive(Clone)]
pub struct SlintWindows {
    hud: slint::Weak<Hud>,
    bar: slint::Weak<MiniBar>,
    scanner: scanner::Slot,
    screen: Size,
}

impl SlintWindows {
    pub fn new(hud: &Hud, bar: &MiniBar, scanner: scanner::Slot, screen: Size) -> Self {
        Self {
            hud: hud.as_weak(),
            bar: bar.as_weak(),
            scanner,
            screen,
        }
    }

    pub fn upgrade(&self) -> Option<(Hud, MiniBar)> {
        Some((self.hud.upgrade()?, self.bar.upgrade()?))
    }
}

fn gone() -> String {
    "window already closed".to_owned()
}

impl WindowHost for SlintWindows {
    fn show(&self, window: WindowId) -> Result<(), String> {
        let shown = match window {
            WindowId::Hud => self.hud.upgrade().ok_or_else(gone)?.show(),
            WindowId::MiniBar => self.bar.upgrade().ok_or_else(gone)?.show(),
        };
        shown.map_err(|e| e.to_string())
    }

    fn hide(&self, window: WindowId) -> Result<(), String> {
        let hidden = match window {
            WindowId::Hud => self.hud.upgrade().ok_or_else(gone)?.hide(),
            WindowId::MiniBar => self.bar.upgrade().ok_or_else(gone)?.hide(),
        };
        hidden.map_err(|e| e.to_string())
    }

    fn positions(&self) -> Option<(Point, Point)> {
        let (hud, bar) = self.upgrade()?;
        let (h, b) = (hud.window().position(), bar.window().position());
        Some((Point { x: h.x, y: h.y }, Point { x: b.x, y: b.y }))
    }

    fn set_pinned(&self, pinned: bool) {
        if let Some(hud) = self.hud.upgrade() {
            hud.set_pinned(pinned);
        }
    }

    fn resize_minibar(&self, size: Size, vertical: bool) {
        if let Some(bar) = self.bar.upgrade() {
            bar.set_vertical(vertical);
            bar.window().set_size(slint::LogicalSize::new(
                size.width as f32,
                size.height as f32,
            ));
        }
    }

    fn set_scale(&self, window: WindowId, scale: f32) {
        match window {
            WindowId::Hud => {
                if let Some(hud) = self.hud.upgrade() {
                    set_window_scale(&hud, scale);
                }
            }
            WindowId::MiniBar => {
                if let Some(bar) = self.bar.upgrade() {
                    set_window_scale(&bar, scale);
                }
            }
        }
    }

    fn keep_on_screen(&self, window: WindowId) {
        let inside = |w: &slint::Window| {
            let (p, s) = (w.position(), w.size());
            let size = Size {
                width: s.width as i32,
                height: s.height as i32,
            };
            let pos = clamp_to_screen(Point { x: p.x, y: p.y }, size, self.screen);
            w.set_position(PhysicalPosition::new(pos.x, pos.y));
        };
        match window {
            WindowId::Hud => {
                if let Some(hud) = self.hud.upgrade() {
                    inside(hud.window());
                }
            }
            WindowId::MiniBar => {
                if let Some(bar) = self.bar.upgrade() {
                    inside(bar.window());
                }
            }
        }
    }

    fn apply_theme(&self, palette: &Palette) {
        if let Some((hud, bar)) = self.upgrade() {
            paint_hud(&hud, palette);
            paint_minibar(&bar, palette);
        }
        scanner::apply_theme(&self.scanner, palette);
    }

    fn quit(&self) {
        let _ = slint::quit_event_loop();
    }

    fn report(&self, message: &str) {
        eprintln!("hud-monitor: {message}");
    }
}

/// Sets the `Theme.scale` of one window: every length in its Slint file follows it.
fn set_window_scale<'a, C: ComponentHandle>(window: &'a C, scale: f32)
where
    Theme<'a>: slint::Global<'a, C>,
{
    use slint::Global as _;
    Theme::get(window).set_scale(scale);
}

/// Moves `window` by a drag delta reported in logical pixels.
fn drag(window: &slint::Window, dx: f32, dy: f32) {
    let p = window.position();
    let s = window.scale_factor();
    window.set_position(PhysicalPosition::new(
        p.x + (dx * s) as i32,
        p.y + (dy * s) as i32,
    ));
}

/// Puts `window` at `saved`, pulled back inside the screen.
fn place(window: &slint::Window, saved: Point, size: Size, screen: Size) {
    let scale = window.scale_factor();
    let physical = Size {
        width: (size.width as f32 * scale).round() as i32,
        height: (size.height as f32 * scale).round() as i32,
    };
    let pos = clamp_to_screen(saved, physical, screen);
    window.set_position(PhysicalPosition::new(pos.x, pos.y));
}

/// First paint of the window chrome from the config: opacity, pin, orientation, size, position.
pub fn apply_config(hud: &Hud, bar: &MiniBar, config: &Config, screen: Size) {
    let palette = config.appearance.theme.palette();
    paint_hud(hud, &palette);
    paint_minibar(bar, &palette);
    hud.set_bg_alpha(effective_opacity(config.hud.opacity));
    hud.set_pinned(config.hud.click_through);
    hud.set_pin_tip(PIN_TIP.into());
    bar.set_bg_alpha(effective_opacity(config.minibar.window.opacity));
    bar.set_vertical(config.minibar.orientation == Orientation::Vertical);
    set_window_scale(hud, config.hud.scale);
    set_window_scale(bar, config.minibar.window.scale);
    let bar_size = scaled_size(
        minibar_size(config.minibar.items.len(), config.minibar.orientation),
        config.minibar.window.scale,
    );
    bar.window().set_size(slint::LogicalSize::new(
        bar_size.width as f32,
        bar_size.height as f32,
    ));
    let hud_at = Point {
        x: config.hud.x,
        y: config.hud.y,
    };
    let hud_size = scaled_size(hud_core::config::HUD_SIZE, config.hud.scale);
    place(hud.window(), hud_at, hud_size, screen);
    let bar_at = Point {
        x: config.minibar.window.x,
        y: config.minibar.window.y,
    };
    place(bar.window(), bar_at, bar_size, screen);
}

/// Wires dragging, releasing, closing, minimising, pinning and expanding to the use case.
pub fn connect(hud: &Hud, bar: &MiniBar, controller: &Rc<Controller>) {
    let weak = hud.as_weak();
    hud.on_moved(move |dx, dy| {
        if let Some(hud) = weak.upgrade() {
            drag(hud.window(), dx, dy);
        }
    });
    let weak = bar.as_weak();
    bar.on_moved(move |dx, dy| {
        if let Some(bar) = weak.upgrade() {
            drag(bar.window(), dx, dy);
        }
    });
    let c = controller.clone();
    hud.on_resize(move |w, h, dx, dy| c.resize_drag(WindowId::Hud, (w, h), dx, dy));
    let c = controller.clone();
    bar.on_resize(move |w, h, dx, dy| c.resize_drag(WindowId::MiniBar, (w, h), dx, dy));
    let c = controller.clone();
    hud.on_resize_done(move || c.finish_resize(WindowId::Hud));
    let c = controller.clone();
    bar.on_resize_done(move || c.finish_resize(WindowId::MiniBar));
    let c = controller.clone();
    hud.on_released(move || c.save());
    let c = controller.clone();
    bar.on_released(move || c.save());
    let c = controller.clone();
    hud.on_quit(move || c.quit());
    let c = controller.clone();
    hud.on_minimize(move || c.switch_to(true));
    let c = controller.clone();
    hud.on_pin(move || c.toggle_pin());
    let c = controller.clone();
    bar.on_quit(move || c.quit());
    let c = controller.clone();
    bar.on_expand(move || c.switch_to(false));
}

/// Native look (acrylic, click-through) of both windows.
#[derive(Default)]
pub struct Looks {
    hud: NativeLook,
    bar: NativeLook,
}

impl Looks {
    pub fn refresh(&self, windows: &SlintWindows, controller: &Controller) {
        let Some((hud, bar)) = windows.upgrade() else {
            return;
        };
        let ctrl = winffi::ctrl_down();
        let config = controller.config();
        let background = config.appearance.theme.palette().background;
        self.hud
            .refresh(hud.window(), &config.hud, ctrl, background, |a| {
                hud.set_bg_alpha(a)
            });
        self.bar.refresh(
            bar.window(),
            &config.minibar.window,
            ctrl,
            background,
            |a| bar.set_bg_alpha(a),
        );
    }
}
