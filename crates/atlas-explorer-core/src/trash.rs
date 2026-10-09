//! The Trash's rules that Files keeps apart from KIO: reading a `.trashinfo`,
//! which trash folders exist on this computer, and "empty items older than N
//! days". The Trash itself (listing, trashing, restoring, the sidebar count)
//! is KIO's; this is only what runs by itself in the background, so it is
//! written to do as little as it can:
//!
//! - It looks at the folders KIO knows (the home Trash and, for each mounted
//!   volume, `.Trash/<uid>` or `.Trash-<uid>`, with the same checks KIO makes)
//!   and nothing else.
//! - An item goes only when its `.trashinfo` can be read, says when it was
//!   deleted, and that was more than N days before `now`. A `.trashinfo` that
//!   cannot be read or understood leaves the item where it is, counted as
//!   skipped. So does an item with no `.trashinfo`, and a `.trashinfo` with no
//!   item.
//! - Nothing outside a trash folder's `files` and `info` is touched, and no
//!   symlink is followed: a link in the Trash is removed as a link, a folder
//!   is removed with `remove_dir_all` (which does not follow links), and a
//!   trash folder whose `files` or `info` is itself a link is left alone.
//!
//! The clock is a parameter everywhere (`now` and the dates are seconds of
//! local wall-clock time counted as if it were UTC, because that is how a
//! `.trashinfo` writes a date: no zone), so tests use a fake one.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The days the setting starts at when it is turned on.
pub const DEFAULT_DAYS: u32 = 30;
/// The least and the most days the setting takes.
pub const MIN_DAYS: u32 = 1;
pub const MAX_DAYS: u32 = 3650;
/// A `.trashinfo` is a few lines; a longer one is not one.
pub const MAX_INFO_BYTES: u64 = 64 * 1024;
/// The `directorysizes` cache is read whole to take a line out; a bigger one is left.
const MAX_SIZES_BYTES: u64 = 16 * 1024 * 1024;
/// At most this many `.trashinfo` files are looked at in one folder per run;
/// the rest wait for the next run.
pub const MAX_ITEMS_PER_DIR: usize = 1_000_000;

/// The user running Files.
pub fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// Brings a number of days from a settings file into the limits.
pub fn clamp_days(days: i64) -> u32 {
    days.clamp(i64::from(MIN_DAYS), i64::from(MAX_DAYS)) as u32
}

/// What a `.trashinfo` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// Where the item was, as the file wrote it (percent-decoded).
    pub original: Vec<u8>,
    /// When it was deleted: seconds of local wall-clock time as if UTC.
    pub deleted: i64,
}

