//! Cleanup scanner window: a background scan thread feeding a short-lived Slint window.
//! After an explicit confirmation a second background thread moves the marked categories to
//! the Recycle Bin. Closing the window cancels both and drops every result.

use crate::clock;
use crate::presentation::{CleanRow, Scanner};
use crate::sources::{env_roots, ShellRecycler, StdFs, WinSystem};
use hud_core::cleanup::{scan_all, selected_text, Category, CategoryId};
use hud_core::removal::{confirmation_lines, execute, plan_removal, selected_ids, Report};
use hud_core::ui_text::{
    categories_measured, confirm_move_to_bin, moving_to_bin, CLEANUP_WORKER_LOST, SCANNING,
    SYSTEM_CLOCK_UNAVAILABLE,
};
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
        drain(&self.events)
    }
}

/// Everything waiting on the channel, plus whether the sender is gone.
fn drain<T>(events: &Receiver<T>) -> (Vec<T>, bool) {
    let mut out = Vec::new();
    loop {
        match events.try_recv() {
            Ok(e) => out.push(e),
            Err(TryRecvError::Empty) => return (out, false),
            Err(TryRecvError::Disconnected) => return (out, true),
        }
    }
}

fn run_scan(tx: &mpsc::Sender<ScanEvent>, cancel: &AtomicBool) {
    let Some(now) = unix_now() else {
        let _ = tx.send(ScanEvent::Failed(SYSTEM_CLOCK_UNAVAILABLE.to_owned()));
        return;
    };
    scan_all(&StdFs, &WinSystem, &env_roots(), now, cancel, &mut |c| {
        let _ = tx.send(ScanEvent::Category(c));
    });
}

fn unix_now() -> Option<u64> {
    clock::unix_now().and_then(|n| u64::try_from(n).ok())
}

enum RemovalEvent {
    Progress { done: usize, total: usize },
    Finished(Report),
    Failed(String),
}

/// A running removal. Dropping it asks the worker to stop after its current batch.
struct RemovalRun {
    events: Receiver<RemovalEvent>,
    cancel: Arc<AtomicBool>,
}

impl Drop for RemovalRun {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl RemovalRun {
    fn spawn(ids: Vec<CategoryId>) -> Self {
        let (tx, events) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        thread::spawn(move || run_removal(&ids, &tx, &flag));
        Self { events, cancel }
    }
}

/// Plans again with the scanner rules (so nothing touched since the scan is moved), then sends
/// the items to the Recycle Bin.
fn run_removal(ids: &[CategoryId], tx: &mpsc::Sender<RemovalEvent>, cancel: &AtomicBool) {
    let Some(now) = unix_now() else {
        let _ = tx.send(RemovalEvent::Failed(SYSTEM_CLOCK_UNAVAILABLE.to_owned()));
        return;
    };
    let plan = plan_removal(ids, &StdFs, &WinSystem, &env_roots(), now, cancel);
    let total = plan.steps();
    let _ = tx.send(RemovalEvent::Progress { done: 0, total });
    let report = execute(&plan, &ShellRecycler, cancel, &mut |done| {
        let _ = tx.send(RemovalEvent::Progress { done, total });
    });
    let _ = tx.send(RemovalEvent::Finished(report));
}

/// The phase a removal event leads to, and whether it is the last event of the run.
fn phase_after(event: RemovalEvent) -> (Phase, bool) {
    match event {
        RemovalEvent::Progress { done, total } => (Phase::Running { done, total }, false),
        RemovalEvent::Finished(report) => {
            let done = Phase::Done {
                headline: report.message(),
                lines: report.skipped_lines(),
            };
            (done, true)
        }
        RemovalEvent::Failed(reason) => {
            let done = Phase::Done {
                headline: reason,
                lines: Vec::new(),
            };
            (done, true)
        }
    }
}

/// What the window shows besides the category list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    List,
    Confirm {
        ids: Vec<CategoryId>,
        lines: Vec<String>,
        total: String,
    },
    Running {
        done: usize,
        total: usize,
    },
    Done {
        headline: String,
        lines: Vec<String>,
    },
}

