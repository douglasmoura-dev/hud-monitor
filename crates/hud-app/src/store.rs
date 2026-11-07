//! Location of the persisted config: `%APPDATA%\hud-monitor\config.toml`.

use hud_core::config::{Config, ConfigError};
use hud_core::position::{Point, Size};
use std::{env, path::Path, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("%APPDATA% is not set")]
    NoAppData,
    #[error(transparent)]
    Config(#[from] ConfigError),
}

pub fn default_path() -> Result<PathBuf, StoreError> {
    let appdata = env::var_os("APPDATA").ok_or(StoreError::NoAppData)?;
    Ok(PathBuf::from(appdata)
        .join("hud-monitor")
        .join("config.toml"))
}

/// Loads the config, or builds the explicit first-run one when no file exists yet.
pub fn load_or_first_run(path: &Path, screen: Size) -> Result<Config, StoreError> {
    match Config::load(path) {
        Ok(c) => Ok(c),
        Err(ConfigError::NotFound) => Ok(Config::first_run(screen)),
        Err(e) => Err(e.into()),
    }
}

/// Stores the HUD position, keeping every other setting as loaded.
pub fn save_hud_position(path: &Path, config: &mut Config, pos: Point) -> Result<(), StoreError> {
    config.hud.x = pos.x;
    config.hud.y = pos.y;
    config.save(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Size = Size {
        width: 1920,
        height: 1080,
    };

    fn temp(name: &str) -> PathBuf {
        env::temp_dir().join(format!("hud-monitor-test-{}-{name}", std::process::id()))
    }

    #[test]
    fn missing_file_yields_first_run_config() {
        let c = load_or_first_run(&temp("missing"), SCREEN).unwrap();
        assert_eq!(c, Config::first_run(SCREEN));
    }

    #[test]
    fn saved_position_is_loaded_back() {
        let path = temp("pos");
        let mut c = Config::first_run(SCREEN);
        save_hud_position(&path, &mut c, Point { x: 400, y: 300 }).unwrap();
        let loaded = load_or_first_run(&path, SCREEN).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!((loaded.hud.x, loaded.hud.y), (400, 300));
    }

    #[test]
    fn invalid_file_is_an_error_not_a_default() {
        let path = temp("bad");
        std::fs::write(&path, "oops = [").unwrap();
        let r = load_or_first_run(&path, SCREEN);
        std::fs::remove_file(path).unwrap();
        assert!(r.is_err());
    }
}
