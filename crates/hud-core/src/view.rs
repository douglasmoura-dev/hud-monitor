//! Display text for the HUD, built from a `Snapshot`. Unavailable metrics become `—` plus the reason.

use crate::ai::{format_reset_in, ProviderStatus, Reading};
use crate::alerts::{evaluate, level_of, Level};
use crate::config::{MiniBarItem, Orientation, Threshold, Thresholds};
use crate::format::{rate, size, used_total_gb};
use crate::metrics::{CpuMetrics, Metric, Snapshot};
use crate::position::Size;
use crate::ui_text::{cores_above_half, net_today, CODEX_NOT_FOUND, DISK_CHIP_LABEL, NO_PROCESSES};

pub const DASH: &str = "—";
pub const BAR_RED: u32 = 0x00E2_4B4A;
pub const BAR_GREEN: u32 = 0x001D_9E75;
pub const ALERT_WARN: u32 = 0x00EF_9F27;
pub const ALERT_CRIT: u32 = BAR_RED;
const HOT_THREAD_PERCENT: f32 = 80.0;

/// Text shown in the HUD and the tooltip explaining why it is `—` (empty when available).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Field {
    pub text: String,
    pub tip: String,
    pub level: Level,
}

impl Field {
    fn ok(text: String) -> Self {
        Self {
            text,
            tip: String::new(),
            level: Level::Ok,
        }
    }

    fn missing(reason: &str) -> Self {
        Self {
            text: DASH.to_string(),
            tip: reason.to_string(),
            level: Level::Ok,
        }
    }

