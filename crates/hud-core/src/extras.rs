//! Gemini CLI, Cursor and the presence-only tools (Copilot, OpenCode, Ollama).
//!
//! Gemini has no read-only quota GET (its quota call is a POST that needs a refreshed token), so
//! requests are counted from local session logs and marked `~`. Cursor's monthly usage sits behind
//! a trait. No credential is read here: detection is a file/folder existence probe.

use crate::ai::{format_reset_in, parse_rfc3339, AiError, ApiError};
use crate::alerts::Level;
use crate::metrics::Metric;
use crate::ui_text::{
    cursor_month_percent, gemini_requests, invalid_date, CURSOR_NOT_FOUND, CURSOR_RENEW_LOGIN_TIP,
    GEMINI_ESTIMATE_TIP, GEMINI_NOT_FOUND, NOT_RUNNING, PRESENT, RUNNING,
};
use crate::view::{Field, Limit, DASH};
use serde::Deserialize;
use std::collections::HashSet;

/// Free daily request limit of Gemini CLI with a Google login (public docs); the real plan is unknown.
pub const GEMINI_DAILY_LIMIT: u64 = 1000;
const DAY: i64 = 86_400;

#[derive(Debug, Clone, PartialEq)]
pub enum Usage {
    Requests {
        used: u64,
        limit: u64,
        estimated: bool,
    },
    Percent {
        percent: f32,
        resets_at: i64,
    },
}

pub type UsageState = Metric<Usage>;

/// Whether a tool is installed on this machine.
pub trait Probe {
    fn present(&self) -> bool;
}

pub trait RequestCounter {
    fn last_day(&self, now: i64) -> Result<u64, AiError>;
}

pub trait CursorUsageApi {
    fn get(&self) -> Result<String, ApiError>;
}

pub fn gemini_state(probe: &impl Probe, counter: &impl RequestCounter, now: i64) -> UsageState {
    if !probe.present() {
        return Metric::unavailable(GEMINI_NOT_FOUND);
    }
    match counter.last_day(now) {
        Ok(used) => Metric::Available(Usage::Requests {
            used,
            limit: GEMINI_DAILY_LIMIT,
            estimated: true,
        }),
        Err(e) => Metric::Unavailable(e.to_string()),
    }
}

pub fn gemini_cutoff(now: i64) -> i64 {
    now - DAY
}

#[derive(Deserialize)]
struct GeminiLine {
    id: String,
    timestamp: String,
    #[serde(rename = "type")]
    kind: String,
}

/// A response is rewritten in the log as it gets tool calls, so requests are counted by unique id.
pub fn tally_gemini_line(seen: &mut HashSet<String>, line: &str, now: i64) {
    if !line.contains("\"gemini\"") {
        return;
    }
    let Ok(parsed) = serde_json::from_str::<GeminiLine>(line) else {
        return;
    };
    let Some(ts) = parse_rfc3339(&parsed.timestamp) else {
        return;
    };
    if parsed.kind == "gemini" && (0..=DAY).contains(&(now - ts)) {
        seen.insert(parsed.id);
    }
}

#[derive(Deserialize)]
struct RawCursor {
    #[serde(rename = "billingCycleEnd")]
    cycle_end: String,
    #[serde(rename = "individualUsage")]
    usage: RawCursorUsage,
}

#[derive(Deserialize)]
struct RawCursorUsage {
    plan: RawCursorPlan,
}

#[derive(Deserialize)]
struct RawCursorPlan {
    #[serde(rename = "totalPercentUsed")]
    percent: f32,
}

pub fn parse_cursor_usage(body: &str) -> Result<Usage, AiError> {
    let raw: RawCursor =
        serde_json::from_str(body).map_err(|e| AiError::Response(e.to_string()))?;
    let resets_at = parse_rfc3339(&raw.cycle_end)
        .ok_or_else(|| AiError::Response(invalid_date(&raw.cycle_end)))?;
    Ok(Usage::Percent {
        percent: raw.usage.plan.percent,
        resets_at,
    })
}

pub fn cursor_state(probe: &impl Probe, api: &impl CursorUsageApi) -> UsageState {
    if !probe.present() {
        return Metric::unavailable(CURSOR_NOT_FOUND);
    }
    match api.get() {
        Ok(body) => match parse_cursor_usage(&body) {
            Ok(usage) => Metric::Available(usage),
            Err(e) => Metric::Unavailable(e.to_string()),
        },
        Err(ApiError::Unauthorized) => Metric::unavailable(CURSOR_RENEW_LOGIN_TIP),
        Err(ApiError::RateLimited { .. }) => Metric::unavailable(crate::ui_text::API_RATE_LIMITED),
        Err(ApiError::Other(reason)) => Metric::Unavailable(reason),
    }
}

/// A provider row is hidden only when the tool itself is missing; any other failure shows `—`.
pub fn listed(state: &UsageState, not_found: &str) -> bool {
    !matches!(state, Metric::Unavailable(r) if r == not_found)
}

