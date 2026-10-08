//! A stand-in for Telamon Archive's `Archive1` D-Bus API (its DESIGN.md, "The
//! API other apps call"), so Files' extract and compress actions can be tried
//! on a private session bus while Archive itself has no server for it yet.
//! Test tool only: it is not installed, and it is not Archive.
//!
//! What it does like Archive: owns `net.eterneon.telamon.archive` at
//! `/net/eterneon/telamon/archive`; `ExtractHere`, `ExtractTo`, `ExtractEntries`
//! and `Compress` queue a job and return its object path at once; a job is an
//! object `.../job/<n>` with `Title`, `State`, `ProcessedBytes`, `TotalBytes`,
//! `ProcessedItems`, `TotalItems` and `Error`, `PropertiesChanged`, `Pause`,
//! `Resume`, `Cancel` and `Finished(state, results)`; the object goes 60 s
//! after the job ends; past 16 jobs (`STANDIN_MAX_JOBS`) a call fails with
//! `...Archive1.Error.TooManyJobs`; a URI that is not an absolute `file://` is
//! `...Error.InvalidArgs`; `ExtractAll` and `CompressDialog` only note the
//! call (Archive's dialogs).
//!
//! What it does not: it never reads archive bytes itself (`unzip`, `tar`,
//! `7z` and `zip` do, and they skip `..` and absolute paths as they do by
//! default), it asks no questions (an archive whose name holds `locked` fails
//! with the wrong-password words), and "Extract Here" names a taken folder
//! `name (2)` instead of asking.
//!
//! Switches, by environment: `STANDIN_LOG` (a file each call and each state
//! is appended to), `STANDIN_SLOW_MS` (every job takes at least this long,
//! reporting progress, so Pause and Cancel can be tried), `STANDIN_MAX_JOBS`,
//! `STANDIN_ASK=1` (each job first waits 1.5 s in `waiting-for-user`).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use zbus::blocking::Connection;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const NAME: &str = "net.eterneon.telamon.archive";
const ROOT: &str = "/net/eterneon/telamon/archive";
const JOB_IFACE: &str = "net.eterneon.telamon.Archive1.Job";

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "net.eterneon.telamon.Archive1.Error")]
enum ArchiveError {
    #[zbus(error)]
    ZBus(zbus::Error),
    TooManyJobs(String),
    InvalidArgs(String),
}

fn log(line: &str) {
    if let Some(path) = std::env::var_os("STANDIN_LOG")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Queued,
    Running,
    Paused,
    WaitingForUser,
    Done,
    Failed,
    Cancelled,
}

impl State {
    fn name(&self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Running => "running",
            State::Paused => "paused",
            State::WaitingForUser => "waiting-for-user",
            State::Done => "done",
            State::Failed => "failed",
            State::Cancelled => "cancelled",
        }
    }
    fn is_over(&self) -> bool {
        matches!(self, State::Done | State::Failed | State::Cancelled)
    }
}

struct JobState {
    title: String,
    state: State,
    processed_bytes: u64,
    total_bytes: u64,
    processed_items: u32,
    total_items: u32,
    error: String,
    pause: bool,
    cancel: bool,
}

struct Shared {
    m: Mutex<JobState>,
    cv: Condvar,
}

struct Job {
    shared: Arc<Shared>,
}

#[zbus::interface(name = "net.eterneon.telamon.Archive1.Job")]
impl Job {
    #[zbus(property)]
    fn title(&self) -> String {
        self.shared.m.lock().unwrap().title.clone()
    }
    #[zbus(property)]
    fn state(&self) -> String {
        self.shared.m.lock().unwrap().state.name().to_string()
    }
    #[zbus(property)]
    fn processed_bytes(&self) -> u64 {
        self.shared.m.lock().unwrap().processed_bytes
    }
    #[zbus(property)]
    fn total_bytes(&self) -> u64 {
        self.shared.m.lock().unwrap().total_bytes
    }
    #[zbus(property)]
    fn processed_items(&self) -> u32 {
        self.shared.m.lock().unwrap().processed_items
    }
    #[zbus(property)]
    fn total_items(&self) -> u32 {
        self.shared.m.lock().unwrap().total_items
    }
    #[zbus(property)]
    fn error(&self) -> String {
        self.shared.m.lock().unwrap().error.clone()
    }

    fn pause(&self) {
        log("JOB Pause");
        let mut s = self.shared.m.lock().unwrap();
        s.pause = true;
        self.shared.cv.notify_all();
    }
    fn resume(&self) {
        log("JOB Resume");
        let mut s = self.shared.m.lock().unwrap();
        s.pause = false;
        self.shared.cv.notify_all();
    }
    fn cancel(&self) {
        log("JOB Cancel");
        let mut s = self.shared.m.lock().unwrap();
        s.cancel = true;
        self.shared.cv.notify_all();
    }
}

