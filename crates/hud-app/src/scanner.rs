//! Cleanup scanner window: a background scan thread feeding a short-lived Slint window.
//! Closing the window cancels the scan and drops every result.

use crate::sources::{env_roots, StdFs, WinSystem};
use crate::{ai, CleanRow, Scanner};
use hud_core::cleanup::{scan_all, selected_text, Category};
use slint::ComponentHandle;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc,
    },
    thread,
    time::Duration,
};

const POLL: Duration = Duration::from_millis(200);
const CLOCK_UNAVAILABLE: &str = "relógio do sistema indisponível";

enum ScanEvent {
    Category(Category),
    Failed(String),
}

/// A running scan. Dropping it cancels the worker, which frees its memory when it exits.
struct ScanRun {
    events: Receiver<ScanEvent>,
    cancel: Arc<AtomicBool>,
}

impl Drop for ScanRun {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl ScanRun {
    fn spawn() -> Self {
        let (tx, events) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        thread::spawn(move || run_scan(&tx, &flag));
        Self { events, cancel }
    }

    /// New events plus whether the worker has finished.
    fn poll(&self) -> (Vec<ScanEvent>, bool) {
        let mut out = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(e) => out.push(e),
                Err(TryRecvError::Empty) => return (out, false),
                Err(TryRecvError::Disconnected) => return (out, true),
            }
        }
    }
}

fn run_scan(tx: &mpsc::Sender<ScanEvent>, cancel: &AtomicBool) {
    let Some(now) = ai::unix_now().and_then(|n| u64::try_from(n).ok()) else {
        let _ = tx.send(ScanEvent::Failed(CLOCK_UNAVAILABLE.to_owned()));
        return;
    };
    scan_all(&StdFs, &WinSystem, &env_roots(), now, cancel, &mut |c| {
        let _ = tx.send(ScanEvent::Category(c));
    });
}

#[derive(Default)]
struct State {
    categories: Vec<Category>,
    run: Option<ScanRun>,
    failure: Option<String>,
}

impl State {
    fn start(&mut self) {
        self.categories = Vec::new();
        self.failure = None;
        self.run = Some(ScanRun::spawn());
    }

    /// Cancels the scan and frees the results.
    fn release(&mut self) {
        self.run = None;
        self.categories = Vec::new();
    }

    /// Absorbs worker events; true while the scan is still running.
    fn absorb(&mut self) -> bool {
        let Some(run) = &self.run else {
            return false;
        };
        let (events, finished) = run.poll();
        for event in events {
            match event {
                ScanEvent::Category(c) => self.categories.push(c),
                ScanEvent::Failed(reason) => self.failure = Some(reason),
            }
        }
        if finished {
            self.run = None;
        }
        !finished
    }
}

pub struct Session {
    window: Scanner,
    state: Rc<RefCell<State>>,
    _timer: slint::Timer,
}

pub type Slot = Rc<RefCell<Option<Session>>>;

fn rows(categories: &[Category]) -> slint::ModelRc<CleanRow> {
    let data: Vec<CleanRow> = categories
        .iter()
        .map(|c| CleanRow {
            name: c.id.name().into(),
            size: c.size_text().into(),
            detail: c.detail().into(),
            review: c.review,
            checked: c.checked,
            enabled: c.usage.available().is_some(),
        })
        .collect();
    Rc::new(slint::VecModel::from(data)).into()
}

fn status_text(state: &State, scanning: bool) -> String {
    match (&state.failure, scanning) {
        (Some(reason), _) => reason.clone(),
        (None, true) => "Escaneando...".to_owned(),
        (None, false) => format!("{} categorias medidas", state.categories.len()),
    }
}

fn refresh(window: &Scanner, state: &State, scanning: bool) {
    window.set_rows(rows(&state.categories));
    window.set_total(selected_text(&state.categories).into());
    window.set_status(status_text(state, scanning).into());
    window.set_scanning(scanning);
}

fn start_timer(window: &Scanner, state: &Rc<RefCell<State>>) -> slint::Timer {
    let timer = slint::Timer::default();
    let (weak, state) = (window.as_weak(), state.clone());
    timer.start(slint::TimerMode::Repeated, POLL, move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut s = state.borrow_mut();
        if s.run.is_none() {
            return;
        }
        let scanning = s.absorb();
        refresh(&window, &s, scanning);
    });
    timer
}

