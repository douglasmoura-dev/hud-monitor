//! Use case: which window is visible, where it sits, and persisting that. The windows themselves
//! and the config file are ports implemented in hud-app.

use crate::config::{Config, Orientation};
use crate::position::{Point, Size};
use crate::theme::{Palette, ThemeId};
use crate::view::minibar_size;
use std::cell::{Ref, RefCell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowId {
    Hud,
    MiniBar,
}

pub trait WindowHost {
    fn show(&self, window: WindowId) -> Result<(), String>;
    fn hide(&self, window: WindowId) -> Result<(), String>;
    /// Top-left of the HUD and of the mini bar; `None` once the windows no longer exist.
    fn positions(&self) -> Option<(Point, Point)>;
    fn set_pinned(&self, pinned: bool);
    fn resize_minibar(&self, size: Size, vertical: bool);
    /// Repaints every window (HUD, mini bar, scanner) with the palette.
    fn apply_theme(&self, palette: &Palette);
    fn quit(&self);
    /// A failure worth a line in the log; never fatal.
    fn report(&self, message: &str);
}

pub trait ConfigStore {
    fn save(&self, config: &Config) -> Result<(), String>;
}

pub struct WindowController<H, S> {
    host: H,
    store: S,
    config: RefCell<Config>,
}

impl<H: WindowHost, S: ConfigStore> WindowController<H, S> {
    pub fn new(host: H, store: S, config: Config) -> Self {
        Self {
            host,
            store,
            config: RefCell::new(config),
        }
    }

    pub fn config(&self) -> Ref<'_, Config> {
        self.config.borrow()
    }

    /// Stores both window positions along with the live visibility flags.
    pub fn save(&self) {
        let Some((hud, bar)) = self.host.positions() else {
            return;
        };
        let saved = {
            let mut config = self.config.borrow_mut();
            config.record_positions(hud, bar);
            self.store.save(&config)
        };
        if let Err(e) = saved {
            self.host.report(&format!("could not save config: {e}"));
        }
    }

    /// Shows or hides each window as the config says, then persists.
    pub fn apply_visibility(&self) {
        if self.host.positions().is_none() {
            return;
        }
        let (hud_on, bar_on) = {
            let c = self.config.borrow();
            (c.hud.visible, c.minibar.window.visible)
        };
        let result = match (hud_on, bar_on) {
            (true, _) => self
                .host
                .show(WindowId::Hud)
                .and_then(|()| self.host.hide(WindowId::MiniBar)),
            (false, true) => self
                .host
                .show(WindowId::MiniBar)
                .and_then(|()| self.host.hide(WindowId::Hud)),
            (false, false) => self
                .host
                .hide(WindowId::Hud)
                .and_then(|()| self.host.hide(WindowId::MiniBar)),
        };
        if let Err(e) = result {
            self.host.report(&format!("could not switch window: {e}"));
        }
        self.save();
    }

    pub fn switch_to(&self, minibar: bool) {
        {
            let mut c = self.config.borrow_mut();
            c.hud.visible = !minibar;
            c.minibar.window.visible = minibar;
        }
        self.apply_visibility();
    }

    /// A crit alert must be seen: shows the mini bar when both windows are hidden.
    pub fn escalate(&self) {
        let revealed = self.config.borrow_mut().reveal_minibar_if_hidden();
        if revealed {
            self.apply_visibility();
        }
    }

    pub fn toggle_hud(&self) {
        self.config.borrow_mut().toggle_hud();
        self.apply_visibility();
    }

    pub fn toggle_minibar(&self) {
        self.config.borrow_mut().toggle_minibar();
        self.apply_visibility();
    }

    pub fn toggle_pin(&self) {
        let pinned = {
            let mut c = self.config.borrow_mut();
            c.hud.click_through = !c.hud.click_through;
            c.hud.click_through
        };
        self.host.set_pinned(pinned);
        self.save();
    }

    pub fn switch_profile(&self, name: &str) {
        let (count, orientation) = {
            let mut c = self.config.borrow_mut();
            if let Err(e) = c.switch_profile(name) {
                drop(c);
                self.host.report(&format!("could not switch profile: {e}"));
                return;
            }
            (c.minibar.items.len(), c.minibar.orientation)
        };
        self.host.resize_minibar(
            minibar_size(count, orientation),
            orientation == Orientation::Vertical,
        );
        self.save();
    }

    /// Applies a theme to every window without a restart and persists the choice.
    pub fn set_theme(&self, theme: ThemeId) {
        self.config.borrow_mut().appearance.theme = theme;
        self.host.apply_theme(&theme.palette());
        self.save();
    }

    pub fn quit(&self) {
        self.save();
        let _ = self.host.hide(WindowId::Hud);
        let _ = self.host.hide(WindowId::MiniBar);
        self.host.quit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MiniBarItem, Profile};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    const SCREEN: Size = Size {
        width: 1920,
        height: 1080,
    };

    #[derive(Default)]
    struct HostLog {
        calls: RefCell<Vec<String>>,
        pinned: Cell<Option<bool>>,
        resized: Cell<Option<(Size, bool)>>,
        fail_show: Cell<bool>,
        gone: Cell<bool>,
    }

    #[derive(Clone, Default)]
    struct FakeHost(Rc<HostLog>);

    impl FakeHost {
        fn calls(&self) -> Vec<String> {
            self.0.calls.borrow().clone()
        }

        fn note(&self, text: String) {
            self.0.calls.borrow_mut().push(text);
        }
    }

    impl WindowHost for FakeHost {
        fn show(&self, window: WindowId) -> Result<(), String> {
            if self.0.fail_show.get() {
                return Err("no display".to_owned());
            }
            self.note(format!("show {window:?}"));
            Ok(())
        }

        fn hide(&self, window: WindowId) -> Result<(), String> {
            self.note(format!("hide {window:?}"));
            Ok(())
        }

        fn positions(&self) -> Option<(Point, Point)> {
            (!self.0.gone.get()).then_some((Point { x: 1, y: 2 }, Point { x: 5, y: 6 }))
        }

        fn set_pinned(&self, pinned: bool) {
            self.0.pinned.set(Some(pinned));
        }

        fn resize_minibar(&self, size: Size, vertical: bool) {
            self.0.resized.set(Some((size, vertical)));
        }

        fn apply_theme(&self, palette: &Palette) {
            self.note(format!("theme {:?}", palette.accent));
        }

        fn quit(&self) {
            self.note("quit".to_owned());
        }

        fn report(&self, message: &str) {
            self.note(format!("report {message}"));
        }
    }

    #[derive(Clone, Default)]
    struct FakeStore(Rc<RefCell<Vec<Config>>>);

    impl ConfigStore for FakeStore {
        fn save(&self, config: &Config) -> Result<(), String> {
            self.0.borrow_mut().push(config.clone());
            Ok(())
        }
    }

    struct FailingStore;

    impl ConfigStore for FailingStore {
        fn save(&self, _config: &Config) -> Result<(), String> {
            Err("disk full".to_owned())
        }
    }

    fn controller() -> (WindowController<FakeHost, FakeStore>, FakeHost, FakeStore) {
        let (host, store) = (FakeHost::default(), FakeStore::default());
        let mut config = Config::first_run(SCREEN);
        config.profiles.push(Profile {
            name: "Compact".to_owned(),
            items: vec![MiniBarItem::Cpu, MiniBarItem::Ram],
            orientation: Orientation::Vertical,
        });
        let c = WindowController::new(host.clone(), store.clone(), config);
        (c, host, store)
    }

    #[test]
    fn switching_theme_repaints_the_windows_and_saves_the_choice() {
        let (c, host, store) = controller();
        c.set_theme(ThemeId::Terminal);
        assert_eq!(host.calls().len(), 1);
        assert!(host.calls()[0].starts_with("theme "));
        assert_eq!(c.config().appearance.theme, ThemeId::Terminal);
        let saved = store.0.borrow();
        assert_eq!(saved.last().unwrap().appearance.theme, ThemeId::Terminal);
    }

    #[test]
    fn first_run_shows_the_hud_and_hides_the_mini_bar() {
        let (c, host, store) = controller();
        c.apply_visibility();
        assert_eq!(host.calls(), ["show Hud", "hide MiniBar"]);
        assert_eq!(store.0.borrow().len(), 1);
    }

    #[test]
    fn switching_to_the_mini_bar_swaps_the_windows_and_persists_the_flags() {
        let (c, host, store) = controller();
        c.switch_to(true);
        assert_eq!(host.calls(), ["show MiniBar", "hide Hud"]);
        let saved = store.0.borrow();
        let last = saved.last().expect("saved");
        assert!(last.minibar.window.visible && !last.hud.visible);
    }

    #[test]
    fn saving_records_both_window_positions() {
        let (c, _host, store) = controller();
        c.save();
        let saved = store.0.borrow();
        let config = saved.last().expect("saved");
        assert_eq!((config.hud.x, config.hud.y), (1, 2));
        assert_eq!((config.minibar.window.x, config.minibar.window.y), (5, 6));
    }

    #[test]
    fn hiding_both_windows_hides_both() {
        let (c, host, _) = controller();
        c.toggle_hud();
        assert_eq!(host.calls(), ["hide Hud", "hide MiniBar"]);
    }

    #[test]
    fn a_critical_alert_reveals_the_mini_bar_only_when_everything_is_hidden() {
        let (c, host, _) = controller();
        c.escalate();
        assert!(host.calls().is_empty());
        c.toggle_hud();
        c.escalate();
        assert_eq!(host.calls().last().map(String::as_str), Some("hide Hud"));
        assert!(c.config().minibar.window.visible);
    }

    #[test]
    fn pin_toggles_click_through_and_tells_the_host() {
        let (c, host, store) = controller();
        c.toggle_pin();
        assert_eq!(host.0.pinned.get(), Some(true));
        assert!(c.config().hud.click_through);
        assert_eq!(store.0.borrow().len(), 1);
        c.toggle_pin();
        assert_eq!(host.0.pinned.get(), Some(false));
    }

    #[test]
    fn unknown_profile_is_reported_and_changes_nothing() {
        let (c, host, store) = controller();
        c.switch_profile("nope");
        assert!(host.calls()[0].starts_with("report could not switch profile"));
        assert!(host.0.resized.get().is_none());
        assert!(store.0.borrow().is_empty());
    }

    #[test]
    fn profile_switch_resizes_the_mini_bar_to_the_new_items() {
        let (c, host, store) = controller();
        c.switch_profile("Compact");
        let (size, vertical) = host.0.resized.get().expect("resized");
        let config = c.config();
        assert_eq!(
            size,
            minibar_size(config.minibar.items.len(), config.minibar.orientation)
        );
        assert_eq!(
            vertical,
            config.minibar.orientation == Orientation::Vertical
        );
        assert_eq!(store.0.borrow().len(), 1);
    }

    #[test]
    fn quit_saves_hides_both_windows_and_stops_the_loop() {
        let (c, host, store) = controller();
        c.quit();
        assert_eq!(host.calls(), ["hide Hud", "hide MiniBar", "quit"]);
        assert_eq!(store.0.borrow().len(), 1);
    }

    #[test]
    fn nothing_happens_after_the_windows_are_gone() {
        let (c, host, store) = controller();
        host.0.gone.set(true);
        c.apply_visibility();
        c.save();
        assert!(host.calls().is_empty() && store.0.borrow().is_empty());
    }

    #[test]
    fn a_failing_window_switch_is_reported_and_still_persisted() {
        let (c, host, store) = controller();
        host.0.fail_show.set(true);
        c.apply_visibility();
        assert!(host.calls()[0].starts_with("report could not switch window: no display"));
        assert_eq!(store.0.borrow().len(), 1);
    }

    #[test]
    fn a_failing_save_is_reported_not_fatal() {
        let host = FakeHost::default();
        let c = WindowController::new(host.clone(), FailingStore, Config::first_run(SCREEN));
        c.save();
        assert_eq!(host.calls(), ["report could not save config: disk full"]);
    }
}
