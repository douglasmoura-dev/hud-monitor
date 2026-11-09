//! Location and file format of the persisted config: `%APPDATA%\hud-monitor\config.toml`.

use hud_core::config::{Config, ConfigError};
use hud_core::position::Size;
use hud_core::windowing::ConfigStore;
use std::{
    env, fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("%APPDATA% is not set")]
    NoAppData,
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("config io: {0}")]
    Io(#[from] io::Error),
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
    match fs::read_to_string(path) {
        Ok(text) => Ok(Config::from_toml(&text)?),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Config::first_run(screen)),
        Err(e) => Err(e.into()),
    }
}

fn write_config(path: &Path, config: &Config) -> Result<(), StoreError> {
    let text = config.to_toml()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, text)?;
    Ok(())
}

/// The config file on disk, as the window use case sees it.
pub struct FileConfigStore {
    path: PathBuf,
}

impl FileConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ConfigStore for FileConfigStore {
    fn save(&self, config: &Config) -> Result<(), String> {
        write_config(&self.path, config).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hud_core::position::Point;

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
        fs::write(&path, "oops = [").unwrap();
        let r = load_or_first_run(&path, SCREEN);
        fs::remove_file(path).unwrap();
        assert!(r.is_err());
    }

    #[test]
    fn window_positions_and_visibility_are_saved() {
        let path = temp("mini");
        let mut c = Config::first_run(SCREEN);
        c.toggle_minibar();
        c.record_positions(Point { x: 1, y: 2 }, Point { x: 5, y: 6 });
        FileConfigStore::new(path.clone()).save(&c).unwrap();
        let l = load_or_first_run(&path, SCREEN).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!((l.hud.x, l.hud.y), (1, 2));
        assert_eq!((l.minibar.window.x, l.minibar.window.y), (5, 6));
        assert!(l.minibar.window.visible && !l.hud.visible);
    }

    #[test]
    fn saved_config_reloads_structurally_equal() {
        let path = temp("roundtrip");
        let c = Config::first_run(SCREEN);
        FileConfigStore::new(path.clone()).save(&c).unwrap();
        let loaded = load_or_first_run(&path, SCREEN).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(loaded, c);
    }

    #[test]
    fn saving_creates_the_missing_directory() {
        let dir = temp("newdir");
        let path = dir.join("config.toml");
        FileConfigStore::new(path.clone())
            .save(&Config::first_run(SCREEN))
            .unwrap();
        assert!(path.is_file());
        fs::remove_dir_all(dir).unwrap();
    }
}
