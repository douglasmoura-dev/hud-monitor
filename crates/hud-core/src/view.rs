//! Display text for the HUD, built from a `Snapshot`. Unavailable metrics become `—` plus the reason.

use crate::format::{rate, size, used_total_gb};
use crate::metrics::{CpuMetrics, Metric, Snapshot};

pub const DASH: &str = "—";
pub const BAR_RED: u32 = 0x00E2_4B4A;
pub const BAR_GREEN: u32 = 0x001D_9E75;
const HOT_THREAD_PERCENT: f32 = 80.0;
const NO_PROCESSES: &str = "processos indisponíveis";

/// Text shown in the HUD and the tooltip explaining why it is `—` (empty when available).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Field {
    pub text: String,
    pub tip: String,
}

impl Field {
    fn ok(text: String) -> Self {
        Self {
            text,
            tip: String::new(),
        }
    }

    fn missing(reason: &str) -> Self {
        Self {
            text: DASH.to_string(),
            tip: reason.to_string(),
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

fn core_label(cpu: &CpuMetrics) -> Field {
    let threads = cpu.threads.len();
    let above = match cpu.above_50 {
        1 => "1 núcleo acima de 50%".to_string(),
        n => format!("{n} núcleos acima de 50%"),
    };
    match &cpu.cores {
        Metric::Available(c) => Field::ok(format!("{c}C/{threads}T · {above}")),
        Metric::Unavailable(reason) => Field {
            text: format!("{DASH}/{threads}T · {above}"),
            tip: reason.clone(),
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
        net_today: Field::of(&s.net.day_total, |&b| format!("hoje {}", size(b))),
        ping: Field::of(&s.ping_ms, |ms| format!("{ms} ms")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::GIB;
    use crate::metrics::{DiskMetrics, MemMetrics, NetMetrics, ProcessMem};
    use crate::sensors::Sensors;

    fn snapshot(threads: Vec<f32>) -> Snapshot {
        let sensors = Sensors::unavailable("sem sensor");
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
                io: Metric::unavailable("contador indisponível"),
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

    #[test]
    fn four_threads_give_four_bars_and_core_label() {
        let v = build(&snapshot(vec![10.0, 60.0, 20.0, 90.0]));
        assert_eq!(v.core_bars.len(), 4);
        assert_eq!(v.cpu_cores.text, "2C/4T · 2 núcleos acima de 50%");
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
        assert_eq!(v.cpu_temp.tip, "sem sensor");
        assert_eq!(v.disk_io.text, "—");
        assert_eq!(v.disk_io.tip, "contador indisponível");
        assert_eq!(v.cpu_freq.text, "3.4GHz");
        assert!(v.cpu_freq.tip.is_empty());
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
        assert_eq!(v.net_today.text, "hoje 3.1 GB");
        assert_eq!(v.ping.text, "12 ms");
    }

    #[test]
    fn unknown_core_count_keeps_thread_count_and_explains() {
        let mut s = snapshot(vec![60.0]);
        s.cpu.cores = Metric::unavailable("sem contagem");
        let v = build(&s);
        assert_eq!(v.cpu_cores.text, "—/1T · 1 núcleo acima de 50%");
        assert_eq!(v.cpu_cores.tip, "sem contagem");
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
}
