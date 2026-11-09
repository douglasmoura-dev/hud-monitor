//! Real sources for Gemini, Cursor and the presence-only tools. Nothing here reads a credential:
//! detection only checks that a file or folder exists.

use hud_core::ai::{AiError, ApiError};
use hud_core::extras::{gemini_cutoff, tally_gemini_line, CursorUsageApi, Probe, RequestCounter};
use hud_core::ui_text::CURSOR_UNAVAILABLE;
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufRead, BufReader, ErrorKind},
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

const OLLAMA_TAGS_URL: &str = "http://127.0.0.1:11434/api/tags";
const OLLAMA_TIMEOUT: Duration = Duration::from_secs(1);

pub struct PathProbe(pub PathBuf);

impl Probe for PathProbe {
    fn present(&self) -> bool {
        self.0.exists()
    }
}

/// Cursor keeps its session token in a SQLite `state.vscdb`; reading it would pull in a SQLite
/// dependency just for one row, so the usage stays unavailable (shown as `—`) instead of guessed.
pub struct CursorNoSource;

impl CursorUsageApi for CursorNoSource {
    fn get(&self) -> Result<String, ApiError> {
        Err(ApiError::Other(CURSOR_UNAVAILABLE.to_owned()))
    }
}

/// Counts model responses in `~/.gemini/tmp/**/session-*.jsonl` written in the last 24 h.
pub struct GeminiLogs {
    root: PathBuf,
}

impl GeminiLogs {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_root(home: &Path) -> PathBuf {
        home.join(".gemini").join("tmp")
    }
}

impl RequestCounter for GeminiLogs {
    fn last_day(&self, now: i64) -> Result<u64, AiError> {
        let mut seen = HashSet::new();
        let mut dirs = vec![self.root.clone()];
        while let Some(dir) = dirs.pop() {
            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => return Err(AiError::Logs(e.kind().to_string())),
            };
            for entry in entries.flatten() {
                let path = entry.path();
                match entry.file_type() {
                    Ok(t) if t.is_dir() => dirs.push(path),
                    Ok(t) if t.is_file() && is_recent_session(&path, now) => {
                        scan_file(&path, now, &mut seen);
                    }
                    _ => {}
                }
            }
        }
        Ok(seen.len() as u64)
    }
}

fn is_recent_session(path: &Path, now: i64) -> bool {
    let named = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("session-") && n.ends_with(".jsonl"));
    let modified = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok());
    named && modified.is_some_and(|m| m >= gemini_cutoff(now))
}

fn scan_file(path: &Path, now: i64, seen: &mut HashSet<String>) {
    let Ok(file) = File::open(path) else {
        return;
    };
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if let Ok(text) = std::str::from_utf8(&line) {
                    tally_gemini_line(seen, text, now);
                }
            }
        }
    }
}

/// Local-only GET; anything but a 200 within 1 s means Ollama is not running.
pub fn ollama_up() -> bool {
    ollama_up_at(OLLAMA_TAGS_URL)
}

fn ollama_up_at(url: &str) -> bool {
    let agent = ureq::AgentBuilder::new()
        .timeout(OLLAMA_TIMEOUT)
        .redirects(0)
        .build();
    agent.get(url).call().is_ok_and(|r| r.status() == 200)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::SystemTime;

    const GEMINI_LOG: &str = include_str!("../../../hud-core/tests/fixtures/gemini_session.jsonl");
    const FAKE_TOKEN: &str = "fake-gemini-token-0000";
    const NOW: i64 = 1_794_066_960; // 2026-11-07T15:56:00Z

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hud-extras-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn hash(path: &Path) -> u64 {
        let mut h = DefaultHasher::new();
        fs::read(path).unwrap().hash(&mut h);
        h.finish()
    }

    fn now_secs() -> i64 {
        let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        i64::try_from(d.as_secs()).unwrap()
    }

    #[test]
    fn probe_detects_without_touching_or_leaking_the_credential_file() {
        let dir = scratch("probe");
        let creds = dir.join("oauth_creds.json");
        fs::write(&creds, format!("{{\"access_token\":\"{FAKE_TOKEN}\"}}")).unwrap();
        let before = hash(&creds);
        let probe = PathProbe(creds.clone());
        assert!(probe.present());
        assert!(!format!("{:?}", probe.0.exists()).contains(FAKE_TOKEN));
        assert_eq!(hash(&creds), before);
        assert!(!PathProbe(dir.join("missing")).present());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn gemini_logs_count_unique_responses_across_nested_session_files() {
        let dir = scratch("gemini");
        let chats = dir.join("app").join("chats");
        fs::create_dir_all(&chats).unwrap();
        fs::write(chats.join("session-1.jsonl"), GEMINI_LOG).unwrap();
        fs::write(chats.join("notes.jsonl"), GEMINI_LOG).unwrap();
        let counter = GeminiLogs::new(dir.clone());
        assert_eq!(counter.last_day(NOW), Ok(2));
        // Files not written in the last 24 h are skipped without being read.
        assert_eq!(counter.last_day(now_secs() + 3 * 86_400), Ok(0));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_gemini_folder_counts_zero() {
        let counter = GeminiLogs::new(scratch("none").join("absent"));
        assert_eq!(counter.last_day(NOW), Ok(0));
    }

    #[test]
    fn cursor_without_source_reports_why() {
        assert_eq!(
            CursorNoSource.get(),
            Err(ApiError::Other(CURSOR_UNAVAILABLE.to_owned()))
        );
    }

    #[test]
    fn ollama_is_up_only_when_the_endpoint_answers_200() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/tags", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf);
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .unwrap();
        });
        assert!(ollama_up_at(&url));
        server.join().unwrap();
        assert!(!ollama_up_at(&url));
    }
}
