//! The engine: owns the published index, the scanner thread and the inotify
//! watches, and serves searches.
//!
//! One worker thread owns all mutable state (watches, pending changes, snapshot
//! timer). It blocks in `poll` on the inotify fd and a wake-up fd, with a timeout
//! only while something is pending (a debounce or a snapshot write), so an idle
//! index uses no CPU. Searches run on the caller's thread against an
//! `Arc<Index>` taken under a short read lock; a scan builds a new index and
//! swaps it in, so no call waits on a scan.

use crate::config::{Config, Excludes};
use crate::index::{Index, NONE, child_hash, path_hash_root};
use crate::query::{Hit, Options, search};
use crate::recent;
use crate::scan::{changed_dirs, rebuild, scan_full};
use crate::snapshot::{self, CacheDir};
use crate::sys::{EventFd, poll_readable, set_idle_priority};
use crate::uri::path_to_uri;
use crate::watch::{Inotify, budget_from, max_user_watches};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Folders `NotifyChanged` accepts in one call.
pub const MAX_NOTIFY: usize = 256;
/// Folders waiting from `NotifyChanged` calls; past this the hints are dropped
/// and one walk over all folders takes their place.
const MAX_PENDING_NOTIFY: usize = 4096;
/// Longest wait for the snapshot write at shutdown.
const FINAL_WRITE_WAIT: Duration = Duration::from_secs(5);
/// Restarts of a crashed worker before the engine gives up.
const MAX_RESTARTS: u32 = 3;

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub config: Config,
    /// `$XDG_CACHE_HOME` (the snapshot lives below it).
    pub cache_home: PathBuf,
    /// `$XDG_DATA_HOME/recently-used.xbel`.
    pub recent_file: PathBuf,
    /// Watches to use; by default from the kernel's limit (see `watch::budget_from`).
    pub watch_budget: Option<usize>,
    /// Quiet time before changes are applied.
    pub debounce: Duration,
    /// Quiet time before a changed index is written to disk.
    pub snapshot_delay: Duration,
    /// Age of the last mtime check of folders without a watch that makes a
    /// query start another.
    pub recheck_after: Duration,
    /// Pause before the first scan (tests only: makes "scanning" observable).
    pub scan_delay: Duration,
    /// Least time between two full scans (`Refresh`, or a root that came back).
    pub refresh_gap: Duration,
}

impl EngineConfig {
    /// Settings from the environment (`HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`,
    /// `XDG_DATA_HOME`) and `indexrc`.
    pub fn from_env() -> Result<EngineConfig, String> {
        let home = std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .ok_or("HOME is not set")?;
        if !home.is_absolute() {
            return Err("HOME is not an absolute path".into());
        }
        let xdg = |var: &str, default: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(default))
        };
        let config_home = xdg("XDG_CONFIG_HOME", ".config");
        let cache_home = cache_home_from(
            std::env::var_os("CACHE_DIRECTORY"),
            xdg("XDG_CACHE_HOME", ".cache"),
        );
        let data_home = xdg("XDG_DATA_HOME", ".local/share");
        Ok(EngineConfig {
            config: Config::load(&config_home, &home),
            cache_home,
            recent_file: data_home.join("recently-used.xbel"),
            watch_budget: None,
            debounce: Duration::from_millis(300),
            snapshot_delay: Duration::from_secs(30),
            recheck_after: Duration::from_secs(300),
            scan_delay: Duration::ZERO,
            refresh_gap: Duration::from_secs(10),
        })
    }
}

