//! Real filesystem and system probes for the cleanup scanner, plus the Recycle Bin backend.
//! Scanning only lists directories and reads metadata without following links; the only thing
//! that ever changes the disk is `ShellRecycler`, which sends items to the Recycle Bin.

use crate::winffi;
use hud_core::cleanup::{Entry, EntryKind, ScanError, ScanFs, SystemProbe, Usage};
use hud_core::removal::Recycler;
use std::{
    fs::{self, Metadata},
    io::ErrorKind,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
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

/// Real Recycle Bin backend (`IFileOperation` with recycle-on-delete).
pub struct ShellRecycler;

const LINK_REASON: &str = "é um link ou junção; não é seguido";
const GONE_REASON: &str = "já não existe";
const STUCK_REASON: &str = "em uso por outro programa ou sem permissão";

/// Checked right before recycling: only plain files and folders are handed to the shell.
fn eligible(path: &Path) -> Result<(), String> {
    match StdFs.stat(path) {
        Ok(Entry {
            kind: EntryKind::Link,
            ..
        }) => Err(LINK_REASON.to_owned()),
        Ok(_) => Ok(()),
        Err(ScanError::NotFound) => Err(GONE_REASON.to_owned()),
        Err(ScanError::Io(reason)) => Err(reason),
    }
}

fn still_there(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

impl Recycler for ShellRecycler {
    fn recycle(&self, paths: &[PathBuf]) -> Vec<Result<(), String>> {
        let checked: Vec<Result<(), String>> = paths.iter().map(|p| eligible(p)).collect();
        let queued: Vec<PathBuf> = paths
            .iter()
            .zip(&checked)
            .filter(|(_, ok)| ok.is_ok())
            .map(|(p, _)| p.clone())
            .collect();
        let call = if queued.is_empty() {
            Ok(())
        } else {
            winffi::recycle_paths(&queued)
        };
        paths
            .iter()
            .zip(checked)
            .map(|(path, check)| {
                check?;
                if still_there(path) {
                    Err(call
                        .clone()
                        .err()
                        .unwrap_or_else(|| STUCK_REASON.to_owned()))
                } else {
                    Ok(())
                }
            })
            .collect()
    }

    fn empty_recycle_bin(&self) -> Result<(), String> {
        winffi::empty_recycle_bin()
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

    fn bin_items() -> Option<u64> {
        winffi::recycle_bin_info().ok().map(|(_, items)| items)
    }

    #[test]
    fn real_recycle_moves_temp_files_and_a_folder_into_the_recycle_bin() {
        let base = temp_dir("recycle");
        let mut paths = Vec::new();
        for name in ["a.tmp", "b.tmp", "c.tmp"] {
            let file = base.join(name);
            fs::write(&file, b"hud-monitor test file").expect("file");
            paths.push(file);
        }
        let folder = base.join("node_modules");
        fs::create_dir_all(folder.join("pkg")).expect("folder");
        fs::write(folder.join("pkg").join("index.js"), b"x").expect("inner");
        paths.push(folder);
        let before = bin_items();
        let results = ShellRecycler.recycle(&paths);
        assert!(results.iter().all(Result::is_ok), "{results:?}");
        assert!(
            paths.iter().all(|p| !p.exists()),
            "items must leave the disk"
        );
        if let (Some(before), Some(after)) = (before, bin_items()) {
            assert!(after >= before + 4, "bin {before} -> {after}");
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn real_recycle_skips_a_file_locked_by_another_handle_and_moves_the_rest() {
        use std::os::windows::fs::OpenOptionsExt;
        let base = temp_dir("locked");
        let locked = base.join("locked.tmp");
        let free = base.join("free.tmp");
        fs::write(&locked, b"locked").expect("locked");
        fs::write(&free, b"free").expect("free");
        let _holder = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked)
            .expect("lock");
        let results = ShellRecycler.recycle(&[locked.clone(), free.clone()]);
        assert!(results[0].is_err(), "locked item must be reported");
        assert!(locked.exists());
        assert!(results[1].is_ok(), "{results:?}");
        assert!(!free.exists());
        drop(_holder);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn real_recycle_never_follows_a_junction_and_reports_missing_items() {
        let (base, outside, scanned) = tree_with_outside_data("recycle-junction");
        let link = scanned.join("link");
        assert!(make_junction(&link, &outside), "mklink /J");
        let missing = scanned.join("gone.tmp");
        let results = ShellRecycler.recycle(&[link.clone(), missing]);
        assert_eq!(results[0], Err(LINK_REASON.to_owned()));
        assert_eq!(results[1], Err(GONE_REASON.to_owned()));
        assert!(
            outside.join("big.bin").exists(),
            "junction target untouched"
        );
        assert!(link.exists(), "the junction itself was not touched");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn app_code_has_no_permanent_deletion_calls() {
        let sources = [include_str!("cleanup.rs"), include_str!("../winffi.rs")];
        for src in sources {
            let code = src.split("#[cfg(test)]").next().unwrap_or_default();
            for forbidden in [
                "remove_file",
                "remove_dir",
                "DeleteFileW",
                "RemoveDirectoryW",
            ] {
                assert!(!code.contains(forbidden), "{forbidden}");
            }
            assert!(
                !code.contains("FOF_NOCONFIRMATION"),
                "silent permanent delete risk"
            );
        }
    }
}
