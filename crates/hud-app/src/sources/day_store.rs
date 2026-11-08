//! Daily network total: `net-YYYY-MM-DD.json` next to the config.

use hud_core::metrics::{Date, DayStore, DayStoreError};
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

impl FileDayStore {
    /// Writes a temp file in the same directory, then renames it over the target.
    fn write_atomically(&self, date: Date, bytes: u64) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let target = self.path(date);
        let tmp = target.with_extension("json.tmp");
        std::fs::write(&tmp, json!({ "bytes": bytes }).to_string())?;
        std::fs::rename(&tmp, &target).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }
}

impl DayStore for FileDayStore {
    fn load(&self, date: Date) -> Option<u64> {
        let text = std::fs::read_to_string(self.path(date)).ok()?;
        serde_json::from_str::<Value>(&text).ok()?["bytes"].as_u64()
    }

    fn save(&self, date: Date, bytes: u64) -> Result<(), DayStoreError> {
        self.write_atomically(date, bytes)
            .map_err(|e| DayStoreError(e.to_string()))
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
        store.save(D1, 3_100).unwrap();
        store.save(D2, 0).unwrap();
        assert_eq!(store.load(D1), Some(3_100));
        assert_eq!(store.load(D2), Some(0));
        assert!(dir.join("net-2026-11-07.json").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_replaces_existing_file_and_leaves_no_temp_file() {
        let dir = temp_dir().join("atomic");
        let store = FileDayStore::new(dir.clone());
        store.save(D1, 1).unwrap();
        store.save(D1, 2).unwrap();
        assert_eq!(store.load(D1), Some(2));
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("net-2026-11-07.json")]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_into_a_file_path_fails_with_error() {
        let dir = temp_dir().join("save-fails");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a-file");
        std::fs::write(&file, "x").unwrap();
        let store = FileDayStore::new(file);
        assert!(store.save(D1, 1).is_err());
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
