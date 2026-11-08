#![windows_subsystem = "windows"]

mod collect;
mod sources;
mod store;
mod winffi;

use hud_core::config::{Config, HUD_SIZE};
use hud_core::metrics::Snapshot;
use hud_core::position::{clamp_to_screen, Point, Size};
use hud_core::view;
use slint::{ComponentHandle, PhysicalPosition};
use std::{cell::RefCell, rc::Rc};

slint::include_modules!();

const WAITING: &str = "aguardando a primeira leitura";

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error(transparent)]
    Store(#[from] store::StoreError),
    #[error("could not create window: {0}")]
    Window(#[from] slint::PlatformError),
}

fn save_position(hud: &Hud, path: &std::path::Path, config: &mut Config) {
    let p = hud.window().position();
    if let Err(e) = store::save_hud_position(path, config, Point { x: p.x, y: p.y }) {
        eprintln!("hud-monitor: could not save config: {e}");
    }
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

fn run() -> Result<(), AppError> {
    let path = Rc::new(store::default_path()?);
    let hud = Hud::new()?;
    show_snapshot(&hud, &Snapshot::unavailable(WAITING));
    show_clock(&hud);
    let scale = hud.window().scale_factor();
    let size = Size {
        width: (HUD_SIZE.width as f32 * scale).round() as i32,
        height: (HUD_SIZE.height as f32 * scale).round() as i32,
    };
    let screen = winffi::screen_size();
    let config = Rc::new(RefCell::new(store::load_or_first_run(&path, screen)?));
    let saved = Point {
        x: config.borrow().hud.x,
        y: config.borrow().hud.y,
    };
    hud.set_bg_alpha(config.borrow().hud.opacity);
    let pos = clamp_to_screen(saved, size, screen);
    hud.window()
        .set_position(PhysicalPosition::new(pos.x, pos.y));

    let weak = hud.as_weak();
    hud.on_moved(move |dx, dy| {
        if let Some(hud) = weak.upgrade() {
            let p = hud.window().position();
            let s = hud.window().scale_factor();
            let (dx, dy) = ((dx * s) as i32, (dy * s) as i32);
            hud.window()
                .set_position(PhysicalPosition::new(p.x + dx, p.y + dy));
        }
    });
    let (weak, saved_to, cfg) = (hud.as_weak(), path.clone(), config.clone());
    hud.on_released(move || {
        if let Some(hud) = weak.upgrade() {
            save_position(&hud, &saved_to, &mut cfg.borrow_mut());
        }
    });
    let weak = hud.as_weak();
    hud.on_quit(move || {
        if let Some(hud) = weak.upgrade() {
            save_position(&hud, &path, &mut config.borrow_mut());
            let _ = hud.hide();
        }
    });
    let (snapshots, collection) = collect::spawn();
    let weak = hud.as_weak();
    let poll = slint::Timer::default();
    poll.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(250),
        move || {
            if let Some(hud) = weak.upgrade() {
                show_clock(&hud);
                if let Some(s) = snapshots.try_iter().last() {
                    show_snapshot(&hud, &s);
                }
            }
        },
    );
    hud.run()?;
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
}
