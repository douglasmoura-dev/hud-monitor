//! Real Claude sources: the credentials file (read-only), the usage endpoint and the transcript scan.

use hud_core::ai::{
    log_cutoff, parse_usage_line, AiError, ApiError, CodexUsageApi, CredentialsSource, Secret,
    TokenSums, TranscriptScanner, UsageApi, UsageEntry,
};
use hud_core::ui_text::{
    no_connection, ANTHROPIC_WITH_ARTICLE, CHATGPT_WITH_ARTICLE, UNREADABLE_RESPONSE,
};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader, ErrorKind, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, UNIX_EPOCH},
};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA_HEADER: &str = "oauth-2025-04-20";
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY: u64 = 64 * 1024;

pub struct FileCredentials {
    path: PathBuf,
}

impl FileCredentials {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path(home: &Path) -> PathBuf {
        home.join(".claude").join(".credentials.json")
    }

    pub fn codex_path(home: &Path) -> PathBuf {
        home.join(".codex").join("auth.json")
    }
}

impl CredentialsSource for FileCredentials {
    fn read(&self) -> Result<Option<String>, AiError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(AiError::Credentials(e.kind().to_string())),
        }
    }
}

pub struct UreqApi {
    agent: ureq::Agent,
}

impl UreqApi {
    pub fn new() -> Self {
        // No redirects: each token must only ever reach its own provider domain.
        let agent = ureq::AgentBuilder::new()
            .timeout(HTTP_TIMEOUT)
            .redirects(0)
            .build();
        Self { agent }
    }
}

impl UsageApi for UreqApi {
    fn get(&self, token: &Secret) -> Result<String, ApiError> {
        let request = self
            .agent
            .get(USAGE_URL)
            .set("Authorization", &format!("Bearer {}", token.expose()))
            .set("anthropic-beta", BETA_HEADER);
        read_response(request.call(), ANTHROPIC_WITH_ARTICLE)
    }
}

impl CodexUsageApi for UreqApi {
    /// Same read-only GET the Codex CLI makes for its limits; it never consumes quota.
    fn get(&self, token: &Secret, account_id: &str) -> Result<String, ApiError> {
        let request = self
            .agent
            .get(CODEX_USAGE_URL)
            .set("Authorization", &format!("Bearer {}", token.expose()))
            .set("chatgpt-account-id", account_id);
        read_response(request.call(), CHATGPT_WITH_ARTICLE)
    }
}

fn read_response(
    response: Result<ureq::Response, ureq::Error>,
    provider: &str,
) -> Result<String, ApiError> {
    match response {
        Ok(r) if r.status() == 200 => read_body(r),
        Ok(r) => Err(ApiError::Other(format!("HTTP {}", r.status()))),
        Err(ureq::Error::Status(401 | 403, _)) => Err(ApiError::Unauthorized),
        Err(ureq::Error::Status(429, r)) => Err(ApiError::RateLimited {
            retry_after: r.header("Retry-After").and_then(parse_retry_after),
        }),
        Err(ureq::Error::Status(code, _)) => Err(ApiError::Other(format!("HTTP {code}"))),
        Err(ureq::Error::Transport(_)) => Err(ApiError::Other(no_connection(provider))),
    }
}

/// `Retry-After` in seconds; the HTTP-date form is ignored (the backoff covers it).
fn parse_retry_after(value: &str) -> Option<u64> {
    value.trim().parse().ok()
}

fn read_body(r: ureq::Response) -> Result<String, ApiError> {
    let mut body = String::new();
    r.into_reader()
        .take(MAX_BODY)
        .read_to_string(&mut body)
        .map_err(|_| ApiError::Other(UNREADABLE_RESPONSE.to_owned()))?;
    Ok(body)
}

/// What was already read of one transcript: the byte offset after its last complete line and
/// the usage lines of the 7 d window found before it.
#[derive(Default)]
struct FileProgress {
    offset: u64,
    entries: Vec<UsageEntry>,
}

/// Reads `~/.claude/projects/**/*.jsonl` (files modified within 7 days). Each poll reads only
/// the bytes appended since the previous one, so a large history is parsed once.
pub struct JsonlScanner {
    root: PathBuf,
    progress: Mutex<HashMap<PathBuf, FileProgress>>,
}

impl JsonlScanner {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            progress: Mutex::new(HashMap::new()),
        }
    }

    pub fn default_root(home: &Path) -> PathBuf {
        home.join(".claude").join("projects")
    }
}