    fn of<T>(m: &Metric<T>, fmt: impl FnOnce(&T) -> String) -> Self {
        match m {
            Metric::Available(v) => Self::ok(fmt(v)),
            Metric::Unavailable(reason) => Self::missing(reason),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoreBar {
    pub percent: f32,
    /// `0xRRGGBB`.
    pub color: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HudView {
    pub cpu_total: Field,
    pub cpu_freq: Field,
    pub cpu_temp: Field,
    pub cpu_cores: Field,
    pub core_bars: Vec<CoreBar>,
    pub ram: Field,
    pub ram_percent: f32,
    pub ram_top: Field,
    pub gpu_intel: Field,
    pub gpu_nvidia: Field,
    pub gpu_vram: Field,
    pub disk: Field,
    pub disk_percent: f32,
    pub disk_io: Field,
    pub net_down: Field,
    pub net_up: Field,
    pub net_today: Field,
    pub ping: Field,
}

pub fn bar_color(percent: f32) -> u32 {
    if percent > HOT_THREAD_PERCENT {
        BAR_RED
    } else {
        BAR_GREEN
    }
}

/// Display colour of an alert level, `None` when there is nothing to flag.
pub fn level_color(level: Level) -> Option<u32> {
    match level {
        Level::Ok => None,
        Level::Warn => Some(ALERT_WARN),
        Level::Crit => Some(ALERT_CRIT),
    }
}

/// `build` plus alert levels on the fields that have thresholds.
pub fn build_alerted(s: &Snapshot, t: &Thresholds) -> HudView {
    let mut v = build(s);
    let a = evaluate(s, t);
    v.cpu_total.level = a.cpu;
    v.ram.level = a.ram;
    v.cpu_temp.level = a.temp;
    v.ping.level = a.ping;
    v
}

fn core_label(cpu: &CpuMetrics) -> Field {
    let threads = cpu.threads.len();
    let above = cores_above_half(cpu.above_50);
    match &cpu.cores {
        Metric::Available(c) => Field::ok(format!("{c}C/{threads}T · {above}")),
        Metric::Unavailable(reason) => Field {
            text: format!("{DASH}/{threads}T · {above}"),
            tip: reason.clone(),
            level: Level::Ok,
        },
    }
}

fn top_processes(s: &Snapshot) -> Field {
    if s.mem.top.is_empty() {
        return Field::missing(NO_PROCESSES);
    }
    let parts: Vec<String> = s
        .mem
        .top
        .iter()
        .map(|p| format!("{} {}", p.name, size(p.bytes)))
        .collect();
    Field::ok(parts.join(" · "))
}

fn percent_of(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64 * 100.0) as f32
    }
}

fn degraded(reason: &str) -> HudView {
    let f = || Field::missing(reason);
    HudView {
        cpu_total: f(),
        cpu_freq: f(),
        cpu_temp: f(),
        cpu_cores: f(),
        ram: f(),
        ram_top: f(),
        gpu_intel: f(),
        gpu_nvidia: f(),
        gpu_vram: f(),
        disk: f(),
        disk_io: f(),
        net_down: f(),
        net_up: f(),
        net_today: f(),
        ping: f(),
        ..HudView::default()
    }
}

pub fn build(s: &Snapshot) -> HudView {
    if let Some(reason) = &s.unavailable {
        return degraded(reason);
    }
    let nv = &s.gpu.nvidia;
    HudView {
        cpu_total: Field::ok(format!("{:.0}%", s.cpu.total)),
        cpu_freq: Field::of(&s.cpu.freq_mhz, |mhz| {
            format!("{:.1}GHz", *mhz as f64 / 1000.0)
        }),
        cpu_temp: Field::of(&s.thermal.cpu_c, |c| format!("{c}°C")),
        cpu_cores: core_label(&s.cpu),
        core_bars: s
            .cpu
            .threads
            .iter()
            .map(|&percent| CoreBar {
                percent,
                color: bar_color(percent),
            })
            .collect(),
        ram: Field::ok(used_total_gb(s.mem.used, s.mem.total)),
        ram_percent: f32::from(s.mem.percent),
        ram_top: top_processes(s),
        gpu_intel: Field::of(&s.gpu.intel_usage_percent, |u| format!("{u:.0}%")),
        gpu_nvidia: Field::of(&nv.usage_percent, |u| format!("{u}%")),
        gpu_vram: Field::of(&nv.vram, |&(used, total)| used_total_gb(used, total)),
        disk: Field::of(&s.disk.usage, |&(used, total)| used_total_gb(used, total)),
        disk_percent: s
            .disk
            .usage
            .available()
            .map_or(0.0, |&(used, total)| percent_of(used, total)),
        disk_io: Field::of(&s.disk.io, |io| {
            format!("R {} W {}", rate(io.read_per_sec), rate(io.write_per_sec))
        }),
        net_down: Field::ok(rate(s.net.down)),
        net_up: Field::ok(rate(s.net.up)),
        net_today: Field::of(&s.net.day_total, |&b| net_today(&size(b))),
        ping: Field::of(&s.ping_ms, |ms| format!("{ms} ms")),
    }
}

/// Claude limits as last fetched; `Unavailable` carries the reason shown as `—`.
pub type ClaudeState = Metric<ProviderStatus>;

/// One limit window row: text, meter fill and time to reset.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Limit {
    pub pct: Field,
    pub reset: String,
    pub percent: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClaudeView {
    /// `PRO`, `MAX` or empty.
    pub plan: String,
    pub five: Limit,
    pub week: Limit,
}

fn tokens_text(tokens: u64) -> String {
    match tokens {
        0 => "~0%".to_owned(),
        1..=999 => format!("~{tokens} tok"),
        1000..=999_999 => format!("~{:.1}k tok", tokens as f64 / 1e3),
        _ => format!("~{:.1}M tok", tokens as f64 / 1e6),
    }
}

fn limit(r: &Reading, tip: &str, now: i64, t: Option<Threshold>) -> Limit {
    match r {
        Reading::Official { percent, resets_at } => Limit {
            pct: Field {
                level: t.map_or(Level::Ok, |t| level_of(*percent, t)),
                ..Field::ok(format!("{percent:.0}%"))
            },
            reset: format_reset_in(*resets_at, now),
            percent: *percent,
        },
        Reading::Estimate { tokens } => Limit {
            pct: Field {
                text: tokens_text(*tokens),
                tip: tip.to_owned(),
                level: Level::Ok,
            },
            reset: String::new(),
            percent: 0.0,
        },
    }
}

/// Claude row for the HUD and chips. Only the 5 h window alerts.
pub fn claude_view(state: &ClaudeState, now: i64, t: &Thresholds) -> ClaudeView {
    provider_view(state, now, Some(t.claude_5h))
}

/// Codex row. It has no threshold of its own, so it never alerts.
pub fn codex_view(state: &ClaudeState, now: i64) -> ClaudeView {
    provider_view(state, now, None)
}

/// The Codex row is hidden only when `auth.json` is missing; any other failure shows `—`.
pub fn codex_listed(state: &ClaudeState) -> bool {
    !matches!(state, Metric::Unavailable(r) if r == CODEX_NOT_FOUND)
}

fn provider_view(state: &ClaudeState, now: i64, five_threshold: Option<Threshold>) -> ClaudeView {
    match state {
        Metric::Available(s) => ClaudeView {
            plan: s.plan.clone().unwrap_or_default(),
            five: limit(&s.five_hour, &s.tip, now, five_threshold),
            week: limit(&s.seven_day, &s.tip, now, None),
        },
        Metric::Unavailable(reason) => {
            let missing = || Limit {
                pct: Field::missing(reason),
                ..Limit::default()
            };
            ClaudeView {
                plan: String::new(),
                five: missing(),
                week: missing(),
            }
        }
    }
}

pub const CHIP_SIZE: Size = Size {
    width: 64,
    height: 36,
};
pub const BAR_GAP: i32 = 4;
pub const BAR_PADDING: i32 = 6;
/// Length of the expand and close buttons together, along the bar's main axis.
pub const BUTTONS_LEN: i32 = 40;

/// One mini bar cell: a short label over a value.
#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    pub label: &'static str,
    pub value: Field,
}

fn chip_label(item: MiniBarItem) -> &'static str {
    match item {
        MiniBarItem::Cpu => "CPU",
        MiniBarItem::Ram => "RAM",
        MiniBarItem::Net => "NET",
        MiniBarItem::Temp => "TEMP",
        MiniBarItem::Ping => "PING",
        MiniBarItem::Gpu => "GPU",
        MiniBarItem::Disk => DISK_CHIP_LABEL,
        MiniBarItem::Claude5h => "5H",
        MiniBarItem::Claude7d => "7D",
        MiniBarItem::Codex5h => "CX5H",
        MiniBarItem::Codex7d => "CX7D",
    }
}