enum Work {
    Extract {
        archive: PathBuf,
        folder: Option<PathBuf>,
    },
    Compress {
        files: Vec<PathBuf>,
        format: String,
        destination: Option<PathBuf>,
    },
}

struct Archive {
    conn: Connection,
    next: AtomicU32,
    active: Arc<Mutex<Vec<Arc<Shared>>>>,
    max_jobs: usize,
    slow: Duration,
    ask: bool,
}

fn file_paths(uris: &[String]) -> Result<Vec<PathBuf>, ArchiveError> {
    let mut out = Vec::new();
    for u in uris {
        let Some(rest) = u.strip_prefix("file://") else {
            return Err(ArchiveError::InvalidArgs(format!("not a file:// URI: {u}")));
        };
        if !rest.starts_with('/') || rest.contains('\0') {
            return Err(ArchiveError::InvalidArgs(format!(
                "not an absolute path: {u}"
            )));
        }
        out.push(PathBuf::from(percent_decode(rest)));
    }
    if out.is_empty() {
        return Err(ArchiveError::InvalidArgs("no files".into()));
    }
    Ok(out)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn file_uri(p: &Path) -> String {
    let mut s = String::from("file://");
    for &b in p.as_os_str().as_encoded_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

impl Archive {
    fn start(&self, title: String, work: Work) -> Result<ObjectPath<'static>, ArchiveError> {
        {
            let mut active = self.active.lock().unwrap();
            active.retain(|s| !s.m.lock().unwrap().state.is_over());
            if active.len() >= self.max_jobs {
                return Err(ArchiveError::TooManyJobs("too many jobs".into()));
            }
        }
        let n = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let path = format!("{ROOT}/job/{n}");
        let shared = Arc::new(Shared {
            m: Mutex::new(JobState {
                title,
                state: State::Queued,
                processed_bytes: 0,
                total_bytes: 0,
                processed_items: 0,
                total_items: 0,
                error: String::new(),
                pause: false,
                cancel: false,
            }),
            cv: Condvar::new(),
        });
        self.conn
            .object_server()
            .at(
                path.as_str(),
                Job {
                    shared: shared.clone(),
                },
            )
            .map_err(ArchiveError::ZBus)?;
        self.active.lock().unwrap().push(shared.clone());
        let conn = self.conn.clone();
        let (slow, ask, jobpath) = (self.slow, self.ask, path.clone());
        std::thread::spawn(move || run(conn, jobpath, shared, work, slow, ask));
        Ok(ObjectPath::try_from(path).expect("a valid path"))
    }
}

#[zbus::interface(name = "net.eterneon.telamon.Archive1")]
impl Archive {
    fn extract_here(
        &self,
        archives: Vec<String>,
        options: HashMap<String, OwnedValue>,
    ) -> Result<ObjectPath<'static>, ArchiveError> {
        note("ExtractHere", &archives, &options);
        let files = file_paths(&archives)?;
        let title = format!("Extract {}", name_of(&files[0]));
        self.start(
            title,
            Work::Extract {
                archive: files[0].clone(),
                folder: None,
            },
        )
    }

    fn extract_to(
        &self,
        archives: Vec<String>,
        folder: String,
        options: HashMap<String, OwnedValue>,
    ) -> Result<ObjectPath<'static>, ArchiveError> {
        note("ExtractTo", &archives, &options);
        let files = file_paths(&archives)?;
        let folder = if folder.is_empty() {
            None
        } else {
            Some(file_paths(&[folder])?.remove(0))
        };
        let title = format!("Extract {}", name_of(&files[0]));
        self.start(
            title,
            Work::Extract {
                archive: files[0].clone(),
                folder,
            },
        )
    }

    fn extract_all(&self, archives: Vec<String>, options: HashMap<String, OwnedValue>) {
        note("ExtractAll", &archives, &options);
    }

    fn extract_entries(
        &self,
        archive: String,
        entries: Vec<String>,
        folder: String,
        options: HashMap<String, OwnedValue>,
    ) -> Result<ObjectPath<'static>, ArchiveError> {
        note("ExtractEntries", std::slice::from_ref(&archive), &options);
        log(&format!("   entries={entries:?} folder={folder}"));
        let files = file_paths(&[archive])?;
        let folder = Some(file_paths(&[folder])?.remove(0));
        let title = format!("Extract {}", name_of(&files[0]));
        self.start(
            title,
            Work::Extract {
                archive: files[0].clone(),
                folder,
            },
        )
    }

    fn compress(
        &self,
        files: Vec<String>,
        format: String,
        destination: String,
        options: HashMap<String, OwnedValue>,
    ) -> Result<ObjectPath<'static>, ArchiveError> {
        note("Compress", &files, &options);
        log(&format!("   format={format} destination={destination}"));
        let paths = file_paths(&files)?;
        if !["zip", "7z", "tar.gz", "tar.xz", "tar.zst"].contains(&format.as_str()) {
            return Err(ArchiveError::InvalidArgs(format!(
                "unknown format {format}"
            )));
        }
        let destination = if destination.is_empty() {
            None
        } else {
            Some(file_paths(&[destination])?.remove(0))
        };
        let title = format!("Compress {}", name_of(&paths[0]));
        self.start(
            title,
            Work::Compress {
                files: paths,
                format,
                destination,
            },
        )
    }

    fn compress_dialog(&self, files: Vec<String>, options: HashMap<String, OwnedValue>) {
        note("CompressDialog", &files, &options);
    }
}

