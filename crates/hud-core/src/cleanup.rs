//! Read-only disk cleanup scanner (spec C6). Nothing here deletes, moves or modifies a file:
//! it only measures. Filesystem and system probes sit behind traits so tests use fakes.
//!
//! Links, junctions and any other reparse point are never followed nor counted.

use crate::format;
use crate::metrics::Metric;
use crate::ui_text::{
    category, item_count, missing_env_var, DOCKER_HINT, NEEDS_ADMIN, PARTIAL_READ_TIP,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const DAY_SECS: u64 = 24 * 60 * 60;
pub const TEMP_MIN_AGE_SECS: u64 = DAY_SECS;
pub const NODE_MODULES_MIN_AGE_SECS: u64 = 90 * DAY_SECS;
pub const DOWNLOADS_MIN_AGE_SECS: u64 = 180 * DAY_SECS;
pub(crate) const PROJECT_MAX_DEPTH: u32 = 5;

/// Folders never searched for projects: hidden ones (`.git`, `.venv`, `.cache`) and build output.
pub(crate) fn skipped_project_dir(name: &str) -> bool {
    name.starts_with('.') || name == "target" || name == "AppData"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError {
    NotFound,
    Io(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    File {
        len: u64,
        modified: Option<u64>,
    },
    Dir {
        modified: Option<u64>,
    },
    /// Symlink, junction or any reparse point: never followed.
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub kind: EntryKind,
}

impl Entry {
    pub(crate) fn name(&self) -> &str {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
    }
}

/// Filesystem access. `list` and `stat` must not follow links (use `symlink_metadata`).
pub trait ScanFs {
    fn list(&self, dir: &Path) -> Result<Vec<Entry>, ScanError>;
    fn stat(&self, path: &Path) -> Result<Entry, ScanError>;
}

/// Machine facts the scanner needs besides the filesystem.
pub trait SystemProbe {
    fn is_admin(&self) -> bool;
    fn docker_installed(&self) -> bool;
    fn recycle_bin(&self) -> Result<Usage, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub bytes: u64,
    pub items: u64,
    /// Some folder could not be read, so `bytes` is a lower bound.
    pub incomplete: bool,
}

impl Usage {
    fn add(&mut self, other: Usage) {
        self.bytes += other.bytes;
        self.items += other.items;
        self.incomplete |= other.incomplete;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryId {
    Temp,
    RecycleBin,
    BrowserCache,
    NodeModules,
    PackageCaches,
    WindowsUpdate,
    Docker,
    OldDownloads,
}

impl CategoryId {
    pub fn name(self) -> &'static str {
        match self {
            CategoryId::Temp => category::TEMP,
            CategoryId::RecycleBin => category::RECYCLE_BIN,
            CategoryId::BrowserCache => category::BROWSER_CACHE,
            CategoryId::NodeModules => category::NODE_MODULES,
            CategoryId::PackageCaches => category::PACKAGE_CACHES,
            CategoryId::WindowsUpdate => category::WINDOWS_UPDATE,
            CategoryId::Docker => category::DOCKER,
            CategoryId::OldDownloads => category::OLD_DOWNLOADS,
        }
    }

    /// Risky categories start unchecked and are labeled "revisar".
    pub fn review(self) -> bool {
        matches!(
            self,
            CategoryId::WindowsUpdate | CategoryId::Docker | CategoryId::OldDownloads
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Category {
    pub id: CategoryId,
    pub review: bool,
    pub checked: bool,
    pub usage: Metric<Usage>,
}

impl Category {
    pub fn new(id: CategoryId, usage: Metric<Usage>) -> Self {
        let review = id.review();
        let checked = !review && usage.available().is_some();
        Self {
            id,
            review,
            checked,
            usage,
        }
    }

    /// Size column: `"512 MB"`, `"≥ 1.2 GB"` for a partial read, `"—"` when unavailable.
    pub fn size_text(&self) -> String {
        match &self.usage {
            Metric::Available(u) if u.incomplete => format!("≥ {}", format::size(u.bytes)),
            Metric::Available(u) => format::size(u.bytes),
            Metric::Unavailable(_) => "—".to_owned(),
        }
    }

    /// Hover text: the reason for an unavailable value, or what was incomplete.
    pub fn tip(&self) -> String {
        match &self.usage {
            Metric::Available(u) if u.incomplete => PARTIAL_READ_TIP.to_owned(),
            Metric::Available(_) => String::new(),
            Metric::Unavailable(reason) => reason.clone(),
        }
    }

    /// Second line of a row: the reason when unavailable, a partial-read note, or the item count.
    pub fn detail(&self) -> String {
        match &self.usage {
            Metric::Available(u) if !u.incomplete => item_count(u.items),
            _ => self.tip(),
        }
    }

    pub fn items(&self) -> Option<u64> {
        self.usage.available().map(|u| u.items)
    }

    /// Flips the mark; unavailable categories cannot be marked.
    pub fn toggle(&mut self) {
        if self.usage.available().is_some() {
            self.checked = !self.checked;
        }
    }
}

/// Bytes of the marked categories that were measured.
pub fn selected_bytes(categories: &[Category]) -> u64 {
    categories
        .iter()
        .filter(|c| c.checked)
        .filter_map(|c| c.usage.available())
        .map(|u| u.bytes)
        .sum()
}

pub fn selected_text(categories: &[Category]) -> String {
    format::size(selected_bytes(categories))
}

pub(crate) fn is_old(modified: Option<u64>, now: u64, min_age: u64) -> bool {
    modified.is_some_and(|m| now.saturating_sub(m) >= min_age)
}

/// Where each category looks; a missing environment variable becomes the reason.
#[derive(Debug, Clone)]
pub struct Roots {
    pub temp: Result<Vec<PathBuf>, String>,
    pub browsers: Result<Vec<PathBuf>, String>,
    pub projects: Result<Vec<PathBuf>, String>,
    pub packages: Result<Vec<PathBuf>, String>,
    pub windows_update: Result<PathBuf, String>,
    pub downloads: Result<PathBuf, String>,
}

fn under(var: Option<String>, name: &str, subs: &[&str]) -> Result<Vec<PathBuf>, String> {
    let base = var.ok_or_else(|| missing_env_var(name))?;
    Ok(subs.iter().map(|s| Path::new(&base).join(s)).collect())
}

fn first(paths: Result<Vec<PathBuf>, String>) -> Result<PathBuf, String> {
    paths.map(|mut p| p.remove(0))
}

fn concat(
    a: Result<Vec<PathBuf>, String>,
    b: Result<Vec<PathBuf>, String>,
) -> Result<Vec<PathBuf>, String> {
    let (mut a, b) = (a?, b?);
    a.extend(b);
    Ok(a)
}

impl Roots {
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let (local, profile, root) = (get("LOCALAPPDATA"), get("USERPROFILE"), get("SystemRoot"));
        let temp = concat(
            under(get("TEMP"), "TEMP", &[""]),
            under(root.clone(), "SystemRoot", &["Temp"]),
        );
        let packages = concat(
            under(local.clone(), "LOCALAPPDATA", &["npm-cache", "pip\\cache"]),
            under(
                profile.clone(),
                "USERPROFILE",
                &[".nuget\\packages", ".cargo\\registry\\cache"],
            ),
        );
        Self {
            temp,
            browsers: under(
                local,
                "LOCALAPPDATA",
                &["Google\\Chrome\\User Data", "Microsoft\\Edge\\User Data"],
            ),
            projects: under(
                profile.clone(),
                "USERPROFILE",
                &["source", "Documents", "Desktop"],
            ),
            packages,
            windows_update: first(under(
                root,
                "SystemRoot",
                &["SoftwareDistribution\\Download"],
            )),
            downloads: first(under(profile, "USERPROFILE", &["Downloads"])),
        }
    }
}

/// Walks the filesystem through a `ScanFs`, aborting early when `cancel` is set.
pub(crate) struct Walker<'a> {
    pub(crate) fs: &'a dyn ScanFs,
    pub(crate) cancel: &'a AtomicBool,
}

impl Walker<'_> {
    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Lists a directory; a missing one is empty, an unreadable one is flagged.
    pub(crate) fn list(&self, dir: &Path, usage: &mut Usage) -> Vec<Entry> {
        match self.fs.list(dir) {
            Ok(entries) => entries,
            Err(ScanError::NotFound) => Vec::new(),
            Err(ScanError::Io(_)) => {
                usage.incomplete = true;
                Vec::new()
            }
        }
    }

    /// Visits every file under `root` (links skipped) that is at least `min_age` old.
    /// Returns a `Usage` whose only meaning is the `incomplete` flag.
    pub(crate) fn each_file(
        &self,
        root: &Path,
        now: u64,
        min_age: Option<u64>,
        visit: &mut dyn FnMut(&Path, u64),
    ) -> Usage {
        let mut flags = Usage::default();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            if self.cancelled() {
                break;
            }
            for entry in self.list(&dir, &mut flags) {
                match entry.kind {
                    EntryKind::Dir { .. } => pending.push(entry.path),
                    EntryKind::File { len, modified } => {
                        if min_age.is_none_or(|age| is_old(modified, now, age)) {
                            visit(&entry.path, len);
                        }
                    }
                    EntryKind::Link => {}
                }
            }
        }
        flags
    }

    /// Sum of files under `root` (links skipped); `min_age` keeps only files at least that old.
    pub(crate) fn usage(&self, root: &Path, now: u64, min_age: Option<u64>) -> Usage {
        let mut sum = Usage::default();
        let flags = self.each_file(root, now, min_age, &mut |_, len| {
            sum.bytes += len;
            sum.items += 1;
        });
        sum.incomplete = flags.incomplete;
        sum
    }

    fn usage_of_all(&self, roots: &[PathBuf], now: u64, min_age: Option<u64>) -> Usage {
        let mut total = Usage::default();
        for root in roots {
            total.add(self.usage(root, now, min_age));
        }
        total
    }

    /// `Cache` and `Code Cache` folders of every Chromium profile under `user_data_dirs`.
    pub(crate) fn browser_cache_dirs(
        &self,
        user_data_dirs: &[PathBuf],
        flags: &mut Usage,
    ) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        for user_data in user_data_dirs {
            for profile in self.list(user_data, flags) {
                let is_profile = matches!(profile.kind, EntryKind::Dir { .. })
                    && (profile.name() == "Default" || profile.name().starts_with("Profile "));
                if is_profile {
                    for cache in ["Cache", "Code Cache"] {
                        dirs.push(profile.path.join(cache));
                    }
                }
            }
        }
        dirs
    }

    fn browser_caches(&self, user_data_dirs: &[PathBuf], now: u64) -> Usage {
        let mut total = Usage::default();
        for cache in self.browser_cache_dirs(user_data_dirs, &mut total) {
            total.add(self.usage(&cache, now, None));
        }
        total
    }

    /// `node_modules` folders whose parent was untouched for 90+ days, handed to `visit`.
    /// Returns a `Usage` whose only meaning is the `incomplete` flag.
    pub(crate) fn each_old_node_modules(
        &self,
        roots: &[PathBuf],
        now: u64,
        visit: &mut dyn FnMut(&Path),
    ) -> Usage {
        let mut flags = Usage::default();
        let mut pending: Vec<(PathBuf, Option<u64>, u32)> = Vec::new();
        for root in roots {
            match self.fs.stat(root) {
                Ok(Entry {
                    kind: EntryKind::Dir { modified },
                    ..
                }) => pending.push((root.clone(), modified, 0)),
                Ok(_) | Err(ScanError::NotFound) => {}
                Err(ScanError::Io(_)) => flags.incomplete = true,
            }
        }
        while let Some((dir, modified, depth)) = pending.pop() {
            if self.cancelled() {
                break;
            }
            for entry in self.list(&dir, &mut flags) {
                let EntryKind::Dir {
                    modified: child_mtime,
                } = entry.kind
                else {
                    continue;
                };
                if entry.name() == "node_modules" {
                    if is_old(modified, now, NODE_MODULES_MIN_AGE_SECS) {
                        visit(&entry.path);
                    }
                } else if depth < PROJECT_MAX_DEPTH && !skipped_project_dir(entry.name()) {
                    pending.push((entry.path, child_mtime, depth + 1));
                }
            }
        }
        flags
    }

    /// One item per old `node_modules` folder.
    fn old_node_modules(&self, roots: &[PathBuf], now: u64) -> Usage {
        let mut total = Usage::default();
        let mut found = Vec::new();
        let flags = self.each_old_node_modules(roots, now, &mut |p| found.push(p.to_path_buf()));
        total.incomplete = flags.incomplete;
        for path in found {
            let mut folder = self.usage(&path, now, None);
            folder.items = 1;
            total.add(folder);
        }
        total
    }
}

fn measured(
    result: &Result<Vec<PathBuf>, String>,
    f: impl Fn(&[PathBuf]) -> Usage,
) -> Metric<Usage> {
    match result {
        Ok(paths) => Metric::Available(f(paths)),
        Err(reason) => Metric::Unavailable(reason.clone()),
    }
}

fn measured_dir(result: &Result<PathBuf, String>, f: impl Fn(&Path) -> Usage) -> Metric<Usage> {
    match result {
        Ok(dir) => Metric::Available(f(dir)),
        Err(reason) => Metric::Unavailable(reason.clone()),
    }
}

/// Measures every category and hands each one to `on_category` as soon as it is ready.
/// Docker is omitted when it is not installed. Returns early when `cancel` is set.
pub fn scan_all(
    fs: &dyn ScanFs,
    system: &dyn SystemProbe,
    roots: &Roots,
    now: u64,
    cancel: &AtomicBool,
    on_category: &mut dyn FnMut(Category),
) {
    let w = Walker { fs, cancel };
    let mut emit = |id: CategoryId, usage: Metric<Usage>| {
        if !cancel.load(Ordering::Relaxed) {
            on_category(Category::new(id, usage));
        }
    };
    emit(
        CategoryId::Temp,
        measured(&roots.temp, |p| {
            w.usage_of_all(p, now, Some(TEMP_MIN_AGE_SECS))
        }),
    );
    emit(
        CategoryId::RecycleBin,
        match system.recycle_bin() {
            Ok(u) => Metric::Available(u),
            Err(reason) => Metric::Unavailable(reason),
        },
    );
    emit(
        CategoryId::BrowserCache,
        measured(&roots.browsers, |p| w.browser_caches(p, now)),
    );
    emit(
        CategoryId::NodeModules,
        measured(&roots.projects, |p| w.old_node_modules(p, now)),
    );
    emit(
        CategoryId::PackageCaches,
        measured(&roots.packages, |p| w.usage_of_all(p, now, None)),
    );
    let update = if system.is_admin() {
        measured_dir(&roots.windows_update, |d| w.usage(d, now, None))
    } else {
        Metric::unavailable(NEEDS_ADMIN)
    };
    emit(CategoryId::WindowsUpdate, update);
    if system.docker_installed() {
        emit(CategoryId::Docker, Metric::unavailable(DOCKER_HINT));
    }
    emit(
        CategoryId::OldDownloads,
        measured_dir(&roots.downloads, |d| {
            w.usage(d, now, Some(DOWNLOADS_MIN_AGE_SECS))
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::MIB;
    use std::collections::HashMap;

    const NOW: u64 = 1_800_000_000;

    fn days_ago(days: u64) -> Option<u64> {
        Some(NOW - days * DAY_SECS)
    }

    #[derive(Default)]
    struct FakeFs {
        dirs: HashMap<PathBuf, Vec<Entry>>,
        unreadable: Vec<PathBuf>,
    }

    impl FakeFs {
        fn add(&mut self, dir: &str, name: &str, kind: EntryKind) {
            let path = Path::new(dir).join(name);
            self.dirs
                .entry(PathBuf::from(dir))
                .or_default()
                .push(Entry { path, kind });
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
            if self.unreadable.iter().any(|p| p == dir) {
                return Err(ScanError::Io("denied".into()));
            }
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
        elevated: bool,
        docker: bool,
    }

    impl SystemProbe for FakeSystem {
        fn is_admin(&self) -> bool {
            self.elevated
        }
        fn docker_installed(&self) -> bool {
            self.docker
        }
        fn recycle_bin(&self) -> Result<Usage, String> {
            Ok(Usage {
                bytes: 5 * MIB,
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

    fn scan(fs: &FakeFs, elevated: bool, docker: bool) -> Vec<Category> {
        let mut out = Vec::new();
        let cancel = AtomicBool::new(false);
        let sys = FakeSystem { elevated, docker };
        scan_all(fs, &sys, &roots(), NOW, &cancel, &mut |c| out.push(c));
        out
    }

    fn find(cats: &[Category], id: CategoryId) -> &Category {
        cats.iter().find(|c| c.id == id).expect("category present")
    }

    fn bytes(c: &Category) -> u64 {
        c.usage.available().expect("available").bytes
    }

    #[test]
    fn only_node_modules_of_projects_idle_for_90_days_are_listed() {
        let mut fs = FakeFs::default();
        fs.dir("C:\\", "src", 1);
        fs.dir("C:\\src", "proj-a", 120);
        fs.dir("C:\\src", "proj-b", 10);
        fs.dir("C:\\src\\proj-a", "node_modules", 1);
        fs.dir("C:\\src\\proj-b", "node_modules", 1);
        fs.file("C:\\src\\proj-a\\node_modules", "big.bin", 500 * MIB, 1);
        fs.file("C:\\src\\proj-b\\node_modules", "big.bin", 300 * MIB, 1);
        let cats = scan(&fs, true, false);
        let nm = find(&cats, CategoryId::NodeModules);
        assert_eq!(bytes(nm), 500 * MIB);
        assert_eq!(nm.items(), Some(1));
        assert!(nm.checked && !nm.review);
    }

    #[test]
    fn hidden_folders_are_not_searched_for_projects() {
        let mut fs = FakeFs::default();
        fs.dir("C:\\", "src", 1);
        fs.dir("C:\\src", ".venv", 200);
        fs.dir("C:\\src\\.venv", "node_modules", 1);
        fs.file("C:\\src\\.venv\\node_modules", "x.bin", MIB, 1);
        let cats = scan(&fs, true, false);
        assert_eq!(bytes(find(&cats, CategoryId::NodeModules)), 0);
    }

    #[test]
    fn old_downloads_are_unchecked_and_marked_for_review() {
        let mut fs = FakeFs::default();
        fs.file("C:\\dl", "old.zip", 10 * MIB, 200);
        fs.file("C:\\dl", "new.zip", 99 * MIB, 5);
        let cats = scan(&fs, true, false);
        let dl = find(&cats, CategoryId::OldDownloads);
        assert_eq!(bytes(dl), 10 * MIB);
        assert!(!dl.checked);
        assert!(dl.review);
    }

    #[test]
    fn links_are_not_followed_nor_counted() {
        let mut fs = FakeFs::default();
        fs.file("C:\\t", "old.tmp", MIB, 3);
        fs.add("C:\\t", "to-users", EntryKind::Link);
        fs.file("C:\\Users", "huge.bin", 900 * MIB, 3);
        fs.file("C:\\t\\to-users", "huge.bin", 900 * MIB, 3);
        let cats = scan(&fs, true, false);
        assert_eq!(bytes(find(&cats, CategoryId::Temp)), MIB);
    }

    #[test]
    fn windows_update_without_admin_is_unavailable() {
        let mut fs = FakeFs::default();
        fs.file("C:\\wu", "a.cab", MIB, 3);
        let cats = scan(&fs, false, false);
        let wu = find(&cats, CategoryId::WindowsUpdate);
        assert_eq!(wu.usage, Metric::unavailable(NEEDS_ADMIN));
        assert!(!wu.checked && wu.review);
        let admin = scan(&fs, true, false);
        assert_eq!(bytes(find(&admin, CategoryId::WindowsUpdate)), MIB);
    }

    #[test]
    fn temp_counts_only_files_older_than_24_hours() {
        let mut fs = FakeFs::default();
        fs.file("C:\\t", "old", MIB, 2);
        fs.file("C:\\t", "fresh", 7 * MIB, 0);
        fs.dir("C:\\t", "sub", 0);
        fs.file("C:\\t\\sub", "deep-old", MIB, 5);
        let cats = scan(&fs, true, false);
        let t = find(&cats, CategoryId::Temp);
        assert_eq!(bytes(t), 2 * MIB);
        assert_eq!(t.items(), Some(2));
    }

    #[test]
    fn file_with_unknown_mtime_is_never_treated_as_old() {
        let mut fs = FakeFs::default();
        fs.add(
            "C:\\dl",
            "mystery",
            EntryKind::File {
                len: MIB,
                modified: None,
            },
        );
        let cats = scan(&fs, true, false);
        assert_eq!(bytes(find(&cats, CategoryId::OldDownloads)), 0);
    }

    #[test]
    fn docker_is_shown_only_when_installed_and_never_measured() {
        let fs = FakeFs::default();
        assert!(scan(&fs, true, false)
            .iter()
            .all(|c| c.id != CategoryId::Docker));
        let cats = scan(&fs, true, true);
        let d = find(&cats, CategoryId::Docker);
        assert!(!d.checked && d.review);
        assert_eq!(d.usage, Metric::unavailable(DOCKER_HINT));
    }

    #[test]
    fn safe_categories_start_checked_and_risky_ones_do_not() {
        let fs = FakeFs::default();
        let cats = scan(&fs, true, true);
        for c in &cats {
            let risky = matches!(
                c.id,
                CategoryId::Docker | CategoryId::OldDownloads | CategoryId::WindowsUpdate
            );
            assert_eq!(c.review, risky, "{:?}", c.id);
            assert_eq!(
                c.checked,
                !risky && c.usage.available().is_some(),
                "{:?}",
                c.id
            );
        }
        assert!(find(&cats, CategoryId::Temp).checked);
    }

    #[test]
    fn browser_caches_sum_cache_and_code_cache_of_each_profile() {
        let mut fs = FakeFs::default();
        fs.dir("C:\\ud", "Default", 1);
        fs.dir("C:\\ud", "Profile 2", 1);
        fs.dir("C:\\ud", "Crashpad", 1);
        fs.dir("C:\\ud\\Default", "Cache", 1);
        fs.dir("C:\\ud\\Default", "Code Cache", 1);
        fs.dir("C:\\ud\\Default", "Cookies", 1);
        fs.dir("C:\\ud\\Profile 2", "Cache", 1);
        fs.file("C:\\ud\\Default\\Cache", "a", MIB, 0);
        fs.file("C:\\ud\\Default\\Code Cache", "b", 2 * MIB, 0);
        fs.file("C:\\ud\\Default\\Cookies", "c", 50 * MIB, 0);
        fs.file("C:\\ud\\Profile 2\\Cache", "d", 4 * MIB, 0);
        fs.file("C:\\ud\\Crashpad", "e", 60 * MIB, 0);
        let cats = scan(&fs, true, false);
        assert_eq!(bytes(find(&cats, CategoryId::BrowserCache)), 7 * MIB);
    }

    #[test]
    fn unreadable_folder_marks_the_total_as_a_lower_bound() {
        let mut fs = FakeFs::default();
        fs.file("C:\\pk", "a", 3 * MIB, 0);
        fs.dir("C:\\pk", "locked", 0);
        fs.unreadable.push("C:\\pk\\locked".into());
        let cats = scan(&fs, true, false);
        let p = find(&cats, CategoryId::PackageCaches);
        assert_eq!(p.size_text(), "≥ 3.0 MB");
        assert!(!p.tip().is_empty());
    }

    #[test]
    fn missing_environment_variable_becomes_the_reason() {
        let r = Roots::from_lookup(|name| (name != "LOCALAPPDATA").then(|| "C:\\u".to_owned()));
        assert_eq!(r.browsers.unwrap_err(), missing_env_var("LOCALAPPDATA"));
        assert_eq!(r.packages.unwrap_err(), missing_env_var("LOCALAPPDATA"));
        assert!(r.temp.is_ok() && r.downloads.is_ok());
    }

    #[test]
    fn roots_follow_the_plan_table() {
        let r = Roots::from_lookup(|name| Some(format!("C:\\{name}")));
        assert_eq!(
            r.temp.unwrap(),
            vec![
                PathBuf::from("C:\\TEMP\\"),
                PathBuf::from("C:\\SystemRoot\\Temp")
            ]
        );
        assert_eq!(
            r.downloads.unwrap(),
            PathBuf::from("C:\\USERPROFILE\\Downloads")
        );
        assert_eq!(r.projects.unwrap().len(), 3);
        assert_eq!(r.packages.unwrap().len(), 4);
    }

    #[test]
    fn selected_total_sums_only_checked_available_categories() {
        let fs = FakeFs::default();
        let mut cats = scan(&fs, false, false);
        assert_eq!(selected_bytes(&cats), 5 * MIB);
        assert_eq!(selected_text(&cats), "5.0 MB");
        let bin = cats
            .iter()
            .position(|c| c.id == CategoryId::RecycleBin)
            .expect("bin");
        cats[bin].toggle();
        assert_eq!(selected_bytes(&cats), 0);
    }

    #[test]
    fn unavailable_category_cannot_be_checked_and_shows_a_dash() {
        let mut c = Category::new(CategoryId::WindowsUpdate, Metric::unavailable(NEEDS_ADMIN));
        c.toggle();
        assert!(!c.checked);
        assert_eq!(c.size_text(), "—");
        assert_eq!(c.tip(), NEEDS_ADMIN);
    }

    #[test]
    fn detail_shows_item_count_or_the_reason() {
        let ok = Category::new(
            CategoryId::Temp,
            Metric::Available(Usage {
                bytes: 1,
                items: 12,
                incomplete: false,
            }),
        );
        assert_eq!(ok.detail(), item_count(12));
        let no = Category::new(CategoryId::WindowsUpdate, Metric::unavailable(NEEDS_ADMIN));
        assert_eq!(no.detail(), NEEDS_ADMIN);
    }

    #[test]
    fn cancel_stops_the_scan_and_emits_nothing() {
        let fs = FakeFs::default();
        let cancel = AtomicBool::new(true);
        let mut out = Vec::new();
        let sys = FakeSystem {
            elevated: true,
            docker: true,
        };
        scan_all(&fs, &sys, &roots(), NOW, &cancel, &mut |c| out.push(c));
        assert!(out.is_empty());
    }
}