/// The folder the snapshot lives below. systemd sets `$CACHE_DIRECTORY` for a
/// unit with `CacheDirectory=atlas-explorer` (the folder itself, made 0700);
/// it is used when it is absolute and so named, else `xdg_cache`.
fn cache_home_from(cache_directory: Option<std::ffi::OsString>, xdg_cache: PathBuf) -> PathBuf {
    let first = cache_directory
        .as_deref()
        .and_then(|v| v.as_bytes().split(|&b| b == b':').next())
        .map(|b| PathBuf::from(std::ffi::OsStr::from_bytes(b)));
    match first {
        Some(p) if p.is_absolute() && p.file_name().is_some_and(|n| n == "atlas-explorer") => {
            p.parent().map_or(xdg_cache, Path::to_path_buf)
        }
        _ => xdg_cache,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Ready,
    Scanning,
    Stale,
    Disabled,
    Error,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Ready => "ready",
            State::Scanning => "scanning",
            State::Stale => "stale",
            State::Disabled => "disabled",
            State::Error => "error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    pub entries: u32,
    /// Last completed scan or change, seconds since the epoch.
    pub updated: i64,
    /// `file://` URIs.
    pub roots: Vec<String>,
    /// Plain-words reason when `state` is `Error`; else a note (the index is
    /// full, file watching is off), or empty.
    pub error: String,
    /// The index worker failed for good: the service should exit so that its
    /// supervisor starts it again.
    pub fatal: bool,
}

/// Requests from other threads, merged: a flood of calls costs one entry each
/// at most, and never a queue.
#[derive(Default)]
struct Pending {
    notify: HashSet<PathBuf>,
    /// More hints than fit: walk all folders instead.
    overflow: bool,
    refresh: bool,
    recheck: bool,
}

struct Shared {
    index: RwLock<Arc<Index>>,
    status: Mutex<Status>,
    pending: Mutex<Pending>,
    wake: EventFd,
    stop: AtomicBool,
    start: Instant,
    /// Milliseconds since `start` of the last mtime check of unwatched folders.
    last_check_ms: AtomicU64,
    has_unwatched: AtomicBool,
    recheck_queued: AtomicBool,
    recheck_after: Duration,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub type StatusCallback = Arc<dyn Fn(&Status) + Send + Sync>;

pub struct Engine {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Engine {
    /// Load the snapshot (if there is a usable one), then start the worker.
    /// Returns as soon as the snapshot is in memory: the caller can answer
    /// searches at once. `on_status` runs on the worker thread on every state
    /// change; it must not block.
    pub fn start(cfg: EngineConfig, on_status: StatusCallback) -> std::io::Result<Engine> {
        let t0 = Instant::now();
        let roots = normalize_roots(&cfg.config.roots);
        let excl = Excludes::new(&cfg.config.exclude);
        let root_uris: Vec<String> = roots
            .iter()
            .map(|r| path_to_uri(r.as_os_str().as_bytes()))
            .collect();
        let used = used_table(&cfg.recent_file, &roots);
        let mut status = Status {
            state: State::Scanning,
            entries: 0,
            updated: 0,
            roots: root_uris,
            error: String::new(),
            fatal: false,
        };
        let mut index = Index::empty();
        let mut have_snapshot = false;
        if roots.is_empty() {
            status.state = State::Disabled;
        } else {
            match load_snapshot(&cfg.cache_home, &roots, &excl, &used) {
                Some((ix, saved)) => {
                    status.state = State::Stale;
                    status.entries = ix.len() as u32;
                    status.updated = saved;
                    index = ix;
                    have_snapshot = true;
                    log::info!(
                        "snapshot loaded: {} entries in {} ms",
                        index.len(),
                        t0.elapsed().as_millis()
                    );
                }
                None => log::info!("no usable snapshot, scanning"),
            }
        }
        let shared = Arc::new(Shared {
            index: RwLock::new(Arc::new(index)),
            status: Mutex::new(status),
            pending: Mutex::new(Pending::default()),
            wake: EventFd::new()?,
            stop: AtomicBool::new(false),
            start: Instant::now(),
            last_check_ms: AtomicU64::new(0),
            has_unwatched: AtomicBool::new(false),
            recheck_queued: AtomicBool::new(false),
            recheck_after: cfg.recheck_after,
        });
        let engine = Engine {
            shared: shared.clone(),
            thread: Mutex::new(None),
        };
        if !roots.is_empty() {
            let h = std::thread::Builder::new()
                .name("index".into())
                .spawn(move || supervise(shared, cfg, roots, excl, on_status, have_snapshot))?;
            *lock(&engine.thread) = Some(h);
        }
        Ok(engine)
    }

    pub fn status(&self) -> Status {
        lock(&self.shared.status).clone()
    }

    /// Search the current index; never waits for a scan.
    pub fn search(&self, query: &str, limit: usize, opts: &Options) -> Vec<Hit> {
        let ix = self
            .shared
            .index
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        self.maybe_recheck();
        search(&ix, query, limit, opts, now_secs())
    }

    /// The current index (for tests and tools).
    pub fn index(&self) -> Arc<Index> {
        self.shared
            .index
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Start an mtime check of the folders that have no watch, when there are
    /// some and the last check is older than `recheck_after`. Two atomic loads
    /// when there is nothing to do.
    fn maybe_recheck(&self) {
        let sh = &self.shared;
        if !sh.has_unwatched.load(Ordering::Relaxed) {
            return;
        }
        let last = Duration::from_millis(sh.last_check_ms.load(Ordering::Relaxed));
        if sh.start.elapsed().saturating_sub(last) < sh.recheck_after {
            return;
        }
        if !sh.recheck_queued.swap(true, Ordering::AcqRel) {
            lock(&sh.pending).recheck = true;
            sh.wake.signal();
        }
    }

    /// Hint from Explorer: these folders changed. Only `file://` URIs are
    /// taken, at most [`MAX_NOTIFY`]; the rest are dropped with a log line.
    /// Returns how many were accepted.
    pub fn notify_changed(&self, uris: &[String]) -> usize {
        if uris.len() > MAX_NOTIFY {
            log::warn!(
                "NotifyChanged: {} URIs, only the first {MAX_NOTIFY} are used",
                uris.len()
            );
        }
        let paths: Vec<PathBuf> = uris
            .iter()
            .take(MAX_NOTIFY)
            .filter_map(|u| crate::uri::uri_to_pathbuf(u))
            .collect();
        let n = paths.len();
        if n > 0 {
            let mut p = lock(&self.shared.pending);
            for path in paths {
                if p.notify.len() >= MAX_PENDING_NOTIFY {
                    p.overflow = true;
                    break;
                }
                p.notify.insert(path);
            }
            drop(p);
            self.shared.wake.signal();
        }
        n
    }

    /// Rescan everything soon: calls are merged into one pending request, and
    /// scans keep `refresh_gap` apart.
    pub fn refresh(&self) {
        lock(&self.shared.pending).refresh = true;
        self.shared.wake.signal();
    }

    /// Stop the worker (it writes a pending snapshot first) and wait for it.
    pub fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.wake.signal();
        if let Some(h) = lock(&self.thread).take() {
            let _ = h.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// `path -> last use` keyed by the path hashes the index computes.
fn used_table(recent_file: &Path, roots: &[PathBuf]) -> HashMap<u64, i64> {
    let mut m: HashMap<u64, i64> = HashMap::new();
    for (path, t) in recent::load(recent_file) {
        for root in roots {
            let rb = root.as_os_str().as_bytes();
            let rest = if path == rb {
                &path[path.len()..]
            } else if rb == b"/" && path.len() > 1 {
                &path[1..]
            } else if path.len() > rb.len() && path.starts_with(rb) && path[rb.len()] == b'/' {
                &path[rb.len() + 1..]
            } else {
                continue;
            };
            let mut h = path_hash_root(rb);
            for comp in rest.split(|&b| b == b'/').filter(|c| !c.is_empty()) {
                h = child_hash(h, comp);
            }
            let e = m.entry(h).or_insert(t);
            *e = (*e).max(t);
        }
    }
    m
}

/// The snapshot, if there is a usable one for these roots.
fn load_snapshot(
    cache_home: &Path,
    roots: &[PathBuf],
    excl: &Excludes,
    used: &HashMap<u64, i64>,
) -> Option<(Index, i64)> {
    let cd = match CacheDir::open(cache_home, false) {
        Ok(Some(cd)) => cd,
        Ok(None) => return None,
        Err(e) => {
            log::warn!("snapshot not used: {e}");
            return None;
        }
    };
    let bytes = match cd.read() {
        Ok(Some(b)) => b,
        Ok(None) => return None,
        Err(e) => {
            log::warn!("snapshot not used: {e}");
            return None;
        }
    };
    match snapshot::decode_with(&bytes, used) {
        Ok((ix, saved, hash)) => {
            if hash != excl.fingerprint() {
                log::info!("snapshot was made under other exclusion rules, scanning");
                return None;
            }
            // roots missing when it was saved are fine (they are scanned when
            // they come back); a root that is no longer wanted is not
            let want: HashSet<&[u8]> = roots.iter().map(|r| r.as_os_str().as_bytes()).collect();
            if ix.roots().any(|r| !want.contains(ix.name(r))) {
                log::info!("snapshot is of other folders, scanning");
                return None;
            }
            Some((ix, saved))
        }
        Err(e) => {
            log::warn!("snapshot not used, it is {e}; scanning again");
            None
        }
    }
}

/// Canonical, distinct roots; one inside another is dropped (the outer one
/// already covers it, and two copies of its entries would be searched twice).
pub fn normalize_roots(configured: &[PathBuf]) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = Vec::new();
    for r in configured {
        let c = std::fs::canonicalize(r).unwrap_or_else(|_| r.clone());
        if !all.contains(&c) {
            all.push(c);
        }
    }
    let keep: Vec<PathBuf> = all
        .iter()
        .filter(|r| {
            let inner = all.iter().any(|o| o != *r && r.starts_with(o));
            if inner {
                log::info!("a folder to index is inside another one and is left out");
                log::debug!("left out: {}", r.display());
            }
            !inner
        })
        .cloned()
        .collect();
    keep
}

/// Wait for `h` for at most `max`; false when it is still running.
fn wait_finished(h: &JoinHandle<()>, max: Duration) -> bool {
    let end = Instant::now() + max;
    while !h.is_finished() {
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

/// Encode and write the snapshot; failures are logged, never fatal.
fn write_snapshot(index: &Index, cache_home: &Path, excl_hash: u32) {
    let t0 = Instant::now();
    let bytes = snapshot::encode_with(index, now_secs(), excl_hash);
    match CacheDir::open(cache_home, true) {
        Ok(Some(cd)) => match cd.write(&bytes) {
            Ok(()) => log::debug!(
                "snapshot saved: {} bytes in {} ms",
                bytes.len(),
                t0.elapsed().as_millis()
            ),
            Err(e) => log::error!("snapshot not saved: {e}"),
        },
        Ok(None) => {}
        Err(e) => log::error!("snapshot not saved: {e}"),
    }
}

/// Run the worker; if it dies (a panic, a poll failure) say so in the status and
/// start it again, with a growing pause. After [`MAX_RESTARTS`] the status says
/// `fatal`, and the service exits so that systemd (`Restart=on-failure`) starts
/// a fresh one.
fn supervise(
    sh: Arc<Shared>,
    cfg: EngineConfig,
    roots: Vec<PathBuf>,
    excl: Excludes,
    on_status: StatusCallback,
    mut have_snapshot: bool,
) {
    let mut restarts = 0;
    loop {
        let worker = Worker::new(
            sh.clone(),
            cfg.clone(),
            roots.clone(),
            excl.clone(),
            on_status.clone(),
            have_snapshot,
        );
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || worker.run()));
        if sh.stop.load(Ordering::SeqCst) {
            return;
        }
        restarts += 1;
        let why = if outcome.is_err() {
            "the index worker crashed"
        } else {
            "the index worker stopped unexpectedly"
        };
        let fatal = restarts > MAX_RESTARTS;
        log::error!("{why} (failure {restarts})");
        let snapshot = {
            let mut s = lock(&sh.status);
            s.state = State::Error;
            s.fatal = fatal;
            s.error = if fatal {
                format!("{why} and could not be restarted")
            } else {
                format!("{why}; restarting")
            };
            s.clone()
        };
        on_status(&snapshot);
        if fatal {
            return;
        }
        let end = Instant::now() + Duration::from_secs(1 << restarts);
        while Instant::now() < end {
            if sh.stop.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        have_snapshot = !sh
            .index
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty();
    }
}

type UsedStamp = Option<(i64, i64, u64)>;

struct Worker {
    sh: Arc<Shared>,
    cfg: EngineConfig,
    roots: Vec<PathBuf>,
    excl: Excludes,
    on_status: StatusCallback,
    have_snapshot: bool,
    ino: Option<Inotify>,
    wd_to_id: HashMap<i32, u32>,
    id_to_wd: HashMap<u32, i32>,
    budget: usize,
    dirty: BTreeSet<u32>,
    need_reconcile: bool,
    first_dirty: Option<Instant>,
    last_event: Instant,
    snapshot_due: Option<Instant>,
    saver: Option<JoinHandle<()>>,
    last_cost: Duration,
    /// A full scan is wanted (a `Refresh`, or a root that came back).
    scan_wanted: bool,
    last_full: Option<Instant>,
    /// The last-use table, with the stamp of the file it came from.
    used_cache: Option<(UsedStamp, Arc<HashMap<u64, i64>>)>,
    /// The index reached its size limit.
    full_note: bool,
}

impl Worker {
    fn new(
        sh: Arc<Shared>,
        cfg: EngineConfig,
        roots: Vec<PathBuf>,
        excl: Excludes,
        on_status: StatusCallback,
        have_snapshot: bool,
    ) -> Worker {
        let budget = Worker::probe_budget(&cfg);
        Worker {
            sh,
            cfg,
            roots,
            excl,
            on_status,
            have_snapshot,
            ino: None,
            wd_to_id: HashMap::new(),
            id_to_wd: HashMap::new(),
            budget,
            dirty: BTreeSet::new(),
            need_reconcile: false,
            first_dirty: None,
            last_event: Instant::now(),
            snapshot_due: None,
            saver: None,
            last_cost: Duration::ZERO,
            scan_wanted: false,
            last_full: None,
            used_cache: None,
            full_note: false,
        }
    }

    fn probe_budget(cfg: &EngineConfig) -> usize {
        cfg.watch_budget
            .unwrap_or_else(|| budget_from(max_user_watches()))
    }

    fn index(&self) -> Arc<Index> {
        self.sh
            .index
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Last-use table; the file is read again only when it changed.
    fn used(&mut self) -> Arc<HashMap<u64, i64>> {
        let stamp: UsedStamp = std::fs::metadata(&self.cfg.recent_file).ok().map(|m| {
            use std::os::unix::fs::MetadataExt;
            (m.mtime(), m.mtime_nsec(), m.len())
        });
        if let Some((s, t)) = &self.used_cache
            && *s == stamp
        {
            return t.clone();
        }
        let t = Arc::new(used_table(&self.cfg.recent_file, &self.roots));
        self.used_cache = Some((stamp, t.clone()));
        t
    }

    /// Notes for the status while it is not an error.
    fn notes(&self) -> String {
        let mut n: Vec<&str> = Vec::new();
        if self.full_note {
            n.push("the index is full, some files are not indexed");
        }
        if self.ino.is_none() {
            n.push("file watching is off, changes are found when you search");
        }
        n.join("; ")
    }

    fn set_state(&self, state: State, error: &str) {
        let error = if error.is_empty() {
            self.notes()
        } else {
            error.to_string()
        };
        let snapshot = {
            let mut s = lock(&self.sh.status);
            let entries = self.index().len() as u32;
            let changed = s.state != state || s.error != error || s.entries != entries;
            s.state = state;
            s.error = error;
            s.entries = entries;
            s.fatal = false;
            if state == State::Ready {
                s.updated = now_secs();
            }
            changed.then(|| s.clone())
        };
        if let Some(s) = snapshot {
            (self.on_status)(&s);
        }
    }

    fn publish(&self, index: Index) {
        *self.sh.index.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(index);
    }

    fn reset_inotify(&mut self) {
        self.ino = match Inotify::new() {
            Ok(i) => Some(i),
            Err(e) => {
                log::error!("inotify is not available ({e}): file watching is off");
                None
            }
        };
        self.wd_to_id.clear();
        self.id_to_wd.clear();
    }

    fn run(mut self) {
        set_idle_priority();
        self.reset_inotify();
        if self.have_snapshot {
            self.watch_all();
            self.reconcile(&|_| true);
            if !self.apply() {
                return;
            }
            self.set_state(State::Ready, "");
            self.mark_checked();
            self.check_missing_roots();
        } else {
            if !self.cfg.scan_delay.is_zero() {
                std::thread::sleep(self.cfg.scan_delay);
            }
            if !self.full_scan() {
                return;
            }
        }
        self.event_loop();
        self.finish_snapshot();
    }

    fn mark_checked(&self) {
        self.sh.last_check_ms.store(
            self.sh.start.elapsed().as_millis() as u64,
            Ordering::Relaxed,
        );
        self.sh.recheck_queued.store(false, Ordering::Release);
    }

    /// Roots that are configured, absent from the index, and exist now.
    fn check_missing_roots(&mut self) {
        let index = self.index();
        let have: HashSet<&[u8]> = index.roots().map(|r| index.name(r)).collect();
        let back = self.roots.iter().any(|r| {
            !have.contains(r.as_os_str().as_bytes())
                && std::fs::metadata(r).is_ok_and(|m| m.is_dir())
        });
        if back {
            log::info!("a folder to index is available, scanning");
            self.scan_wanted = true;
        }
    }

    /// Is a configured root missing from the index? (They are looked at again
    /// when a query rechecks.)
    fn roots_missing(&self, index: &Index) -> bool {
        let have: HashSet<&[u8]> = index.roots().map(|r| index.name(r)).collect();
        self.roots
            .iter()
            .any(|r| !have.contains(r.as_os_str().as_bytes()))
    }

    /// Scan every root and publish. Returns false when stopped.
    fn full_scan(&mut self) -> bool {
        self.set_state(State::Scanning, "");
        self.scan_wanted = false;
        let t0 = Instant::now();
        let used = self.used();
        let res = scan_full(&self.roots, &self.excl, &self.sh.stop, &used);
        if res.stopped {
            return false;
        }
        self.last_full = Some(Instant::now());
        self.log_scan_errors(&res.errors);
        if res.failed_roots == self.roots.len() {
            log::error!("scan failed: none of the folders to index can be read");
            let why = res
                .errors
                .first()
                .cloned()
                .unwrap_or_else(|| "the folders to index cannot be read".into());
            self.set_state(State::Error, &why);
            self.sh.has_unwatched.store(true, Ordering::Relaxed);
            return true;
        }
        log::info!(
            "scan done: {} entries in {} ms",
            res.index.len(),
            t0.elapsed().as_millis()
        );
        self.full_note = res.full;
        if res.full {
            log::warn!("the index is full at {} entries", res.index.len());
        }
        self.publish(res.index);
        // new ids: start the watches again, and probe the limit again (it may
        // have been raised since a watch failed with ENOSPC)
        self.budget = Worker::probe_budget(&self.cfg);
        self.reset_inotify();
        self.dirty.clear();
        self.need_reconcile = false;
        self.first_dirty = None;
        self.watch_all();
        self.snapshot_due = Some(Instant::now());
        // changes between the scan and the watches
        self.reconcile(&|_| true);
        if !self.apply() {
            return false;
        }
        self.set_state(State::Ready, "");
        self.mark_checked();
        true
    }

    /// Problems found by a scan: how many at warning level, the folders at debug.
    fn log_scan_errors(&self, errors: &[String]) {
        if !errors.is_empty() {
            log::warn!("{} folders could not be read completely", errors.len());
            for e in errors {
                log::debug!("scan: {e}");
            }
        }
    }

    /// Add watches for folders that have none, shallow folders first, up to the
    /// budget. Returns the folders watched now.
    fn watch_all(&mut self) -> HashSet<u32> {
        let index = self.index();
        let dirs = index.records().iter().filter(|r| r.is_dir()).count();
        let mut added = HashSet::new();
        if let Some(ino) = &self.ino
            && self.id_to_wd.len() < self.budget
        {
            let mut todo: Vec<u32> = (0..index.len() as u32)
                .filter(|&i| index.record(i).is_dir() && !self.id_to_wd.contains_key(&i))
                .collect();
            todo.sort_by_key(|&i| index.depth(i));
            for id in todo {
                if self.id_to_wd.len() >= self.budget {
                    break;
                }
                let path = PathBuf::from(std::ffi::OsStr::from_bytes(&index.path_of(id)));
                match ino.add_watch(&path) {
                    Ok(wd) => {
                        if let Some(prev) = self.wd_to_id.insert(wd, id) {
                            self.id_to_wd.remove(&prev);
                        }
                        self.id_to_wd.insert(id, wd);
                        added.insert(id);
                    }
                    Err(e) if e.raw_os_error() == Some(libc::ENOSPC) => {
                        log::warn!(
                            "inotify watch limit reached at {} folders; the rest are rechecked on queries",
                            self.id_to_wd.len()
                        );
                        self.budget = self.id_to_wd.len();
                        break;
                    }
                    Err(e) => log::debug!("no watch for {}: {e}", path.display()),
                }
            }
        }
        self.sh.has_unwatched.store(
            self.id_to_wd.len() < dirs || self.roots_missing(&index),
            Ordering::Relaxed,
        );
        added
    }

    /// Mark the folders whose mtime differs from the index's as dirty.
    fn reconcile(&mut self, only: &dyn Fn(u32) -> bool) {
        let index = self.index();
        let ids = changed_dirs(&index, only, &self.sh.stop);
        if !ids.is_empty() {
            log::debug!("{} changed folders found", ids.len());
        }
        self.dirty.extend(ids);
    }

    /// Rebuild with the dirty folders listed again and publish. Returns false
    /// when stopped.
    fn apply(&mut self) -> bool {
        self.first_dirty = None;
        self.need_reconcile = false;
        if self.dirty.is_empty() {
            return true;
        }
        let t0 = Instant::now();
        let old = self.index();
        let ids: Vec<u32> = std::mem::take(&mut self.dirty)
            .into_iter()
            .filter(|&i| (i as usize) < old.len())
            .collect();
        let used = self.used();
        let res = rebuild(&old, &ids, &self.excl, &self.sh.stop, &used);
        if res.stopped {
            return false;
        }
        self.log_scan_errors(&res.errors);
        if res.full {
            // a truncated index would lose files that are there: keep the old one
            log::warn!("the index is full: changes are not applied");
            self.full_note = true;
            self.set_state(State::Ready, "");
            return true;
        }
        self.full_note = false;
        // carry the watches over to the new ids
        let mut wd_to_id = HashMap::with_capacity(self.wd_to_id.len());
        let mut id_to_wd = HashMap::with_capacity(self.id_to_wd.len());
        for (&wd, &id) in &self.wd_to_id {
            let new = res.map.get(id as usize).copied().unwrap_or(NONE);
            if new == NONE {
                if let Some(ino) = &self.ino {
                    ino.rm_watch(wd);
                }
            } else {
                wd_to_id.insert(wd, new);
                id_to_wd.insert(new, wd);
            }
        }
        self.wd_to_id = wd_to_id;
        self.id_to_wd = id_to_wd;
        self.publish(res.index);
        // folders that got a watch only now may have changed before it
        let added = self.watch_all();
        if !added.is_empty() {
            self.reconcile(&|i| added.contains(&i));
            if !self.dirty.is_empty() {
                self.last_event = Instant::now();
                self.first_dirty.get_or_insert(self.last_event);
            }
        }
        self.last_cost = t0.elapsed();
        log::debug!(
            "applied {} changed folders in {} ms",
            ids.len(),
            self.last_cost.as_millis()
        );
        self.snapshot_due = Some(Instant::now() + self.cfg.snapshot_delay);
        self.set_state(State::Ready, "");
        true
    }

    /// Write the snapshot on its own thread, so a slow disk never delays
    /// handling of changes. One write at a time: if the last is still running,
    /// try again in a second.
    fn save_snapshot(&mut self) {
        if let Some(h) = &self.saver
            && !h.is_finished()
        {
            self.snapshot_due = Some(Instant::now() + Duration::from_secs(1));
            return;
        }
        if let Some(h) = self.saver.take() {
            let _ = h.join();
        }
        self.snapshot_due = None;
        self.saver = self.spawn_write();
    }

    fn spawn_write(&self) -> Option<JoinHandle<()>> {
        let index = self.index();
        let cache_home = self.cfg.cache_home.clone();
        let hash = self.excl.fingerprint();
        std::thread::Builder::new()
            .name("snapshot".into())
            .spawn(move || {
                set_idle_priority();
                write_snapshot(&index, &cache_home, hash);
            })
            .map_err(|e| log::error!("snapshot not saved: cannot start its thread: {e}"))
            .ok()
    }

    /// At exit: wait for a running write, then write what is still pending.
    /// Each wait is capped, so a stuck disk cannot make the service overrun
    /// its stop timeout; an unfinished write leaves only a temp file.
    fn finish_snapshot(&mut self) {
        if let Some(h) = self.saver.take()
            && !wait_finished(&h, FINAL_WRITE_WAIT)
        {
            log::warn!("the last snapshot write is still running, not waiting");
            return;
        }
        if self.snapshot_due.take().is_some()
            && let Some(h) = self.spawn_write()
            && !wait_finished(&h, FINAL_WRITE_WAIT)
        {
            log::warn!("the final snapshot write did not finish in time");
        }
    }

    fn handle_cmds(&mut self) {
        let p = std::mem::take(&mut *lock(&self.sh.pending));
        if p.overflow {
            log::warn!("too many folder hints, checking all folders");
            self.need_reconcile = true;
            self.last_event = Instant::now();
            self.first_dirty.get_or_insert(self.last_event);
        }
        if !p.notify.is_empty() {
            let index = self.index();
            let table = index.dir_hashes();
            for path in &p.notify {
                // a folder: itself; an indexed file, or a path not indexed yet:
                // the deepest folder that is
                if let Some(dir) = index.deepest_dir(&table, path.as_os_str().as_bytes()) {
                    self.mark_dirty(dir);
                }
            }
        }
        if p.refresh {
            log::info!("rescan requested");
            self.scan_wanted = true;
        }
        if p.recheck {
            log::debug!("rechecking folders without a watch");
            let wd = &self.id_to_wd;
            let ids = changed_dirs(&self.index(), &|i| !wd.contains_key(&i), &self.sh.stop);
            if !ids.is_empty() {
                // due at once: the folders have been quiet already
                self.dirty.extend(ids);
                self.last_event = Instant::now()
                    .checked_sub(self.cfg.debounce * 2)
                    .unwrap_or_else(Instant::now);
                self.first_dirty.get_or_insert(self.last_event);
            }
            self.check_missing_roots();
            self.mark_checked();
        }
    }

    /// When a wanted full scan may start, if one is wanted.
    fn scan_deadline(&self) -> Option<Instant> {
        if !self.scan_wanted {
            return None;
        }
        Some(
            self.last_full
                .map_or_else(Instant::now, |t| t + self.cfg.refresh_gap),
        )
    }

    fn mark_dirty(&mut self, id: u32) {
        self.dirty.insert(id);
        self.last_event = Instant::now();
        self.first_dirty.get_or_insert(self.last_event);
    }

    fn handle_events(&mut self) {
        let Some(ino) = &self.ino else { return };
        let mut ev = Vec::new();
        if let Err(e) = ino.read_events(&mut ev) {
            log::error!("inotify read failed: {e}");
            return;
        }
        let mut overflow = false;
        let mut hit: Vec<u32> = Vec::new();
        let mut gone: Vec<i32> = Vec::new();
        for e in ev {
            if e.overflow() {
                overflow = true;
            } else if e.ignored() {
                gone.push(e.wd);
            } else if let Some(&id) = self.wd_to_id.get(&e.wd) {
                hit.push(id);
            }
        }
        for wd in gone {
            if let Some(id) = self.wd_to_id.remove(&wd) {
                self.id_to_wd.remove(&id);
                self.sh.has_unwatched.store(true, Ordering::Relaxed);
            }
        }
        for id in hit {
            self.mark_dirty(id);
        }
        if overflow {
            log::warn!("inotify queue overflowed, checking all folders");
            self.need_reconcile = true;
            self.last_event = Instant::now();
            self.first_dirty.get_or_insert(self.last_event);
        }
    }

    /// When the pending changes are due, if any.
    fn change_deadline(&self) -> Option<Instant> {
        let first = self.first_dirty?;
        let gap = self.cfg.debounce.max(self.last_cost * 10);
        // quiet for `gap`, but never later than 2 s (or `gap`) after the first change
        Some((self.last_event + gap).min(first + gap.max(Duration::from_secs(2))))
    }

    fn event_loop(&mut self) {
        loop {
            if self.sh.stop.load(Ordering::SeqCst) {
                return;
            }
            let now = Instant::now();
            let due = [
                self.change_deadline(),
                self.snapshot_due,
                self.scan_deadline(),
            ]
            .into_iter()
            .flatten()
            .min();
            let timeout: i32 = match due {
                None => -1,
                Some(t) => {
                    t.saturating_duration_since(now)
                        .as_millis()
                        .min(i32::MAX as u128 - 1) as i32
                        + 1
                }
            };
            let mut fds = vec![self.sh.wake.fd()];
            if let Some(i) = &self.ino {
                fds.push(i.fd());
            }
            let ready = match poll_readable(&fds, timeout) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("poll failed: {e}");
                    return;
                }
            };
            if ready[0] {
                self.sh.wake.drain();
            }
            if self.sh.stop.load(Ordering::SeqCst) {
                return;
            }
            self.handle_cmds();
            if ready.get(1).copied().unwrap_or(false) {
                self.handle_events();
            }
            if self.scan_deadline().is_some_and(|t| t <= Instant::now()) && !self.full_scan() {
                return;
            }
            let now = Instant::now();
            if self.change_deadline().is_some_and(|t| t <= now) {
                if self.need_reconcile {
                    self.reconcile(&|_| true);
                }
                if !self.apply() {
                    return;
                }
                self.check_missing_roots();
            }
            if self.snapshot_due.is_some_and(|t| t <= Instant::now()) {
                self.save_snapshot();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_home_prefers_systemd_cache_directory() {
        let x = PathBuf::from("/home/u/.cache");
        let got = cache_home_from(Some("/var/c/atlas-explorer".into()), x.clone());
        assert_eq!(got, PathBuf::from("/var/c"));
        assert_eq!(cache_home_from(None, x.clone()), x);
        assert_eq!(
            cache_home_from(Some("rel/atlas-explorer".into()), x.clone()),
            x
        );
        assert_eq!(cache_home_from(Some("/var/c/other".into()), x.clone()), x);
    }

    #[test]
    fn nested_and_repeated_roots_are_dropped() {
        let t = crate::testdir::Scratch::new("roots");
        let a = t.0.join("a");
        let b = a.join("b");
        std::fs::create_dir_all(&b).unwrap();
        let out = normalize_roots(&[b.clone(), a.clone(), a.clone()]);
        assert_eq!(out, vec![std::fs::canonicalize(&a).unwrap()]);
    }
}
