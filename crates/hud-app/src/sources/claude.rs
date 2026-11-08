//! Real Claude sources: the credentials file (read-only), the usage endpoint and the transcript scan.

use hud_core::ai::{
    log_cutoff, tally_line, AiError, ApiError, CodexUsageApi, CredentialsSource, Secret, TokenSums,
    TranscriptScanner, UsageApi,
};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, ErrorKind, Read},
    path::{Path, PathBuf},
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
        read_response(request.call(), "a Anthropic")
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
        read_response(request.call(), "o ChatGPT")
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
        Err(ureq::Error::Status(code, _)) => Err(ApiError::Other(format!("HTTP {code}"))),
        Err(ureq::Error::Transport(_)) => {
            Err(ApiError::Other(format!("sem conexão com {provider}")))
        }
    }
}

fn read_body(r: ureq::Response) -> Result<String, ApiError> {
    let mut body = String::new();
    r.into_reader()
        .take(MAX_BODY)
        .read_to_string(&mut body)
        .map_err(|_| ApiError::Other("resposta ilegível".to_owned()))?;
    Ok(body)
}

/// Streams `~/.claude/projects/**/*.jsonl` line by line, skipping files older than 7 days.
pub struct JsonlScanner {
    root: PathBuf,
}

impl JsonlScanner {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_root(home: &Path) -> PathBuf {
        home.join(".claude").join("projects")
    }
}

impl TranscriptScanner for JsonlScanner {
    fn sums(&self, now: i64) -> Result<TokenSums, AiError> {
        let mut sums = TokenSums::default();
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
                    Ok(t) if t.is_file() && is_recent_jsonl(&path, now) => {
                        scan_file(&path, now, &mut sums, &mut line);
                    }
                    _ => {}
                }
            }
        }
        Ok(sums)
    }
}

fn is_recent_jsonl(path: &Path, now: i64) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return false;
    }
    let modified = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok());
    modified.is_some_and(|m| m >= log_cutoff(now))
}

/// One reused byte buffer keeps memory flat; unreadable files and non-UTF-8 lines are skipped.
fn scan_file(path: &Path, now: i64, sums: &mut TokenSums, line: &mut Vec<u8>) {
    let Ok(file) = File::open(path) else {
        return;
    };
    let mut reader = BufReader::new(file);
    loop {
        line.clear();
        match reader.read_until(b'\n', line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if let Ok(text) = std::str::from_utf8(line) {
                    tally_line(sums, text, now);
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
        let provider = ClaudeProvider {
            credentials: FileCredentials::new(creds.clone()),
            api: Unauthorized,
            transcripts: JsonlScanner::new(JsonlScanner::default_root(&dir)),
        };
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
            hud_core::metrics::Metric::unavailable("abra o Codex para renovar o login")
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
            hud_core::metrics::Metric::unavailable("Codex não encontrado")
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
