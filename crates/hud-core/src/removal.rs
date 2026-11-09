//! Moves the cleanup categories to the Recycle Bin (spec C6.3).
//!
//! Nothing here deletes permanently: files only go through a `Recycler`, whose real
//! implementation sends them to the Recycle Bin. The Recycle Bin category itself is the one
//! exception and is emptied only through `Recycler::empty_recycle_bin`.
//!
//! The rules of the scanner are checked again when the plan is built, so a file touched after
//! the scan is no longer selected. Links, junctions and reparse points are never listed.

use crate::cleanup::Usage;
use crate::cleanup::{
    Category, CategoryId, Roots, ScanFs, SystemProbe, Walker, DOWNLOADS_MIN_AGE_SECS,
    TEMP_MIN_AGE_SECS,
};
use crate::format;
use crate::ui_text::{
    freed, items_moved_to_bin, more_skipped, skipped_count, BIN_EMPTIED, INTERRUPTED,
    NO_SYSTEM_ANSWER, RECYCLE_BIN_NAME,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Items handed to the recycler per call; progress is reported between batches.
pub const BATCH: usize = 50;
/// How many skipped items the result message lists.
pub const SKIPPED_SHOWN: usize = 5;

/// Whatever sends things to the Recycle Bin. The real one is in `hud-app`; tests use a fake.
pub trait Recycler {
    /// One result per path, in order. `Err` carries why that item stayed where it was.
    fn recycle(&self, paths: &[PathBuf]) -> Vec<Result<(), String>>;
    /// Empties the Recycle Bin of every drive, without asking again.
    fn empty_recycle_bin(&self) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Work {
    Items(Vec<Item>),
    EmptyBin { bytes: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryPlan {
    pub id: CategoryId,
    pub work: Work,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub categories: Vec<CategoryPlan>,
}

impl Plan {
    /// Number of recycle actions (an emptied bin counts as one step).
    pub fn steps(&self) -> usize {
        self.categories
            .iter()
            .map(|c| match &c.work {
                Work::Items(items) => items.len(),
                Work::EmptyBin { .. } => 1,
            })
            .sum()
    }
}

/// Categories the user marked and that may be removed: measured, and never Docker.
pub fn selected_ids(categories: &[Category]) -> Vec<CategoryId> {
    categories
        .iter()
        .filter(|c| c.checked && c.usage.available().is_some() && c.id != CategoryId::Docker)
        .map(|c| c.id)
        .collect()
}

/// The marked categories, one line each, for the confirmation step.
pub fn confirmation_lines(categories: &[Category]) -> Vec<String> {
    let ids = selected_ids(categories);
    categories
        .iter()
        .filter(|c| ids.contains(&c.id))
        .map(|c| format!("{} — {}", c.id.name(), c.size_text()))
        .collect()
}

fn collect_files(
    w: &Walker,
    roots: &[PathBuf],
    now: u64,
    min_age: Option<u64>,
    out: &mut Vec<Item>,
) {
    for root in roots {
        w.each_file(root, now, min_age, &mut |path, bytes| {
            out.push(Item {
                path: path.to_path_buf(),
                bytes,
            });
        });
    }
}

fn files_in(w: &Walker, roots: &[PathBuf], now: u64, min_age: Option<u64>) -> Vec<Item> {
    let mut items = Vec::new();
    collect_files(w, roots, now, min_age, &mut items);
    items
}

fn browser_cache_items(w: &Walker, profiles_roots: &[PathBuf], now: u64) -> Vec<Item> {
    let mut flags = Usage::default();
    let caches = w.browser_cache_dirs(profiles_roots, &mut flags);
    files_in(w, &caches, now, None)
}

fn node_modules_items(w: &Walker, projects: &[PathBuf], now: u64) -> Vec<Item> {
    let mut folders = Vec::new();
    w.each_old_node_modules(projects, now, &mut |p| folders.push(p.to_path_buf()));
    folders
        .into_iter()
        .map(|path| {
            let bytes = w.usage(&path, now, None).bytes;
            Item { path, bytes }
        })
        .collect()
}

fn collect_one(
    w: &Walker,
    system: &dyn SystemProbe,
    roots: &Roots,
    id: CategoryId,
    now: u64,
) -> Option<Work> {
    let items = match id {
        CategoryId::Temp => files_in(w, roots.temp.as_ref().ok()?, now, Some(TEMP_MIN_AGE_SECS)),
        CategoryId::RecycleBin => {
            let usage = system.recycle_bin().ok()?;
            return Some(Work::EmptyBin { bytes: usage.bytes });
        }
        CategoryId::BrowserCache => browser_cache_items(w, roots.browsers.as_ref().ok()?, now),
        CategoryId::NodeModules => node_modules_items(w, roots.projects.as_ref().ok()?, now),
        CategoryId::PackageCaches => files_in(w, roots.packages.as_ref().ok()?, now, None),
        CategoryId::WindowsUpdate if system.is_admin() => {
            let dir = roots.windows_update.as_ref().ok()?;
            files_in(w, std::slice::from_ref(dir), now, None)
        }
        CategoryId::OldDownloads => {
            let dir = roots.downloads.as_ref().ok()?;
            let min_age = Some(DOWNLOADS_MIN_AGE_SECS);
            files_in(w, std::slice::from_ref(dir), now, min_age)
        }
        CategoryId::WindowsUpdate | CategoryId::Docker => return None,
    };
    Some(Work::Items(items))
}

/// Lists what would be moved for each id, applying the scanner rules again at `now`.
/// Docker and categories that cannot be measured now produce no entry.
pub fn plan_removal(
    ids: &[CategoryId],
    fs: &dyn ScanFs,
    system: &dyn SystemProbe,
    roots: &Roots,
    now: u64,
    cancel: &AtomicBool,
) -> Plan {
    let w = Walker { fs, cancel };
    let mut plan = Plan::default();
    for &id in ids {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if let Some(work) = collect_one(&w, system, roots, id, now) {
            plan.categories.push(CategoryPlan { id, work });
        }
    }
    plan
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub moved_items: u64,
    /// Bytes of the items that really moved; skipped items are not counted.
    pub freed_bytes: u64,
    pub emptied_bin: bool,
    pub skipped: Vec<Skipped>,
    pub cancelled: bool,
}

impl Report {
    /// The freed-space line plus what was skipped; the amount is what really moved.
    pub fn message(&self) -> String {
        let mut text = freed(&format::size(self.freed_bytes));
        let mut parts = Vec::new();
        if self.moved_items > 0 {
            parts.push(items_moved_to_bin(self.moved_items));
        }
        if self.emptied_bin {
            parts.push(BIN_EMPTIED.to_owned());
        }
        if !self.skipped.is_empty() {
            parts.push(skipped_count(self.skipped.len()));
        }
        if self.cancelled {
            parts.push(INTERRUPTED.to_owned());
        }
        if !parts.is_empty() {
            text.push_str(" (");
            text.push_str(&parts.join("; "));
            text.push(')');
        }
        text
    }

    /// The first skipped items as `"path — reason"`, for the "could not move" list.
    pub fn skipped_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .skipped
            .iter()
            .take(SKIPPED_SHOWN)
            .map(|s| format!("{} — {}", s.path, s.reason))
            .collect();
        if self.skipped.len() > SKIPPED_SHOWN {
            lines.push(more_skipped(self.skipped.len() - SKIPPED_SHOWN));
        }
        lines
    }
}

fn recycle_items(
    items: &[Item],
    recycler: &dyn Recycler,
    cancel: &AtomicBool,
    report: &mut Report,
    done: &mut usize,
    on_progress: &mut dyn FnMut(usize),
) {
    for batch in items.chunks(BATCH) {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            return;
        }
        let paths: Vec<PathBuf> = batch.iter().map(|i| i.path.clone()).collect();
        let results = recycler.recycle(&paths);
        for (index, item) in batch.iter().enumerate() {
            match results.get(index) {
                Some(Ok(())) => {
                    report.moved_items += 1;
                    report.freed_bytes += item.bytes;
                }
                Some(Err(reason)) => report.skipped.push(skip(&item.path, reason)),
                None => report.skipped.push(skip(&item.path, NO_SYSTEM_ANSWER)),
            }
        }
        *done += batch.len();
        on_progress(*done);
    }
}

fn skip(path: &Path, reason: &str) -> Skipped {
    Skipped {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}

/// Runs the plan: every item goes to the Recycle Bin through `recycler`. A failing item is
/// skipped and listed; the rest continues. `on_progress` gets the number of steps done so far.
pub fn execute(
    plan: &Plan,
    recycler: &dyn Recycler,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(usize),
) -> Report {
    let mut report = Report::default();
    let mut done = 0;
    for category in &plan.categories {
        match &category.work {
            Work::Items(items) => {
                recycle_items(items, recycler, cancel, &mut report, &mut done, on_progress);
            }
            Work::EmptyBin { bytes } => {
                if cancel.load(Ordering::Relaxed) {
                    report.cancelled = true;
                } else {
                    match recycler.empty_recycle_bin() {
                        Ok(()) => {
                            report.emptied_bin = true;
                            report.freed_bytes += bytes;
                        }
                        Err(reason) => report
                            .skipped
                            .push(skip(Path::new(RECYCLE_BIN_NAME), &reason)),
                    }
                    done += 1;
                    on_progress(done);
                }
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::{Entry, EntryKind, ScanError, DAY_SECS};
    use crate::metrics::Metric;
    use crate::ui_text::category;
    use std::cell::RefCell;
    use std::collections::HashMap;

    const NOW: u64 = 1_800_000_000;

    fn days_ago(days: u64) -> Option<u64> {
        Some(NOW - days * DAY_SECS)
    }

    #[derive(Default)]
    struct FakeFs {
        dirs: HashMap<PathBuf, Vec<Entry>>,
    }

    impl FakeFs {
        fn add(&mut self, dir: &str, name: &str, kind: EntryKind) {
            self.dirs
                .entry(PathBuf::from(dir))
                .or_default()
                .push(Entry {
                    path: Path::new(dir).join(name),
                    kind,
                });
        }

        fn dir(&mut self, parent: &str, name: &str, days: u64) {
            self.add(
                parent,
                name,
                EntryKind::Dir {
                    modified: days_ago(days),
                },
            );
            self.dirs.entry(Path::new(parent).join(name)).or_default();
        }

        fn file(&mut self, parent: &str, name: &str, len: u64, days: u64) {
            self.add(
                parent,
                name,
                EntryKind::File {
                    len,
                    modified: days_ago(days),
                },
            );
        }
    }

    impl ScanFs for FakeFs {
        fn list(&self, dir: &Path) -> Result<Vec<Entry>, ScanError> {
            self.dirs.get(dir).cloned().ok_or(ScanError::NotFound)
        }

        fn stat(&self, path: &Path) -> Result<Entry, ScanError> {
            let parent = path.parent().unwrap_or(path);
            self.dirs
                .get(parent)
                .and_then(|es| es.iter().find(|e| e.path == path))
                .cloned()
                .ok_or(ScanError::NotFound)
        }
    }

    struct FakeSystem {
        admin: bool,
    }

    impl SystemProbe for FakeSystem {
        fn is_admin(&self) -> bool {
            self.admin
        }
        fn docker_installed(&self) -> bool {
            true
        }
        fn recycle_bin(&self) -> Result<Usage, String> {
            Ok(Usage {
                bytes: 7,
                items: 2,
                incomplete: false,
            })
        }
    }

    fn roots() -> Roots {
        Roots {
            temp: Ok(vec!["C:\\t".into()]),
            browsers: Ok(vec!["C:\\ud".into()]),
            projects: Ok(vec!["C:\\src".into()]),
            packages: Ok(vec!["C:\\pk".into()]),
            windows_update: Ok("C:\\wu".into()),
            downloads: Ok("C:\\dl".into()),
        }
    }

    fn plan(fs: &FakeFs, ids: &[CategoryId], admin: bool) -> Plan {
        let cancel = AtomicBool::new(false);
        plan_removal(ids, fs, &FakeSystem { admin }, &roots(), NOW, &cancel)
    }

    fn items_of(plan: &Plan, id: CategoryId) -> Vec<Item> {
        match plan.categories.iter().find(|c| c.id == id).map(|c| &c.work) {
            Some(Work::Items(items)) => items.clone(),
            other => panic!("unexpected {other:?}"),
        }
    }

    /// Records every path it is asked to recycle; paths in `locked` fail, the rest succeed.
    #[derive(Default)]
    struct FakeRecycler {
        locked: Vec<PathBuf>,
        recycled: RefCell<Vec<PathBuf>>,
        bin_emptied: RefCell<u32>,
    }

    impl Recycler for FakeRecycler {
        fn recycle(&self, paths: &[PathBuf]) -> Vec<Result<(), String>> {
            paths
                .iter()
                .map(|p| {
                    if self.locked.contains(p) {
                        Err("locked by another program".to_owned())
                    } else {
                        self.recycled.borrow_mut().push(p.clone());
                        Ok(())
                    }
                })
                .collect()
        }
        fn empty_recycle_bin(&self) -> Result<(), String> {
            *self.bin_emptied.borrow_mut() += 1;
            Ok(())
        }
    }

    fn run(plan: &Plan, recycler: &FakeRecycler) -> Report {
        execute(plan, recycler, &AtomicBool::new(false), &mut |_| {})
    }

    fn category(id: CategoryId, checked: bool, available: bool) -> Category {
        let usage = if available {
            Metric::Available(Usage {
                bytes: 10,
                items: 1,
                incomplete: false,
            })
        } else {
            Metric::unavailable("n/a")
        };
        let mut c = Category::new(id, usage);
        c.checked = checked;
        c
    }

    #[test]
    fn plan_lists_three_temp_files_and_one_node_modules_folder() {
        let mut fs = FakeFs::default();
        for name in ["a.tmp", "b.tmp", "c.tmp"] {
            fs.file("C:\\t", name, 100, 3);
        }
        fs.dir("C:\\", "src", 1);
        fs.dir("C:\\src", "proj", 120);
        fs.dir("C:\\src\\proj", "node_modules", 120);
        fs.file("C:\\src\\proj\\node_modules", "x.js", 500, 120);
        let plan = plan(&fs, &[CategoryId::Temp, CategoryId::NodeModules], false);
        assert_eq!(items_of(&plan, CategoryId::Temp).len(), 3);
        let modules = items_of(&plan, CategoryId::NodeModules);
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].path, Path::new("C:\\src\\proj\\node_modules"));
        assert_eq!(modules[0].bytes, 500);
        assert_eq!(plan.steps(), 4);
    }

    #[test]
    fn rules_are_checked_again_at_removal_time() {
        let mut fs = FakeFs::default();
        fs.file("C:\\t", "old.tmp", 100, 3);
        fs.file("C:\\t", "fresh.tmp", 100, 0);
        fs.dir("C:\\", "src", 1);
        fs.dir("C:\\src", "touched", 10);
        fs.dir("C:\\src\\touched", "node_modules", 400);
        fs.file("C:\\dl", "recent.zip", 9, 10);
        fs.file("C:\\dl", "old.zip", 9, 200);
        let ids = [
            CategoryId::Temp,
            CategoryId::NodeModules,
            CategoryId::OldDownloads,
        ];
        let plan = plan(&fs, &ids, false);
        let temp = items_of(&plan, CategoryId::Temp);
        assert_eq!(temp.len(), 1);
        assert!(temp[0].path.ends_with("old.tmp"));
        assert!(items_of(&plan, CategoryId::NodeModules).is_empty());
        let downloads = items_of(&plan, CategoryId::OldDownloads);
        assert_eq!(downloads.len(), 1);
        assert!(downloads[0].path.ends_with("old.zip"));
    }

    #[test]
    fn links_are_never_planned() {
        let mut fs = FakeFs::default();
        fs.add("C:\\t", "link", EntryKind::Link);
        fs.file("C:\\t", "real.tmp", 1, 3);
        let plan = plan(&fs, &[CategoryId::Temp], false);
        let items = items_of(&plan, CategoryId::Temp);
        assert_eq!(items.len(), 1);
        assert!(items[0].path.ends_with("real.tmp"));
    }

    #[test]
    fn docker_and_windows_update_without_admin_are_never_planned() {
        let mut fs = FakeFs::default();
        fs.file("C:\\wu", "u.bin", 1, 1);
        let ids = [CategoryId::Docker, CategoryId::WindowsUpdate];
        assert!(plan(&fs, &ids, false).categories.is_empty());
        assert_eq!(plan(&fs, &[CategoryId::WindowsUpdate], true).steps(), 1);
    }

    #[test]
    fn recycle_bin_is_planned_as_an_empty_with_its_size() {
        let fs = FakeFs::default();
        let plan = plan(&fs, &[CategoryId::RecycleBin], false);
        assert_eq!(plan.categories[0].work, Work::EmptyBin { bytes: 7 });
    }

    #[test]
    fn browser_caches_come_from_profile_cache_folders_only() {
        let mut fs = FakeFs::default();
        fs.dir("C:\\ud", "Default", 1);
        fs.dir("C:\\ud", "Crashpad", 1);
        fs.dir("C:\\ud\\Default", "Cache", 1);
        fs.dir("C:\\ud\\Default", "Bookmarks-dir", 1);
        fs.file("C:\\ud\\Default\\Cache", "f", 5, 1);
        fs.file("C:\\ud\\Default\\Bookmarks-dir", "keep", 5, 1);
        fs.file("C:\\ud\\Crashpad", "other", 5, 1);
        let items = items_of(
            &plan(&fs, &[CategoryId::BrowserCache], false),
            CategoryId::BrowserCache,
        );
        assert_eq!(items.len(), 1);
        assert!(items[0].path.ends_with("f"));
    }

    #[test]
    fn only_marked_measured_non_docker_categories_are_selected() {
        let cats = [
            category(CategoryId::Temp, true, true),
            category(CategoryId::PackageCaches, false, true),
            category(CategoryId::Docker, true, false),
            category(CategoryId::WindowsUpdate, true, false),
            category(CategoryId::OldDownloads, true, true),
        ];
        assert_eq!(
            selected_ids(&cats),
            vec![CategoryId::Temp, CategoryId::OldDownloads]
        );
        let lines = confirmation_lines(&cats);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(&format!("{} — ", category::TEMP)));
        let docker_forced = [category(CategoryId::Docker, true, true)];
        assert!(selected_ids(&docker_forced).is_empty());
    }

    #[test]
    fn every_planned_item_goes_to_the_recycler_and_the_total_is_the_moved_sum() {
        let mut fs = FakeFs::default();
        for name in ["a", "b", "c"] {
            fs.file("C:\\t", name, 100, 3);
        }
        let plan = plan(&fs, &[CategoryId::Temp], false);
        let recycler = FakeRecycler::default();
        let report = run(&plan, &recycler);
        assert_eq!(recycler.recycled.borrow().len(), 3);
        assert_eq!(report.moved_items, 3);
        assert_eq!(report.freed_bytes, 300);
        assert!(report.skipped.is_empty());
        assert_eq!(
            report.message(),
            format!("{} ({})", freed("300 B"), items_moved_to_bin(3))
        );
    }

    #[test]
    fn a_locked_item_is_skipped_listed_and_the_rest_continues() {
        let mut fs = FakeFs::default();
        for name in ["a", "locked", "c"] {
            fs.file("C:\\t", name, 100, 3);
        }
        let plan = plan(&fs, &[CategoryId::Temp], false);
        let recycler = FakeRecycler {
            locked: vec![PathBuf::from("C:\\t\\locked")],
            ..FakeRecycler::default()
        };
        let report = run(&plan, &recycler);
        assert_eq!(report.moved_items, 2);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].path, "C:\\t\\locked");
        assert_eq!(report.skipped[0].reason, "locked by another program");
        assert!(report.skipped_lines()[0].contains("locked"));
    }

    #[test]
    fn freed_amount_counts_moved_items_not_selected_ones() {
        let mut fs = FakeFs::default();
        fs.file("C:\\t", "small", 100, 3);
        fs.file("C:\\t", "big-locked", 900, 3);
        let plan = plan(&fs, &[CategoryId::Temp], false);
        let recycler = FakeRecycler {
            locked: vec![PathBuf::from("C:\\t\\big-locked")],
            ..FakeRecycler::default()
        };
        let report = run(&plan, &recycler);
        assert_eq!(report.freed_bytes, 100);
        assert!(report.message().starts_with(&freed("100 B")));
        assert!(report.message().contains(&skipped_count(1)));
    }

    #[test]
    fn missing_recycler_answers_count_as_skipped() {
        struct Silent;
        impl Recycler for Silent {
            fn recycle(&self, _: &[PathBuf]) -> Vec<Result<(), String>> {
                Vec::new()
            }
            fn empty_recycle_bin(&self) -> Result<(), String> {
                Err("falhou".into())
            }
        }
        let plan = Plan {
            categories: vec![
                CategoryPlan {
                    id: CategoryId::Temp,
                    work: Work::Items(vec![Item {
                        path: "C:\\t\\a".into(),
                        bytes: 5,
                    }]),
                },
                CategoryPlan {
                    id: CategoryId::RecycleBin,
                    work: Work::EmptyBin { bytes: 9 },
                },
            ],
        };
        let report = execute(&plan, &Silent, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(report.moved_items, 0);
        assert_eq!(report.freed_bytes, 0);
        assert_eq!(report.skipped.len(), 2);
        assert!(!report.emptied_bin);
    }

    #[test]
    fn emptying_the_bin_adds_its_size_and_runs_once() {
        let plan = Plan {
            categories: vec![CategoryPlan {
                id: CategoryId::RecycleBin,
                work: Work::EmptyBin { bytes: 2048 },
            }],
        };
        let recycler = FakeRecycler::default();
        let report = run(&plan, &recycler);
        assert_eq!(*recycler.bin_emptied.borrow(), 1);
        assert_eq!(report.freed_bytes, 2048);
        assert_eq!(
            report.message(),
            format!("{} ({BIN_EMPTIED})", freed("2.0 KB"))
        );
    }

    #[test]
    fn progress_is_reported_per_batch_and_cancel_stops_the_run() {
        let items: Vec<Item> = (0..BATCH * 2 + 1)
            .map(|i| Item {
                path: format!("C:\\t\\{i}").into(),
                bytes: 1,
            })
            .collect();
        let total = items.len();
        let plan = Plan {
            categories: vec![CategoryPlan {
                id: CategoryId::Temp,
                work: Work::Items(items),
            }],
        };
        let mut seen = Vec::new();
        let report = execute(
            &plan,
            &FakeRecycler::default(),
            &AtomicBool::new(false),
            &mut |d| seen.push(d),
        );
        assert_eq!(seen, vec![BATCH, BATCH * 2, total]);
        assert!(!report.cancelled);

        let cancel = AtomicBool::new(false);
        let recycler = FakeRecycler::default();
        let report = execute(&plan, &recycler, &cancel, &mut |_| {
            cancel.store(true, Ordering::Relaxed)
        });
        assert!(report.cancelled);
        assert_eq!(report.moved_items, BATCH as u64);
        assert!(report.message().contains("interrompido"));
    }

    #[test]
    fn skipped_list_is_truncated_with_a_remainder() {
        let report = Report {
            skipped: (0..SKIPPED_SHOWN + 2)
                .map(|i| Skipped {
                    path: format!("p{i}"),
                    reason: "r".into(),
                })
                .collect(),
            ..Report::default()
        };
        let lines = report.skipped_lines();
        assert_eq!(lines.len(), SKIPPED_SHOWN + 1);
        assert_eq!(lines[SKIPPED_SHOWN], more_skipped(2));
    }

    #[test]
    fn no_permanent_deletion_call_exists_in_the_removal_code() {
        let src = include_str!("removal.rs");
        let code = src.split("#[cfg(test)]").next().unwrap_or_default();
        for forbidden in ["remove_file", "remove_dir", "DeleteFile", "RemoveDirectory"] {
            assert!(!code.contains(forbidden), "{forbidden}");
        }
    }
}
