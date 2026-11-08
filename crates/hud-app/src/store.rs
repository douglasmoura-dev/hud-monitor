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

/// `%APPDATA%\hud-monitor`, home of every persisted file.
pub fn data_dir() -> Result<PathBuf, StoreError> {
    let appdata = env::var_os("APPDATA").ok_or(StoreError::NoAppData)?;
    Ok(PathBuf::from(appdata).join("hud-monitor"))
}

pub fn default_path() -> Result<PathBuf, StoreError> {
    Ok(data_dir()?.join("config.toml"))
}

/// Loads the config, or builds the explicit first-run one when no file exists yet.
pub fn load_or_first_run(path: &Path, screen: Size) -> Result<Config, StoreError> {
    match Config::load(path) {
        Ok(c) => Ok(c),
        Err(ConfigError::NotFound) => Ok(Config::first_run(screen)),
        Err(e) => Err(e.into()),
    }
}

/// Stores the mini bar position and which of the two windows is shown.
pub fn save_minibar_state(
    path: &Path,
    config: &mut Config,
    pos: Point,
    minibar_visible: bool,
) -> Result<(), StoreError> {
    config.minibar.window.x = pos.x;
    config.minibar.window.y = pos.y;
    config.minibar.window.visible = minibar_visible;
    config.hud.visible = !minibar_visible;
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
    fn invalid_file_is_an_error_not_a_default() {
        let path = temp("bad");
        std::fs::write(&path, "oops = [").unwrap();
        let r = load_or_first_run(&path, SCREEN);
        std::fs::remove_file(path).unwrap();
        assert!(r.is_err());
    }

    #[test]
    fn minibar_position_is_saved_apart_from_the_hud_position() {
        let path = temp("mini");
        let mut c = Config::first_run(SCREEN);
        let hud = (c.hud.x, c.hud.y);
        save_minibar_state(&path, &mut c, Point { x: 5, y: 6 }, true).unwrap();
        let l = load_or_first_run(&path, SCREEN).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!((l.minibar.window.x, l.minibar.window.y), (5, 6));
        assert_eq!((l.hud.x, l.hud.y), hud);
        assert!(l.minibar.window.visible && !l.hud.visible);
    }
}