impl Phase {
    fn name(&self) -> &'static str {
        match self {
            Phase::List => "list",
            Phase::Confirm { .. } => "confirm",
            Phase::Running { .. } => "running",
            Phase::Done { .. } => "done",
        }
    }

    fn headline(&self) -> String {
        match self {
            Phase::List => String::new(),
            Phase::Confirm { total, .. } => confirm_move_to_bin(total),
            Phase::Running { done, total } => moving_to_bin(*done, *total),
            Phase::Done { headline, .. } => headline.clone(),
        }
    }

    fn lines(&self) -> Vec<String> {
        match self {
            Phase::Confirm { lines, .. } | Phase::Done { lines, .. } => lines.clone(),
            Phase::List | Phase::Running { .. } => Vec::new(),
        }
    }
}

#[derive(Default)]
struct State {
    categories: Vec<Category>,
    run: Option<ScanRun>,
    removal: Option<RemovalRun>,
    phase: Phase,
    failure: Option<String>,
}

impl State {
    fn start(&mut self) {
        self.categories = Vec::new();
        self.failure = None;
        self.phase = Phase::List;
        self.run = Some(ScanRun::spawn());
    }

    /// Cancels the scan and any removal, and frees the results.
    fn release(&mut self) {
        self.run = None;
        self.removal = None;
        self.phase = Phase::List;
        self.categories = Vec::new();
    }

    fn busy(&self) -> bool {
        self.run.is_some() || self.removal.is_some()
    }

    /// Absorbs worker events; true while the scan is still running.
    fn absorb(&mut self) -> bool {
        self.absorb_removal();
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

    fn absorb_removal(&mut self) {
        let Some(run) = &self.removal else {
            return;
        };
        let (events, ended) = drain(&run.events);
        let mut finished = false;
        for event in events {
            let (phase, last) = phase_after(event);
            self.phase = phase;
            finished |= last;
        }
        if ended && !finished {
            self.phase = Phase::Done {
                headline: CLEANUP_WORKER_LOST.to_owned(),
                lines: Vec::new(),
            };
        }
        if finished || ended {
            self.removal = None;
        }
    }

    /// Opens the confirmation step for the marked categories; nothing moves yet.
    fn ask_remove(&mut self) {
        if !self.can_remove() {
            return;
        }
        self.phase = Phase::Confirm {
            ids: selected_ids(&self.categories),
            lines: confirmation_lines(&self.categories),
            total: selected_text(&self.categories),
        };
    }

    /// Starts the removal; only possible from the confirmation step.
    fn confirm_remove(&mut self) {
        let Phase::Confirm { ids, .. } = &self.phase else {
            return;
        };
        self.removal = Some(RemovalRun::spawn(ids.clone()));
        self.phase = Phase::Running { done: 0, total: 0 };
    }

    /// Backs out of the confirmation, or asks a running removal to stop after its batch.
    fn cancel_remove(&mut self) {
        match &self.phase {
            Phase::Confirm { .. } => self.phase = Phase::List,
            Phase::Running { .. } => {
                if let Some(run) = &self.removal {
                    run.cancel.store(true, Ordering::Relaxed);
                }
            }
            Phase::List | Phase::Done { .. } => {}
        }
    }

    /// Leaves the result screen and measures again.
    fn finish(&mut self) {
        if matches!(self.phase, Phase::Done { .. }) {
            self.start();
        }
    }

    fn can_remove(&self) -> bool {
        !self.busy() && self.phase == Phase::List && !selected_ids(&self.categories).is_empty()
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

fn text_lines(lines: Vec<String>) -> slint::ModelRc<slint::SharedString> {
    let data: Vec<slint::SharedString> = lines.into_iter().map(Into::into).collect();
    Rc::new(slint::VecModel::from(data)).into()
}

fn status_text(state: &State, scanning: bool) -> String {
    match (&state.failure, scanning) {
        (Some(reason), _) => reason.clone(),
        (None, true) => SCANNING.to_owned(),
        (None, false) => categories_measured(state.categories.len()),
    }
}

fn refresh(window: &Scanner, state: &State) {
    let scanning = state.run.is_some();
    window.set_rows(rows(&state.categories));
    window.set_total(selected_text(&state.categories).into());
    window.set_status(status_text(state, scanning).into());
    window.set_scanning(scanning);
    window.set_can_remove(state.can_remove());
    window.set_phase(state.phase.name().into());
    window.set_headline(state.phase.headline().into());
    window.set_lines(text_lines(state.phase.lines()));
}

fn start_timer(window: &Scanner, state: &Rc<RefCell<State>>) -> slint::Timer {
    let timer = slint::Timer::default();
    let (weak, state) = (window.as_weak(), state.clone());
    timer.start(slint::TimerMode::Repeated, POLL, move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut s = state.borrow_mut();
        if !s.busy() {
            return;
        }
        s.absorb();
        refresh(&window, &s);
    });
    timer
}

/// A callback body that changes the session state and redraws the window.
fn updater(window: &Scanner, state: &Rc<RefCell<State>>, change: fn(&mut State)) -> impl Fn() {
    let (weak, state) = (window.as_weak(), state.clone());
    move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut s = state.borrow_mut();
        change(&mut s);
        refresh(&window, &s);
    }
}

/// Callback body that flips the checkbox of row `i` and redraws.
fn row_toggler(window: &Scanner, state: &Rc<RefCell<State>>) -> impl Fn(i32) {
    let (weak, state) = (window.as_weak(), state.clone());
    move |i| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut s = state.borrow_mut();
        if let Some(c) = usize::try_from(i)
            .ok()
            .and_then(|i| s.categories.get_mut(i))
        {
            c.toggle();
        }
        refresh(&window, &s);
    }
}