pub fn usage_limit(state: &UsageState, now: i64) -> Limit {
    match state {
        Metric::Available(Usage::Requests {
            used,
            limit,
            estimated,
        }) => Limit {
            pct: Field {
                text: gemini_requests(*used, *limit, *estimated),
                tip: if *estimated { GEMINI_ESTIMATE_TIP } else { "" }.to_owned(),
                level: Level::Ok,
            },
            reset: String::new(),
            percent: (*used as f64 / *limit as f64 * 100.0).min(100.0) as f32,
        },
        Metric::Available(Usage::Percent { percent, resets_at }) => Limit {
            pct: Field {
                text: cursor_month_percent(*percent),
                tip: String::new(),
                level: Level::Ok,
            },
            reset: format_reset_in(*resets_at, now),
            percent: *percent,
        },
        Metric::Unavailable(reason) => Limit {
            pct: Field {
                text: DASH.to_owned(),
                tip: reason.clone(),
                level: Level::Ok,
            },
            ..Limit::default()
        },
    }
}

/// A tool shown without limits: just whether it is there (or running).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceRow {
    pub name: &'static str,
    pub status: &'static str,
}

/// Copilot and OpenCode are listed only when their folder exists; Ollama is always listed.
pub fn presence_rows(copilot: bool, opencode: bool, ollama_up: bool) -> Vec<PresenceRow> {
    let mut rows = Vec::new();
    if copilot {
        rows.push(PresenceRow {
            name: "COPILOT",
            status: PRESENT,
        });
    }
    if opencode {
        rows.push(PresenceRow {
            name: "OPENCODE",
            status: PRESENT,
        });
    }
    rows.push(PresenceRow {
        name: "OLLAMA",
        status: if ollama_up { RUNNING } else { NOT_RUNNING },
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_text::CURSOR_UNAVAILABLE;

    const NOW: i64 = 1_794_066_960; // 2026-11-07T15:56:00Z
    const CURSOR: &str = include_str!("../tests/fixtures/cursor_usage.json");
    const GEMINI_LOG: &str = include_str!("../tests/fixtures/gemini_session.jsonl");

    struct Is(bool);
    impl Probe for Is {
        fn present(&self) -> bool {
            self.0
        }
    }

    struct Count(Result<u64, &'static str>);
    impl RequestCounter for Count {
        fn last_day(&self, _: i64) -> Result<u64, AiError> {
            self.0.map_err(|e| AiError::Logs(e.to_owned()))
        }
    }

    struct Cursor(Result<String, ApiError>);
    impl CursorUsageApi for Cursor {
        fn get(&self) -> Result<String, ApiError> {
            self.0.clone()
        }
    }

    #[test]
    fn gemini_120_of_1000_shows_requests_and_12_percent_marked_estimate() {
        let s = gemini_state(&Is(true), &Count(Ok(120)), NOW);
        let l = usage_limit(&s, NOW);
        assert_eq!(l.pct.text, gemini_requests(120, 1000, true));
        assert_eq!(l.percent, 12.0);
        assert!(!l.pct.tip.is_empty());
    }

    #[test]
    fn official_requests_have_no_tilde() {
        let s = Metric::Available(Usage::Requests {
            used: 120,
            limit: 1000,
            estimated: false,
        });
        assert_eq!(
            usage_limit(&s, NOW).pct.text,
            gemini_requests(120, 1000, false)
        );
    }

    #[test]
    fn gemini_log_counts_unique_responses_of_the_last_24h() {
        let mut seen = HashSet::new();
        for line in GEMINI_LOG.lines() {
            tally_gemini_line(&mut seen, line, NOW);
        }
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn gemini_is_unlisted_without_oauth_file_and_dash_when_logs_fail() {
        let s = gemini_state(&Is(false), &Count(Ok(1)), NOW);
        assert!(!listed(&s, GEMINI_NOT_FOUND));
        let s = gemini_state(&Is(true), &Count(Err("denied")), NOW);
        assert!(listed(&s, GEMINI_NOT_FOUND));
        let l = usage_limit(&s, NOW);
        assert_eq!(l.pct.text, "—");
        assert!(l.pct.tip.contains("denied"));
    }

    #[test]
    fn cursor_64_percent_resetting_in_18_days_is_mes_64_18d() {
        let s = cursor_state(&Is(true), &Cursor(Ok(CURSOR.to_owned())));
        let l = usage_limit(&s, NOW);
        assert_eq!(
            (l.pct.text.as_str(), l.reset.as_str()),
            (cursor_month_percent(64.0).as_str(), "18d")
        );
        assert_eq!(l.percent, 64.0);
    }

    #[test]
    fn cursor_without_a_source_is_dash_with_reason_and_not_a_guess() {
        let s = cursor_state(
            &Is(true),
            &Cursor(Err(ApiError::Other(CURSOR_UNAVAILABLE.into()))),
        );
        assert!(listed(&s, CURSOR_NOT_FOUND));
        let l = usage_limit(&s, NOW);
        assert_eq!(l.pct.text, "—");
        assert_eq!(l.pct.tip, CURSOR_UNAVAILABLE);
        assert!(!listed(
            &cursor_state(&Is(false), &Cursor(Ok(CURSOR.to_owned()))),
            CURSOR_NOT_FOUND
        ));
        assert!(matches!(
            cursor_state(&Is(true), &Cursor(Ok("{}".into()))),
            Metric::Unavailable(_)
        ));
    }

    #[test]
    fn copilot_folder_is_listed_present_and_ollama_follows_the_probe() {
        let up = presence_rows(true, false, true);
        assert_eq!(up.len(), 2);
        assert_eq!((up[0].name, up[0].status), ("COPILOT", PRESENT));
        assert_eq!((up[1].name, up[1].status), ("OLLAMA", RUNNING));
        let down = presence_rows(false, true, false);
        assert_eq!(down[0].name, "OPENCODE");
        assert_eq!(down[1].status, NOT_RUNNING);
    }
}
