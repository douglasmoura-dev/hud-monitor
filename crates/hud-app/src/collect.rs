//! Collector threads: a 1 Hz snapshot builder and a 5 s ping loop.

use crate::sources::{FileDayStore, IcmpPing, PdhDiskIo, SysinfoSource};
use crate::{store, winffi};
use hud_core::metrics::{ping_metric, Collector, Metric, PingSource, Snapshot};
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
const PING_PENDING: &str = "aguardando a primeira medição";

type SharedPing = Arc<Mutex<Metric<u32>>>;

/// Shutdown handle of the collector threads.
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
    let ping: SharedPing = Arc::new(Mutex::new(Metric::unavailable(PING_PENDING)));
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
        (Err(e), _) => return eprintln!("hud-monitor: system metrics unavailable: {e}"),
        (_, Err(e)) => return eprintln!("hud-monitor: {e}"),
    };
    let mut collector = Collector::new(
        system,
        PdhDiskIo::new(),
        FileDayStore::new(dir),
        winffi::local_date(),
    );
    let mut last = Instant::now();
    while stop.recv_timeout(TICK) == Err(RecvTimeoutError::Timeout) {
        let elapsed = last.elapsed().as_secs_f64();
        last = Instant::now();
        let ping_now = ping
            .lock()
            .map_or(Metric::unavailable(PING_PENDING), |g| g.clone());
        let snapshot = collector.tick(elapsed, winffi::local_date(), ping_now);
        if tx.send(snapshot).is_err() {
            break;
        }
    }
    collector.flush();
}