impl TranscriptScanner for JsonlScanner {
    fn sums(&self, now: i64) -> Result<TokenSums, AiError> {
        let mut seen: HashMap<PathBuf, FileProgress> = HashMap::new();
        let mut known = self
            .progress
            .lock()
            .map_err(|_| AiError::Logs("scan state poisoned".to_owned()))?;
        let mut dirs = vec![self.root.clone()];
        let mut line = Vec::new();
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
                    Ok(t) if t.is_file() => {
                        let Ok(meta) = entry.metadata() else {
                            continue;
                        };
                        if is_recent_jsonl(&path, &meta, now) {
                            let mut progress = known.remove(&path).unwrap_or_default();
                            read_new_lines(&path, meta.len(), now, &mut progress, &mut line);
                            seen.insert(path, progress);
                        }
                    }
                    _ => {}
                }
            }
        }
        *known = seen;
        let mut sums = TokenSums::default();
        for entry in known.values().flat_map(|p| &p.entries) {
            sums.add(*entry, now);
        }
        Ok(sums)
    }
}

fn is_recent_jsonl(path: &Path, meta: &fs::Metadata, now: i64) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return false;
    }
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok());
    modified.is_some_and(|m| m >= log_cutoff(now))
}

/// Parses the complete lines after `progress.offset` of a file now `len` bytes long (a shorter file restarts from zero) and
/// forgets entries that left the 7 d window. One reused byte buffer keeps memory flat;
/// unreadable files and non-UTF-8 lines are skipped.
fn read_new_lines(
    path: &Path,
    len: u64,
    now: i64,
    progress: &mut FileProgress,
    line: &mut Vec<u8>,
) {
    if len < progress.offset {
        *progress = FileProgress::default();
    }
    progress.entries.retain(|e| e.timestamp >= log_cutoff(now));
    if len == progress.offset {
        return;
    }
    let Ok(mut file) = File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(progress.offset)).is_err() {
        return;
    }
    let mut reader = BufReader::new(file);
    loop {
        line.clear();
        match reader.read_until(b'\n', line) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if line.last() != Some(&b'\n') {
                    break;
                }
                progress.offset += n as u64;
                if let Some(entry) = std::str::from_utf8(line)
                    .ok()
                    .and_then(parse_usage_line)
                    .filter(|e| e.timestamp >= log_cutoff(now))
                {
                    progress.entries.push(entry);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hud_core::ai::{AiProvider, ClaudeProvider, Reading};
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    const NOW: i64 = 1_794_066_960; // 2026-11-07T15:56:00Z
    const FAKE_TOKEN: &str = "fake-token-for-tests-0000";
    const CREDS: &str =
        include_str!("../../../hud-core/tests/fixtures/claude_credentials_pro.json");
    const CODEX_AUTH: &str = include_str!("../../../hud-core/tests/fixtures/codex_auth.json");
    const SESSION: &str = include_str!("../../../hud-core/tests/fixtures/claude_session.jsonl");

    struct Unauthorized;
    impl UsageApi for Unauthorized {
        fn get(&self, _: &Secret) -> Result<String, ApiError> {
            Err(ApiError::Unauthorized)
        }
    }

    #[test]
    fn http_429_is_rate_limited_with_the_retry_after_seconds() {
        let response = |header: Option<&str>| {
            let head = header.map_or(String::new(), |v| format!("Retry-After: {v}\r\n"));
            let raw = format!("HTTP/1.1 429 Too Many Requests\r\n{head}\r\n");
            let r: ureq::Response = raw.parse().unwrap();
            Err(ureq::Error::Status(429, r))
        };
        let limited = |retry_after| Err(ApiError::RateLimited { retry_after });
        assert_eq!(
            read_response(response(Some("120")), "x"),
            limited(Some(120))
        );
        assert_eq!(read_response(response(None), "x"), limited(None));
        assert_eq!(read_response(response(Some("soon")), "x"), limited(None));
        assert_eq!(parse_retry_after(" 30 "), Some(30));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hud-claude-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn hash(path: &Path) -> u64 {
        let mut h = DefaultHasher::new();
        fs::read(path).unwrap().hash(&mut h);
        h.finish()
    }

    fn set_age_days(path: &Path, days: u64) {
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(NOW as u64 - days * 86400);
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn scanner_sums_recent_files_recursively_and_skips_stale_ones() {
        let dir = scratch("scan");
        fs::create_dir_all(dir.join("proj-a")).unwrap();
        fs::write(dir.join("proj-a/s.jsonl"), SESSION).unwrap();
        fs::write(dir.join("proj-a/notes.txt"), SESSION).unwrap();
        fs::write(dir.join("stale.jsonl"), SESSION).unwrap();
        set_age_days(&dir.join("proj-a/s.jsonl"), 0);
        set_age_days(&dir.join("proj-a/notes.txt"), 0);
        set_age_days(&dir.join("stale.jsonl"), 8);
        let sums = JsonlScanner::new(dir.clone()).sums(NOW).unwrap();
        assert_eq!((sums.five_hour, sums.seven_day), (1150, 1180));
        fs::remove_dir_all(dir).unwrap();
    }

    const LINE: &str = "{\"timestamp\":\"2026-11-07T15:00:00Z\",\"message\":{\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}";

    #[test]
    fn scanner_reads_only_appended_lines_and_waits_for_complete_ones() {
        let dir = scratch("incremental");
        let file = dir.join("s.jsonl");
        let scanner = JsonlScanner::new(dir.clone());
        let write = |text: String| {
            fs::write(&file, text).unwrap();
            set_age_days(&file, 0);
        };
        write(format!("{LINE}\n"));
        assert_eq!(scanner.sums(NOW).unwrap().seven_day, 15);
        write(format!("{LINE}\n{LINE}"));
        assert_eq!(scanner.sums(NOW).unwrap().seven_day, 15);
        write(format!("{LINE}\n{LINE}\n"));
        assert_eq!(scanner.sums(NOW).unwrap().seven_day, 30);
        write(format!("{LINE}\n"));
        assert_eq!(scanner.sums(NOW).unwrap().seven_day, 15);
        fs::remove_file(&file).unwrap();
        assert_eq!(scanner.sums(NOW).unwrap(), TokenSums::default());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_projects_folder_counts_zero() {
        let sums = JsonlScanner::new(scratch("none").join("absent"))
            .sums(NOW)
            .unwrap();
        assert_eq!(sums, TokenSums::default());
    }

    #[test]
    fn expired_login_uses_local_logs_and_leaves_credentials_untouched() {
        let dir = scratch("creds");
        let creds = FileCredentials::default_path(&dir);
        fs::create_dir_all(creds.parent().unwrap()).unwrap();
        fs::write(&creds, CREDS).unwrap();
        let before = hash(&creds);
        let provider = ClaudeProvider::new(
            FileCredentials::new(creds.clone()),
            Unauthorized,
            JsonlScanner::new(JsonlScanner::default_root(&dir)),
        );
        assert!(provider.detect());
        let status = provider.fetch(NOW).unwrap();
        assert_eq!(status.five_hour, Reading::Estimate { tokens: 0 });
        assert_eq!(status.plan.as_deref(), Some("PRO"));
        assert!(!format!("{status:?}").contains(FAKE_TOKEN));
        assert_eq!(hash(&creds), before);
        assert_eq!(fs::read_to_string(&creds).unwrap(), CREDS);
        fs::remove_dir_all(dir).unwrap();
    }

    struct CodexUnauthorized;
    impl CodexUsageApi for CodexUnauthorized {
        fn get(&self, _: &Secret, _: &str) -> Result<String, ApiError> {
            Err(ApiError::Unauthorized)
        }
    }

    #[test]
    fn codex_401_shows_renew_tip_logs_no_token_and_leaves_auth_file_untouched() {
        use hud_core::ai::{codex_state, CodexProvider};
        let dir = scratch("codex");
        let auth = FileCredentials::codex_path(&dir);
        fs::create_dir_all(auth.parent().unwrap()).unwrap();
        fs::write(&auth, CODEX_AUTH).unwrap();
        let before = hash(&auth);
        let provider = CodexProvider {
            credentials: FileCredentials::new(auth.clone()),
            api: CodexUnauthorized,
        };
        let state = codex_state(&provider, NOW);
        assert_eq!(
            state,
            hud_core::metrics::Metric::unavailable(hud_core::ui_text::CODEX_RENEW_LOGIN_TIP)
        );
        assert!(!format!("{state:?}").contains("fake-codex-token-0000"));
        assert_eq!(hash(&auth), before);
        assert_eq!(fs::read_to_string(&auth).unwrap(), CODEX_AUTH);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn codex_without_auth_file_is_not_found() {
        use hud_core::ai::{codex_state, CodexProvider};
        let dir = scratch("nocodex");
        let provider = CodexProvider {
            credentials: FileCredentials::new(FileCredentials::codex_path(&dir)),
            api: CodexUnauthorized,
        };
        assert_eq!(
            codex_state(&provider, NOW),
            hud_core::metrics::Metric::unavailable(hud_core::ui_text::CODEX_NOT_FOUND)
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn absent_credentials_file_is_not_detected() {
        let dir = scratch("nocreds");
        let source = FileCredentials::new(FileCredentials::default_path(&dir));
        assert_eq!(source.read(), Ok(None));
        fs::remove_dir_all(dir).unwrap();
    }
}