fn new_session(slot: &Slot) -> Result<Session, slint::PlatformError> {
    let window = Scanner::new()?;
    let state = Rc::new(RefCell::new(State::default()));
    state.borrow_mut().start();
    refresh(&window, &state.borrow(), true);

    let (weak, st) = (window.as_weak(), state.clone());
    window.on_toggle(move |i| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut s = st.borrow_mut();
        let scanning = s.run.is_some();
        if let Some(c) = usize::try_from(i)
            .ok()
            .and_then(|i| s.categories.get_mut(i))
        {
            c.toggle();
        }
        refresh(&window, &s, scanning);
    });
    let (weak, st) = (window.as_weak(), state.clone());
    window.on_rescan(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut s = st.borrow_mut();
        s.release();
        s.start();
        refresh(&window, &s, true);
    });
    let closer = slot.clone();
    window.on_dismiss(move || close(&closer));
    let closer = slot.clone();
    window.window().on_close_requested(move || {
        close(&closer);
        slint::CloseRequestResponse::HideWindow
    });
    let timer = start_timer(&window, &state);
    Ok(Session {
        window,
        state,
        _timer: timer,
    })
}

/// Opens the scanner window (or brings the open one forward) and starts a fresh scan.
pub fn open(slot: &Slot) {
    if let Some(session) = slot.borrow().as_ref() {
        if let Err(e) = session.window.show() {
            eprintln!("hud-monitor: could not show scanner: {e}");
        }
        return;
    }
    let shown = new_session(slot).and_then(|s| s.window.show().map(|()| s));
    match shown {
        Ok(session) => *slot.borrow_mut() = Some(session),
        Err(e) => eprintln!("hud-monitor: could not open scanner: {e}"),
    }
}

/// Cancels the scan, hides the window and drops the session on the next event-loop turn.
fn close(slot: &Slot) {
    if let Some(session) = slot.borrow().as_ref() {
        session.state.borrow_mut().release();
        if let Err(e) = session.window.hide() {
            eprintln!("hud-monitor: could not hide scanner: {e}");
        }
    }
    let slot = slot.clone();
    slint::Timer::single_shot(Duration::ZERO, move || {
        slot.borrow_mut().take();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use hud_core::cleanup::{CategoryId, Usage};
    use hud_core::metrics::Metric;

    fn category(id: CategoryId, bytes: u64) -> Category {
        Category::new(
            id,
            Metric::Available(Usage {
                bytes,
                items: 1,
                incomplete: false,
            }),
        )
    }

    #[test]
    fn dropping_a_run_cancels_the_worker() {
        let (_tx, events) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let run = ScanRun {
            events,
            cancel: cancel.clone(),
        };
        drop(run);
        assert!(cancel.load(Ordering::Relaxed));
    }

    #[test]
    fn release_drops_results_and_the_run() {
        let (_tx, events) = mpsc::channel();
        let mut state = State {
            categories: vec![category(CategoryId::Temp, 5)],
            run: Some(ScanRun {
                events,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            failure: None,
        };
        state.release();
        assert!(state.categories.is_empty());
        assert_eq!(state.categories.capacity(), 0);
        assert!(state.run.is_none());
    }

    #[test]
    fn absorb_collects_events_and_reports_when_the_worker_ends() {
        let (tx, events) = mpsc::channel();
        let mut state = State {
            run: Some(ScanRun {
                events,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            ..State::default()
        };
        tx.send(ScanEvent::Category(category(CategoryId::Temp, 5)))
            .expect("send");
        assert!(state.absorb());
        assert_eq!(state.categories.len(), 1);
        drop(tx);
        assert!(!state.absorb());
        assert!(state.run.is_none());
    }

    #[test]
    fn failure_text_replaces_the_status() {
        let state = State {
            failure: Some(CLOCK_UNAVAILABLE.to_owned()),
            ..State::default()
        };
        assert_eq!(status_text(&state, false), CLOCK_UNAVAILABLE);
        assert_eq!(status_text(&State::default(), true), "Escaneando...");
        assert_eq!(
            status_text(&State::default(), false),
            "0 categorias medidas"
        );
    }
}
