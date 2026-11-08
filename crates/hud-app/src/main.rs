#![windows_subsystem = "windows"]

mod collect;
mod sources;
mod store;
mod winffi;

use hud_core::config::{Config, HUD_SIZE};
use hud_core::format::used_total_gb;
use hud_core::metrics::{Metric, Snapshot};
use hud_core::position::{clamp_to_screen, Point, Size};
use slint::{ComponentHandle, PhysicalPosition};
use std::{cell::RefCell, rc::Rc};

slint::slint! {
    export component Hud inherits Window {
        no-frame: true;
        always-on-top: true;
        background: transparent;
        width: 320px;
        height: 200px;
        callback moved(length, length);
        callback released();
        callback quit();
        in property <string> cpu-text: "CPU —";
        in property <string> ram-text: "RAM —";
        in property <string> temp-text: "TEMP —";
        in property <string> gpu-text: "GPU —";

        Rectangle {
            background: #070d14e6;
            border-color: #1fe0c8;
            border-width: 1px;

            Rectangle {
                y: 0px;
                height: 28px;
                background: #0e1c2a;
                TouchArea {
                    moved => { root.moved(self.mouse-x - self.pressed-x, self.mouse-y - self.pressed-y); }
                    pointer-event(e) => { if e.kind == PointerEventKind.up { root.released(); } }
                }
                Text {
                    x: 10px;
                    text: "SYS // MONITOR";
                    color: #1fe0c8;
                    font-size: 13px;
                    vertical-alignment: center;
                }
                TouchArea {
                    x: parent.width - 28px;
                    width: 28px;
                    clicked => { root.quit(); }
                    Text { text: "x"; color: #1fe0c8; horizontal-alignment: center; vertical-alignment: center; }
                }
            }

            Text {
                x: 10px;
                y: 44px;
                text: root.cpu-text;
                color: #e6f7f5;
                font-size: 16px;
            }
            Text {
                x: 10px;
                y: 70px;
                text: root.ram-text;
                color: #e6f7f5;
                font-size: 16px;
            }
            Text {
                x: 10px;
                y: 96px;
                text: root.temp-text;
                color: #e6f7f5;
                font-size: 16px;
            }
            Text {
                x: 10px;
                y: 122px;
                width: parent.width - 20px;
                wrap: word-wrap;
                text: root.gpu-text;
                color: #e6f7f5;
                font-size: 13px;
            }
        }
    }
}

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

fn show_snapshot(hud: &Hud, s: &Snapshot) {
    if let Some(reason) = &s.unavailable {
        hud.set_cpu_text(format!("CPU — ({reason})").into());
        hud.set_ram_text(format!("RAM — ({reason})").into());
        hud.set_temp_text(format!("TEMP — ({reason})").into());
        hud.set_gpu_text(format!("GPU — ({reason})").into());
        return;
    }
    hud.set_cpu_text(format!("CPU {:.0}%", s.cpu.total).into());
    hud.set_ram_text(
        format!(
            "RAM {}% {}",
            s.mem.percent,
            used_total_gb(s.mem.used, s.mem.total)
        )
        .into(),
    );
    hud.set_temp_text(format!("TEMP {}", shown(&s.thermal.cpu_c, |c| format!("{c}°C"))).into());
    hud.set_gpu_text(gpu_text(s).into());
}

fn shown<T>(m: &Metric<T>, fmt: impl Fn(&T) -> String) -> String {
    match m {
        Metric::Available(v) => fmt(v),
        Metric::Unavailable(reason) => format!("— ({reason})"),
    }
}

fn gpu_text(s: &Snapshot) -> String {
    let nv = &s.gpu.nvidia;
    format!(
        "NVIDIA {} {}\nIntel {}",
        shown(&nv.usage_percent, |u| format!("{u}%")),
        shown(&nv.temp_c, |t| format!("{t}°C")),
        shown(&s.gpu.intel_usage_percent, |u| format!("{u:.0}%")),
    )
}

fn run() -> Result<(), AppError> {
    let path = Rc::new(store::default_path()?);
    let hud = Hud::new()?;
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
            if let (Some(hud), Some(s)) = (weak.upgrade(), snapshots.try_iter().last()) {
                show_snapshot(&hud, &s);
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