fn chip_value(item: MiniBarItem, v: &HudView, c: &ClaudeView, x: &ClaudeView) -> Field {
    match item {
        MiniBarItem::Cpu => v.cpu_total.clone(),
        MiniBarItem::Ram => percent_field(&v.ram, v.ram_percent, v.ram.level),
        MiniBarItem::Net => v.net_down.clone(),
        MiniBarItem::Temp => v.cpu_temp.clone(),
        MiniBarItem::Ping => v.ping.clone(),
        MiniBarItem::Gpu => v.gpu_nvidia.clone(),
        MiniBarItem::Disk => percent_field(&v.disk, v.disk_percent, Level::Ok),
        MiniBarItem::Claude5h => chip_limit(&c.five),
        MiniBarItem::Claude7d => chip_limit(&c.week),
        MiniBarItem::Codex5h => chip_limit(&x.five),
        MiniBarItem::Codex7d => chip_limit(&x.week),
    }
}

/// Chips are narrow: the unit of a token estimate is dropped.
fn chip_limit(l: &Limit) -> Field {
    Field {
        text: l.pct.text.trim_end_matches(" tok").to_owned(),
        ..l.pct.clone()
    }
}

fn percent_field(source: &Field, percent: f32, level: Level) -> Field {
    if source.tip.is_empty() {
        Field {
            level,
            ..Field::ok(format!("{percent:.0}%"))
        }
    } else {
        source.clone()
    }
}

/// Chips in the configured order, built from the same view as the HUD.
pub fn minibar_chips(
    items: &[MiniBarItem],
    s: &Snapshot,
    t: &Thresholds,
    claude: &ClaudeView,
    codex: &ClaudeView,
) -> Vec<Chip> {
    let v = build_alerted(s, t);
    items
        .iter()
        .map(|&item| Chip {
            label: chip_label(item),
            value: chip_value(item, &v, claude, codex),
        })
        .collect()
}

/// One-line tray tooltip: CPU, RAM and temperature, `—` where unavailable.
pub fn tray_tooltip(s: &Snapshot) -> String {
    let v = build(s);
    let ram = percent_field(&v.ram, v.ram_percent, Level::Ok);
    format!(
        "CPU {} · RAM {} · {}",
        v.cpu_total.text, ram.text, v.cpu_temp.text
    )
}

