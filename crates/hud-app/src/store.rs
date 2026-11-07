//! Minimal window-position persistence. Issue #3 replaces this with config.toml.

use hud_core::position::Point;
use std::{env, fs, io, path::Path, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("%APPDATA% is not set")]
    NoAppData,
    #[error("position file io: {0}")]
    Io(#[from] io::Error),
    #[error("position file is malformed: {0:?}")]
    Malformed(String),
}

pub fn default_path() -> Result<PathBuf, StoreError> {
    let appdata = env::var_os("APPDATA").ok_or(StoreError::NoAppData)?;
    Ok(PathBuf::from(appdata)
        .join("hud-monitor")
        .join("window-position.txt"))
}

fn parse(text: &str) -> Result<Point, StoreError> {
    let bad = || StoreError::Malformed(text.to_owned());
    let (x, y) = text.trim().split_once(',').ok_or_else(bad)?;
    Ok(Point {
        x: x.parse().map_err(|_| bad())?,
        y: y.parse().map_err(|_| bad())?,
    })
}

/// `Ok(None)` means first run (no file yet).
pub fn load(path: &Path) -> Result<Option<Point>, StoreError> {
    match fs::read_to_string(path) {
        Ok(text) => parse(&text).map(Some),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn save(path: &Path, pos: Point) -> Result<(), StoreError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, format!("{},{}", pos.x, pos.y))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        env::temp_dir().join(format!("hud-monitor-test-{}-{name}", std::process::id()))
    }

    #[test]
    fn missing_file_is_first_run() {
        assert_eq!(load(&temp("missing")).unwrap(), None);
    }

    #[test]
    fn save_then_load_roundtrips() {
        let path = temp("roundtrip");
        save(&path, Point { x: 400, y: 300 }).unwrap();
        assert_eq!(load(&path).unwrap(), Some(Point { x: 400, y: 300 }));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_file_is_an_error_not_a_default() {
        let path = temp("bad");
        fs::write(&path, "oops").unwrap();
        assert!(matches!(load(&path), Err(StoreError::Malformed(_))));
        fs::remove_file(path).unwrap();
    }
}
