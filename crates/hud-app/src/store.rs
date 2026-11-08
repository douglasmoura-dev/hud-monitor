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

/// Stores both window positions; the visibility flags are already live in `config`.
pub fn save_window_state(
    path: &Path,
    config: &mut Config,
    hud: Point,
    minibar: Point,
) -> Result<(), StoreError> {
    config.hud.x = hud.x;
    config.hud.y = hud.y;
    config.minibar.window.x = minibar.x;
    config.minibar.window.y = minibar.y;
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
    fn window_positions_and_visibility_are_saved() {
        let path = temp("mini");
        let mut c = Config::first_run(SCREEN);
        c.toggle_minibar();
        save_window_state(&path, &mut c, Point { x: 1, y: 2 }, Point { x: 5, y: 6 }).unwrap();
        let l = load_or_first_run(&path, SCREEN).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!((l.hud.x, l.hud.y), (1, 2));
        assert_eq!((l.minibar.window.x, l.minibar.window.y), (5, 6));
        assert!(l.minibar.window.visible && !l.hud.visible);
    }
}