/// Window size that fits `count` chips in a row or a column.
pub fn minibar_size(count: usize, orientation: Orientation) -> Size {
    let n = count as i32;
    let along = |chip: i32| BUTTONS_LEN + n * (chip + BAR_GAP) + 2 * BAR_PADDING;
    let across = |chip: i32| chip + 2 * BAR_PADDING;
    match orientation {
        Orientation::Horizontal => Size {
            width: along(CHIP_SIZE.width),
            height: across(CHIP_SIZE.height),
        },
        Orientation::Vertical => Size {
            width: across(CHIP_SIZE.width),
            height: along(CHIP_SIZE.height),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{ProviderStatus, Reading};
    use crate::config::Config;
    use crate::format::GIB;
    use crate::metrics::{DiskMetrics, MemMetrics, NetMetrics, ProcessMem};
    use crate::sensors::Sensors;
    use crate::ui_text::{
        CLAUDE_NOT_FOUND, CLAUDE_RENEW_LOGIN_TIP, CODEX_RENEW_LOGIN_TIP, DISK_IO_UNAVAILABLE,
    };

    const NOW: i64 = 1_794_066_960;

    fn official() -> ClaudeState {
        Metric::Available(ProviderStatus {
            plan: Some("PRO".into()),
            five_hour: Reading::Official {
                percent: 72.0,
                resets_at: NOW + 6480,
            },
            seven_day: Reading::Official {
                percent: 41.0,
                resets_at: NOW + 2 * 86400 + 72_240,
            },
            tip: String::new(),
        })
    }

    fn estimate(five: u64, seven: u64) -> ClaudeState {
        Metric::Available(ProviderStatus {
            plan: None,
            five_hour: Reading::Estimate { tokens: five },
            seven_day: Reading::Estimate { tokens: seven },
            tip: CLAUDE_RENEW_LOGIN_TIP.into(),
        })
    }

    fn snapshot(threads: Vec<f32>) -> Snapshot {
        let sensors = Sensors::unavailable("no sensor");
        let above_50 = threads.iter().filter(|t| **t > 50.0).count();
        Snapshot {
            cpu: CpuMetrics {
                total: 61.6,
                threads,
                above_50,
                freq_mhz: Metric::Available(3400),
                cores: Metric::Available(2),
            },
            mem: MemMetrics {
                used: 8 * GIB,
                total: 16 * GIB,
                percent: 50,
                top: vec![ProcessMem {
                    name: "chrome".into(),
                    bytes: GIB,
                }],
            },
            disk: DiskMetrics {
                usage: Metric::Available((50 * GIB, 200 * GIB)),
                io: Metric::unavailable(DISK_IO_UNAVAILABLE),
            },
            net: NetMetrics {
                down: 4_404_019,
                up: 327_680,
                day_total: Metric::Available(3_328_599_654),
            },
            ping_ms: Metric::Available(12),
            thermal: sensors.thermal,
            gpu: sensors.gpu,
            unavailable: None,
        }
    }

    fn thresholds() -> Thresholds {
        Config::first_run(Size {
            width: 1920,
            height: 1080,
        })
        .thresholds
    }

    #[test]
    fn alerts_colour_hud_values_and_chips_but_not_unavailable_ones() {
        let mut s = snapshot(vec![10.0]);
        s.cpu.total = 95.0;
        s.mem.percent = 80;
        let v = build_alerted(&s, &thresholds());
        assert_eq!(v.cpu_total.level, Level::Crit);
        assert_eq!(v.ram.level, Level::Warn);
        assert_eq!(v.cpu_temp.level, Level::Ok);
        let chips = minibar_chips(
            &[MiniBarItem::Cpu, MiniBarItem::Ram],
            &s,
            &thresholds(),
            &ClaudeView::default(),
            &ClaudeView::default(),
        );
        assert_eq!(chips[0].value.level, Level::Crit);
        assert_eq!(chips[1].value.level, Level::Warn);
        assert_eq!(level_color(Level::Warn), Some(0xEF9F27));
        assert_eq!(level_color(Level::Crit), Some(0xE24B4A));
        assert_eq!(level_color(Level::Ok), None);
    }

    #[test]
    fn four_threads_give_four_bars_and_core_label() {
        let v = build(&snapshot(vec![10.0, 60.0, 20.0, 90.0]));
        assert_eq!(v.core_bars.len(), 4);
        assert_eq!(v.cpu_cores.text, format!("2C/4T · {}", cores_above_half(2)));
    }

    #[test]
    fn hot_thread_is_red_and_calm_thread_is_green() {
        let v = build(&snapshot(vec![85.0, 40.0]));
        assert_eq!(v.core_bars[0].color, 0xE24B4A);
        assert_eq!(v.core_bars[1].color, 0x1D9E75);
        assert_eq!(bar_color(80.0), BAR_GREEN);
    }

    #[test]
    fn unavailable_metric_shows_dash_with_reason_tooltip() {
        let v = build(&snapshot(vec![10.0]));
        assert_eq!(v.cpu_temp.text, "—");
        assert_eq!(v.cpu_temp.tip, "no sensor");
        assert_eq!(v.disk_io.text, "—");
        assert_eq!(v.disk_io.tip, DISK_IO_UNAVAILABLE);
        assert_eq!(v.cpu_freq.text, "3.4GHz");
        assert!(v.cpu_freq.tip.is_empty());
    }

    #[test]
    fn tray_tooltip_summarises_cpu_ram_and_temperature() {
        assert_eq!(tray_tooltip(&snapshot(vec![10.0])), "CPU 62% · RAM 50% · —");
    }

    #[test]
    fn available_values_are_formatted() {
        let v = build(&snapshot(vec![10.0]));
        assert_eq!(v.cpu_total.text, "62%");
        assert_eq!(v.ram.text, "8/16 GB");
        assert_eq!(v.ram_top.text, "chrome 1.0 GB");
        assert_eq!(v.disk.text, "50/200 GB");
        assert_eq!(v.disk_percent, 25.0);
        assert_eq!(v.net_down.text, "4.2 MB/s");
        assert_eq!(v.net_up.text, "320 KB/s");
        assert_eq!(v.net_today.text, net_today("3.1 GB"));
        assert_eq!(v.ping.text, "12 ms");
    }

    #[test]
    fn unknown_core_count_keeps_thread_count_and_explains() {
        let mut s = snapshot(vec![60.0]);
        s.cpu.cores = Metric::unavailable("no core count");
        let v = build(&s);
        assert_eq!(v.cpu_cores.text, format!("—/1T · {}", cores_above_half(1)));
        assert_eq!(v.cpu_cores.tip, "no core count");
    }

    #[test]
    fn degraded_snapshot_is_all_dashes_with_the_reason() {
        let v = build(&Snapshot::unavailable("coleta falhou"));
        assert_eq!(v.cpu_total.text, "—");
        assert_eq!(v.ram.tip, "coleta falhou");
        assert_eq!(v.net_down.tip, "coleta falhou");
        assert!(v.core_bars.is_empty());
    }

    #[test]
    fn no_top_processes_is_unavailable() {
        let mut s = snapshot(vec![1.0]);
        s.mem.top.clear();
        assert_eq!(build(&s).ram_top.text, "—");
    }

    #[test]
    fn chips_follow_configured_order_and_unavailable_provider_is_dash() {
        use MiniBarItem::*;
        let chips = minibar_chips(
            &[Cpu, Ram, Net, Claude5h],
            &snapshot(vec![10.0]),
            &thresholds(),
            &claude_view(&Metric::unavailable("no login"), NOW, &thresholds()),
            &ClaudeView::default(),
        );
        let labels: Vec<_> = chips.iter().map(|c| c.label).collect();
        assert_eq!(labels, ["CPU", "RAM", "NET", "5H"]);
        assert_eq!(chips[0].value.text, "62%");
        assert_eq!(chips[1].value.text, "50%");
        assert_eq!(chips[2].value.text, "4.2 MB/s");
        assert_eq!(chips[3].value.text, "—");
        assert_eq!(chips[3].value.tip, "no login");
    }

    #[test]
    fn unavailable_source_gives_dash_chip_with_reason() {
        use MiniBarItem::*;
        let chips = minibar_chips(
            &[Temp, Ram],
            &Snapshot::unavailable("coleta falhou"),
            &thresholds(),
            &ClaudeView::default(),
            &ClaudeView::default(),
        );
        assert!(chips.iter().all(|c| c.value.text == "—"));
        assert_eq!(chips[1].value.tip, "coleta falhou");
    }

    #[test]
    fn size_is_row_when_horizontal_and_column_when_vertical() {
        let h = minibar_size(4, Orientation::Horizontal);
        let v = minibar_size(4, Orientation::Vertical);
        assert_eq!((h.width, h.height), (40 + 4 * 68 + 12, 48));
        assert_eq!((v.width, v.height), (76, 40 + 4 * 40 + 12));
    }

    #[test]
    fn official_claude_shows_badge_percent_reset_and_warns_at_72() {
        let v = claude_view(&official(), NOW, &thresholds());
        assert_eq!(v.plan, "PRO");
        assert_eq!(
            (v.five.pct.text.as_str(), v.five.reset.as_str()),
            ("72%", "1h48")
        );
        assert_eq!(v.five.pct.level, Level::Warn);
        assert_eq!(
            (v.week.pct.text.as_str(), v.week.reset.as_str()),
            ("41%", "2d")
        );
        assert_eq!(v.week.pct.level, Level::Ok);
        assert_eq!(v.five.percent, 72.0);
    }

    #[test]
    fn estimated_claude_is_tilde_prefixed_with_renew_tooltip_and_no_percent() {
        let v = claude_view(&estimate(1_200_000, 480_000), NOW, &thresholds());
        assert_eq!(v.five.pct.text, "~1.2M tok");
        assert_eq!(v.week.pct.text, "~480.0k tok");
        assert_eq!(v.five.pct.tip, CLAUDE_RENEW_LOGIN_TIP);
        assert_eq!((v.five.percent, v.five.reset.as_str()), (0.0, ""));
        assert_eq!(v.five.pct.level, Level::Ok);
    }

    #[test]
    fn estimate_without_tokens_is_tilde_zero_percent() {
        let v = claude_view(&estimate(0, 900), NOW, &thresholds());
        assert_eq!(v.five.pct.text, "~0%");
        assert_eq!(v.week.pct.text, "~900 tok");
    }

    #[test]
    fn claude_chips_show_real_values_and_drop_the_tok_unit() {
        use MiniBarItem::*;
        let t = thresholds();
        let s = snapshot(vec![1.0]);
        let ok = minibar_chips(
            &[Claude5h, Claude7d],
            &s,
            &t,
            &claude_view(&official(), NOW, &t),
            &ClaudeView::default(),
        );
        assert_eq!(
            (ok[0].value.text.as_str(), ok[1].value.text.as_str()),
            ("72%", "41%")
        );
        assert_eq!(ok[0].value.level, Level::Warn);
        let est = minibar_chips(
            &[Claude5h],
            &s,
            &t,
            &claude_view(&estimate(1_200_000, 0), NOW, &t),
            &ClaudeView::default(),
        );
        assert_eq!(est[0].value.text, "~1.2M");
    }

    #[test]
    fn unavailable_claude_is_dash_with_reason_and_no_badge() {
        let v = claude_view(&Metric::unavailable(CLAUDE_NOT_FOUND), NOW, &thresholds());
        assert_eq!(v.plan, "");
        assert_eq!(v.five.pct.text, "—");
        assert_eq!(v.week.pct.tip, CLAUDE_NOT_FOUND);
    }

    fn codex_ok() -> ClaudeState {
        Metric::Available(ProviderStatus {
            plan: None,
            five_hour: Reading::Official {
                percent: 23.0,
                resets_at: NOW + 11_400,
            },
            seven_day: Reading::Official {
                percent: 58.0,
                resets_at: NOW + 432_000,
            },
            tip: String::new(),
        })
    }

    #[test]
    fn codex_row_shows_5h_and_semana_and_never_alerts() {
        let v = codex_view(&codex_ok(), NOW);
        assert_eq!(
            (v.five.pct.text.as_str(), v.five.reset.as_str()),
            ("23%", "3h10")
        );
        assert_eq!(
            (v.week.pct.text.as_str(), v.week.reset.as_str()),
            ("58%", "5d")
        );
        let hot = Metric::Available(ProviderStatus {
            five_hour: Reading::Official {
                percent: 99.0,
                resets_at: NOW + 60,
            },
            ..codex_ok().available().cloned().unwrap()
        });
        assert_eq!(codex_view(&hot, NOW).five.pct.level, Level::Ok);
    }

    #[test]
    fn codex_is_listed_unless_auth_json_is_missing() {
        assert!(codex_listed(&codex_ok()));
        assert!(codex_listed(&Metric::unavailable(CODEX_RENEW_LOGIN_TIP)));
        assert!(!codex_listed(&Metric::unavailable(CODEX_NOT_FOUND)));
    }

    #[test]
    fn expired_codex_login_is_dash_with_renew_tip() {
        let v = codex_view(&Metric::unavailable(CODEX_RENEW_LOGIN_TIP), NOW);
        assert_eq!(v.five.pct.text, "—");
        assert_eq!(v.week.pct.tip, CODEX_RENEW_LOGIN_TIP);
    }

    #[test]
    fn codex_chips_use_their_own_view() {
        use MiniBarItem::*;
        let t = thresholds();
        let chips = minibar_chips(
            &[Claude5h, Codex5h, Codex7d],
            &snapshot(vec![1.0]),
            &t,
            &claude_view(&official(), NOW, &t),
            &codex_view(&codex_ok(), NOW),
        );
        let got: Vec<_> = chips
            .iter()
            .map(|c| (c.label, c.value.text.as_str()))
            .collect();
        assert_eq!(got, [("5H", "72%"), ("CX5H", "23%"), ("CX7D", "58%")]);
    }
}
