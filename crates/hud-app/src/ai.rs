//! Background poller for the Claude limits: one fetch every 60 s, results sent to the UI thread.

use crate::sources::{FileCredentials, JsonlScanner, UreqApi};
use hud_core::ai::{claude_state, ClaudeProvider};
use hud_core::metrics::Metric;
use hud_core::view::ClaudeState;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const POLL_EVERY: Duration = Duration::from_secs(60);
pub const WAITING: &str = "aguardando a primeira leitura";

pub struct Poller {
    stop: Sender<()>,
    worker: thread::JoinHandle<()>,
}

impl Poller {
    pub fn shutdown(self) {
        drop(self.stop);
        let _ = self.worker.join();
    }
}

/// Current time in Unix seconds, `None` if the clock is before 1970.
pub fn unix_now() -> Option<i64> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

pub fn spawn() -> (Receiver<ClaudeState>, Poller) {
    let (tx, rx) = mpsc::channel();
    let (stop, stop_rx) = mpsc::channel();
    let worker = thread::spawn(move || poll_loop(&tx, &stop_rx));
    (rx, Poller { stop, worker })
}

fn poll_loop(tx: &Sender<ClaudeState>, stop: &Receiver<()>) {
    let (Some(home), Some(_)) = (home(), unix_now()) else {
        let _ = tx.send(Metric::unavailable(
            "pasta do usuário ou relógio indisponível",
        ));
        return;
    };
    let provider = ClaudeProvider {
        credentials: FileCredentials::new(FileCredentials::default_path(&home)),
        api: UreqApi::new(),
        transcripts: JsonlScanner::new(JsonlScanner::default_root(&home)),
    };
    loop {
        let state = match unix_now() {
            Some(now) => claude_state(&provider, now),
            None => Metric::unavailable("relógio indisponível"),
        };
        if tx.send(state).is_err() {
            break;
        }
        if stop.recv_timeout(POLL_EVERY) != Err(RecvTimeoutError::Timeout) {
            break;
        }
    }
}