fn note(method: &str, files: &[String], options: &HashMap<String, OwnedValue>) {
    let mut keys: Vec<String> = options
        .iter()
        .map(|(k, v)| format!("{k}={}", short(v)))
        .collect();
    keys.sort();
    log(&format!(
        "CALL {method} files={files:?} options=[{}]",
        keys.join(" ")
    ));
}

fn short(v: &OwnedValue) -> String {
    match &**v {
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => s.to_string(),
        other => format!("{other:?}"),
    }
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn tree_size(p: &Path) -> (u64, u32) {
    let Ok(meta) = std::fs::symlink_metadata(p) else {
        return (0, 0);
    };
    if meta.is_dir() {
        let mut total = (0, 1);
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let (b, n) = tree_size(&e.path());
                total.0 += b;
                total.1 += n;
            }
        }
        total
    } else {
        (meta.len(), 1)
    }
}

fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if std::fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let (stem, ext) = match name.find('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 2.. {
        let p = dir.join(format!("{stem} ({n}){ext}"));
        if std::fs::symlink_metadata(&p).is_err() {
            return p;
        }
    }
    unreachable!()
}

fn stem_of(archive: &str) -> String {
    let lower = archive.to_lowercase();
    for e in [
        ".tar.gz", ".tar.xz", ".tar.zst", ".tar.bz2", ".tgz", ".zip", ".7z", ".tar",
    ] {
        if lower.ends_with(e) {
            return archive[..archive.len() - e.len()].to_string();
        }
    }
    archive.to_string()
}

/// Sets what changed and tells the bus (`PropertiesChanged`, as Archive does).
fn publish(conn: &Connection, path: &str, shared: &Shared, f: impl FnOnce(&mut JobState)) {
    let (changed, state) = {
        let mut s = shared.m.lock().unwrap();
        let before = (
            s.state.clone(),
            s.processed_bytes,
            s.total_bytes,
            s.processed_items,
            s.total_items,
            s.error.clone(),
        );
        f(&mut s);
        let mut m: HashMap<&str, Value> = HashMap::new();
        if s.state != before.0 {
            m.insert("State", Value::from(s.state.name()));
        }
        if s.processed_bytes != before.1 {
            m.insert("ProcessedBytes", Value::from(s.processed_bytes));
        }
        if s.total_bytes != before.2 {
            m.insert("TotalBytes", Value::from(s.total_bytes));
        }
        if s.processed_items != before.3 {
            m.insert("ProcessedItems", Value::from(s.processed_items));
        }
        if s.total_items != before.4 {
            m.insert("TotalItems", Value::from(s.total_items));
        }
        if s.error != before.5 {
            m.insert("Error", Value::from(s.error.clone()));
        }
        let owned: HashMap<String, OwnedValue> = m
            .into_iter()
            .map(|(k, v)| (k.to_string(), OwnedValue::try_from(v).unwrap()))
            .collect();
        (owned, s.state.clone())
    };
    if !changed.is_empty() {
        let _ = conn.emit_signal(
            None::<&str>,
            path,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(JOB_IFACE, changed, Vec::<String>::new()),
        );
    }
    log(&format!("STATE {path} {}", state.name()));
}

