//! Use case: turn the latest samples into everything the HUD, the mini bar and the tray show,
//! and decide when a critical alert must pull the user's attention.

use crate::alerts::{claude_level, evaluate, Level};
use crate::config::{MiniBarItem, Thresholds};
use crate::extras::{self, usage_limit, PresenceRow, UsageState};
use crate::metrics::{Metric, Snapshot};
use crate::ui_text::{CURSOR_NOT_FOUND, GEMINI_NOT_FOUND, WAITING_FIRST_READING};
use crate::view::{
    build_alerted, claude_view, codex_listed, codex_view, minibar_chips, tray_tooltip, Chip,
    ClaudeState, ClaudeView, HudView, Limit,
};

/// Latest state of every AI provider and tool.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderStates {
    pub claude: ClaudeState,
    pub codex: ClaudeState,
    pub gemini: UsageState,
    pub cursor: UsageState,
    pub presence: Vec<PresenceRow>,
}

impl ProviderStates {
    /// Before the first poll: Claude and Codex wait, Gemini and Cursor are not listed.
    pub fn waiting() -> Self {
        Self {
            claude: Metric::unavailable(WAITING_FIRST_READING),
            codex: Metric::unavailable(WAITING_FIRST_READING),
            gemini: Metric::unavailable(GEMINI_NOT_FOUND),
            cursor: Metric::unavailable(CURSOR_NOT_FOUND),
            presence: Vec::new(),
        }
    }

    /// Every provider unavailable for the same `reason`.
    pub fn all_unavailable(reason: &str, presence: Vec<PresenceRow>) -> Self {
        Self {
            claude: Metric::unavailable(reason),
            codex: Metric::unavailable(reason),
            gemini: Metric::unavailable(reason),
            cursor: Metric::unavailable(reason),
            presence,
        }
    }
}

/// Everything the screens show for one refresh.
#[derive(Debug, Clone, PartialEq)]
pub struct DashboardView {
    pub hud: HudView,
    pub claude: ClaudeView,
    pub codex: ClaudeView,
    pub codex_listed: bool,
    pub gemini: Limit,
    pub gemini_listed: bool,
    pub cursor: Limit,
    pub cursor_listed: bool,
    pub presence: Vec<PresenceRow>,
    pub chips: Vec<Chip>,
    pub tray_tooltip: String,
}

/// Result of one refresh: the view and whether a new critical alert asks for attention.
#[derive(Debug, Clone, PartialEq)]
pub struct Refresh {
    pub view: DashboardView,
    pub escalate: bool,
}

pub struct Dashboard {
    providers: ProviderStates,
    was_crit: bool,
}

impl Default for Dashboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Dashboard {
    pub fn new() -> Self {
        Self {
            providers: ProviderStates::waiting(),
            was_crit: false,
        }
    }

    /// What the windows show before the first sample arrives.
    pub fn waiting_view(&self, items: &[MiniBarItem], thresholds: &Thresholds) -> DashboardView {
        let waiting = Snapshot::unavailable(WAITING_FIRST_READING);
        DashboardView {
            codex_listed: false,
            ..build_view(&waiting, &self.providers, items, thresholds, 0)
        }
    }

    /// Absorbs a sample (and fresh provider states, when a poll finished) at time `now`.
    pub fn refresh(
        &mut self,
        snapshot: &Snapshot,
        fresh: Option<ProviderStates>,
        items: &[MiniBarItem],
        thresholds: &Thresholds,
        now: i64,
    ) -> Refresh {
        if let Some(states) = fresh {
            self.providers = states;
        }
        let crit = evaluate(snapshot, thresholds).any_crit()
            || claude_level(&self.providers.claude, thresholds) == Level::Crit;
        let escalate = crit && !self.was_crit;
        self.was_crit = crit;
        Refresh {
            view: build_view(snapshot, &self.providers, items, thresholds, now),
            escalate,
        }
    }
}

