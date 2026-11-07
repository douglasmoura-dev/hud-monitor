#![windows_subsystem = "windows"]

mod store;
mod winffi;

use hud_core::position::{clamp_to_screen, first_run_position, Point, Size};
use slint::{ComponentHandle, PhysicalPosition};
use std::rc::Rc;

const WINDOW_WIDTH: f32 = 320.0;
const WINDOW_HEIGHT: f32 = 200.0;

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

fn start_position(path: &std::path::Path, window: Size) -> Result<Point, AppError> {
    let screen = winffi::screen_size();
    Ok(match store::load(path)? {
        Some(saved) => clamp_to_screen(saved, window, screen),
        None => first_run_position(screen, window),
    })
}

fn save_position(hud: &Hud, path: &std::path::Path) {
    let p = hud.window().position();
    if let Err(e) = store::save(path, Point { x: p.x, y: p.y }) {
        eprintln!("hud-monitor: could not save position: {e}");
    }
}

fn run() -> Result<(), AppError> {
    let path = Rc::new(store::default_path()?);
    let hud = Hud::new()?;
    let scale = hud.window().scale_factor();
    let size = Size {
        width: (WINDOW_WIDTH * scale).round() as i32,
        height: (WINDOW_HEIGHT * scale).round() as i32,
    };
    let pos = start_position(&path, size)?;
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
    let (weak, saved_to) = (hud.as_weak(), path.clone());
    hud.on_released(move || {
        if let Some(hud) = weak.upgrade() {
            save_position(&hud, &saved_to);
        }
    });
    let weak = hud.as_weak();
    hud.on_quit(move || {
        if let Some(hud) = weak.upgrade() {
            save_position(&hud, &path);
            let _ = hud.hide();
        }
    });
    hud.run()?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("hud-monitor: {e}");
        std::process::exit(1);
    }
}
