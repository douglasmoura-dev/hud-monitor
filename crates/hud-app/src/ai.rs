//! Background poller for the AI limits and tool presence: one fetch every 60 s, results sent to the UI thread.

use crate::sources::{
    ollama_up, CursorNoSource, FileCredentials, GeminiLogs, JsonlScanner, PathProbe, UreqApi,
};
use hud_core::ai::{claude_state, codex_state, ClaudeProvider, CodexProvider};
use hud_core::extras::{cursor_state, gemini_state, presence_rows, PresenceRow, Probe, UsageState};
use hud_core::metrics::Metric;
use hud_core::view::ClaudeState;
use std::{
    path::{Path, PathBuf},
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
    pub gemini: UsageState,
    pub cursor: UsageState,
    pub presence: Vec<PresenceRow>,
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
            gemini: Metric::unavailable(reason),
            cursor: Metric::unavailable(reason),
            presence: Vec::new(),
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
    let sources = Extras::new(&home);
    loop {
        let update = match unix_now() {
            Some(now) => AiUpdate {
                claude: claude_state(&provider, now),
                codex: codex_state(&codex, now),
                gemini: gemini_state(&sources.gemini_probe, &sources.gemini_logs, now),
                cursor: cursor_state(&sources.cursor_probe, &CursorNoSource),
                presence: sources.presence(),
            },
            None => AiUpdate {
                claude: Metric::unavailable("relógio indisponível"),
                codex: Metric::unavailable("relógio indisponível"),
                gemini: Metric::unavailable("relógio indisponível"),
                cursor: Metric::unavailable("relógio indisponível"),
                presence: sources.presence(),
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

struct Extras {
    gemini_probe: PathProbe,
    gemini_logs: GeminiLogs,
    cursor_probe: PathProbe,
    copilot: PathProbe,
    opencode: PathProbe,
}

impl Extras {
    fn new(home: &Path) -> Self {
        Self {
            gemini_probe: PathProbe(home.join(".gemini").join("oauth_creds.json")),
            gemini_logs: GeminiLogs::new(GeminiLogs::default_root(home)),
            cursor_probe: PathProbe(appdata(home).join("Cursor")),
            copilot: PathProbe(home.join(".copilot")),
            opencode: PathProbe(home.join(".local").join("share").join("opencode")),
        }
    }

    fn presence(&self) -> Vec<PresenceRow> {
        presence_rows(self.copilot.present(), self.opencode.present(), ollama_up())
    }
}

fn appdata(home: &Path) -> PathBuf {
    std::env::var_os("APPDATA").map_or_else(|| home.join("AppData").join("Roaming"), PathBuf::from)
}
