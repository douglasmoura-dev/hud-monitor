//! Collector threads: a 1 Hz snapshot builder and a 5 s ping loop.

use crate::sources::{
    FileDayStore, IcmpPing, NvmlSource, PdhDiskIo, PdhIntelGpu, PdhThermal, SysinfoSource,
};
use crate::{store, winffi};
use hud_core::metrics::{ping_metric, Collector, DayClock, Metric, PingSource, Snapshot};
use hud_core::sensors::read_sensors;
use hud_core::ui_text::{collection_unavailable, WAITING_FIRST_PING};
use std::{
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const TICK: Duration = Duration::from_secs(1);
const PING_EVERY: Duration = Duration::from_secs(5);

type SharedPing = Arc<Mutex<Metric<u32>>>;

pub struct Collection {
    stops: [Sender<()>; 2],
    workers: [thread::JoinHandle<()>; 2],
}

impl Collection {
    /// Stops both threads; the snapshot thread persists the daily total first.
    pub fn shutdown(self) {
        drop(self.stops);
        for worker in self.workers {
            let _ = worker.join();
        }
    }
}

/// Starts the collector threads.
pub fn spawn() -> (Receiver<Snapshot>, Collection) {
    let (tx, rx) = mpsc::channel();
    let ping: SharedPing = Arc::new(Mutex::new(Metric::unavailable(WAITING_FIRST_PING)));
    let ping_slot = ping.clone();
    let (ping_stop, ping_stop_rx) = mpsc::channel();
    let ping_worker = thread::spawn(move || ping_loop(IcmpPing::new(), &ping_slot, &ping_stop_rx));
    let (stop, stop_rx) = mpsc::channel();
    let worker = thread::spawn(move || snapshot_loop(&tx, &ping, &stop_rx));
    let collection = Collection {
        stops: [stop, ping_stop],
        workers: [worker, ping_worker],
    };
    (rx, collection)
}

fn ping_loop(mut source: impl PingSource, slot: &SharedPing, stop: &Receiver<()>) {
    loop {
        let value = ping_metric(&mut source);
        if let Ok(mut guard) = slot.lock() {
            *guard = value;
        }
        if stop.recv_timeout(PING_EVERY) != Err(RecvTimeoutError::Timeout) {
            break;
        }
    }
}

fn snapshot_loop(tx: &Sender<Snapshot>, ping: &SharedPing, stop: &Receiver<()>) {
    let (system, dir) = match (SysinfoSource::new(), store::data_dir()) {
        (Ok(s), Ok(d)) => (s, d),
        (Err(e), _) => return report_init_failure(tx, &collection_unavailable(&e.to_string())),
        (_, Err(e)) => return report_init_failure(tx, &e.to_string()),
    };
    let start = Instant::now();
    let mut clock = DayClock::new(start.elapsed(), winffi::local_now);
    let mut collector = Collector::new(
        system,
        PdhDiskIo::new(),
        FileDayStore::new(dir),
        clock.today(start.elapsed(), winffi::local_now),
    );
    let (mut thermal, mut nvidia, mut intel) =
        (PdhThermal::new(), NvmlSource::new(), PdhIntelGpu::new());
    let mut last = Instant::now();
    while stop.recv_timeout(TICK) == Err(RecvTimeoutError::Timeout) {
        let elapsed = last.elapsed().as_secs_f64();
        last = Instant::now();
        let ping_now = ping
            .lock()
            .map_or(Metric::unavailable(WAITING_FIRST_PING), |g| g.clone());
        let today = clock.today(start.elapsed(), winffi::local_now);
        let sensors = read_sensors(&mut thermal, &mut nvidia, &mut intel);
        let snapshot = collector.tick(elapsed, today, ping_now, sensors);
        log_save_error(&mut collector);
        if tx.send(snapshot).is_err() {
            break;
        }
    }
    collector.flush();
    log_save_error(&mut collector);
}

fn log_save_error<S, D, T>(collector: &mut Collector<S, D, T>)
where
    S: hud_core::metrics::SystemSource,
    D: hud_core::metrics::DiskIoSource,
    T: hud_core::metrics::DayStore,
{
    if let Some(e) = collector.take_save_error() {
        eprintln!("hud-monitor: could not save network total: {e}");
    }
}

fn report_init_failure(tx: &Sender<Snapshot>, reason: &str) {
    eprintln!("hud-monitor: {reason}");
    let _ = tx.send(Snapshot::unavailable(reason));
}