/// Waits while paused; true when the job was cancelled.
fn cancelled_after_pause(conn: &Connection, path: &str, shared: &Shared) -> bool {
    loop {
        let (pause, cancel) = {
            let s = shared.m.lock().unwrap();
            (s.pause, s.cancel)
        };
        if cancel {
            return true;
        }
        if !pause {
            let paused = shared.m.lock().unwrap().state == State::Paused;
            if paused {
                publish(conn, path, shared, |s| s.state = State::Running);
            }
            return false;
        }
        let paused = shared.m.lock().unwrap().state == State::Paused;
        if !paused {
            publish(conn, path, shared, |s| s.state = State::Paused);
        }
        let guard = shared.m.lock().unwrap();
        let _ = shared.cv.wait_timeout(guard, Duration::from_millis(100));
    }
}

fn run(conn: Connection, path: String, shared: Arc<Shared>, work: Work, slow: Duration, ask: bool) {
    if ask {
        publish(&conn, &path, &shared, |s| s.state = State::WaitingForUser);
        std::thread::sleep(Duration::from_millis(1500));
    }
    let (total_bytes, total_items) = match &work {
        Work::Extract { archive, .. } => {
            (std::fs::metadata(archive).map(|m| m.len()).unwrap_or(0), 1)
        }
        Work::Compress { files, .. } => files
            .iter()
            .map(|f| tree_size(f))
            .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1)),
    };
    publish(&conn, &path, &shared, |s| {
        s.state = State::Running;
        s.total_bytes = total_bytes;
        s.total_items = total_items;
    });
    // The slow phase: progress in twenty steps.
    if !slow.is_zero() {
        let steps = 20u64;
        let tick = Duration::from_millis(20);
        for i in 1..=steps {
            // Only time spent running counts: a pause holds the clock.
            let mut left = slow / (steps as u32);
            while !left.is_zero() {
                let t = left.min(tick);
                std::thread::sleep(t);
                left -= t;
                if cancelled_after_pause(&conn, &path, &shared) {
                    return end(&conn, &path, &shared, State::Cancelled, "", Vec::new());
                }
            }
            publish(&conn, &path, &shared, |s| {
                s.processed_bytes = total_bytes * i / steps;
                s.processed_items = (u64::from(total_items) * i / steps) as u32;
            });
        }
    }
    if cancelled_after_pause(&conn, &path, &shared) {
        return end(&conn, &path, &shared, State::Cancelled, "", Vec::new());
    }
    match do_work(&work) {
        Ok(results) => {
            publish(&conn, &path, &shared, |s| {
                s.processed_bytes = s.total_bytes;
                s.processed_items = s.total_items;
            });
            end(&conn, &path, &shared, State::Done, "", results)
        }
        Err(why) => end(&conn, &path, &shared, State::Failed, &why, Vec::new()),
    }
}

fn end(
    conn: &Connection,
    path: &str,
    shared: &Shared,
    state: State,
    error: &str,
    results: Vec<PathBuf>,
) {
    publish(conn, path, shared, |s| {
        s.error = error.to_string();
        s.state = state.clone();
    });
    let uris: Vec<String> = results.iter().map(|p| file_uri(p)).collect();
    log(&format!(
        "FINISHED {path} {} {uris:?} {error}",
        state.name()
    ));
    let _ = conn.emit_signal(
        None::<&str>,
        path,
        "net.eterneon.telamon.Archive1.Job",
        "Finished",
        &(state.name(), uris),
    );
    // Archive removes a job's object a minute after it ends.
    let (conn, path) = (conn.clone(), path.to_string());
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        let _ = conn.object_server().remove::<Job, _>(path.as_str());
    });
}

fn explain(tool: &str, stderr: &str) -> String {
    let first = stderr
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let low = stderr.to_lowercase();
    if low.contains("no space left") || low.contains("disk full") || low.contains("cannot write") {
        "There isn't enough space on the disk. Nothing was extracted.".into()
    } else if low.contains("password") || low.contains("wrong key") {
        "That password didn't work.".into()
    } else if low.contains("end-of-central-directory")
        || low.contains("unexpected end")
        || low.contains("truncated")
        || low.contains("not in gzip")
        || low.contains("is not a zip")
        || low.contains("cannot open")
        || low.contains("headers error")
        || low.contains("data error")
    {
        "This archive is damaged. Nothing was extracted.".into()
    } else {
        format!("{tool} stopped: {first}")
    }
}

fn run_tool(cmd: &mut Command, tool: &str) -> Result<(), String> {
    let out = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{tool} could not be started: {e}"))?;
    if out.status.success() || (tool == "unzip" && out.status.code() == Some(1)) {
        return Ok(());
    }
    let mut stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if let Some(i) = stderr.find("\n") {
        // `tar` and `unzip` lead with the file's name; keep the reason.
        stderr = format!("{}\n{}", &stderr[..i], &stderr[i + 1..]);
    }
    Err(explain(tool, &stderr))
}

