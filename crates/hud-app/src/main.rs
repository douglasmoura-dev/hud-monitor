#![windows_subsystem = "windows"]
//! Composition root: builds the object graph and starts the event loop. No decisions live here.

mod ai;
mod clock;
mod collect;
mod presentation;
mod runtime;
mod scanner;
mod shell;
mod sources;
mod store;
mod winffi;

use hud_core::dashboard::Dashboard;
use hud_core::windowing::WindowController;
use presentation::{Hud, Looks, MiniBar, SlintWindows};
use runtime::{Feeds, Services};
use std::rc::Rc;

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error(transparent)]
    Store(#[from] store::StoreError),
    #[error("could not create window: {0}")]
    Window(#[from] slint::PlatformError),
}

/// `--scanner-cycles N`: opens and closes the scanner N times (memory budget measurement).
fn scanner_cycles() -> Option<u32> {
    let mut args = std::env::args().skip_while(|a| a != "--scanner-cycles");
    args.nth(1)?.parse().ok()
}

fn run() -> Result<(), AppError> {
    let path = store::default_path()?;
    let (hud, bar) = (Hud::new()?, MiniBar::new()?);
    let screen = winffi::screen_size();
    let config = store::load_or_first_run(&path, screen)?;

    let dashboard = Dashboard::new();
    let waiting = dashboard.waiting_view(&config.minibar.items, &config.thresholds);
    presentation::show_dashboard(&hud, &bar, None, &waiting);
    hud.set_clock(presentation::clock_text(winffi::local_now().1).into());
    presentation::apply_config(&hud, &bar, &config, screen);
    shell::sync_autostart(config.autostart);

    let shell = shell::start(&config);
    if !shell.notice.is_empty() {
        eprintln!("hud-monitor: {}", shell.notice);
        hud.set_notice(shell.notice.as_str().into());
    }
    let toggle_hud_combo = config.hotkeys.toggle_hud.clone();

    let windows = SlintWindows::new(&hud, &bar);
    let store = store::FileConfigStore::new(path);
    let controller = Rc::new(WindowController::new(windows.clone(), store, config));
    presentation::connect(&hud, &bar, &controller);
    let services = Rc::new(Services {
        windows,
        controller: controller.clone(),
        looks: Rc::new(Looks::default()),
        tray: Rc::new(shell.tray),
        scanner: scanner::Slot::default(),
    });
    if std::env::args().any(|a| a == "--scanner") {
        scanner::open(&services.scanner);
    }
    let _stress = scanner_cycles().map(|n| scanner::stress(&services.scanner, n));

    let (snapshots, collection) = collect::spawn();
    let (providers, poller) = ai::spawn();
    let feeds = Feeds {
        snapshots,
        providers,
    };
    let _slow = runtime::start_slow(services.clone(), feeds, dashboard);
    let _fast = runtime::start_fast(services, shell.hotkeys, toggle_hud_combo);
    controller.apply_visibility();
    slint::run_event_loop_until_quit()?;
    collection.shutdown();
    poller.shutdown();
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("hud-monitor: {e}");
        std::process::exit(1);
    }
}
