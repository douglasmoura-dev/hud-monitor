//! AI provider limits (Claude Pro/Max). Parsers, reset formatting and the provider logic live here;
//! the HTTP call, the credentials file and the transcript scan are traits implemented in hud-app.
//!
//! Credentials are read-only (ADR 0002): the token is wrapped in `Secret`, never printed, and only
//! ever handed to `UsageApi`.

use crate::metrics::Metric;
use crate::ui_text::{
    ai_error_reason, invalid_date, missing_window, stale_tip, API_RATE_LIMITED, CLAUDE_NOT_FOUND,
    CLAUDE_RENEW_LOGIN_TIP, CODEX_NOT_FOUND, CODEX_RENEW_LOGIN_TIP, NO_LIMIT_WINDOWS,
    PRIMARY_WINDOW, RESET_NOW, SECONDARY_WINDOW,
};
use crate::view::ClaudeState;
use serde::Deserialize;
use std::fmt;
use std::sync::{Mutex, PoisonError};

const FIVE_HOURS: i64 = 5 * 3600;
const SEVEN_DAYS: i64 = 7 * 86400;
const SECS_PER_DAY: i64 = 86400;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AiError {
    #[error("unreadable credentials: {0}")]
    Credentials(String),
    #[error("invalid limits response: {0}")]
    Response(String),
    #[error("unreadable local logs: {0}")]
    Logs(String),
    /// Reason shown as `—` when a provider has no usable data and no local fallback.
    #[error("{0}")]
    Api(String),
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ApiError {
    #[error("login expired")]
    Unauthorized,
    /// HTTP 429; `retry_after` is the `Retry-After` header in seconds, when sent.
    #[error("rate limited")]
    RateLimited { retry_after: Option<u64> },
    #[error("{0}")]
    Other(String),
}

/// A credential that never shows up in `Debug` or `Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// Only for building the `Authorization` header sent to the provider.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// One limit window, either read from the provider or estimated from local logs.
#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    Official {
        percent: f32,
        resets_at: i64,
    },
    /// Tokens counted in the local transcripts; no honest percentage exists (limits are not public).
    Estimate {
        tokens: u64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderStatus {
    /// `PRO` or `MAX` when the credentials say so.
    pub plan: Option<String>,
    pub five_hour: Reading,
    pub seven_day: Reading,
    /// Why the data is an estimate (empty when official).
    pub tip: String,
}

pub trait AiProvider {
    fn detect(&self) -> bool;
    fn fetch(&self, now: i64) -> Result<ProviderStatus, AiError>;
}

// ---------- parsing ----------

#[derive(Deserialize)]
struct RawWindow {
    utilization: f32,
    resets_at: String,
}

#[derive(Deserialize)]
struct RawUsage {
    five_hour: RawWindow,
    seven_day: RawWindow,
}

fn official(w: &RawWindow) -> Result<Reading, AiError> {
    let resets_at =
        parse_rfc3339(&w.resets_at).ok_or_else(|| AiError::Response(invalid_date(&w.resets_at)))?;
    Ok(Reading::Official {
        percent: w.utilization,
        resets_at,
    })
}

/// Parses the `/api/oauth/usage` body into `(five_hour, seven_day)`.
pub fn parse_usage(body: &str) -> Result<(Reading, Reading), AiError> {
    let raw: RawUsage = serde_json::from_str(body).map_err(|e| AiError::Response(e.to_string()))?;
    Ok((official(&raw.five_hour)?, official(&raw.seven_day)?))
}

#[derive(Deserialize)]
struct RawCredentials {
    #[serde(rename = "claudeAiOauth")]
    oauth: RawOauth,
}

#[derive(Deserialize)]
struct RawOauth {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
}

pub struct Credentials {
    pub token: Secret,
    pub plan: Option<String>,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("token", &self.token)
            .field("plan", &self.plan)
            .finish()
    }
}

/// `pro` -> `PRO`, `max` -> `MAX`; anything else shows no badge rather than a guess.
pub fn plan_label(subscription_type: &str) -> Option<String> {
    match subscription_type.to_ascii_lowercase().as_str() {
        "pro" => Some("PRO".to_owned()),
        "max" => Some("MAX".to_owned()),
        _ => None,
    }
}

pub fn parse_credentials(text: &str) -> Result<Credentials, AiError> {
    // serde errors quote the input only by position, never by content.
    let raw: RawCredentials =
        serde_json::from_str(text).map_err(|e| AiError::Credentials(e.to_string()))?;
    Ok(Credentials {
        token: Secret::new(raw.oauth.access_token),
        plan: raw.oauth.subscription_type.as_deref().and_then(plan_label),
    })
}

