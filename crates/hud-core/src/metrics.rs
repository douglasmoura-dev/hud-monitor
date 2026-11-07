//! Snapshot model and the collector that builds it from abstract sources.

use std::{fmt, time::Duration};

pub const DISK_IO_UNAVAILABLE: &str = "contador indisponível";
pub const DISK_USAGE_UNAVAILABLE: &str = "disco C: indisponível";
pub const FREQ_UNAVAILABLE: &str = "frequência indisponível";
pub const SAVE_FAILED: &str = "não foi possível salvar o total do dia";
pub const PING_UNAVAILABLE: &str = "sem resposta";
const SECS_PER_DAY: u32 = 86_400;
const SAVE_EVERY_TICKS: u32 = 10;

/// A value that is either known or explicitly unavailable (shown as `—`).
#[derive(Debug, Clone, PartialEq)]
pub enum Metric<T> {
    Available(T),
    Unavailable(String),
}

impl<T> Metric<T> {
    pub fn unavailable(reason: &str) -> Self {
        Metric::Unavailable(reason.to_string())
    }

    pub fn available(&self) -> Option<&T> {
        match self {
            Metric::Available(v) => Some(v),
            Metric::Unavailable(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpuMetrics {
    pub total: f32,
    pub threads: Vec<f32>,
    pub above_50: usize,
    pub freq_mhz: Metric<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessMem {
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemMetrics {
    pub used: u64,
    pub total: u64,
    pub percent: u8,
    pub top: Vec<ProcessMem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskIo {
    pub read_per_sec: u64,
    pub write_per_sec: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiskMetrics {
    pub usage: Metric<(u64, u64)>,
    pub io: Metric<DiskIo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetMetrics {
    pub down: u64,
    pub up: u64,
    pub day_total: Metric<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub cpu: CpuMetrics,
    pub mem: MemMetrics,
    pub disk: DiskMetrics,
    pub net: NetMetrics,
    pub ping_ms: Metric<u32>,
    /// Set when collection could not start; numeric fields are then placeholders to ignore.
    pub unavailable: Option<String>,
}

impl Snapshot {
    /// Degraded snapshot: every metric is `Unavailable(reason)`.
    pub fn unavailable(reason: &str) -> Self {
        Self {
            cpu: CpuMetrics {
                total: 0.0,
                threads: Vec::new(),
                above_50: 0,
                freq_mhz: Metric::unavailable(reason),
            },
            mem: MemMetrics {
                used: 0,
                total: 0,
                percent: 0,
                top: Vec::new(),
            },
            disk: DiskMetrics {
                usage: Metric::unavailable(reason),
                io: Metric::unavailable(reason),
            },
            net: NetMetrics {
                down: 0,
                up: 0,
                day_total: Metric::unavailable(reason),
            },
            ping_ms: Metric::unavailable(reason),
            unavailable: Some(reason.to_string()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct DayStoreError(pub String);

/// Midnight detection without reading the clock every tick.
pub struct DayClock {
    date: Date,
    rollover_at: Duration,
}

impl DayClock {
    /// `mono` is a monotonic time since process start; `read` returns local date and seconds into the day.
    pub fn new(mono: Duration, read: impl FnOnce() -> (Date, u32)) -> Self {
        let (date, secs) = read();
        Self {
            date,
            rollover_at: mono + Duration::from_secs(u64::from(SECS_PER_DAY.saturating_sub(secs))),
        }
    }

    pub fn today(&mut self, mono: Duration, read: impl FnOnce() -> (Date, u32)) -> Date {
        if mono >= self.rollover_at {
            *self = Self::new(mono, read);
        }
        self.date
    }
}

pub trait SystemSource {
    /// Usage in percent of every logical thread.
    fn cpu_threads(&mut self) -> Vec<f32>;
    fn cpu_freq_mhz(&mut self) -> Option<u64>;
    /// `(used, total)` bytes.
    fn memory(&mut self) -> (u64, u64);
    fn top_processes(&mut self) -> Vec<ProcessMem>;
    /// `(used, total)` bytes of the system drive.
    fn disk_usage(&mut self) -> Option<(u64, u64)>;
    /// Cumulative `(received, sent)` bytes over active interfaces.
    fn net_counters(&mut self) -> (u64, u64);
}

pub trait DiskIoSource {
    fn sample(&mut self) -> Option<DiskIo>;
}

pub trait PingSource {
    fn ping_ms(&mut self) -> Option<u32>;
}

/// Persistence of the daily network total, one record per date.
pub trait DayStore {
    fn load(&self, date: Date) -> Option<u64>;
    fn save(&self, date: Date, bytes: u64) -> Result<(), DayStoreError>;
}

pub fn ping_metric(source: &mut dyn PingSource) -> Metric<u32> {
    source
        .ping_ms()
        .map_or(Metric::unavailable(PING_UNAVAILABLE), Metric::Available)
}

pub struct Collector<S, D, T> {
    system: S,
    disk_io: D,
    store: T,
    last_net: (u64, u64),
    day: Date,
    day_total: u64,
    ticks: u32,
    save_error: Option<DayStoreError>,
    save_failed: bool,
}

impl<S: SystemSource, D: DiskIoSource, T: DayStore> Collector<S, D, T> {
    pub fn new(mut system: S, disk_io: D, store: T, today: Date) -> Self {
        let last_net = system.net_counters();
        let day_total = store.load(today).unwrap_or(0);
        Self {
            system,
            disk_io,
            store,
            last_net,
            day: today,
            day_total,
            ticks: 0,
            save_error: None,
            save_failed: false,
        }
    }

    /// Builds one snapshot; `elapsed_secs` is the time since the previous tick.
    pub fn tick(&mut self, elapsed_secs: f64, today: Date, ping_ms: Metric<u32>) -> Snapshot {
        Snapshot {
            cpu: self.cpu(),
            mem: self.mem(),
            disk: self.disk(),
            net: self.net(elapsed_secs, today),
            ping_ms,
            unavailable: None,
        }
    }

    /// Persists the current day total (call on shutdown); failures are kept for `take_save_error`.
    pub fn flush(&mut self) {
        match self.store.save(self.day, self.day_total) {
            Ok(()) => self.save_failed = false,
            Err(e) => {
                self.save_failed = true;
                self.save_error = Some(e);
            }
        }
    }

    pub fn take_save_error(&mut self) -> Option<DayStoreError> {
        self.save_error.take()
    }

    fn cpu(&mut self) -> CpuMetrics {
        let threads = self.system.cpu_threads();
        let total = if threads.is_empty() {
            0.0
        } else {
            threads.iter().sum::<f32>() / threads.len() as f32
        };
        let above_50 = threads.iter().filter(|t| **t > 50.0).count();
        let freq_mhz = match self.system.cpu_freq_mhz() {
            Some(f) if f > 0 => Metric::Available(f),
            _ => Metric::unavailable(FREQ_UNAVAILABLE),
        };
        CpuMetrics {
            total,
            threads,
            above_50,
            freq_mhz,
        }
    }

    fn mem(&mut self) -> MemMetrics {
        let (used, total) = self.system.memory();
        let percent = if total == 0 {
            0
        } else {
            (used as f64 / total as f64 * 100.0).round() as u8
        };
        MemMetrics {
            used,
            total,
            percent,
            top: self.system.top_processes(),
        }
    }

    fn disk(&mut self) -> DiskMetrics {
        DiskMetrics {
            usage: self.system.disk_usage().map_or(
                Metric::unavailable(DISK_USAGE_UNAVAILABLE),
                Metric::Available,
            ),
            io: self
                .disk_io
                .sample()
                .map_or(Metric::unavailable(DISK_IO_UNAVAILABLE), Metric::Available),
        }
    }

    fn net(&mut self, elapsed_secs: f64, today: Date) -> NetMetrics {
        let (rx, tx) = self.system.net_counters();
        let drx = rx.saturating_sub(self.last_net.0);
        let dtx = tx.saturating_sub(self.last_net.1);
        self.last_net = (rx, tx);
        self.roll_day(today);
        self.day_total += drx + dtx;
        self.ticks += 1;
        if self.ticks.is_multiple_of(SAVE_EVERY_TICKS) {
            self.flush();
        }
        NetMetrics {
            down: per_sec(drx, elapsed_secs),
            up: per_sec(dtx, elapsed_secs),
            day_total: if self.save_failed {
                Metric::unavailable(SAVE_FAILED)
            } else {
                Metric::Available(self.day_total)
            },
        }
    }

    /// At a date change the finished day is saved untouched and the new one starts at 0.
    fn roll_day(&mut self, today: Date) {
        if today != self.day {
            self.flush();
            self.day = today;
            self.day_total = 0;
        }
    }
}

fn per_sec(bytes: u64, elapsed_secs: f64) -> u64 {
    if elapsed_secs <= 0.0 {
        return 0;
    }
    (bytes as f64 / elapsed_secs).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{rate, used_total_gb, GIB};
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    #[derive(Default)]
    struct FakeSystem {
        threads: Vec<f32>,
        mem: (u64, u64),
        net: Rc<RefCell<(u64, u64)>>,
    }

    impl SystemSource for FakeSystem {
        fn cpu_threads(&mut self) -> Vec<f32> {
            self.threads.clone()
        }
        fn cpu_freq_mhz(&mut self) -> Option<u64> {
            Some(2500)
        }
        fn memory(&mut self) -> (u64, u64) {
            self.mem
        }
        fn top_processes(&mut self) -> Vec<ProcessMem> {
            Vec::new()
        }
        fn disk_usage(&mut self) -> Option<(u64, u64)> {
            Some((100, 200))
        }
        fn net_counters(&mut self) -> (u64, u64) {
            *self.net.borrow()
        }
    }

    struct NoDiskIo;
    impl DiskIoSource for NoDiskIo {
        fn sample(&mut self) -> Option<DiskIo> {
            None
        }
    }

    #[derive(Clone, Default)]
    struct MemStore(Rc<RefCell<HashMap<String, u64>>>);
    impl DayStore for MemStore {
        fn load(&self, date: Date) -> Option<u64> {
            self.0.borrow().get(&date.to_string()).copied()
        }
        fn save(&self, date: Date, bytes: u64) -> Result<(), DayStoreError> {
            self.0.borrow_mut().insert(date.to_string(), bytes);
            Ok(())
        }
    }

    const D1: Date = Date {
        year: 2026,
        month: 11,
        day: 7,
    };
    const D2: Date = Date {
        year: 2026,
        month: 11,
        day: 8,
    };

    fn collector(system: FakeSystem, store: MemStore) -> Collector<FakeSystem, NoDiskIo, MemStore> {
        Collector::new(system, NoDiskIo, store, D1)
    }

    fn no_ping() -> Metric<u32> {
        Metric::unavailable(PING_UNAVAILABLE)
    }

    #[test]
    fn cpu_total_is_mean_of_threads() {
        let sys = FakeSystem {
            threads: vec![10.0, 90.0, 50.0, 51.0],
            ..Default::default()
        };
        let s = collector(sys, MemStore::default()).tick(1.0, D1, no_ping());
        assert_eq!(s.cpu.total, 50.25);
        assert_eq!(s.cpu.threads.len(), 4);
        assert_eq!(s.cpu.above_50, 2);
    }

    #[test]
    fn network_rate_comes_from_counter_delta() {
        let net = Rc::new(RefCell::new((0, 0)));
        let sys = FakeSystem {
            net: net.clone(),
            ..Default::default()
        };
        let mut c = collector(sys, MemStore::default());
        *net.borrow_mut() = (4_404_019, 327_680);
        let s = c.tick(1.0, D1, no_ping());
        assert_eq!(rate(s.net.down), "4.2 MB/s");
        assert_eq!(rate(s.net.up), "320 KB/s");
        assert_eq!(s.net.day_total, Metric::Available(4_404_019 + 327_680));
    }

    #[test]
    fn ram_percent_and_text() {
        let used = (11.2 * GIB as f64) as u64;
        let sys = FakeSystem {
            mem: (used, 16 * GIB),
            ..Default::default()
        };
        let s = collector(sys, MemStore::default()).tick(1.0, D1, no_ping());
        assert_eq!(s.mem.percent, 70);
        assert_eq!(used_total_gb(s.mem.used, s.mem.total), "11.2/16 GB");
    }

    #[test]
    fn day_total_resets_at_midnight_and_keeps_previous_day() {
        let net = Rc::new(RefCell::new((0, 0)));
        let sys = FakeSystem {
            net: net.clone(),
            ..Default::default()
        };
        let store = MemStore::default();
        let mut c = collector(sys, store.clone());
        let three_gb = 3_328_599_654;
        *net.borrow_mut() = (three_gb, 0);
        assert_eq!(
            c.tick(1.0, D1, no_ping()).net.day_total,
            Metric::Available(three_gb)
        );
        let s = c.tick(1.0, D2, no_ping());
        assert_eq!(s.net.day_total, Metric::Available(0));
        assert_eq!(store.load(D1), Some(three_gb));
        assert_eq!(store.load(D2), None);
    }

    #[test]
    fn day_total_resumes_from_store_on_start() {
        let store = MemStore::default();
        store.save(D1, 500).unwrap();
        let s = collector(FakeSystem::default(), store).tick(1.0, D1, no_ping());
        assert_eq!(s.net.day_total, Metric::Available(500));
    }

    #[test]
    fn unavailable_disk_io_is_explicit() {
        let s = collector(FakeSystem::default(), MemStore::default()).tick(1.0, D1, no_ping());
        assert_eq!(
            s.disk.io,
            Metric::Unavailable("contador indisponível".into())
        );
        assert_eq!(s.disk.usage, Metric::Available((100, 200)));
    }

    #[test]
    fn ping_without_reply_is_unavailable() {
        struct Dead;
        impl PingSource for Dead {
            fn ping_ms(&mut self) -> Option<u32> {
                None
            }
        }
        assert_eq!(ping_metric(&mut Dead), no_ping());
    }

    struct FailingStore;
    impl DayStore for FailingStore {
        fn load(&self, _: Date) -> Option<u64> {
            None
        }
        fn save(&self, _: Date, _: u64) -> Result<(), DayStoreError> {
            Err(DayStoreError("disk full".into()))
        }
    }

    #[test]
    fn failed_save_marks_day_total_unavailable() {
        let mut c = Collector::new(FakeSystem::default(), NoDiskIo, FailingStore, D1);
        c.flush();
        assert!(c.take_save_error().is_some());
        assert!(c.take_save_error().is_none());
        let s = c.tick(1.0, D1, no_ping());
        assert_eq!(s.net.day_total, Metric::unavailable(SAVE_FAILED));
    }

    #[test]
    fn successful_save_clears_unavailable_day_total() {
        let mut c = collector(FakeSystem::default(), MemStore::default());
        c.save_failed = true;
        c.flush();
        let s = c.tick(1.0, D1, no_ping());
        assert_eq!(s.net.day_total, Metric::Available(0));
    }

    #[test]
    fn init_failure_snapshot_is_all_unavailable_with_reason() {
        let s = Snapshot::unavailable("sysinfo falhou");
        assert_eq!(s.unavailable.as_deref(), Some("sysinfo falhou"));
        assert_eq!(s.ping_ms, Metric::unavailable("sysinfo falhou"));
        assert_eq!(s.disk.io, Metric::unavailable("sysinfo falhou"));
        assert_eq!(s.net.day_total, Metric::unavailable("sysinfo falhou"));
        assert_eq!(s.cpu.freq_mhz, Metric::unavailable("sysinfo falhou"));
    }

    #[test]
    fn day_clock_reads_clock_only_at_midnight() {
        let secs = Duration::from_secs;
        let mut reads = 0;
        let mut clock = DayClock::new(secs(0), || (D1, 86_398));
        assert_eq!(
            clock.today(secs(1), || {
                reads += 1;
                (D2, 0)
            }),
            D1
        );
        assert_eq!(reads, 0);
        let day = clock.today(secs(2), || {
            reads += 1;
            (D2, 0)
        });
        assert_eq!((day, reads), (D2, 1));
        assert_eq!(clock.today(secs(3), || (D1, 0)), D2);
    }

    #[test]
    fn date_displays_iso() {
        assert_eq!(D1.to_string(), "2026-11-07");
    }
}
