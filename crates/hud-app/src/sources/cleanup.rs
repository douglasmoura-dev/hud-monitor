//! Real filesystem and system probes for the read-only cleanup scanner.
//! Nothing here writes: it lists directories and reads metadata without following links.

use crate::winffi;
use hud_core::cleanup::{Entry, EntryKind, ScanError, ScanFs, SystemProbe, Usage};
use std::{
    fs::{self, Metadata},
    io::ErrorKind,
    os::windows::fs::MetadataExt,
    path::Path,
    time::UNIX_EPOCH,
};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

pub struct StdFs;

fn scan_error(e: &std::io::Error) -> ScanError {
    if e.kind() == ErrorKind::NotFound {
        ScanError::NotFound
    } else {
        ScanError::Io(e.to_string())
    }
}

fn modified_secs(meta: &Metadata) -> Option<u64> {
    let age = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(age.as_secs())
}

/// Junctions and symlinks are both reparse points; either one becomes `Link`.
fn kind_of(meta: &Metadata) -> EntryKind {
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || meta.is_symlink() {
        EntryKind::Link
    } else if meta.is_dir() {
        EntryKind::Dir {
            modified: modified_secs(meta),
        }
    } else {
        EntryKind::File {
            len: meta.len(),
            modified: modified_secs(meta),
        }
    }
}

impl ScanFs for StdFs {
    fn list(&self, dir: &Path) -> Result<Vec<Entry>, ScanError> {
        let mut entries = Vec::new();
        for item in fs::read_dir(dir).map_err(|e| scan_error(&e))? {
            let item = item.map_err(|e| scan_error(&e))?;
            // DirEntry::metadata does not follow links on Windows.
            let meta = item.metadata().map_err(|e| scan_error(&e))?;
            entries.push(Entry {
                path: item.path(),
                kind: kind_of(&meta),
            });
        }
        Ok(entries)
    }

    fn stat(&self, path: &Path) -> Result<Entry, ScanError> {
        let meta = fs::symlink_metadata(path).map_err(|e| scan_error(&e))?;
        Ok(Entry {
            path: path.to_path_buf(),
            kind: kind_of(&meta),
        })
    }
}

pub struct WinSystem;

fn docker_in(path_var: &std::ffi::OsStr) -> bool {
    std::env::split_paths(path_var).any(|dir| dir.join("docker.exe").is_file())
}

impl SystemProbe for WinSystem {
    fn is_admin(&self) -> bool {
        winffi::is_elevated()
    }

    /// Looks for docker.exe on PATH; never starts it (the daemon may be down).
    fn docker_installed(&self) -> bool {
        std::env::var_os("PATH").is_some_and(|p| docker_in(&p))
    }

    fn recycle_bin(&self) -> Result<Usage, String> {
        let (bytes, items) = winffi::recycle_bin_info()?;
        Ok(Usage {
            bytes,
            items,
            incomplete: false,
        })
    }
}

pub fn env_roots() -> hud_core::cleanup::Roots {
    hud_core::cleanup::Roots::from_lookup(|name| std::env::var(name).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hud_core::cleanup::{scan_all, CategoryId, Roots};
    use hud_core::metrics::Metric;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::AtomicBool;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hud-cleanup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn make_junction(link: &Path, target: &Path) -> bool {
        Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    struct NoSystem;

    impl SystemProbe for NoSystem {
        fn is_admin(&self) -> bool {
            false
        }
        fn docker_installed(&self) -> bool {
            false
        }
        fn recycle_bin(&self) -> Result<Usage, String> {
            Err("fake".into())
        }
    }

    fn packages_bytes(root: &Path) -> u64 {
        let nowhere = || Err("n/a".to_owned());
        let roots = Roots {
            temp: nowhere(),
            browsers: nowhere(),
            projects: nowhere(),
            packages: Ok(vec![root.to_path_buf()]),
            windows_update: Err("n/a".to_owned()),
            downloads: Err("n/a".to_owned()),
        };
        let mut found = None;
        let cancel = AtomicBool::new(false);
        scan_all(&StdFs, &NoSystem, &roots, 0, &cancel, &mut |c| {
            if c.id == CategoryId::PackageCaches {
                found = Some(c);
            }
        });
        match found.map(|c| c.usage) {
            Some(Metric::Available(u)) => u.bytes,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn tree_with_outside_data(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = temp_dir(name);
        let outside = base.join("outside");
        let scanned = base.join("scanned");
        fs::create_dir_all(&outside).expect("outside");
        fs::create_dir_all(&scanned).expect("scanned");
        fs::write(outside.join("big.bin"), vec![0u8; 4096]).expect("big");
        fs::write(scanned.join("own.bin"), vec![0u8; 100]).expect("own");
        (base, outside, scanned)
    }

    #[test]
    fn junction_inside_the_scanned_tree_is_not_followed() {
        let (base, outside, scanned) = tree_with_outside_data("junction");
        assert!(make_junction(&scanned.join("link"), &outside), "mklink /J");
        assert_eq!(packages_bytes(&scanned), 100);
        assert!(
            outside.join("big.bin").exists(),
            "scan must not touch files"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn symlink_inside_the_scanned_tree_is_not_followed_when_creatable() {
        let (base, outside, scanned) = tree_with_outside_data("symlink");
        let made = std::os::windows::fs::symlink_dir(&outside, scanned.join("link")).is_ok();
        if made {
            assert_eq!(packages_bytes(&scanned), 100);
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn real_listing_reports_files_dirs_and_missing_folders() {
        let base = temp_dir("list");
        fs::create_dir_all(base.join("sub")).expect("sub");
        fs::write(base.join("f.bin"), vec![0u8; 7]).expect("file");
        let mut entries = StdFs.list(&base).expect("list");
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        assert!(matches!(entries[0].kind, EntryKind::File { len: 7, .. }));
        assert!(matches!(entries[1].kind, EntryKind::Dir { .. }));
        assert_eq!(StdFs.list(&base.join("nope")), Err(ScanError::NotFound));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn docker_is_detected_by_docker_exe_on_path() {
        let dir = temp_dir("docker");
        let path = std::env::join_paths([dir.as_path()]).expect("join");
        assert!(!docker_in(&path));
        fs::write(dir.join("docker.exe"), b"").expect("exe");
        assert!(docker_in(&path));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recycle_bin_query_answers_or_explains() {
        if let Err(reason) = WinSystem.recycle_bin() {
            assert!(!reason.is_empty());
        }
    }
}