/// `YYYY-MM-DDTHH:MM:SS[.fff](Z|±HH:MM)` to Unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let (date, rest) = s.split_once('T')?;
    let mut d = date.split('-').map(str::parse::<i64>);
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let (clock, offset) = split_offset(rest)?;
    let mut t = clock.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let min: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t.next()?.split('.').next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 23 || min > 59 || sec > 60 {
        return None;
    }
    Some(days_from_civil(y, m, day) * SECS_PER_DAY + h * 3600 + min * 60 + sec - offset)
}

fn split_offset(rest: &str) -> Option<(&str, i64)> {
    if let Some(clock) = rest.strip_suffix('Z') {
        return Some((clock, 0));
    }
    let at = rest.rfind(['+', '-'])?;
    let (clock, off) = rest.split_at(at);
    let sign = if off.starts_with('-') { -1 } else { 1 };
    let (oh, om) = off[1..].split_once(':')?;
    Some((
        clock,
        sign * (oh.parse::<i64>().ok()? * 3600 + om.parse::<i64>().ok()? * 60),
    ))
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Time until a reset: `2d` from one day up, `1h48` from one hour up, else `35min`.
pub fn format_reset_in(resets_at: i64, now: i64) -> String {
    let left = resets_at - now;
    if left <= 0 {
        return RESET_NOW.to_owned();
    }
    if left >= SECS_PER_DAY {
        return format!("{}d", left / SECS_PER_DAY);
    }
    if left >= 3600 {
        return format!("{}h{:02}", left / 3600, left % 3600 / 60);
    }
    format!("{}min", (left / 60).max(1))
}

// ---------- plan B: local transcripts ----------

/// Tokens in the rolling 5 h and 7 d windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenSums {
    pub five_hour: u64,
    pub seven_day: u64,
}

#[derive(Deserialize)]
struct LogLine {
    timestamp: String,
    message: LogMessage,
}

#[derive(Deserialize)]
struct LogMessage {
    usage: Usage,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
}

/// Adds one transcript line to `sums`. Lines without usage or with bad JSON are skipped.
/// Cache reads are not counted: they repeat the same context on every turn.
// ponytail: no per-message de-duplication, so streaming repeats can overcount; the `~` marks it.
pub fn tally_line(sums: &mut TokenSums, line: &str, now: i64) {
    if let Some(entry) = parse_usage_line(line) {
        sums.add(entry, now);
    }
}

/// Tokens one transcript line spent, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageEntry {
    pub timestamp: i64,
    pub tokens: u64,
}

pub fn parse_usage_line(line: &str) -> Option<UsageEntry> {
    if !line.contains("\"usage\"") {
        return None;
    }
    let parsed = serde_json::from_str::<LogLine>(line).ok()?;
    let timestamp = parse_rfc3339(&parsed.timestamp)?;
    let u = parsed.message.usage;
    Some(UsageEntry {
        timestamp,
        tokens: u.input_tokens + u.output_tokens + u.cache_creation_input_tokens,
    })
}

impl TokenSums {
    /// Counts `entry` in the windows that still contain it at `now`.
    pub fn add(&mut self, entry: UsageEntry, now: i64) {
        let age = now - entry.timestamp;
        if (0..=FIVE_HOURS).contains(&age) {
            self.five_hour += entry.tokens;
        }
        if (0..=SEVEN_DAYS).contains(&age) {
            self.seven_day += entry.tokens;
        }
    }
}

/// Oldest file modification that can still hold a line of the 7 d window.
pub fn log_cutoff(now: i64) -> i64 {
    now - SEVEN_DAYS
}

// ---------- provider ----------

pub trait CredentialsSource {
    /// `None` when the tool is not installed. Never writes.
    fn read(&self) -> Result<Option<String>, AiError>;
}

pub trait UsageApi {
    /// Response body of the usage endpoint.
    fn get(&self, token: &Secret) -> Result<String, ApiError>;
}

pub trait TranscriptScanner {
    fn sums(&self, now: i64) -> Result<TokenSums, AiError>;
}

/// Normal gap between two calls to the Claude usage endpoint.
pub const CLAUDE_POLL_SECS: i64 = 300;
/// Ceiling of the wait after HTTP 429.
pub const CLAUDE_BACKOFF_MAX_SECS: i64 = 1800;

/// Seconds to wait after the `streak`-th consecutive 429: the doubling backoff (5, 10, 20, 30,
/// 30 min...) or the server's `Retry-After`, whichever is larger.
pub fn rate_limit_delay(streak: u32, retry_after: Option<u64>) -> i64 {
    let doublings = streak.saturating_sub(1).min(8);
    let backoff = (CLAUDE_POLL_SECS << doublings).min(CLAUDE_BACKOFF_MAX_SECS);
    let asked = retry_after.map_or(0, |s| i64::try_from(s).unwrap_or(i64::MAX));
    backoff.max(asked)
}

