//! Daily network total: `net-YYYY-MM-DD.json` next to the config.

use hud_core::metrics::{Date, DayStore};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct FileDayStore {
    dir: PathBuf,
}

impl FileDayStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self, date: Date) -> PathBuf {
        self.dir.join(format!("net-{date}.json"))
    }
}

impl DayStore for FileDayStore {
    fn load(&self, date: Date) -> Option<u64> {
        let text = std::fs::read_to_string(self.path(date)).ok()?;
        serde_json::from_str::<Value>(&text).ok()?["bytes"].as_u64()
    }

    fn save(&self, date: Date, bytes: u64) {
        let write = std::fs::create_dir_all(&self.dir)
            .and_then(|()| std::fs::write(self.path(date), json!({ "bytes": bytes }).to_string()));
        if let Err(e) = write {
            eprintln!("hud-monitor: could not save network total: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("hud-monitor-daystore-{}", std::process::id()))
    }

    #[test]
    fn saves_one_file_per_day_and_loads_it_back() {
        let dir = temp_dir();
        let store = FileDayStore::new(dir.clone());
        store.save(D1, 3_100);
        store.save(D2, 0);
        assert_eq!(store.load(D1), Some(3_100));
        assert_eq!(store.load(D2), Some(0));
        assert!(dir.join("net-2026-11-07.json").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_or_corrupt_file_loads_as_none() {
        let dir = temp_dir().join("corrupt");
        let store = FileDayStore::new(dir.clone());
        assert_eq!(store.load(D1), None);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("net-2026-11-07.json"), "oops").unwrap();
        assert_eq!(store.load(D1), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