fn build_view(
    snapshot: &Snapshot,
    providers: &ProviderStates,
    items: &[MiniBarItem],
    thresholds: &Thresholds,
    now: i64,
) -> DashboardView {
    let claude = claude_view(&providers.claude, now, thresholds);
    let codex = codex_view(&providers.codex, now);
    DashboardView {
        hud: build_alerted(snapshot, thresholds),
        chips: minibar_chips(items, snapshot, thresholds, &claude, &codex),
        claude,
        codex,
        codex_listed: codex_listed(&providers.codex),
        gemini: usage_limit(&providers.gemini, now),
        gemini_listed: extras::listed(&providers.gemini, GEMINI_NOT_FOUND),
        cursor: usage_limit(&providers.cursor, now),
        cursor_listed: extras::listed(&providers.cursor, CURSOR_NOT_FOUND),
        presence: providers.presence.clone(),
        tray_tooltip: tray_tooltip(snapshot),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{ProviderStatus, Reading};
    use crate::config::Config;
    use crate::position::Size;
    use crate::ui_text::CODEX_NOT_FOUND;

    const NOW: i64 = 1_794_066_960;
    const SCREEN: Size = Size {
        width: 1920,
        height: 1080,
    };

    fn config() -> Config {
        Config::first_run(SCREEN)
    }

    fn sample(cpu_total: f32) -> Snapshot {
        let mut s = Snapshot::unavailable("sample");
        s.unavailable = None;
        s.cpu.total = cpu_total;
        s
    }

    fn claude_at(percent: f32) -> ClaudeState {
        Metric::Available(ProviderStatus {
            plan: Some("PRO".to_owned()),
            five_hour: Reading::Official {
                percent,
                resets_at: NOW + 3600,
            },
            seven_day: Reading::Official {
                percent: 1.0,
                resets_at: NOW + 86_400,
            },
            tip: String::new(),
        })
    }

    fn providers_with(claude: ClaudeState) -> ProviderStates {
        ProviderStates {
            claude,
            ..ProviderStates::waiting()
        }
    }

    fn refresh(
        dashboard: &mut Dashboard,
        snapshot: &Snapshot,
        fresh: Option<ProviderStates>,
    ) -> Refresh {
        let c = config();
        dashboard.refresh(snapshot, fresh, &c.minibar.items, &c.thresholds, NOW)
    }

    #[test]
    fn waiting_view_hides_codex_gemini_and_cursor_and_shows_the_waiting_reason() {
        let c = config();
        let view = Dashboard::new().waiting_view(&c.minibar.items, &c.thresholds);
        assert!(!view.codex_listed && !view.gemini_listed && !view.cursor_listed);
        assert_eq!(view.hud.cpu_total.tip, WAITING_FIRST_READING);
        assert_eq!(view.claude.five.pct.tip, WAITING_FIRST_READING);
        assert_eq!(view.chips.len(), c.minibar.items.len());
    }

    #[test]
    fn refresh_builds_the_hud_chips_and_tray_text_from_the_sample() {
        let mut dashboard = Dashboard::new();
        let out = refresh(&mut dashboard, &sample(37.0), None);
        assert_eq!(out.view.hud.cpu_total.text, "37%");
        assert!(out.view.tray_tooltip.starts_with("CPU 37%"));
        assert_eq!(out.view.chips[0].value.text, "37%");
    }

    #[test]
    fn providers_are_remembered_until_a_newer_poll_arrives() {
        let mut dashboard = Dashboard::new();
        refresh(
            &mut dashboard,
            &sample(1.0),
            Some(providers_with(claude_at(40.0))),
        );
        let out = refresh(&mut dashboard, &sample(1.0), None);
        assert_eq!(out.view.claude.five.pct.text, "40%");
        assert_eq!(out.view.claude.plan, "PRO");
    }

    #[test]
    fn codex_row_is_hidden_only_while_its_tool_is_missing() {
        let mut dashboard = Dashboard::new();
        let missing = ProviderStates {
            codex: Metric::unavailable(CODEX_NOT_FOUND),
            ..ProviderStates::waiting()
        };
        assert!(
            !refresh(&mut dashboard, &sample(1.0), Some(missing))
                .view
                .codex_listed
        );
        let waiting = ProviderStates::waiting();
        assert!(
            refresh(&mut dashboard, &sample(1.0), Some(waiting))
                .view
                .codex_listed
        );
    }

    #[test]
    fn a_critical_cpu_escalates_once_until_it_clears() {
        let mut dashboard = Dashboard::new();
        assert!(!refresh(&mut dashboard, &sample(10.0), None).escalate);
        assert!(refresh(&mut dashboard, &sample(99.0), None).escalate);
        assert!(!refresh(&mut dashboard, &sample(99.0), None).escalate);
        assert!(!refresh(&mut dashboard, &sample(10.0), None).escalate);
        assert!(refresh(&mut dashboard, &sample(99.0), None).escalate);
    }

    #[test]
    fn a_critical_claude_limit_escalates() {
        let mut dashboard = Dashboard::new();
        let hot = providers_with(claude_at(99.0));
        assert!(refresh(&mut dashboard, &sample(1.0), Some(hot)).escalate);
    }

    #[test]
    fn all_unavailable_gives_every_provider_the_same_reason() {
        let states = ProviderStates::all_unavailable("why", Vec::new());
        for state in [&states.claude, &states.codex] {
            assert_eq!(*state, Metric::unavailable("why"));
        }
        assert_eq!(states.gemini, Metric::unavailable("why"));
        assert_eq!(states.cursor, Metric::unavailable("why"));
    }
}
