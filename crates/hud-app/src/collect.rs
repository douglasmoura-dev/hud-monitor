//! Collector threads: a 1 Hz snapshot builder and a 5 s ping loop.

use crate::sources::{FileDayStore, IcmpPing, PdhDiskIo, SysinfoSource};
use crate::{store, winffi};
use hud_core::metrics::{ping_metric, Collector, Metric, PingSource, Snapshot};
use std::{
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const TICK: Duration = Duration::from_secs(1);
const PING_EVERY: Duration = Duration::from_secs(5);
const PING_PENDING: &str = "aguardando a primeira medição";

type SharedPing = Arc<Mutex<Metric<u32>>>;

/// Starts the collector threads and returns the stream of snapshots.
pub fn spawn() -> Receiver<Snapshot> {
    let (tx, rx) = mpsc::channel();
    let ping: SharedPing = Arc::new(Mutex::new(Metric::unavailable(PING_PENDING)));
    let ping_slot = ping.clone();
    thread::spawn(move || ping_loop(IcmpPing::new(), &ping_slot));
    thread::spawn(move || snapshot_loop(&tx, &ping));
    rx
}

fn ping_loop(mut source: impl PingSource, slot: &SharedPing) {
    loop {
        let value = ping_metric(&mut source);
        if let Ok(mut guard) = slot.lock() {
            *guard = value;
        }
        thread::sleep(PING_EVERY);
    }
}

fn snapshot_loop(tx: &Sender<Snapshot>, ping: &SharedPing) {
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
    loop {
        thread::sleep(TICK);
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
