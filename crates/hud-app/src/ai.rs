//! Background poller for the Claude limits: one fetch every 60 s, results sent to the UI thread.

use crate::sources::{FileCredentials, JsonlScanner, UreqApi};
use hud_core::ai::{claude_state, codex_state, ClaudeProvider, CodexProvider};
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

/// Latest state of every AI provider.
pub struct AiUpdate {
    pub claude: ClaudeState,
    pub codex: ClaudeState,
}

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

pub fn spawn() -> (Receiver<AiUpdate>, Poller) {
    let (tx, rx) = mpsc::channel();
    let (stop, stop_rx) = mpsc::channel();
    let worker = thread::spawn(move || poll_loop(&tx, &stop_rx));
    (rx, Poller { stop, worker })
}

fn poll_loop(tx: &Sender<AiUpdate>, stop: &Receiver<()>) {
    let (Some(home), Some(_)) = (home(), unix_now()) else {
        let reason = "pasta do usuário ou relógio indisponível";
        let _ = tx.send(AiUpdate {
            claude: Metric::unavailable(reason),
            codex: Metric::unavailable(reason),
        });
        return;
    };
    let provider = ClaudeProvider {
        credentials: FileCredentials::new(FileCredentials::default_path(&home)),
        api: UreqApi::new(),
        transcripts: JsonlScanner::new(JsonlScanner::default_root(&home)),
    };
    let codex = CodexProvider {
        credentials: FileCredentials::new(FileCredentials::codex_path(&home)),
        api: UreqApi::new(),
    };
    loop {
        let update = match unix_now() {
            Some(now) => AiUpdate {
                claude: claude_state(&provider, now),
                codex: codex_state(&codex, now),
            },
            None => AiUpdate {
                claude: Metric::unavailable("relógio indisponível"),
                codex: Metric::unavailable("relógio indisponível"),
            },
        };
        if tx.send(update).is_err() {
            break;
        }
        if stop.recv_timeout(POLL_EVERY) != Err(RecvTimeoutError::Timeout) {
            break;
        }
    }
}