/// The last successful API reading, kept to show while the API refuses to answer.
#[derive(Debug, Clone)]
struct GoodReading {
    five_hour: Reading,
    seven_day: Reading,
    at: i64,
}

/// What the provider remembers between polls (the poller calls `fetch` every minute).
#[derive(Debug, Default)]
struct PollMemory {
    last_good: Option<GoodReading>,
    /// No API call before this instant.
    next_attempt_at: i64,
    rate_limited_streak: u32,
    /// Why the last call failed; `None` when the last call succeeded.
    failure: Option<String>,
}

impl PollMemory {
    fn throttled(&self, now: i64) -> bool {
        now < self.next_attempt_at && (self.last_good.is_some() || self.failure.is_some())
    }

    fn succeeded(&mut self, five_hour: Reading, seven_day: Reading, now: i64) {
        self.last_good = Some(GoodReading {
            five_hour,
            seven_day,
            at: now,
        });
        self.next_attempt_at = now + CLAUDE_POLL_SECS;
        self.rate_limited_streak = 0;
        self.failure = None;
    }

    fn rate_limited(&mut self, retry_after: Option<u64>, now: i64) {
        self.rate_limited_streak = self.rate_limited_streak.saturating_add(1);
        let delay = rate_limit_delay(self.rate_limited_streak, retry_after);
        self.next_attempt_at = now.saturating_add(delay);
        self.failure = Some(API_RATE_LIMITED.to_owned());
    }

    /// Any other failure is retried on the next poll.
    fn failed(&mut self, reason: String) {
        self.rate_limited_streak = 0;
        self.next_attempt_at = 0;
        self.failure = Some(reason);
    }
}

pub struct ClaudeProvider<C, A, T> {
    credentials: C,
    api: A,
    transcripts: T,
    memory: Mutex<PollMemory>,
}

impl<C: CredentialsSource, A: UsageApi, T: TranscriptScanner> ClaudeProvider<C, A, T> {
    pub fn new(credentials: C, api: A, transcripts: T) -> Self {
        Self {
            credentials,
            api,
            transcripts,
            memory: Mutex::new(PollMemory::default()),
        }
    }

    fn estimate(
        &self,
        plan: Option<String>,
        tip: &str,
        now: i64,
    ) -> Result<ProviderStatus, AiError> {
        let sums = self.transcripts.sums(now)?;
        Ok(ProviderStatus {
            plan,
            five_hour: Reading::Estimate {
                tokens: sums.five_hour,
            },
            seven_day: Reading::Estimate {
                tokens: sums.seven_day,
            },
            tip: tip.to_owned(),
        })
    }

    /// What to show without a fresh answer: the last good reading (marked stale with its age
    /// when the last call failed), or the local estimate when there never was one.
    fn from_memory(
        &self,
        memory: &PollMemory,
        plan: Option<String>,
        now: i64,
    ) -> Result<ProviderStatus, AiError> {
        match (&memory.last_good, &memory.failure) {
            (Some(good), failure) => Ok(ProviderStatus {
                plan,
                five_hour: good.five_hour.clone(),
                seven_day: good.seven_day.clone(),
                tip: failure
                    .as_deref()
                    .map_or_else(String::new, |reason| stale_tip(now - good.at, reason)),
            }),
            (None, Some(reason)) => self.estimate(plan, reason, now),
            (None, None) => self.estimate(plan, "", now),
        }
    }
}

impl<C: CredentialsSource, A: UsageApi, T: TranscriptScanner> AiProvider
    for ClaudeProvider<C, A, T>
{
    fn detect(&self) -> bool {
        matches!(self.credentials.read(), Ok(Some(_)))
    }

    fn fetch(&self, now: i64) -> Result<ProviderStatus, AiError> {
        let text = self
            .credentials
            .read()?
            .ok_or_else(|| AiError::Credentials(CLAUDE_NOT_FOUND.to_owned()))?;
        let creds = parse_credentials(&text)?;
        let mut memory = self.memory.lock().unwrap_or_else(PoisonError::into_inner);
        if memory.throttled(now) {
            return self.from_memory(&memory, creds.plan, now);
        }
        match self.api.get(&creds.token) {
            Ok(body) => {
                let (five_hour, seven_day) = parse_usage(&body)?;
                memory.succeeded(five_hour, seven_day, now);
                self.from_memory(&memory, creds.plan, now)
            }
            Err(ApiError::Unauthorized) => {
                memory.failed(CLAUDE_RENEW_LOGIN_TIP.to_owned());
                self.estimate(creds.plan, CLAUDE_RENEW_LOGIN_TIP, now)
            }
            Err(ApiError::RateLimited { retry_after }) => {
                memory.rate_limited(retry_after, now);
                self.from_memory(&memory, creds.plan, now)
            }
            Err(ApiError::Other(reason)) => {
                memory.failed(reason);
                self.from_memory(&memory, creds.plan, now)
            }
        }
    }
}