fn do_work(work: &Work) -> Result<Vec<PathBuf>, String> {
    match work {
        Work::Extract { archive, folder } => {
            let name = name_of(archive);
            if name.contains("locked") {
                return Err("That password didn't work.".into());
            }
            let dest = folder
                .clone()
                .unwrap_or_else(|| archive.parent().unwrap_or(Path::new("/")).to_path_buf());
            let staging = free_name(&dest, &format!(".{name}.standin-partial"));
            std::fs::create_dir(&staging)
                .map_err(|e| format!("The folder can't be written: {e}"))?;
            let lower = name.to_lowercase();
            let res = if lower.ends_with(".zip") {
                run_tool(
                    Command::new("unzip")
                        .arg("-qq")
                        .arg("-d")
                        .arg(&staging)
                        .arg(archive),
                    "unzip",
                )
            } else if lower.ends_with(".7z") {
                let mut o = std::ffi::OsString::from("-o");
                o.push(&staging);
                run_tool(
                    Command::new("7z")
                        .args(["x", "-y", "-bso0", "-bsp0"])
                        .arg(o)
                        .arg(archive),
                    "7z",
                )
            } else {
                run_tool(
                    Command::new("tar")
                        .arg("-xf")
                        .arg(archive)
                        .arg("-C")
                        .arg(&staging),
                    "tar",
                )
            };
            if let Err(why) = res {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(why);
            }
            // Smart: one top-level item stays as it is, several go in a folder.
            let items: Vec<_> = std::fs::read_dir(&staging)
                .map_err(|e| e.to_string())?
                .flatten()
                .collect();
            let made =
                if items.len() == 1 && !items[0].file_name().to_string_lossy().starts_with('.') {
                    let target = free_name(&dest, &items[0].file_name().to_string_lossy());
                    std::fs::rename(items[0].path(), &target).map_err(|e| e.to_string())?;
                    let _ = std::fs::remove_dir(&staging);
                    target
                } else {
                    let target = free_name(&dest, &stem_of(&name));
                    std::fs::rename(&staging, &target).map_err(|e| e.to_string())?;
                    target
                };
            Ok(vec![made])
        }
        Work::Compress {
            files,
            format,
            destination,
        } => {
            let first = &files[0];
            let parent = first.parent().unwrap_or(Path::new("/")).to_path_buf();
            let base = if files.len() == 1 {
                name_of(first)
            } else {
                "Archive".to_string()
            };
            let ext = format!(".{format}");
            let target = match destination {
                Some(d) => d.clone(),
                None => free_name(&parent, &format!("{base}{ext}")),
            };
            let names: Vec<String> = files.iter().map(|f| name_of(f)).collect();
            let mut cmd = match format.as_str() {
                "zip" => {
                    let mut c = Command::new("zip");
                    c.arg("-qry").arg(&target).arg("--").args(&names);
                    c
                }
                "7z" => {
                    let mut c = Command::new("7z");
                    c.args(["a", "-bso0", "-bsp0"]).arg(&target).args(&names);
                    c
                }
                other => {
                    let flag = match other {
                        "tar.xz" => "-cJf",
                        "tar.zst" => "--zstd -cf",
                        _ => "-czf",
                    };
                    let mut c = Command::new("tar");
                    c.args(flag.split(' ')).arg(&target).arg("--").args(&names);
                    c
                }
            };
            cmd.current_dir(&parent);
            let tool = if format == "zip" {
                "zip"
            } else if format == "7z" {
                "7z"
            } else {
                "tar"
            };
            if let Err(why) = run_tool(&mut cmd, tool) {
                let _ = std::fs::remove_file(&target);
                return Err(why);
            }
            Ok(vec![target])
        }
    }
}

fn main() -> zbus::Result<()> {
    let slow = std::env::var("STANDIN_SLOW_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or_default();
    let max_jobs = std::env::var("STANDIN_MAX_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    let ask = std::env::var("STANDIN_ASK").is_ok_and(|v| v == "1");
    // The connection is built first so that jobs can add objects to it.
    let conn = zbus::blocking::connection::Builder::session()?.build()?;
    let archive = Archive {
        conn: conn.clone(),
        next: AtomicU32::new(0),
        active: Arc::new(Mutex::new(Vec::new())),
        max_jobs,
        slow,
        ask,
    };
    conn.object_server().at(ROOT, archive)?;
    conn.request_name(NAME)?;
    log("READY");
    loop {
        std::thread::park();
    }
}