fn rescan(s: &mut State) {
    s.release();
    s.start();
}

fn new_session(slot: &Slot) -> Result<Session, slint::PlatformError> {
    let window = Scanner::new()?;
    let state = Rc::new(RefCell::new(State::default()));
    state.borrow_mut().start();
    refresh(&window, &state.borrow());

    window.on_toggle(row_toggler(&window, &state));
    window.on_rescan(updater(&window, &state, rescan));
    window.on_ask_remove(updater(&window, &state, State::ask_remove));
    window.on_confirm_remove(updater(&window, &state, State::confirm_remove));
    window.on_cancel_remove(updater(&window, &state, State::cancel_remove));
    window.on_finish(updater(&window, &state, State::finish));
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
    use hud_core::removal::Skipped;

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
            ..State::default()
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
            failure: Some(SYSTEM_CLOCK_UNAVAILABLE.to_owned()),
            ..State::default()
        };
        assert_eq!(status_text(&state, false), SYSTEM_CLOCK_UNAVAILABLE);
        assert_eq!(status_text(&State::default(), true), "Escaneando...");
        assert_eq!(
            status_text(&State::default(), false),
            "0 categorias medidas"
        );
    }

    fn marked(id: CategoryId, bytes: u64) -> Category {
        category(id, bytes)
    }

    #[test]
    fn nothing_to_remove_without_marked_categories() {
        let mut state = State::default();
        assert!(!state.can_remove());
        state.ask_remove();
        assert_eq!(state.phase, Phase::List);
        let mut unmarked = marked(CategoryId::Temp, 5);
        unmarked.toggle();
        state.categories = vec![unmarked];
        state.ask_remove();
        assert_eq!(state.phase, Phase::List);
    }

    #[test]
    fn asking_shows_the_categories_and_total_without_removing_anything() {
        let mut state = State {
            categories: vec![
                marked(CategoryId::Temp, 100),
                marked(CategoryId::PackageCaches, 200),
            ],
            ..State::default()
        };
        assert!(state.can_remove());
        state.ask_remove();
        let Phase::Confirm { ids, lines, total } = &state.phase else {
            panic!("expected confirmation, got {:?}", state.phase);
        };
        assert_eq!(ids, &[CategoryId::Temp, CategoryId::PackageCaches]);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(CategoryId::Temp.name()));
        assert_eq!(total, "300 B");
        assert!(state.removal.is_none(), "no worker before confirming");
        assert!(state.phase.headline().contains("300 B"));
        assert_eq!(state.phase.name(), "confirm");
    }

    #[test]
    fn cancelling_the_confirmation_goes_back_to_the_list() {
        let mut state = State {
            categories: vec![marked(CategoryId::Temp, 100)],
            ..State::default()
        };
        state.ask_remove();
        state.cancel_remove();
        assert_eq!(state.phase, Phase::List);
        assert!(state.removal.is_none());
    }

    #[test]
    fn confirming_outside_the_confirmation_step_does_nothing() {
        let mut state = State {
            categories: vec![marked(CategoryId::Temp, 100)],
            ..State::default()
        };
        state.confirm_remove();
        assert!(state.removal.is_none());
        assert_eq!(state.phase, Phase::List);
    }

    #[test]
    fn removal_events_drive_progress_and_the_result_screen() {
        let (tx, events) = mpsc::channel();
        let mut state = State {
            removal: Some(RemovalRun {
                events,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            phase: Phase::Running { done: 0, total: 0 },
            ..State::default()
        };
        tx.send(RemovalEvent::Progress { done: 2, total: 5 })
            .expect("send");
        state.absorb();
        assert_eq!(state.phase, Phase::Running { done: 2, total: 5 });
        assert_eq!(state.phase.headline(), moving_to_bin(2, 5));
        assert!(state.removal.is_some());
        let report = Report {
            moved_items: 2,
            freed_bytes: 300,
            skipped: vec![Skipped {
                path: r"C:\t\locked".into(),
                reason: "in use".into(),
            }],
            ..Report::default()
        };
        tx.send(RemovalEvent::Finished(report)).expect("send");
        state.absorb();
        let Phase::Done { headline, lines } = &state.phase else {
            panic!("expected result, got {:?}", state.phase);
        };
        assert!(headline.starts_with(&hud_core::ui_text::freed("300 B")));
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("locked"));
        assert!(state.removal.is_none());
    }

    #[test]
    fn a_failed_removal_shows_its_reason() {
        let (tx, events) = mpsc::channel();
        let mut state = State {
            removal: Some(RemovalRun {
                events,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            phase: Phase::Running { done: 0, total: 0 },
            ..State::default()
        };
        tx.send(RemovalEvent::Failed(SYSTEM_CLOCK_UNAVAILABLE.to_owned()))
            .expect("send");
        state.absorb();
        assert_eq!(state.phase.headline(), SYSTEM_CLOCK_UNAVAILABLE);
        assert!(state.phase.lines().is_empty());
    }

    #[test]
    fn stopping_a_running_removal_raises_its_cancel_flag() {
        let (_tx, events) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut state = State {
            removal: Some(RemovalRun {
                events,
                cancel: cancel.clone(),
            }),
            phase: Phase::Running { done: 1, total: 9 },
            ..State::default()
        };
        assert!(!state.can_remove());
        state.cancel_remove();
        assert!(cancel.load(Ordering::Relaxed));
        drop(state);
    }

    #[test]
    fn closing_during_a_removal_cancels_it() {
        let (_tx, events) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut state = State {
            removal: Some(RemovalRun {
                events,
                cancel: cancel.clone(),
            }),
            phase: Phase::Running { done: 0, total: 3 },
            ..State::default()
        };
        state.release();
        assert!(cancel.load(Ordering::Relaxed));
        assert_eq!(state.phase, Phase::List);
    }

    #[test]
    fn a_worker_that_vanishes_does_not_leave_the_window_running() {
        let (tx, events) = mpsc::channel::<RemovalEvent>();
        let mut state = State {
            removal: Some(RemovalRun {
                events,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            phase: Phase::Running { done: 1, total: 4 },
            ..State::default()
        };
        drop(tx);
        state.absorb();
        assert_eq!(state.phase.headline(), CLEANUP_WORKER_LOST);
        assert!(state.removal.is_none());
    }
}