/// Runs one poll: `Unavailable(reason)` when the tool is missing or the data cannot be read.
pub fn claude_state(provider: &impl AiProvider, now: i64) -> ClaudeState {
    if !provider.detect() {
        return Metric::unavailable(CLAUDE_NOT_FOUND);
    }
    match provider.fetch(now) {
        Ok(status) => Metric::Available(status),
        Err(e) => Metric::Unavailable(ai_error_reason(&e)),
    }
}

// ---------- Codex ----------

#[derive(Deserialize)]
struct RawCodexWindow {
    used_percent: f32,
    reset_after_seconds: i64,
}

#[derive(Deserialize)]
struct RawCodexLimits {
    primary_window: Option<RawCodexWindow>,
    secondary_window: Option<RawCodexWindow>,
}

#[derive(Deserialize)]
struct RawCodexUsage {
    rate_limit: Option<RawCodexLimits>,
}

fn codex_reading(w: Option<RawCodexWindow>, name: &str, now: i64) -> Result<Reading, AiError> {
    let w = w.ok_or_else(|| AiError::Response(missing_window(name)))?;
    Ok(Reading::Official {
        percent: w.used_percent,
        resets_at: now + w.reset_after_seconds,
    })
}

/// Parses the `GET /backend-api/wham/usage` body (the one the Codex CLI reads) into
/// `(primary 5 h, secondary weekly)`. Resets are relative to `now`.
pub fn parse_codex_usage(body: &str, now: i64) -> Result<(Reading, Reading), AiError> {
    let raw: RawCodexUsage =
        serde_json::from_str(body).map_err(|e| AiError::Response(e.to_string()))?;
    let limits = raw
        .rate_limit
        .ok_or_else(|| AiError::Response(NO_LIMIT_WINDOWS.to_owned()))?;
    Ok((
        codex_reading(limits.primary_window, PRIMARY_WINDOW, now)?,
        codex_reading(limits.secondary_window, SECONDARY_WINDOW, now)?,
    ))
}

#[derive(Deserialize)]
struct RawCodexAuth {
    tokens: RawCodexTokens,
}

#[derive(Deserialize)]
struct RawCodexTokens {
    access_token: String,
    account_id: String,
}

pub struct CodexCredentials {
    pub token: Secret,
    pub account_id: String,
}

impl fmt::Debug for CodexCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexCredentials")
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

/// Reads only `access_token` and `account_id`; refresh and id tokens are never kept.
pub fn parse_codex_credentials(text: &str) -> Result<CodexCredentials, AiError> {
    let raw: RawCodexAuth =
        serde_json::from_str(text).map_err(|e| AiError::Credentials(e.to_string()))?;
    Ok(CodexCredentials {
        token: Secret::new(raw.tokens.access_token),
        account_id: raw.tokens.account_id,
    })
}

pub trait CodexUsageApi {
    /// Response body of the usage endpoint; a read that spends no quota.
    fn get(&self, token: &Secret, account_id: &str) -> Result<String, ApiError>;
}

pub struct CodexProvider<C, A> {
    pub credentials: C,
    pub api: A,
}

impl<C: CredentialsSource, A: CodexUsageApi> AiProvider for CodexProvider<C, A> {
    fn detect(&self) -> bool {
        matches!(self.credentials.read(), Ok(Some(_)))
    }

    fn fetch(&self, now: i64) -> Result<ProviderStatus, AiError> {
        let text = self
            .credentials
            .read()?
            .ok_or_else(|| AiError::Credentials(CODEX_NOT_FOUND.to_owned()))?;
        let creds = parse_codex_credentials(&text)?;
        match self.api.get(&creds.token, &creds.account_id) {
            Ok(body) => {
                let (five_hour, seven_day) = parse_codex_usage(&body, now)?;
                Ok(ProviderStatus {
                    plan: None,
                    five_hour,
                    seven_day,
                    tip: String::new(),
                })
            }
            Err(ApiError::Unauthorized) => Err(AiError::Api(CODEX_RENEW_LOGIN_TIP.to_owned())),
            Err(ApiError::RateLimited { .. }) => Err(AiError::Api(API_RATE_LIMITED.to_owned())),
            Err(ApiError::Other(reason)) => Err(AiError::Api(reason)),
        }
    }
}

