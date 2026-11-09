//! Background poller for the AI limits and tool presence: one fetch every 60 s, results sent to the UI thread.

use crate::clock::unix_now;
use crate::sources::{
    ollama_up, CursorNoSource, FileCredentials, GeminiLogs, JsonlScanner, PathProbe, UreqApi,
};
use hud_core::ai::{claude_state, codex_state, ClaudeProvider, CodexProvider};
use hud_core::dashboard::ProviderStates;
use hud_core::extras::{cursor_state, gemini_state, presence_rows, PresenceRow, Probe};
use hud_core::ui_text::{CLOCK_UNAVAILABLE, HOME_OR_CLOCK_UNAVAILABLE};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::Duration,
};

const POLL_EVERY: Duration = Duration::from_secs(60);

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

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

pub fn spawn() -> (Receiver<ProviderStates>, Poller) {
    let (tx, rx) = mpsc::channel();
    let (stop, stop_rx) = mpsc::channel();
    let worker = thread::spawn(move || poll_loop(&tx, &stop_rx));
    (rx, Poller { stop, worker })
}

fn poll_loop(tx: &Sender<ProviderStates>, stop: &Receiver<()>) {
    let (Some(home), Some(_)) = (home(), unix_now()) else {
        let _ = tx.send(ProviderStates::all_unavailable(
            HOME_OR_CLOCK_UNAVAILABLE,
            Vec::new(),
        ));
        return;
    };
    let sources = Sources::new(&home);
    loop {
        if tx.send(sources.poll()).is_err() {
            break;
        }
        if stop.recv_timeout(POLL_EVERY) != Err(RecvTimeoutError::Timeout) {
            break;
        }
    }
}

/// Every real source one poll reads, built once per run.
struct Sources {
    claude: ClaudeProvider<FileCredentials, UreqApi, JsonlScanner>,
    codex: CodexProvider<FileCredentials, UreqApi>,
    gemini_probe: PathProbe,
    gemini_logs: GeminiLogs,
    cursor_probe: PathProbe,
    copilot: PathProbe,
    opencode: PathProbe,
}

impl Sources {
    fn new(home: &Path) -> Self {
        Self {
            claude: ClaudeProvider {
                credentials: FileCredentials::new(FileCredentials::default_path(home)),
                api: UreqApi::new(),
                transcripts: JsonlScanner::new(JsonlScanner::default_root(home)),
            },
            codex: CodexProvider {
                credentials: FileCredentials::new(FileCredentials::codex_path(home)),
                api: UreqApi::new(),
            },
            gemini_probe: PathProbe(home.join(".gemini").join("oauth_creds.json")),
            gemini_logs: GeminiLogs::new(GeminiLogs::default_root(home)),
            cursor_probe: PathProbe(appdata(home).join("Cursor")),
            copilot: PathProbe(home.join(".copilot")),
            opencode: PathProbe(home.join(".local").join("share").join("opencode")),
        }
    }

    fn poll(&self) -> ProviderStates {
        let Some(now) = unix_now() else {
            return ProviderStates::all_unavailable(CLOCK_UNAVAILABLE, self.presence());
        };
        ProviderStates {
            claude: claude_state(&self.claude, now),
            codex: codex_state(&self.codex, now),
            gemini: gemini_state(&self.gemini_probe, &self.gemini_logs, now),
            cursor: cursor_state(&self.cursor_probe, &CursorNoSource),
            presence: self.presence(),
        }
    }

    fn presence(&self) -> Vec<PresenceRow> {
        presence_rows(self.copilot.present(), self.opencode.present(), ollama_up())
    }
}

fn appdata(home: &Path) -> PathBuf {
    std::env::var_os("APPDATA").map_or_else(|| home.join("AppData").join("Roaming"), PathBuf::from)
}
