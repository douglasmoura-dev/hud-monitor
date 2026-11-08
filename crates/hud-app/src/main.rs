#![windows_subsystem = "windows"]

mod collect;
mod shell;
mod sources;
mod store;
mod winffi;

use hud_core::config::{Config, Orientation, HUD_SIZE};
use hud_core::integration::{apply_autostart, register_hotkeys, IntegrationError};
use hud_core::metrics::Snapshot;
use hud_core::position::{clamp_to_screen, Point, Size};
use hud_core::view;
use shell::{effective_opacity, NativeLook, Tray, TrayAction};
use slint::{ComponentHandle, PhysicalPosition};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

slint::include_modules!();

const POLL_MS: u64 = 100;
const WAITING: &str = "aguardando a primeira leitura";

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error(transparent)]
    Store(#[from] store::StoreError),
    #[error("could not create window: {0}")]
    Window(#[from] slint::PlatformError),
}

fn field(f: &view::Field) -> Field {
    Field {
        text: f.text.as_str().into(),
        tip: f.tip.as_str().into(),
    }
}

fn bar(b: &view::CoreBar) -> Bar {
    let [_, r, g, bl] = b.color.to_be_bytes();
    Bar {
        percent: b.percent,
        color: slint::Color::from_rgb_u8(r, g, bl),
    }
}

fn show_snapshot(hud: &Hud, s: &Snapshot) {
    let v = view::build(s);
    hud.set_cpu_total(field(&v.cpu_total));
    hud.set_cpu_freq(field(&v.cpu_freq));
    hud.set_cpu_temp(field(&v.cpu_temp));
    hud.set_cpu_cores(field(&v.cpu_cores));
    let bars: Vec<Bar> = v.core_bars.iter().map(bar).collect();
    hud.set_core_bars(Rc::new(slint::VecModel::from(bars)).into());
    hud.set_ram(field(&v.ram));
    hud.set_ram_percent(v.ram_percent);
    hud.set_ram_top(field(&v.ram_top));
    hud.set_gpu_intel(field(&v.gpu_intel));
    hud.set_gpu_nvidia(field(&v.gpu_nvidia));
    hud.set_gpu_vram(field(&v.gpu_vram));
    hud.set_disk(field(&v.disk));
    hud.set_disk_percent(v.disk_percent);
    hud.set_disk_io(field(&v.disk_io));
    hud.set_net_down(field(&v.net_down));
    hud.set_net_up(field(&v.net_up));
    hud.set_net_today(field(&v.net_today));
    hud.set_ping(field(&v.ping));
}

fn show_clock(hud: &Hud) {
    let (_, secs) = winffi::local_now();
    hud.set_clock(format!("{:02}:{:02}", secs / 3600, secs / 60 % 60).into());
}

fn chip_model(chips: &[view::Chip]) -> slint::ModelRc<ChipData> {
    let data: Vec<ChipData> = chips
        .iter()
        .map(|c| ChipData {
            label: c.label.into(),
            field: field(&c.value),
        })
        .collect();
    Rc::new(slint::VecModel::from(data)).into()
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

fn place(window: &slint::Window, saved: Point, size: Size, screen: Size) {
    let scale = window.scale_factor();
    let physical = Size {
        width: (size.width as f32 * scale).round() as i32,
        height: (size.height as f32 * scale).round() as i32,
    };
    let pos = clamp_to_screen(saved, physical, screen);
    window.set_position(PhysicalPosition::new(pos.x, pos.y));
}

/// Handles to both windows plus the persisted config; cloned into each callback.
#[derive(Clone)]
struct Windows {
    hud: slint::Weak<Hud>,
    bar: slint::Weak<MiniBar>,
    path: Rc<PathBuf>,
    config: Rc<RefCell<Config>>,
}

impl Windows {
    fn save(&self) {
        let (Some(hud), Some(bar)) = (self.hud.upgrade(), self.bar.upgrade()) else {
            return;
        };
        let h = hud.window().position();
        let b = bar.window().position();
        let mut config = self.config.borrow_mut();
        let saved = store::save_window_state(
            &self.path,
            &mut config,
            Point { x: h.x, y: h.y },
            Point { x: b.x, y: b.y },
        );
        if let Err(e) = saved {
            eprintln!("hud-monitor: could not save config: {e}");
        }
    }

    /// Shows or hides each window as the config says, then persists.
    fn apply_visibility(&self) {
        let (Some(hud), Some(bar)) = (self.hud.upgrade(), self.bar.upgrade()) else {
            return;
        };
        let (hud_on, bar_on) = {
            let c = self.config.borrow();
            (c.hud.visible, c.minibar.window.visible)
        };
        let result = match (hud_on, bar_on) {
            (true, _) => hud.show().and_then(|()| bar.hide()),
            (false, true) => bar.show().and_then(|()| hud.hide()),
            (false, false) => hud.hide().and_then(|()| bar.hide()),
        };
        if let Err(e) = result {
            eprintln!("hud-monitor: could not switch window: {e}");
        }
        self.save();
    }

    fn switch_to(&self, minibar: bool) {
        {
            let mut c = self.config.borrow_mut();
            c.hud.visible = !minibar;
            c.minibar.window.visible = minibar;
        }
        self.apply_visibility();
    }

    fn toggle_hud(&self) {
        self.config.borrow_mut().toggle_hud();
        self.apply_visibility();
    }

    fn toggle_minibar(&self) {
        self.config.borrow_mut().toggle_minibar();
        self.apply_visibility();
    }

    fn toggle_pin(&self) {
        let pinned = {
            let mut c = self.config.borrow_mut();
            c.hud.click_through = !c.hud.click_through;
            c.hud.click_through
        };
        if let Some(hud) = self.hud.upgrade() {
            hud.set_pinned(pinned);
        }
        self.save();
    }

    fn switch_profile(&self, name: &str) {
        let (items, orientation) = {
            let mut c = self.config.borrow_mut();
            if let Err(e) = c.switch_profile(name) {
                eprintln!("hud-monitor: could not switch profile: {e}");
                return;
            }
            (c.minibar.items.clone(), c.minibar.orientation)
        };
        if let Some(bar) = self.bar.upgrade() {
            let size = view::minibar_size(items.len(), orientation);
            bar.set_vertical(orientation == Orientation::Vertical);
            bar.window().set_size(slint::LogicalSize::new(
                size.width as f32,
                size.height as f32,
            ));
        }
        self.save();
    }

    fn handle(&self, action: TrayAction) {
        match action {
            TrayAction::ShowHud => self.switch_to(false),
            TrayAction::ShowMinibar => self.switch_to(true),
            TrayAction::Profile(name) => self.switch_profile(&name),
            TrayAction::Quit => self.quit(),
        }
    }

    fn quit(&self) {
        self.save();
        if let (Some(hud), Some(bar)) = (self.hud.upgrade(), self.bar.upgrade()) {
            let _ = hud.hide();
            let _ = bar.hide();
        }
        let _ = slint::quit_event_loop();
    }
}

/// Syncs the Run key with the config; failures are logged, never fatal.
fn sync_autostart(enabled: bool) {
    let result = std::env::current_exe()
        .map_err(|e| IntegrationError(e.to_string()))
        .and_then(|exe| apply_autostart(&mut winffi::RegistryRun, enabled, &exe));
    if let Err(e) = result {
        eprintln!("hud-monitor: autostart not applied: {e}");
    }
}

fn run() -> Result<(), AppError> {
    let path = Rc::new(store::default_path()?);
    let (hud, bar) = (Hud::new()?, MiniBar::new()?);
    let screen = winffi::screen_size();
    let config = store::load_or_first_run(&path, screen)?;
    let waiting = Snapshot::unavailable(WAITING);
    show_snapshot(&hud, &waiting);
    show_clock(&hud);
    let items = config.minibar.items.clone();
    bar.set_chips(chip_model(&view::minibar_chips(&items, &waiting)));
    hud.set_bg_alpha(effective_opacity(config.hud.opacity));
    hud.set_pinned(config.hud.click_through);
    bar.set_bg_alpha(effective_opacity(config.minibar.window.opacity));
    bar.set_vertical(config.minibar.orientation == Orientation::Vertical);
    let bar_size = view::minibar_size(items.len(), config.minibar.orientation);
    bar.window().set_size(slint::LogicalSize::new(
        bar_size.width as f32,
        bar_size.height as f32,
    ));
    let hud_at = Point {
        x: config.hud.x,
        y: config.hud.y,
    };
    place(hud.window(), hud_at, HUD_SIZE, screen);
    let bar_at = Point {
        x: config.minibar.window.x,
        y: config.minibar.window.y,
    };
    place(bar.window(), bar_at, bar_size, screen);
    sync_autostart(config.autostart);

    let tray = Tray::new(&config.profiles)
        .map_err(|e| eprintln!("hud-monitor: {e}"))
        .ok();
    let mut hotkeys = shell::Hotkeys::new()
        .map_err(|e| eprintln!("hud-monitor: global hotkeys unavailable: {e}"))
        .ok();
    if let Some(h) = hotkeys.as_mut() {
        for warning in register_hotkeys(h, &config.hotkeys) {
            eprintln!("hud-monitor: {warning}");
        }
    }
    let toggle_hud_combo = config.hotkeys.toggle_hud.clone();

    let w = Windows {
        hud: hud.as_weak(),
        bar: bar.as_weak(),
        path,
        config: Rc::new(RefCell::new(config)),
    };
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
    let saver = w.clone();
    hud.on_released(move || saver.save());
    let saver = w.clone();
    bar.on_released(move || saver.save());
    let quitter = w.clone();
    hud.on_quit(move || quitter.quit());
    let to_bar = w.clone();
    hud.on_minimize(move || to_bar.switch_to(true));
    let pinner = w.clone();
    hud.on_pin(move || pinner.toggle_pin());
    let quitter = w.clone();
    bar.on_quit(move || quitter.quit());
    let to_hud = w.clone();
    bar.on_expand(move || to_hud.switch_to(false));

    let (snapshots, collection) = collect::spawn();
    let (hud_look, bar_look) = (NativeLook::default(), NativeLook::default());
    let poll = slint::Timer::default();
    let ticker = w.clone();
    poll.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(POLL_MS),
        move || {
            let (Some(hud), Some(bar)) = (ticker.hud.upgrade(), ticker.bar.upgrade()) else {
                return;
            };
            show_clock(&hud);
            if let Some(s) = snapshots.try_iter().last() {
                show_snapshot(&hud, &s);
                let items = ticker.config.borrow().minibar.items.clone();
                bar.set_chips(chip_model(&view::minibar_chips(&items, &s)));
                if let Some(tray) = &tray {
                    tray.set_tooltip(&view::tray_tooltip(&s));
                }
            }
            if let Some(tray) = &tray {
                for action in tray.poll() {
                    ticker.handle(action);
                }
            }
            if let Some(h) = &hotkeys {
                for combo in h.pressed() {
                    if combo == toggle_hud_combo {
                        ticker.toggle_hud();
                    } else {
                        ticker.toggle_minibar();
                    }
                }
            }
            let ctrl = winffi::ctrl_down();
            let c = ticker.config.borrow();
            hud_look.refresh(hud.window(), &c.hud, ctrl, |a| hud.set_bg_alpha(a));
            bar_look.refresh(bar.window(), &c.minibar.window, ctrl, |a| {
                bar.set_bg_alpha(a)
            });
        },
    );
    w.apply_visibility();
    slint::run_event_loop_until_quit()?;
    collection.shutdown();
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("hud-monitor: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    const HUD: &str = include_str!("../ui/hud.slint");
    const THEME: &str = include_str!("../ui/theme.slint");
    const MINIBAR: &str = include_str!("../ui/minibar.slint");

    fn families(src: &str) -> Vec<&str> {
        src.lines()
            .filter_map(|l| l.trim().strip_prefix("font-family:"))
            .map(|v| v.trim().trim_end_matches(';'))
            .collect()
    }

    #[test]
    fn theme_maps_labels_to_rajdhani_and_numbers_to_share_tech_mono() {
        assert!(THEME.contains(r#"label-font: "Rajdhani""#));
        assert!(THEME.contains(r#"number-font: "Share Tech Mono""#));
        assert!(THEME.contains("fonts/Rajdhani-Medium.ttf"));
        assert!(THEME.contains("fonts/Rajdhani-SemiBold.ttf"));
        assert!(THEME.contains("fonts/ShareTechMono-Regular.ttf"));
    }

    #[test]
    fn every_text_in_the_hud_uses_one_of_the_two_fonts() {
        let used = families(HUD);
        assert!(!used.is_empty());
        assert!(used
            .iter()
            .all(|f| matches!(*f, "Theme.label-font" | "Theme.number-font")));
        assert!(used.contains(&"Theme.number-font"));
    }

    #[test]
    fn unavailable_values_report_their_reason_on_hover() {
        assert!(HUD.contains("root.field.tip != \"\""));
        assert!(HUD.contains("root.tip(self.has-hover ? root.field.tip"));
    }

    #[test]
    fn bar_colors_come_from_the_view_not_the_slint_file() {
        assert!(HUD.contains("background: bar.color"));
    }

    #[test]
    fn slint_window_size_matches_hud_size() {
        use hud_core::config::HUD_SIZE;
        assert!(HUD.contains(&format!("width: {}px;", HUD_SIZE.width)));
        assert!(HUD.contains(&format!("height: {}px;", HUD_SIZE.height)));
    }

    #[test]
    fn minibar_is_borderless_topmost_and_uses_theme_fonts() {
        assert!(MINIBAR.contains("no-frame: true;"));
        assert!(MINIBAR.contains("always-on-top: true;"));
        assert!(families(MINIBAR)
            .iter()
            .all(|f| matches!(*f, "Theme.label-font" | "Theme.number-font")));
    }

    #[test]
    fn minibar_lays_out_by_orientation_and_both_windows_expose_the_toggle() {
        assert!(MINIBAR.contains("root.vertical ? 6px : 30px + i * 68px"));
        assert!(MINIBAR.contains("root.vertical ? 30px + i * 40px : 6px"));
        assert!(MINIBAR.contains("root.expand()"));
        assert!(HUD.contains("root.minimize()"));
    }

    #[test]
    fn hud_pin_icon_toggles_click_through_and_shows_its_state() {
        assert!(HUD.contains("root.pin()"));
        assert!(HUD.contains("root.pinned ? Theme.teal"));
    }
}