/// Why a `.trashinfo` was not understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoError {
    /// Longer than [`MAX_INFO_BYTES`].
    TooLong,
    /// Not text, or no `[Trash Info]` group.
    NotAnInfo,
    /// No `Path`, or one that is empty or badly escaped.
    BadPath,
    /// No `DeletionDate`, or one that is not `YYYY-MM-DDThh:mm:ss`.
    BadDate,
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // Howard Hinnant's algorithm.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// Seconds since 1970-01-01T00:00:00 for a wall-clock date and time, or
/// `None` when it is not a real one.
pub fn civil_seconds(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Option<i64> {
    if !(1970..=9999).contains(&y)
        || !(1..=12).contains(&mo)
        || d < 1
        || d > days_in_month(y, mo)
        || !(0..24).contains(&h)
        || !(0..60).contains(&mi)
        || !(0..60).contains(&s)
    {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s)
}

/// `2026-10-07T10:00:00` (nothing before or after it).
pub fn parse_date(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() != 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        let s = text.get(from..to)?;
        if s.bytes().all(|c| c.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    };
    civil_seconds(
        num(0, 4)?,
        num(5, 7)?,
        num(8, 10)?,
        num(11, 13)?,
        num(14, 16)?,
        num(17, 19)?,
    )
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Undoes percent-encoding; `None` for a `%` not followed by two hex digits.
fn percent_decode(text: &str) -> Option<Vec<u8>> {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hi = hex(*b.get(i + 1)?)?;
            let lo = hex(*b.get(i + 2)?)?;
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

/// Percent-encodes a name the way the `directorysizes` file writes it
/// (everything but unreserved characters).
fn percent_encode(name: &[u8]) -> String {
    let mut out = String::with_capacity(name.len());
    for &c in name {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

/// Reads a `.trashinfo`. The first group must be `[Trash Info]` (comments and
/// blank lines may come before it); within it `Path` and `DeletionDate` are
/// both needed, the first of each counts, and other keys are ignored.
pub fn parse_info(content: &[u8]) -> Result<Info, InfoError> {
    if content.len() as u64 > MAX_INFO_BYTES {
        return Err(InfoError::TooLong);
    }
    let text = std::str::from_utf8(content).map_err(|_| InfoError::NotAnInfo)?;
    let mut in_group = false;
    let mut seen_group = false;
    let mut path: Option<&str> = None;
    let mut date: Option<&str> = None;
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') {
            if t == "[Trash Info]" && !seen_group {
                in_group = true;
                seen_group = true;
                continue;
            }
            if !seen_group {
                return Err(InfoError::NotAnInfo);
            }
            in_group = false;
            continue;
        }
        if !seen_group {
            return Err(InfoError::NotAnInfo);
        }
        if !in_group {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Path" if path.is_none() => path = Some(v.trim_start()),
                "DeletionDate" if date.is_none() => date = Some(v.trim()),
                _ => {}
            }
        }
    }
    if !seen_group {
        return Err(InfoError::NotAnInfo);
    }
    let original = match path {
        Some(p) if !p.is_empty() => percent_decode(p).ok_or(InfoError::BadPath)?,
        _ => return Err(InfoError::BadPath),
    };
    if original.is_empty() || original.contains(&0) {
        return Err(InfoError::BadPath);
    }
    let deleted = date.and_then(parse_date).ok_or(InfoError::BadDate)?;
    Ok(Info { original, deleted })
}

/// Whether an item deleted at `deleted` is older than `days` at `now`. An
/// item from the future (a clock that was wrong) is never old; 0 days never
/// expires anything.
pub fn expired(deleted: i64, now: i64, days: u32) -> bool {
    days >= MIN_DAYS && now.saturating_sub(deleted) > i64::from(days) * 86_400
}

// ---- Putting an item back ----

/// Whether an item may be put back at `target` by one click. The place an
/// item was is written in its `.trashinfo`, and that file is whatever the
/// trash folder holds: on a drive someone else made, a `.Trash-<uid>` with a
/// `Path=` into the settings, the autostart folder or a shell's start-up file
/// would put a file where it runs. The Trash in the home folder is Files' and
/// KIO's own (`from_home_trash`) and is believed; for any other, a target in
/// a hidden folder or file of the home folder is not restored (the item can
/// still be dragged out, which is a choice of the person's).
pub fn restore_allowed(from_home_trash: bool, target: &[u8], home: &[u8]) -> bool {
    if from_home_trash {
        return true;
    }
    let home = home.strip_suffix(b"/").unwrap_or(home);
    if home.is_empty() {
        return true;
    }
    let Some(rest) = target.strip_prefix(home) else {
        return true;
    };
    let Some(rest) = rest.strip_prefix(b"/") else {
        return true;
    };
    // `..` is not trusted to mean what it says: such a path is not restored.
    if rest.split(|&b| b == b'/').any(|c| c == b"..") {
        return false;
    }
    !rest.starts_with(b".")
}

// ---- Which trash folders exist ----

/// One line of `/proc/self/mountinfo`: the mount point and the file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub point: PathBuf,
    pub fstype: String,
}

fn unescape_mount(field: &str) -> Vec<u8> {
    // The kernel writes space, tab, newline and backslash as \040 \011 \012 \134.
    let b = field.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && i + 4 <= b.len()
            && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c))
        {
            let v = u32::from(b[i + 1] - b'0') * 64
                + u32::from(b[i + 2] - b'0') * 8
                + u32::from(b[i + 3] - b'0');
            out.push(v as u8);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// The mounts in `/proc/self/mountinfo` text. Lines that don't parse are left out.
pub fn parse_mountinfo(text: &str) -> Vec<Mount> {
    use std::os::unix::ffi::OsStringExt;
    let mut out = Vec::new();
    for line in text.lines() {
        // id parent major:minor root mountpoint options [optional...] - fstype source superopts
        let mut f = line.split(' ');
        let Some(point) = f.nth(4) else { continue };
        let mut rest = f.skip_while(|x| *x != "-");
        if rest.next().is_none() {
            continue;
        }
        let Some(fstype) = rest.next() else { continue };
        let point = unescape_mount(point);
        if point.first() != Some(&b'/') {
            continue;
        }
        out.push(Mount {
            point: PathBuf::from(std::ffi::OsString::from_vec(point)),
            fstype: fstype.to_string(),
        });
    }
    out
}

/// File systems KIO takes for "pseudo" ones (`KMountPoint::isPseudoFs`: the
/// kernel's own, the memory ones such as tmpfs, and FUSE unless it is an
/// encrypted one) or that may hang a `stat` (a server that went away): KIO
/// neither lists nor uses a trash folder there, so Files does not look either.
fn skipped_fs(fstype: &str) -> bool {
    const ENCRYPTED_FUSE: [&str; 3] = ["fuse.gocryptfs", "fuse.cryfs", "fuse.encfs"];
    matches!(
        fstype,
        "proc"
            | "sysfs"
            | "devtmpfs"
            | "devpts"
            | "tmpfs"
            | "cgroup"
            | "cgroup2"
            | "securityfs"
            | "debugfs"
            | "tracefs"
            | "bpf"
            | "pstore"
            | "mqueue"
            | "hugetlbfs"
            | "configfs"
            | "fusectl"
            | "autofs"
            | "binfmt_misc"
            | "selinuxfs"
            | "efivarfs"
            | "ramfs"
            | "rpc_pipefs"
            | "nsfs"
            | "nfs"
            | "nfs4"
            | "cifs"
            | "smb3"
            | "smbfs"
            | "ncpfs"
            | "afs"
            | "9p"
            | "ceph"
            | "glusterfs"
            | "lustre"
    ) || (fstype.starts_with("fuse") && fstype != "fuseblk" && !ENCRYPTED_FUSE.contains(&fstype))
}

fn is_real_dir(path: &Path) -> Option<fs::Metadata> {
    fs::symlink_metadata(path).ok().filter(|m| m.is_dir())
}

/// The trash folder a volume has for `uid`, by the rules KIO uses: the
/// administrator's `<top>/.Trash` (a real folder with the sticky bit) holds
/// `<uid>`; otherwise `<top>/.Trash-<uid>`. Either must be a real folder (not
/// a link), owned by `uid`, with mode 0700. Nothing is created.
pub fn volume_trash(top: &Path, uid: u32) -> Option<PathBuf> {
    let mine = |m: &fs::Metadata| m.uid() == uid && m.mode() & 0o777 == 0o700;
    let admin = top.join(".Trash");
    if let Some(m) = is_real_dir(&admin)
        && m.mode() & 0o1000 != 0
    {
        let dir = admin.join(uid.to_string());
        if let Some(d) = is_real_dir(&dir)
            && mine(&d)
        {
            return Some(dir);
        }
    }
    let dir = top.join(format!(".Trash-{uid}"));
    is_real_dir(&dir).filter(mine).map(|_| dir)
}

/// Every trash folder the user has: the home one first (when it exists), then
/// one for each mounted volume that has one. The volume the home trash is on
/// uses the home trash (KIO does not look for another there). Folders are
/// listed once.
pub fn trash_dirs(home_trash: &Path, mounts: &[Mount], uid: u32) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let home_dev = fs::metadata(home_trash).ok().map(|m| m.dev());
    // The home Trash itself may be a link the user made (to a bigger disk, say):
    // KIO follows it, and so does this, once; what is inside is checked below.
    if fs::metadata(home_trash).is_ok_and(|m| m.is_dir()) {
        seen.insert(home_trash.to_path_buf());
        out.push(home_trash.to_path_buf());
    }
    let mut points: HashSet<&Path> = HashSet::new();
    for m in mounts {
        if skipped_fs(&m.fstype) || !points.insert(m.point.as_path()) {
            continue;
        }
        if home_dev.is_some() && fs::metadata(&m.point).ok().map(|x| x.dev()) == home_dev {
            continue;
        }
        if let Some(dir) = volume_trash(&m.point, uid)
            && seen.insert(dir.clone())
        {
            out.push(dir);
        }
    }
    out
}