/// Runs one Codex poll. No `auth.json` means `CODEX_NOT_FOUND`, which the HUD does not list.
pub fn codex_state(provider: &impl AiProvider, now: i64) -> ClaudeState {
    if !provider.detect() {
        return Metric::unavailable(CODEX_NOT_FOUND);
    }
    match provider.fetch(now) {
        Ok(status) => Metric::Available(status),
        Err(e) => Metric::Unavailable(ai_error_reason(&e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USAGE_OK: &str = include_str!("../tests/fixtures/claude_usage_ok.json");
    const CREDS_PRO: &str = include_str!("../tests/fixtures/claude_credentials_pro.json");
    const CREDS_MAX: &str = include_str!("../tests/fixtures/claude_credentials_max.json");
    const SESSION: &str = include_str!("../tests/fixtures/claude_session.jsonl");
    const FAKE_TOKEN: &str = "fake-token-for-tests-0000";
    const NOW: i64 = 1_794_066_960; // 2026-11-07T15:56:00Z

    const CODEX_USAGE: &str = include_str!("../tests/fixtures/codex_usage_ok.json");
    const CODEX_AUTH: &str = include_str!("../tests/fixtures/codex_auth.json");
    const CODEX_TOKEN: &str = "fake-codex-token-0000";

    struct CodexApi(Result<String, ApiError>);
    impl CodexUsageApi for CodexApi {
        fn get(&self, _: &Secret, account_id: &str) -> Result<String, ApiError> {
            assert_eq!(account_id, "acct-fake-1");
            self.0.clone()
        }
    }

    fn codex(
        auth: Option<&'static str>,
        api: Result<String, ApiError>,
    ) -> CodexProvider<Creds, CodexApi> {
        CodexProvider {
            credentials: Creds(auth),
            api: CodexApi(api),
        }
    }

    #[test]
    fn codex_fixture_gives_23_percent_3h10_and_58_percent_5d() {
        let (five, seven) = parse_codex_usage(CODEX_USAGE, NOW).unwrap();
        let Reading::Official { percent, resets_at } = five else {
            panic!("not official");
        };
        assert_eq!(
            (percent, format_reset_in(resets_at, NOW).as_str()),
            (23.0, "3h10")
        );
        let Reading::Official { percent, resets_at } = seven else {
            panic!("not official");
        };
        assert_eq!(
            (percent, format_reset_in(resets_at, NOW).as_str()),
            (58.0, "5d")
        );
    }

    #[test]
    fn codex_without_windows_is_an_error_not_a_guess() {
        assert!(parse_codex_usage("{}", NOW).is_err());
        assert!(parse_codex_usage(r#"{"rate_limit":null}"#, NOW).is_err());
        let one = r#"{"rate_limit":{"primary_window":{"used_percent":1,"reset_after_seconds":1}}}"#;
        assert!(matches!(
            parse_codex_usage(one, NOW),
            Err(AiError::Response(_))
        ));
    }

    #[test]
    fn codex_without_auth_json_is_not_listed() {
        let p = codex(None, Ok(CODEX_USAGE.to_owned()));
        assert!(!p.detect());
        assert_eq!(codex_state(&p, NOW), Metric::unavailable(CODEX_NOT_FOUND));
    }

    #[test]
    fn codex_401_is_unavailable_with_renew_tip() {
        let p = codex(Some(CODEX_AUTH), Err(ApiError::Unauthorized));
        assert_eq!(
            codex_state(&p, NOW),
            Metric::unavailable(CODEX_RENEW_LOGIN_TIP)
        );
    }

    #[test]
    fn codex_official_response_is_available() {
        let p = codex(Some(CODEX_AUTH), Ok(CODEX_USAGE.to_owned()));
        let s = codex_state(&p, NOW);
        assert!(matches!(
            s.available().map(|s| &s.five_hour),
            Some(Reading::Official { percent, .. }) if *percent == 23.0
        ));
    }

    #[test]
    fn codex_token_never_appears_in_any_output() {
        let failing = codex(Some(CODEX_AUTH), Err(ApiError::Other("x".into())));
        let expired = codex(Some(CODEX_AUTH), Err(ApiError::Unauthorized));
        let outputs = [
            format!("{:?}", parse_codex_credentials(CODEX_AUTH)),
            format!("{:?}", parse_codex_credentials("{\"tokens\":")),
            format!("{:?}", codex_state(&failing, NOW)),
            format!("{:?}", expired.fetch(NOW)),
        ];
        assert!(outputs.iter().all(|o| !o.contains(CODEX_TOKEN)));
        assert_eq!(
            parse_codex_credentials(CODEX_AUTH).unwrap().token.expose(),
            CODEX_TOKEN
        );
    }

    struct Creds(Option<&'static str>);
    impl CredentialsSource for Creds {
        fn read(&self) -> Result<Option<String>, AiError> {
            Ok(self.0.map(str::to_owned))
        }
    }

    struct Api(Result<String, ApiError>);
    impl UsageApi for Api {
        fn get(&self, _: &Secret) -> Result<String, ApiError> {
            self.0.clone()
        }
    }

    struct Logs(TokenSums);
    impl TranscriptScanner for Logs {
        fn sums(&self, _: i64) -> Result<TokenSums, AiError> {
            Ok(self.0)
        }
    }

    fn provider(
        creds: Option<&'static str>,
        api: Result<String, ApiError>,
        sums: TokenSums,
    ) -> ClaudeProvider<Creds, Api, Logs> {
        ClaudeProvider::new(Creds(creds), Api(api), Logs(sums))
    }

    #[test]
    fn rfc3339_matches_the_known_clock() {
        assert_eq!(parse_rfc3339("2026-11-07T15:56:00Z"), Some(NOW));
        assert_eq!(parse_rfc3339("2026-11-07T15:56:00.123456+00:00"), Some(NOW));
        assert_eq!(parse_rfc3339("2026-11-07T12:56:00-03:00"), Some(NOW));
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("lixo"), None);
        assert_eq!(parse_rfc3339("2026-13-07T15:56:00Z"), None);
    }

    #[test]
    fn usage_fixture_gives_72_percent_1h48_and_41_percent_2d() {
        let (five, seven) = parse_usage(USAGE_OK).unwrap();
        let Reading::Official { percent, resets_at } = five else {
            panic!("not official");
        };
        assert_eq!(
            (percent, format_reset_in(resets_at, NOW).as_str()),
            (72.0, "1h48")
        );
        let Reading::Official { percent, resets_at } = seven else {
            panic!("not official");
        };
        assert_eq!(
            (percent, format_reset_in(resets_at, NOW).as_str()),
            (41.0, "2d")
        );
    }

    #[test]
    fn reset_format_covers_minutes_and_past() {
        assert_eq!(format_reset_in(NOW + 35 * 60, NOW), "35min");
        assert_eq!(format_reset_in(NOW + 5, NOW), "1min");
        assert_eq!(format_reset_in(NOW - 1, NOW), RESET_NOW);
        assert_eq!(format_reset_in(NOW + 3600, NOW), "1h00");
    }

    #[test]
    fn malformed_usage_is_an_error_not_a_guess() {
        assert!(matches!(parse_usage("{}"), Err(AiError::Response(_))));
        let bad_date = r#"{"five_hour":{"utilization":1,"resets_at":"x"},"seven_day":{"utilization":1,"resets_at":"x"}}"#;
        assert!(matches!(parse_usage(bad_date), Err(AiError::Response(_))));
    }

    #[test]
    fn subscription_type_gives_pro_and_max_badges() {
        assert_eq!(
            parse_credentials(CREDS_PRO).unwrap().plan.as_deref(),
            Some("PRO")
        );
        assert_eq!(
            parse_credentials(CREDS_MAX).unwrap().plan.as_deref(),
            Some("MAX")
        );
        assert_eq!(plan_label("team"), None);
    }

    #[test]
    fn token_is_redacted_in_debug_output() {
        let creds = parse_credentials(CREDS_PRO).unwrap();
        assert!(!format!("{creds:?}").contains(FAKE_TOKEN));
        assert_eq!(creds.token.expose(), FAKE_TOKEN);
    }

    #[test]
    fn unauthorized_falls_back_to_local_tokens_with_renew_tip() {
        let sums = TokenSums {
            five_hour: 1_200_000,
            seven_day: 9_000_000,
        };
        let s = provider(Some(CREDS_PRO), Err(ApiError::Unauthorized), sums)
            .fetch(NOW)
            .unwrap();
        assert_eq!(s.five_hour, Reading::Estimate { tokens: 1_200_000 });
        assert_eq!(s.seven_day, Reading::Estimate { tokens: 9_000_000 });
        assert_eq!(s.tip, CLAUDE_RENEW_LOGIN_TIP);
        assert_eq!(s.plan.as_deref(), Some("PRO"));
    }

    #[test]
    fn network_failure_also_estimates_with_its_own_reason() {
        let s = provider(
            Some(CREDS_MAX),
            Err(ApiError::Other("no connection".into())),
            TokenSums::default(),
        )
        .fetch(NOW)
        .unwrap();
        assert_eq!(s.tip, "no connection");
        assert_eq!(s.plan.as_deref(), Some("MAX"));
    }

    #[test]
    fn official_response_wins_and_has_no_tip() {
        let s = provider(
            Some(CREDS_PRO),
            Ok(USAGE_OK.to_owned()),
            TokenSums::default(),
        )
        .fetch(NOW)
        .unwrap();
        assert!(matches!(s.five_hour, Reading::Official { percent, .. } if percent == 72.0));
        assert!(s.tip.is_empty());
    }

    #[test]
    fn missing_credentials_means_not_detected_and_fetch_fails() {
        let p = provider(None, Err(ApiError::Unauthorized), TokenSums::default());
        assert!(!p.detect());
        assert!(p.fetch(NOW).is_err());
        assert!(provider(
            Some(CREDS_PRO),
            Err(ApiError::Unauthorized),
            TokenSums::default()
        )
        .detect());
    }

    #[test]
    fn transcript_tally_counts_windows_and_skips_noise() {
        let mut sums = TokenSums::default();
        for line in SESSION.lines() {
            tally_line(&mut sums, line, NOW);
        }
        // 15:30 is in both windows (100+50+1000, cache reads excluded); 08:00 only in 7 d.
        assert_eq!(sums.five_hour, 1150);
        assert_eq!(sums.seven_day, 1150 + 30);
    }

    #[test]
    fn no_transcripts_in_five_hours_is_zero() {
        let mut sums = TokenSums::default();
        tally_line(&mut sums, SESSION.lines().nth(2).unwrap_or(""), NOW);
        assert_eq!(sums.five_hour, 0);
    }

    #[test]
    fn poll_maps_missing_tool_and_errors_to_unavailable() {
        let none = provider(None, Err(ApiError::Unauthorized), TokenSums::default());
        assert_eq!(
            claude_state(&none, NOW),
            Metric::unavailable(CLAUDE_NOT_FOUND)
        );
        let bad = provider(Some("{"), Err(ApiError::Unauthorized), TokenSums::default());
        assert!(matches!(claude_state(&bad, NOW), Metric::Unavailable(_)));
        let ok = provider(
            Some(CREDS_PRO),
            Ok(USAGE_OK.to_owned()),
            TokenSums::default(),
        );
        assert!(claude_state(&ok, NOW).available().is_some());
    }

    #[test]
    fn no_output_of_a_full_run_contains_the_token() {
        let failing = provider(
            Some(CREDS_PRO),
            Err(ApiError::Other("network failure".into())),
            TokenSums::default(),
        );
        let outputs = [
            format!("{:?}", failing.fetch(NOW)),
            format!("{:?}", parse_credentials(CREDS_PRO)),
            format!("{:?}", parse_credentials("{\"claudeAiOauth\":")),
            format!(
                "{:?}",
                provider(None, Err(ApiError::Unauthorized), TokenSums::default()).fetch(NOW)
            ),
        ];
        assert!(outputs.iter().all(|o| !o.contains(FAKE_TOKEN)));
    }

    /// Answers one scripted result per call and counts the calls.
    struct Scripted {
        answers: std::cell::RefCell<std::collections::VecDeque<Result<String, ApiError>>>,
        calls: std::cell::Cell<u32>,
    }
    impl UsageApi for &Scripted {
        fn get(&self, _: &Secret) -> Result<String, ApiError> {
            self.calls.set(self.calls.get() + 1);
            self.answers
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(ApiError::Other("sem resposta".into())))
        }
    }

    fn scripted(answers: Vec<Result<String, ApiError>>) -> Scripted {
        Scripted {
            answers: std::cell::RefCell::new(answers.into()),
            calls: std::cell::Cell::new(0),
        }
    }

    fn limited(retry_after: Option<u64>) -> Result<String, ApiError> {
        Err(ApiError::RateLimited { retry_after })
    }

    fn claude<'a>(api: &'a Scripted) -> ClaudeProvider<Creds, &'a Scripted, Logs> {
        let sums = TokenSums {
            five_hour: 3_500_000,
            seven_day: 9_000_000,
        };
        ClaudeProvider::new(Creds(Some(CREDS_PRO)), api, Logs(sums))
    }

    #[test]
    fn rate_limit_without_a_prior_reading_shows_the_estimate_with_the_reason() {
        let api = scripted(vec![limited(None)]);
        let s = claude(&api).fetch(NOW).unwrap();
        assert_eq!(s.five_hour, Reading::Estimate { tokens: 3_500_000 });
        assert_eq!(s.tip, API_RATE_LIMITED);
        assert_eq!(s.plan.as_deref(), Some("PRO"));
    }

    #[test]
    fn rate_limit_keeps_the_last_reading_marked_stale_with_its_age() {
        let api = scripted(vec![Ok(USAGE_OK.to_owned()), limited(None)]);
        let p = claude(&api);
        p.fetch(NOW).unwrap();
        let s = p.fetch(NOW + 12 * 60 + CLAUDE_POLL_SECS).unwrap();
        assert!(matches!(s.five_hour, Reading::Official { percent, .. } if percent == 72.0));
        assert!(matches!(s.seven_day, Reading::Official { percent, .. } if percent == 41.0));
        assert_eq!(s.tip, "dados de há 17 min — API limitou as consultas");
    }

    #[test]
    fn stale_age_grows_while_the_backoff_holds_the_api_calls() {
        let api = scripted(vec![Ok(USAGE_OK.to_owned()), limited(None)]);
        let p = claude(&api);
        p.fetch(NOW).unwrap();
        p.fetch(NOW + 300).unwrap();
        let s = p.fetch(NOW + 300 + 120).unwrap();
        assert_eq!(api.calls.get(), 2);
        assert_eq!(s.tip, "dados de há 7 min — API limitou as consultas");
    }

    #[test]
    fn transient_errors_also_keep_the_last_reading() {
        let api = scripted(vec![
            Ok(USAGE_OK.to_owned()),
            Err(ApiError::Other("sem conexão".into())),
        ]);
        let p = claude(&api);
        p.fetch(NOW).unwrap();
        let s = p.fetch(NOW + 600).unwrap();
        assert!(matches!(s.five_hour, Reading::Official { .. }));
        assert_eq!(s.tip, "dados de há 10 min — sem conexão");
    }

    #[test]
    fn expired_login_still_falls_back_to_the_estimate_even_after_a_good_reading() {
        let api = scripted(vec![Ok(USAGE_OK.to_owned()), Err(ApiError::Unauthorized)]);
        let p = claude(&api);
        p.fetch(NOW).unwrap();
        let s = p.fetch(NOW + 600).unwrap();
        assert!(matches!(s.five_hour, Reading::Estimate { .. }));
        assert_eq!(s.tip, CLAUDE_RENEW_LOGIN_TIP);
    }

    #[test]
    fn backoff_schedule_is_5_10_20_30_30_minutes() {
        let minutes = |streak, ra| rate_limit_delay(streak, ra) / 60;
        let schedule: Vec<i64> = (1..=5).map(|n| minutes(n, None)).collect();
        assert_eq!(schedule, [5, 10, 20, 30, 30]);
        assert_eq!(minutes(u32::MAX, None), 30);
    }

    #[test]
    fn retry_after_wins_only_when_larger() {
        assert_eq!(rate_limit_delay(1, Some(3600)), 3600);
        assert_eq!(rate_limit_delay(3, Some(60)), 20 * 60);
        assert_eq!(rate_limit_delay(1, Some(u64::MAX)), i64::MAX);
    }

    #[test]
    fn the_api_is_not_called_again_before_the_poll_interval() {
        let api = scripted(vec![Ok(USAGE_OK.to_owned())]);
        let p = claude(&api);
        for minute in 0..5 {
            let s = p.fetch(NOW + minute * 60).unwrap();
            assert!(s.tip.is_empty());
            assert!(matches!(s.five_hour, Reading::Official { .. }));
        }
        assert_eq!(api.calls.get(), 1);
        p.fetch(NOW + CLAUDE_POLL_SECS).unwrap();
        assert_eq!(api.calls.get(), 2);
    }

    #[test]
    fn consecutive_rate_limits_follow_the_schedule_and_success_resets_it() {
        let api = scripted(vec![
            limited(None),
            limited(None),
            limited(Some(45 * 60)),
            Ok(USAGE_OK.to_owned()),
            limited(None),
        ]);
        let p = claude(&api);
        let mut t = NOW;
        for wait in [5 * 60, 10 * 60, 45 * 60] {
            p.fetch(t).unwrap();
            let calls = api.calls.get();
            p.fetch(t + wait - 1).unwrap();
            assert_eq!(api.calls.get(), calls, "must wait {wait} s");
            t += wait;
        }
        let ok = p.fetch(t).unwrap();
        assert!(ok.tip.is_empty());
        // After the success the next 429 starts again at 5 min, not at 20.
        let t = t + CLAUDE_POLL_SECS;
        p.fetch(t).unwrap();
        let calls = api.calls.get();
        p.fetch(t + 5 * 60 - 1).unwrap();
        assert_eq!(api.calls.get(), calls);
        p.fetch(t + 5 * 60).unwrap();
        assert_eq!(api.calls.get(), calls + 1);
    }

    #[test]
    fn stale_tip_formats_minutes_then_hours() {
        assert_eq!(stale_tip(5, "x"), "dados de há 1 min — x");
        assert_eq!(stale_tip(59 * 60, "x"), "dados de há 59 min — x");
        assert_eq!(stale_tip(125 * 60, "x"), "dados de há 2h05 — x");
    }

    #[test]
    fn codex_maps_rate_limit_to_a_reason() {
        let p = codex(
            Some(CODEX_AUTH),
            Err(ApiError::RateLimited { retry_after: None }),
        );
        assert_eq!(p.fetch(NOW), Err(AiError::Api(API_RATE_LIMITED.to_owned())));
    }
}
