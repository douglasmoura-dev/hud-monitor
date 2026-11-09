//! The two UI-thread timers: a 1 Hz refresh of the dashboard and a 200 ms poll of tray and
//! hotkey events. Both only wire inputs to use cases and the presentation.

use crate::clock::unix_now;
use crate::presentation::{clock_text, show_dashboard, Controller, Looks, SlintWindows};
use crate::scanner;
use crate::shell::{Hotkeys, Tray, TrayAction};
use crate::winffi;
use hud_core::dashboard::{Dashboard, DashboardView, ProviderStates};
use hud_core::metrics::Snapshot;
use std::{cell::Cell, rc::Rc, sync::mpsc::Receiver, time::Duration};

const SLOW_TICK: Duration = Duration::from_secs(1);
const FAST_TICK: Duration = Duration::from_millis(200);

/// Everything both timers share.
pub struct Services {
    pub windows: SlintWindows,
    pub controller: Rc<Controller>,
    pub looks: Rc<Looks>,
    pub tray: Rc<Option<Tray>>,
    pub scanner: scanner::Slot,
}

pub struct Feeds {
    pub snapshots: Receiver<Snapshot>,
    pub providers: Receiver<ProviderStates>,
}

struct SlowTick {
    services: Rc<Services>,
    feeds: Feeds,
    dashboard: Dashboard,
    shown: Option<DashboardView>,
}

impl SlowTick {
    fn run(&mut self) {
        let Some((hud, bar)) = self.services.windows.upgrade() else {
            return;
        };
        hud.set_clock(clock_text(winffi::local_now().1).into());
        if let Some(snapshot) = self.feeds.snapshots.try_iter().last() {
            let (items, thresholds) = {
                let config = self.services.controller.config();
                (config.minibar.items.clone(), config.thresholds.clone())
            };
            let fresh = self.feeds.providers.try_iter().last();
            let now = unix_now().unwrap_or(0);
            let refresh = self
                .dashboard
                .refresh(&snapshot, fresh, &items, &thresholds, now);
            show_dashboard(&hud, &bar, self.shown.as_ref(), &refresh.view);
            if let Some(tray) = self.services.tray.as_ref() {
                tray.set_tooltip(&refresh.view.tray_tooltip);
            }
            if refresh.escalate {
                self.services.controller.escalate();
            }
            self.shown = Some(refresh.view);
        }
        self.services
            .looks
            .refresh(&self.services.windows, &self.services.controller);
    }
}

/// 1 Hz: clock, metrics, tray tooltip, and a look refresh that catches recreated windows.
pub fn start_slow(services: Rc<Services>, feeds: Feeds, dashboard: Dashboard) -> slint::Timer {
    let mut tick = SlowTick {
        services,
        feeds,
        dashboard,
        shown: None,
    };
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, SLOW_TICK, move || tick.run());
    timer
}

fn dispatch(services: &Services, action: TrayAction) {
    let controller = &services.controller;
    match action {
        TrayAction::ShowHud => controller.switch_to(false),
        TrayAction::ShowMinibar => controller.switch_to(true),
        TrayAction::Profile(name) => controller.switch_profile(&name),
        TrayAction::Scanner => scanner::open(&services.scanner),
        TrayAction::Quit => controller.quit(),
    }
}

/// 200 ms: drains tray and hotkey events and reacts when Ctrl changes.
pub fn start_fast(
    services: Rc<Services>,
    hotkeys: Option<Hotkeys>,
    toggle_hud_combo: String,
) -> slint::Timer {
    let ctrl_was_down = Cell::new(false);
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, FAST_TICK, move || {
        if let Some(tray) = services.tray.as_ref() {
            for action in tray.poll() {
                dispatch(&services, action);
            }
        }
        if let Some(hotkeys) = &hotkeys {
            for combo in hotkeys.pressed() {
                if combo == toggle_hud_combo {
                    services.controller.toggle_hud();
                } else {
                    services.controller.toggle_minibar();
                }
            }
        }
        let ctrl = winffi::ctrl_down();
        if ctrl_was_down.replace(ctrl) != ctrl {
            services
                .looks
                .refresh(&services.windows, &services.controller);
        }
    });
    timer
}