// ---- Emptying what is old ----

/// What a run found.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// Trash folders looked at.
    pub folders: usize,
    /// Items removed (with `apply` false: items that would be).
    pub removed: usize,
    /// Items not old enough.
    pub kept: usize,
    /// Items left alone because their `.trashinfo` could not be read or
    /// understood, or had no item.
    pub skipped: usize,
    /// Items that were old but could not be removed.
    pub failed: usize,
}

impl Report {
    fn add(&mut self, o: Report) {
        self.folders += o.folders;
        self.removed += o.removed;
        self.kept += o.kept;
        self.skipped += o.skipped;
        self.failed += o.failed;
    }
}

fn read_info(path: &Path) -> Result<Info, ()> {
    use std::os::unix::fs::OpenOptionsExt;
    // Not through a link, and not blocking on a named pipe.
    let mut f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| ())?;
    let meta = f.metadata().map_err(|_| ())?;
    if !meta.is_file() {
        return Err(());
    }
    let mut buf = Vec::new();
    (&mut f)
        .take(MAX_INFO_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|_| ())?;
    parse_info(&buf).map_err(|_| ())
}

/// Takes the line for `stem` out of `<trash>/directorysizes`, the cache of
/// folder sizes KIO keeps (`<size> <mtime> <percent-encoded name>`).
fn forget_directory_size(trash: &Path, stem: &[u8]) {
    let path = trash.join("directorysizes");
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return;
    };
    if !meta.is_file() || meta.len() > MAX_SIZES_BYTES {
        return;
    }
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let want = percent_encode(stem);
    let mut changed = false;
    let mut kept = String::with_capacity(text.len());
    for line in text.lines() {
        if line.splitn(3, ' ').nth(2) == Some(want.as_str()) {
            changed = true;
        } else {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    if !changed {
        return;
    }
    // A name of its own: two runs at once must not write one file.
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let tmp = trash.join(format!(
        "directorysizes.files-tmp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    // New, not through a link, private, and on the disk before it takes the
    // old file's place.
    let wrote = (|| -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&tmp)?;
        f.write_all(kept.as_bytes())?;
        f.sync_all()
    })()
    .is_ok();
    if !wrote
        || fs::set_permissions(&tmp, meta.permissions()).is_err()
        || fs::rename(&tmp, &path).is_err()
    {
        let _ = fs::remove_file(&tmp);
    }
}

/// Removes what is older than `days` at `now` from one trash folder (`trash`
/// holds `files` and `info`). With `apply` false nothing is changed and
/// `removed` counts what would go.
pub fn purge_dir(trash: &Path, now: i64, days: u32, apply: bool) -> Report {
    use std::os::unix::ffi::OsStrExt;
    let mut r = Report::default();
    let files = trash.join("files");
    let info = trash.join("info");
    // A link here would lead out of the Trash.
    if is_real_dir(&files).is_none() || is_real_dir(&info).is_none() {
        return r;
    }
    r.folders = 1;
    let Ok(entries) = fs::read_dir(&info) else {
        return r;
    };
    for (n, entry) in entries.enumerate() {
        if n >= MAX_ITEMS_PER_DIR {
            break;
        }
        let Ok(entry) = entry else {
            r.skipped += 1;
            continue;
        };
        let file_name = entry.file_name();
        let Some(stem) = file_name.as_bytes().strip_suffix(b".trashinfo") else {
            continue;
        };
        if stem.is_empty() || stem == b"." || stem == b".." {
            r.skipped += 1;
            continue;
        }
        let info_path = info.join(&file_name);
        let item = files.join(std::ffi::OsStr::from_bytes(stem));
        let Ok(item_meta) = fs::symlink_metadata(&item) else {
            // A `.trashinfo` with no item: not ours to tidy.
            r.skipped += 1;
            continue;
        };
        let Ok(details) = read_info(&info_path) else {
            r.skipped += 1;
            continue;
        };
        if !expired(details.deleted, now, days) {
            r.kept += 1;
            continue;
        }
        if !apply {
            r.removed += 1;
            continue;
        }
        let gone = if item_meta.is_dir() {
            fs::remove_dir_all(&item)
        } else {
            // A file, a link (removed as a link, never followed) or a special file.
            fs::remove_file(&item)
        };
        match gone {
            Ok(()) => {
                if item_meta.is_dir() {
                    forget_directory_size(trash, stem);
                }
                // The item is gone; the `.trashinfo` of a gone item is no use.
                if fs::remove_file(&info_path).is_ok() {
                    r.removed += 1;
                } else {
                    r.failed += 1;
                }
            }
            // Taken out of the Trash by someone else meanwhile.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => r.skipped += 1,
            Err(_) => r.failed += 1,
        }
    }
    r
}

/// [`purge_dir`] over every folder in `dirs`.
pub fn purge_all(dirs: &[PathBuf], now: i64, days: u32, apply: bool) -> Report {
    let mut total = Report::default();
    for d in dirs {
        total.add(purge_dir(d, now, days, apply));
    }
    total
}

/// The line for the journal.
pub fn log_line(r: &Report, days: u32, applied: bool) -> String {
    let verb = if applied { "removed" } else { "would remove" };
    let mut s = format!(
        "trash auto-empty: {verb} {} {} older than {days} {} ({} {} looked at, {} newer kept",
        r.removed,
        if r.removed == 1 { "item" } else { "items" },
        if days == 1 { "day" } else { "days" },
        r.folders,
        if r.folders == 1 {
            "trash folder"
        } else {
            "trash folders"
        },
        r.kept
    );
    if r.skipped > 0 {
        s.push_str(&format!(", {} left alone", r.skipped));
    }
    if r.failed > 0 {
        s.push_str(&format!(", {} could not be removed", r.failed));
    }
    s.push(')');
    s
}

// ---- Words ----

fn count_items(n: usize) -> String {
    if n == 1 {
        "1 item".to_string()
    } else {
        format!("{n} items")
    }
}

/// The question before folders that no longer exist are made again so items
/// can go back. `folders` are the missing folders, written to be shown; at
/// most three are named. Returns the dialog's title and its text.
pub fn recreate_question(folders: &[String], items: usize) -> (String, String) {
    let what = count_items(items);
    match folders {
        [] => (String::new(), String::new()),
        [one] => (
            "Folder Is Gone".to_string(),
            format!(
                "The folder \"{one}\" doesn't exist any more. Create it and put {what} back there?"
            ),
        ),
        many => {
            let named: Vec<String> = many.iter().take(3).map(|f| format!("\"{f}\"")).collect();
            let mut list = named.join(", ");
            if many.len() > 3 {
                list.push_str(&format!(" and {} more", many.len() - 3));
            }
            (
                "Folders Are Gone".to_string(),
                format!(
                    "{} folders don't exist any more: {list}. Create them and put {what} back?",
                    many.len()
                ),
            )
        }
    }
}

/// The question asked before "Empty items older than N days" is turned on.
/// `would_go` is how many items are older than that right now.
pub fn auto_empty_question(days: u32, would_go: usize) -> String {
    let unit = if days == 1 { "day" } else { "days" };
    let now = match would_go {
        0 => "Nothing in the Trash is that old now.".to_string(),
        1 => "1 item in the Trash is older than that and will be deleted now.".to_string(),
        n => format!("{n} items in the Trash are older than that and will be deleted now."),
    };
    format!(
        "Items that have been in the Trash for more than {days} {unit} will be deleted for good when Files starts and once a day while it is open. {now} This can't be undone."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    const T: i64 = 1_000_000_000;

    fn date(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
        civil_seconds(y, mo, d, h, mi, s).unwrap()
    }

    fn info_text(path: &str, when: &str) -> String {
        format!("[Trash Info]\nPath={path}\nDeletionDate={when}\n")
    }

    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "telamon-trash-{tag}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A trash folder (`<root>/Trash`) with `files` and `info`.
    fn trash(root: &Path) -> PathBuf {
        let t = root.join("Trash");
        fs::create_dir_all(t.join("files")).unwrap();
        fs::create_dir_all(t.join("info")).unwrap();
        t
    }

    fn put(t: &Path, name: &str, content: &str, info: &str) {
        fs::write(t.join("files").join(name), content).unwrap();
        fs::write(t.join("info").join(format!("{name}.trashinfo")), info).unwrap();
    }

    #[test]
    fn a_hostile_drives_trash_cannot_put_a_file_where_it_runs() {
        let home = b"/home/u";
        // KIO's own Trash: believed.
        assert!(restore_allowed(
            true,
            b"/home/u/.config/autostart/x.desktop",
            home
        ));
        // another drive's: not into hidden places of the home folder
        for bad in [
            &b"/home/u/.config/autostart/x.desktop"[..],
            b"/home/u/.bashrc",
            b"/home/u/.local/bin/x",
            b"/home/u/.ssh/authorized_keys",
            b"/home/u/Documents/../.bashrc",
        ] {
            assert!(
                !restore_allowed(false, bad, home),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
        for ok in [
            &b"/home/u/Documents/a.txt"[..],
            b"/run/media/u/stick/a.txt",
            b"/home/u/Pictures/.hidden-name-deeper/x",
            b"/home/user2/.bashrc",
        ] {
            assert!(
                restore_allowed(false, ok, home),
                "{}",
                String::from_utf8_lossy(ok)
            );
        }
        assert!(restore_allowed(false, b"/home/u/.x", b""));
    }

    #[test]
    fn a_date_is_read_only_in_the_one_form() {
        assert_eq!(parse_date("1970-01-01T00:00:00"), Some(0));
        assert_eq!(
            parse_date("2026-10-07T10:00:00"),
            Some(date(2026, 10, 7, 10, 0, 0))
        );
        assert_eq!(
            parse_date("2000-02-29T23:59:59"),
            Some(date(2000, 2, 29, 23, 59, 59))
        );
        for bad in [
            "",
            "2026-10-07",
            "2026-10-07 10:00:00",
            "2026-10-07T10:00:00Z",
            "2026-10-07T10:00:00+02:00",
            "2026-13-07T10:00:00",
            "2026-00-07T10:00:00",
            "2026-02-30T10:00:00",
            "2025-02-29T10:00:00",
            "2026-10-07T24:00:00",
            "2026-10-07T10:60:00",
            "2026-10-07T10:00:60",
            "2026-10-07T1x:00:00",
            "1969-12-31T23:59:59",
            "+026-10-07T10:00:00",
            "2026-10-07T10:00:00 ",
            "２０２６-10-07T10:00:00",
        ] {
            assert_eq!(parse_date(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn civil_seconds_matches_known_days() {
        // 2001-09-09T01:46:40 is 1,000,000,000 seconds after the epoch.
        assert_eq!(date(2001, 9, 9, 1, 46, 40), T);
        assert_eq!(date(1970, 1, 2, 0, 0, 0), 86_400);
    }

    #[test]
    fn a_trashinfo_needs_its_group_a_path_and_a_date() {
        let ok =
            parse_info(info_text("/home/u/a%20b.txt", "2026-10-07T10:00:00").as_bytes()).unwrap();
        assert_eq!(ok.original, b"/home/u/a b.txt");
        assert_eq!(ok.deleted, date(2026, 10, 7, 10, 0, 0));
        // Comments and blank lines first, CRLF, other keys and groups, spaces.
        let odd = "# note\n\n[Trash Info]\r\nPath = /x\r\nExtra=1\r\nDeletionDate= 2026-10-07T10:00:00\r\n[Other]\nPath=/y\n";
        assert_eq!(parse_info(odd.as_bytes()).unwrap().original, b"/x");
        let date_line = "DeletionDate=2026-10-07T10:00:00\n";
        assert_eq!(parse_info(b""), Err(InfoError::NotAnInfo));
        assert_eq!(
            parse_info(format!("{date_line}Path=/x\n").as_bytes()),
            Err(InfoError::NotAnInfo)
        );
        assert_eq!(
            parse_info(format!("[Other]\nPath=/x\n{date_line}").as_bytes()),
            Err(InfoError::NotAnInfo)
        );
        for bad_path in ["", "%zz", "/x%zz", "/x%2", "/x%00y"] {
            let t = format!("[Trash Info]\nPath={bad_path}\n{date_line}");
            assert_eq!(
                parse_info(t.as_bytes()),
                Err(InfoError::BadPath),
                "{bad_path:?}"
            );
        }
        assert_eq!(
            parse_info(format!("[Trash Info]\n{date_line}").as_bytes()),
            Err(InfoError::BadPath)
        );
        assert_eq!(
            parse_info(b"[Trash Info]\nPath=/x\n"),
            Err(InfoError::BadDate)
        );
        assert_eq!(
            parse_info(b"[Trash Info]\nPath=/x\nDeletionDate=yesterday\n"),
            Err(InfoError::BadDate)
        );
        assert_eq!(
            parse_info(b"[Trash Info]\nPath=/x\nDeletionDate=\n"),
            Err(InfoError::BadDate)
        );
        assert_eq!(
            parse_info(b"\xff\xfe[Trash Info]\n"),
            Err(InfoError::NotAnInfo)
        );
        let long = format!("[Trash Info]\nPath=/{}\n{date_line}", "a".repeat(70_000));
        assert_eq!(parse_info(long.as_bytes()), Err(InfoError::TooLong));
    }

    #[test]
    fn the_first_path_and_date_count() {
        let t = "[Trash Info]\nPath=/first\nPath=/second\nDeletionDate=2026-10-07T10:00:00\nDeletionDate=1999-01-01T00:00:00\n";
        let i = parse_info(t.as_bytes()).unwrap();
        assert_eq!(i.original, b"/first");
        assert_eq!(i.deleted, date(2026, 10, 7, 10, 0, 0));
    }

    #[test]
    fn expiry_follows_a_fake_clock() {
        let del = date(2026, 9, 1, 12, 0, 0);
        let day = 86_400;
        // Exactly 30 days is not yet older than 30 days; one second later is.
        assert!(!expired(del, del + 30 * day, 30));
        assert!(expired(del, del + 30 * day + 1, 30));
        assert!(!expired(del, del + 29 * day, 30));
        assert!(expired(del, del + 31 * day, 30));
        assert!(!expired(del, del + day, 1));
        assert!(expired(del, del + day + 1, 1));
        // The clock behind the date (the item is "from the future"), 0 days.
        assert!(!expired(del, del - 100 * day, 30));
        assert!(!expired(del, del + 1000 * day, 0));
        // Extremes do not overflow.
        assert!(!expired(i64::MAX, i64::MIN, 30));
        assert!(expired(i64::MIN, i64::MAX, 30));
    }

    #[test]
    fn days_are_brought_into_their_limits() {
        assert_eq!(clamp_days(-5), 1);
        assert_eq!(clamp_days(0), 1);
        assert_eq!(clamp_days(30), 30);
        assert_eq!(clamp_days(100_000), 3650);
    }

    #[test]
    fn only_old_items_go_and_newer_ones_are_never_touched() {
        let d = Dir::new("old");
        let t = trash(&d.0);
        let now = date(2026, 10, 8, 12, 0, 0);
        put(
            &t,
            "old.txt",
            "o",
            &info_text("/h/old.txt", "2026-08-01T09:00:00"),
        );
        put(
            &t,
            "edge.txt",
            "e",
            &info_text("/h/edge.txt", "2026-09-08T12:00:00"),
        );
        put(
            &t,
            "new.txt",
            "n",
            &info_text("/h/new.txt", "2026-10-07T09:00:00"),
        );
        put(
            &t,
            "future.txt",
            "f",
            &info_text("/h/future.txt", "2030-01-01T00:00:00"),
        );
        fs::create_dir_all(t.join("files/olddir/sub")).unwrap();
        fs::write(t.join("files/olddir/sub/x"), "x").unwrap();
        fs::write(
            t.join("info/olddir.trashinfo"),
            info_text("/h/olddir", "2026-07-01T00:00:00"),
        )
        .unwrap();
        let r = purge_dir(&t, now, 30, true);
        assert_eq!(r.removed, 2, "{r:?}");
        assert_eq!(r.kept, 3, "{r:?}");
        assert_eq!((r.skipped, r.failed, r.folders), (0, 0, 1));
        assert!(!t.join("files/old.txt").exists());
        assert!(!t.join("info/old.txt.trashinfo").exists());
        assert!(!t.join("files/olddir").exists());
        assert!(!t.join("info/olddir.trashinfo").exists());
        // 30 days to the second is not older than 30 days.
        for kept in ["edge.txt", "new.txt", "future.txt"] {
            assert!(t.join("files").join(kept).exists(), "{kept}");
            assert!(
                t.join("info").join(format!("{kept}.trashinfo")).exists(),
                "{kept}"
            );
        }
        // A second run, a day on: the edge item has gone over.
        let r = purge_dir(&t, now + 86_400, 30, true);
        assert_eq!((r.removed, r.kept), (1, 2), "{r:?}");
        assert!(!t.join("files/edge.txt").exists());
    }

    #[test]
    fn a_dry_run_changes_nothing() {
        let d = Dir::new("dry");
        let t = trash(&d.0);
        put(
            &t,
            "old.txt",
            "o",
            &info_text("/h/old.txt", "2020-01-01T00:00:00"),
        );
        let r = purge_dir(&t, date(2026, 10, 8, 0, 0, 0), 30, false);
        assert_eq!(r.removed, 1);
        assert!(t.join("files/old.txt").exists());
        assert!(t.join("info/old.txt.trashinfo").exists());
    }

    #[test]
    fn a_trashinfo_that_cannot_be_read_leaves_its_item_alone() {
        let d = Dir::new("bad");
        let t = trash(&d.0);
        let now = date(2026, 10, 8, 0, 0, 0);
        put(&t, "nodate.txt", "a", "[Trash Info]\nPath=/h/nodate.txt\n");
        put(
            &t,
            "baddate.txt",
            "b",
            &info_text("/h/baddate.txt", "long ago"),
        );
        put(
            &t,
            "zone.txt",
            "c",
            &info_text("/h/zone.txt", "2020-01-01T00:00:00Z"),
        );
        put(&t, "garbage.txt", "d", "\u{0}\u{1}not an ini");
        put(&t, "empty.txt", "e", "");
        let huge = format!(
            "[Trash Info]\nPath=/{}\nDeletionDate=2020-01-01T00:00:00\n",
            "x".repeat(70_000)
        );
        put(&t, "huge.txt", "f", &huge);
        put(
            &t,
            "good.txt",
            "g",
            &info_text("/h/good.txt", "2020-01-01T00:00:00"),
        );
        let r = purge_dir(&t, now, 30, true);
        assert_eq!(r.removed, 1, "{r:?}");
        assert_eq!(r.skipped, 6, "{r:?}");
        for n in [
            "nodate.txt",
            "baddate.txt",
            "zone.txt",
            "garbage.txt",
            "empty.txt",
            "huge.txt",
        ] {
            assert!(t.join("files").join(n).exists(), "{n}");
            assert!(
                t.join("info").join(format!("{n}.trashinfo")).exists(),
                "{n}"
            );
        }
        assert!(!t.join("files/good.txt").exists());
    }

    #[test]
    fn items_without_a_trashinfo_and_infos_without_an_item_are_left() {
        let d = Dir::new("orphan");
        let t = trash(&d.0);
        fs::write(t.join("files/stray.txt"), "s").unwrap();
        fs::write(
            t.join("info/ghost.txt.trashinfo"),
            info_text("/h/ghost.txt", "2020-01-01T00:00:00"),
        )
        .unwrap();
        fs::write(t.join("info/readme"), "not a trashinfo name").unwrap();
        let r = purge_dir(&t, date(2026, 10, 8, 0, 0, 0), 30, true);
        assert_eq!(r.removed, 0);
        assert_eq!(r.skipped, 1);
        assert!(t.join("files/stray.txt").exists());
        assert!(t.join("info/ghost.txt.trashinfo").exists());
        assert!(t.join("info/readme").exists());
    }

    #[test]
    fn a_link_is_removed_as_a_link_and_its_target_is_never_touched() {
        let d = Dir::new("links");
        let t = trash(&d.0);
        let outside = d.0.join("outside");
        fs::create_dir_all(outside.join("inner")).unwrap();
        fs::write(outside.join("keep.txt"), "keep").unwrap();
        fs::write(outside.join("inner/keep2.txt"), "keep").unwrap();
        // A link to a folder, a link to a file, a dangling link.
        symlink(&outside, t.join("files/dirlink")).unwrap();
        symlink(outside.join("keep.txt"), t.join("files/filelink")).unwrap();
        symlink("/nonexistent/x", t.join("files/dangling")).unwrap();
        for n in ["dirlink", "filelink", "dangling"] {
            fs::write(
                t.join("info").join(format!("{n}.trashinfo")),
                info_text(&format!("/h/{n}"), "2020-01-01T00:00:00"),
            )
            .unwrap();
        }
        // A folder with a link inside to the same place.
        fs::create_dir_all(t.join("files/folder")).unwrap();
        symlink(&outside, t.join("files/folder/up")).unwrap();
        fs::write(
            t.join("info/folder.trashinfo"),
            info_text("/h/folder", "2020-01-01T00:00:00"),
        )
        .unwrap();
        let r = purge_dir(&t, date(2026, 10, 8, 0, 0, 0), 30, true);
        assert_eq!(r.removed, 4, "{r:?}");
        assert!(fs::symlink_metadata(t.join("files/dirlink")).is_err());
        assert!(fs::symlink_metadata(t.join("files/filelink")).is_err());
        assert!(fs::symlink_metadata(t.join("files/dangling")).is_err());
        assert!(!t.join("files/folder").exists());
        assert_eq!(
            fs::read_to_string(outside.join("keep.txt")).unwrap(),
            "keep"
        );
        assert_eq!(
            fs::read_to_string(outside.join("inner/keep2.txt")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn a_trashinfo_that_is_a_link_is_not_followed() {
        let d = Dir::new("infolink");
        let t = trash(&d.0);
        let real = d.0.join("real.trashinfo");
        fs::write(&real, info_text("/h/a.txt", "2020-01-01T00:00:00")).unwrap();
        fs::write(t.join("files/a.txt"), "a").unwrap();
        symlink(&real, t.join("info/a.txt.trashinfo")).unwrap();
        let r = purge_dir(&t, date(2026, 10, 8, 0, 0, 0), 30, true);
        assert_eq!((r.removed, r.skipped), (0, 1), "{r:?}");
        assert!(t.join("files/a.txt").exists());
        assert!(real.exists());
    }

    #[test]
    fn a_trash_folder_whose_files_or_info_is_a_link_is_left_alone() {
        let d = Dir::new("dirlink");
        let t = d.0.join("Trash");
        let elsewhere = d.0.join("elsewhere");
        fs::create_dir_all(elsewhere.join("files")).unwrap();
        fs::create_dir_all(t.join("info")).unwrap();
        fs::write(elsewhere.join("files/a.txt"), "a").unwrap();
        fs::write(
            t.join("info/a.txt.trashinfo"),
            info_text("/h/a.txt", "2020-01-01T00:00:00"),
        )
        .unwrap();
        symlink(elsewhere.join("files"), t.join("files")).unwrap();
        let r = purge_dir(&t, date(2026, 10, 8, 0, 0, 0), 30, true);
        assert_eq!(r, Report::default());
        assert!(elsewhere.join("files/a.txt").exists());
        assert!(t.join("info/a.txt.trashinfo").exists());
    }

    #[test]
    fn a_folder_removed_takes_its_line_out_of_directorysizes() {
        let d = Dir::new("sizes");
        let t = trash(&d.0);
        fs::create_dir_all(t.join("files/old dir")).unwrap();
        fs::create_dir_all(t.join("files/new")).unwrap();
        fs::write(
            t.join("info/old dir.trashinfo"),
            info_text("/h/old%20dir", "2020-01-01T00:00:00"),
        )
        .unwrap();
        fs::write(
            t.join("info/new.trashinfo"),
            info_text("/h/new", "2026-10-07T00:00:00"),
        )
        .unwrap();
        fs::write(
            t.join("directorysizes"),
            "4096 1700000000 old%20dir\n4096 1700000001 new\n",
        )
        .unwrap();
        let r = purge_dir(&t, date(2026, 10, 8, 0, 0, 0), 30, true);
        assert_eq!(r.removed, 1);
        assert_eq!(
            fs::read_to_string(t.join("directorysizes")).unwrap(),
            "4096 1700000001 new\n"
        );
        assert!(fs::read_dir(&t).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .contains("files-tmp")
        }));
    }

    #[test]
    fn names_with_odd_bytes_work() {
        let d = Dir::new("odd");
        let t = trash(&d.0);
        let name = "we ird\u{202e}\n.txt";
        put(&t, name, "x", &info_text("/h/x", "2020-01-01T00:00:00"));
        let r = purge_dir(&t, date(2026, 10, 8, 0, 0, 0), 30, true);
        assert_eq!(r.removed, 1);
        assert!(!t.join("files").join(name).exists());
    }

    #[test]
    fn mountinfo_gives_mount_points_and_file_systems() {
        let text = "\
22 1 0:21 / / rw,relatime shared:1 - ext4 /dev/sda1 rw
23 22 0:22 / /proc rw - proc proc rw
24 22 0:30 / /mnt/my\\040disk rw - ext4 /dev/sdb1 rw
25 22 0:31 / /srv/share rw - nfs4 server:/x rw
26 22 0:32 / /vol1 rw master:2 shared:3 - tmpfs tmpfs rw
nonsense
27 22 0:33 relative-not-absolute rw - ext4 x rw
";
        let m = parse_mountinfo(text);
        let points: Vec<_> = m
            .iter()
            .map(|x| (x.point.to_str().unwrap(), x.fstype.as_str()))
            .collect();
        assert_eq!(
            points,
            vec![
                ("/", "ext4"),
                ("/proc", "proc"),
                ("/mnt/my disk", "ext4"),
                ("/srv/share", "nfs4"),
                ("/vol1", "tmpfs")
            ]
        );
        assert!(
            skipped_fs("proc")
                && skipped_fs("nfs4")
                && skipped_fs("fuse.sshfs")
                && skipped_fs("cifs")
                && skipped_fs("tmpfs")
        );
        assert!(
            !skipped_fs("ext4")
                && !skipped_fs("fuse.gocryptfs")
                && !skipped_fs("btrfs")
                && !skipped_fs("fuseblk")
                && !skipped_fs("exfat")
        );
    }

    fn mode(p: &Path, m: u32) {
        fs::set_permissions(p, fs::Permissions::from_mode(m)).unwrap();
    }

    fn uid() -> u32 {
        current_uid()
    }

    #[test]
    fn a_volume_trash_follows_kios_rules() {
        let d = Dir::new("vol");
        let u = uid();
        // .Trash-<uid>, mode 0700.
        let a = d.0.join("a");
        fs::create_dir_all(a.join(format!(".Trash-{u}"))).unwrap();
        mode(&a.join(format!(".Trash-{u}")), 0o700);
        assert_eq!(volume_trash(&a, u), Some(a.join(format!(".Trash-{u}"))));
        // Wrong mode or owner: KIO would not use it.
        mode(&a.join(format!(".Trash-{u}")), 0o755);
        assert_eq!(volume_trash(&a, u), None);
        mode(&a.join(format!(".Trash-{u}")), 0o700);
        assert_eq!(volume_trash(&a, u + 1), None);
        // A link in its place.
        let b = d.0.join("b");
        fs::create_dir_all(&b).unwrap();
        symlink(&a, b.join(format!(".Trash-{u}"))).unwrap();
        assert_eq!(volume_trash(&b, u), None);
        // .Trash/<uid> needs the sticky bit on .Trash.
        let c = d.0.join("c");
        fs::create_dir_all(c.join(format!(".Trash/{u}"))).unwrap();
        mode(&c.join(format!(".Trash/{u}")), 0o700);
        assert_eq!(volume_trash(&c, u), None);
        mode(&c.join(".Trash"), 0o1777);
        assert_eq!(volume_trash(&c, u), Some(c.join(format!(".Trash/{u}"))));
        // .Trash as a link is refused; with .Trash-<uid> beside it, that one is used.
        let e = d.0.join("e");
        fs::create_dir_all(e.join(format!(".Trash-{u}"))).unwrap();
        mode(&e.join(format!(".Trash-{u}")), 0o700);
        symlink(&c, e.join(".Trash")).unwrap();
        assert_eq!(volume_trash(&e, u), Some(e.join(format!(".Trash-{u}"))));
        // Nothing there.
        assert_eq!(volume_trash(&d.0.join("none"), u), None);
    }

    #[test]
    fn every_known_trash_folder_is_listed_once() {
        let d = Dir::new("all");
        let u = uid();
        let home = trash(&d.0);
        // On the home Trash's own file system a volume's folder is not looked for.
        let same = d.0.join("same");
        fs::create_dir_all(same.join(format!(".Trash-{u}"))).unwrap();
        mode(&same.join(format!(".Trash-{u}")), 0o700);
        let mounts = vec![
            Mount {
                point: same.clone(),
                fstype: "ext4".into(),
            },
            Mount {
                point: PathBuf::from("/proc"),
                fstype: "proc".into(),
            },
        ];
        assert_eq!(trash_dirs(&home, &mounts, u), vec![home.clone()]);
        // A home Trash that does not exist is not listed; one that is a link to a folder is.
        assert!(trash_dirs(&d.0.join("no-such"), &[], u).is_empty());
        let linked = d.0.join("linked-trash");
        symlink(&home, &linked).unwrap();
        assert_eq!(trash_dirs(&linked, &[], u), vec![linked.clone()]);
        // A volume on another file system is (the memory file system stands in
        // for one, when this machine has one apart from the temporary folder's).
        let shm = PathBuf::from("/dev/shm");
        let other_fs =
            fs::metadata(&shm).ok().map(|m| m.dev()) != fs::metadata(&home).ok().map(|m| m.dev());
        if shm.is_dir() && other_fs {
            let vol = shm.join(format!("telamon-trash-vol-{}", std::process::id()));
            let _ = fs::remove_dir_all(&vol);
            fs::create_dir_all(vol.join(format!(".Trash-{u}/files"))).unwrap();
            mode(&vol.join(format!(".Trash-{u}")), 0o700);
            let mounts = vec![
                // KIO takes a tmpfs for a pseudo file system and has no trash there.
                Mount {
                    point: vol.clone(),
                    fstype: "tmpfs".into(),
                },
                Mount {
                    point: vol.clone(),
                    fstype: "ext4".into(),
                },
                Mount {
                    point: vol.clone(),
                    fstype: "ext4".into(),
                },
                Mount {
                    point: vol.clone(),
                    fstype: "nfs4".into(),
                },
            ];
            let listed = trash_dirs(&home, &mounts, u);
            let _ = fs::remove_dir_all(&vol);
            assert_eq!(listed, vec![home.clone(), vol.join(format!(".Trash-{u}"))]);
        }
    }

    #[test]
    fn purge_all_adds_up_and_the_line_says_it() {
        let d = Dir::new("sum");
        let a = trash(&d.0.join("x").tap_create());
        let b = trash(&d.0.join("y").tap_create());
        put(&a, "o", "o", &info_text("/h/o", "2020-01-01T00:00:00"));
        put(&b, "o", "o", &info_text("/h/o", "2020-01-01T00:00:00"));
        put(&b, "n", "n", &info_text("/h/n", "2026-10-07T00:00:00"));
        put(&b, "bad", "b", "junk");
        let r = purge_all(&[a, b], date(2026, 10, 8, 0, 0, 0), 30, true);
        assert_eq!(
            (r.folders, r.removed, r.kept, r.skipped, r.failed),
            (2, 2, 1, 1, 0)
        );
        assert_eq!(
            log_line(&r, 30, true),
            "trash auto-empty: removed 2 items older than 30 days (2 trash folders looked at, 1 newer kept, 1 left alone)"
        );
        assert_eq!(
            log_line(
                &Report {
                    folders: 1,
                    removed: 1,
                    ..Report::default()
                },
                1,
                false
            ),
            "trash auto-empty: would remove 1 item older than 1 day (1 trash folder looked at, 0 newer kept)"
        );
    }

    #[test]
    fn the_questions_name_what_is_asked() {
        let (t, q) = recreate_question(&["~/Projects/Old".to_string()], 2);
        assert_eq!(t, "Folder Is Gone");
        assert_eq!(
            q,
            "The folder \"~/Projects/Old\" doesn't exist any more. Create it and put 2 items back there?"
        );
        assert!(
            recreate_question(&["~/a".to_string()], 1)
                .1
                .contains("put 1 item back")
        );
        let many: Vec<String> = (1..=5).map(|i| format!("~/f{i}")).collect();
        let (t, q) = recreate_question(&many, 7);
        assert_eq!(t, "Folders Are Gone");
        assert_eq!(
            q,
            "5 folders don't exist any more: \"~/f1\", \"~/f2\", \"~/f3\" and 2 more. Create them and put 7 items back?"
        );
        assert_eq!(recreate_question(&[], 3), (String::new(), String::new()));
        let q = auto_empty_question(30, 12);
        assert!(q.starts_with("Items that have been in the Trash for more than 30 days"));
        assert!(q.contains("12 items in the Trash are older than that and will be deleted now."));
        assert!(q.ends_with("This can't be undone."));
        assert!(auto_empty_question(1, 1).contains("more than 1 day will"));
        assert!(auto_empty_question(1, 1).contains("1 item in the Trash is older"));
        assert!(auto_empty_question(7, 0).contains("Nothing in the Trash is that old now."));
    }

    trait TapCreate {
        fn tap_create(self) -> PathBuf;
    }
    impl TapCreate for PathBuf {
        fn tap_create(self) -> PathBuf {
            fs::create_dir_all(&self).unwrap();
            self
        }
    }
}
