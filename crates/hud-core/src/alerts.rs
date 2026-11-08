//! Threshold alerts: maps a `Snapshot` to a `Level` per metric. Unavailable metrics never alert.

use crate::config::{Threshold, Thresholds};
use crate::metrics::{Metric, Snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Level {
    #[default]
    Ok,
    Warn,
    Crit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Alerts {
    pub cpu: Level,
    pub ram: Level,
    pub temp: Level,
    pub ping: Level,
}

/// `Crit` at or above `crit`, `Warn` at or above `warn`.
pub fn level_of(value: f32, t: Threshold) -> Level {
    if value >= t.crit {
        Level::Crit
    } else if value >= t.warn {
        Level::Warn
    } else {
        Level::Ok
    }
}

fn level_of_metric(m: &Metric<impl Copy + Into<f64>>, t: Threshold) -> Level {
    m.available()
        .map_or(Level::Ok, |&v| level_of(v.into() as f32, t))
}

pub fn evaluate(s: &Snapshot, t: &Thresholds) -> Alerts {
    if s.unavailable.is_some() {
        return Alerts::default();
    }
    Alerts {
        cpu: level_of(s.cpu.total, t.cpu_percent),
        ram: level_of(f32::from(s.mem.percent), t.ram_percent),
        temp: level_of_metric(&s.thermal.cpu_c, t.temp_c),
        ping: level_of_metric(&s.ping_ms, t.ping_ms),
    }
}

impl Alerts {
    pub fn any_crit(&self) -> bool {
        [self.cpu, self.ram, self.temp, self.ping].contains(&Level::Crit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::position::Size;

    const CLAUDE_5H: Threshold = Threshold {
        warn: 70.0,
        crit: 90.0,
    };

    fn thresholds() -> Thresholds {
        Config::first_run(Size {
            width: 1920,
            height: 1080,
        })
        .thresholds
    }

    fn snapshot() -> Snapshot {
        let mut s = Snapshot::unavailable("sem leitura");
        s.unavailable = None;
        s.cpu.total = 10.0;
        s.mem.percent = 10;
        s
    }

    #[test]
    fn usage_72_is_warn_and_91_is_crit() {
        assert_eq!(level_of(72.0, CLAUDE_5H), Level::Warn);
        assert_eq!(level_of(91.0, CLAUDE_5H), Level::Crit);
        assert_eq!(level_of(69.9, CLAUDE_5H), Level::Ok);
        assert_eq!(level_of(90.0, CLAUDE_5H), Level::Crit);
    }

    #[test]
    fn unavailable_temperature_raises_no_alert() {
        let a = evaluate(&snapshot(), &thresholds());
        assert_eq!(a.temp, Level::Ok);
        assert!(!a.any_crit());
    }

    #[test]
    fn each_metric_uses_its_own_threshold() {
        let mut s = snapshot();
        s.cpu.total = 95.0;
        s.mem.percent = 80;
        s.thermal.cpu_c = Metric::Available(76);
        s.ping_ms = Metric::Available(250);
        let a = evaluate(&s, &thresholds());
        assert_eq!(
            (a.cpu, a.ram, a.temp, a.ping),
            (Level::Crit, Level::Warn, Level::Warn, Level::Crit)
        );
        assert!(a.any_crit());
    }

    #[test]
    fn degraded_snapshot_raises_no_alert() {
        let mut s = Snapshot::unavailable("coleta falhou");
        s.cpu.total = 100.0;
        assert_eq!(evaluate(&s, &thresholds()), Alerts::default());
    }
}
