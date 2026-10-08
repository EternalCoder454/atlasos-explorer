//! The Home page: its address, and Frequent folders.
//!
//! Home shows three plain sections (Pinned, Recent files, Frequent folders)
//! and nothing the user did not open or pin themselves. Frequent is Files'
//! own count of the folders the user went to, kept in Files' settings on
//! this computer and nowhere else; it is bounded in how many folders it
//! holds, in how big a count can grow and in how long an unused folder stays,
//! and the user can clear it.
//!
//! The counts are text, one folder per line (`count`, `last visit`, key,
//! separated by tabs), so the settings file stays readable and the app has
//! nothing to parse: [`Frequent::parse`] reads what was saved (anything can be
//! in a file; bad lines are dropped and numbers are brought into their limits)
//! and [`Frequent::to_text`] writes it back.

/// The Home page's address. It is not a folder KIO lists: the window shows
/// the page in its place.
pub const HOME_URL: &str = "home:/";

/// How many folders are counted; the least visited, longest unused one goes
/// first.
pub const MAX_FOLDERS: usize = 200;
/// The biggest count kept. A folder at the limit stays there; the others
/// catch up as they are visited (nothing overflows).
pub const MAX_COUNT: u32 = 1000;
/// A folder not visited for this long is forgotten, in seconds (180 days).
pub const MAX_AGE: i64 = 180 * 24 * 3600;
/// A folder is listed as frequent from this many visits on: one visit is not a habit.
pub const MIN_VISITS: u32 = 2;
/// Most folders the page lists.
pub const SHOWN: usize = 8;
/// The longest key kept, in bytes (a longer location is not counted).
pub const MAX_KEY: usize = 2048;

/// Whether `url` is the Home page.
pub fn is_home(url: &str) -> bool {
    let rest = match url.get(..5) {
        Some(s) if s.eq_ignore_ascii_case("home:") => &url[5..],
        _ => return false,
    };
    rest.is_empty() || rest == "/"
}

/// The schemes whose folders are counted: places the user goes to on purpose
/// that can be opened again later. The Trash, Recent, the network browser,
/// archives and the Home page are views, not places to come back to.
fn countable_scheme(scheme: &str) -> bool {
    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "file" | "smb" | "sftp" | "ftp" | "ftps" | "webdav" | "webdavs" | "nfs" | "fish" | "mtp"
    )
}

/// The key a location is counted under, or `None` when it is not counted: a
/// plain location (`scheme://...`, percent-encoded as KIO writes it) without a
/// password, query or fragment and without a trailing slash, on one line,
/// not too long; the home folder itself (`home`, a plain path) and the top of
/// the disk are not counted (they would always lead).
pub fn key_of(url: &str, home: &str) -> Option<String> {
    if url.is_empty() || url.len() > MAX_KEY || url.chars().any(char::is_control) {
        return None;
    }
    let colon = url.find(':')?;
    if !countable_scheme(&url[..colon]) {
        return None;
    }
    let rest = &url[colon + 1..];
    // Never a query, a fragment or a password: the settings file holds locations.
    if rest.contains(['?', '#']) {
        return None;
    }
    let after = rest.strip_prefix("//")?;
    let (authority, path) = match after.find('/') {
        Some(i) => (&after[..i], &after[i..]),
        None => (after, "/"),
    };
    if authority
        .rsplit_once('@')
        .is_some_and(|(u, _)| u.contains(':'))
    {
        return None;
    }
    let local = url[..colon].eq_ignore_ascii_case("file");
    if local && !(authority.is_empty() || authority.eq_ignore_ascii_case("localhost")) {
        return None;
    }
    let path = path.trim_end_matches('/');
    if local {
        if path.is_empty() {
            return None;
        }
        let home = home.trim_end_matches('/');
        if !home.is_empty() && decode_lossy(path) == home {
            return None;
        }
        return Some(format!("file://{path}"));
    }
    if authority.is_empty() {
        return None;
    }
    Some(format!(
        "{}://{authority}{path}",
        url[..colon].to_ascii_lowercase()
    ))
}

fn decode_lossy(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(h) = b
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(h);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One counted folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub count: u32,
    /// Seconds since the epoch of the last visit.
    pub last: i64,
}

/// The counted folders, in no particular order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frequent {
    entries: Vec<Entry>,
}

impl Frequent {
    /// Reads a saved list. Lines that do not parse are dropped, a folder
    /// listed twice keeps its first line, counts are brought into their
    /// limits and only [`MAX_FOLDERS`] are kept (the most visited first).
    pub fn parse(text: &str) -> Frequent {
        let mut entries: Vec<Entry> = Vec::new();
        for line in text.lines() {
            let mut it = line.splitn(3, '\t');
            let (Some(count), Some(last), Some(key)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            let (Ok(count), Ok(last)) = (count.parse::<u32>(), last.parse::<i64>()) else {
                continue;
            };
            if count == 0
                || key.is_empty()
                || key.len() > MAX_KEY
                || key.chars().any(char::is_control)
            {
                continue;
            }
            if entries.iter().any(|e| e.key == key) {
                continue;
            }
            entries.push(Entry {
                key: key.to_string(),
                count: count.min(MAX_COUNT),
                last,
            });
        }
        let mut f = Frequent { entries };
        f.bound();
        f
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&format!("{}\t{}\t{}\n", e.count, e.last, e.key));
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The least visited, longest unused folders go first.
    fn rank(a: &Entry, b: &Entry) -> std::cmp::Ordering {
        b.count
            .cmp(&a.count)
            .then(b.last.cmp(&a.last))
            .then(a.key.cmp(&b.key))
    }

    fn bound(&mut self) {
        self.entries.sort_by(Frequent::rank);
        self.entries.truncate(MAX_FOLDERS);
    }

    /// The folder `key` was visited at time `now`. Returns whether the
    /// list changed (a key that is not valid changes nothing). Folders not
    /// visited for [`MAX_AGE`] are forgotten here.
    pub fn visit(&mut self, key: &str, now: i64) -> bool {
        if key.is_empty() || key.len() > MAX_KEY || key.chars().any(char::is_control) {
            return false;
        }
        self.entries
            .retain(|e| now.saturating_sub(e.last) <= MAX_AGE);
        match self.entries.iter_mut().find(|e| e.key == key) {
            Some(e) => {
                e.count = (e.count + 1).min(MAX_COUNT);
                e.last = now;
            }
            None => {
                // When the list is full the least visited, longest unused
                // folder makes room.
                if self.entries.len() >= MAX_FOLDERS {
                    self.entries.sort_by(Frequent::rank);
                    self.entries.pop();
                }
                self.entries.push(Entry {
                    key: key.to_string(),
                    count: 1,
                    last: now,
                });
            }
        }
        self.bound();
        true
    }

    /// Forgets one folder (it is gone, or the user removed it).
    pub fn forget(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.key != key);
        self.entries.len() != before
    }

    /// Forgets everything (the "Clear" action).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The most visited folders, first the most visited, then the most
    /// recently visited, only those visited [`MIN_VISITS`] times or more, at
    /// most `n`.
    pub fn top(&self, n: usize) -> Vec<&Entry> {
        let mut v: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| e.count >= MIN_VISITS)
            .collect();
        v.sort_by(|a, b| Frequent::rank(a, b));
        v.truncate(n);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/me";

    #[test]
    fn the_home_page_address() {
        assert!(is_home("home:/"));
        assert!(is_home("home:"));
        assert!(is_home("HOME:/"));
        assert!(!is_home("home:/x"));
        assert!(!is_home("file:///home/me"));
        assert!(!is_home("homes:/"));
        assert!(!is_home(""));
        assert!(!is_home("ho"));
    }

    #[test]
    fn keys_are_plain_locations_and_never_a_password() {
        let k = |u: &str| key_of(u, HOME);
        assert_eq!(
            k("file:///home/me/Documents/").as_deref(),
            Some("file:///home/me/Documents")
        );
        assert_eq!(
            k("file://localhost/srv/data").as_deref(),
            Some("file:///srv/data")
        );
        assert_eq!(
            k("sftp://me@nas.lan/share/").as_deref(),
            Some("sftp://me@nas.lan/share")
        );
        assert_eq!(k("SMB://nas/").as_deref(), Some("smb://nas"));
        // the home folder (also written encoded) and the top of the disk lead every list
        assert_eq!(k("file:///home/me"), None);
        assert_eq!(k("file:///home/me/"), None);
        assert_eq!(k("file:///home/%6De"), None);
        assert_eq!(k("file:///"), None);
        // a password, a query or a fragment is never kept
        assert_eq!(k("sftp://me:secret@nas/x"), None);
        assert_eq!(k("ftp://nas/x?pass=1"), None);
        assert_eq!(k("ftp://nas/x#frag"), None);
        // views, not places
        for u in [
            "trash:/",
            "recentlyused:/",
            "network:/",
            "home:/",
            "zip:/a.zip/b",
            "remote:/",
        ] {
            assert_eq!(k(u), None, "{u}");
        }
        // hostile text
        assert_eq!(k("file:///a\nb"), None);
        // bidi characters are made visible when a name is shown, not here
        assert!(k("file:///a\u{202e}b").is_some());
        assert_eq!(k("file://evil.example/etc"), None);
        assert_eq!(k("nonsense"), None);
        assert_eq!(k(""), None);
        assert_eq!(k(&format!("file:///{}", "a".repeat(MAX_KEY))), None);
        assert_eq!(k("sftp:///nohost"), None);
    }

    #[test]
    fn visits_are_counted_and_bounded() {
        let mut f = Frequent::default();
        assert!(f.visit("file:///a", 100));
        assert_eq!(f.top(5).len(), 0, "one visit is not frequent");
        f.visit("file:///a", 101);
        f.visit("file:///b", 102);
        f.visit("file:///b", 103);
        f.visit("file:///b", 104);
        let top = f.top(5);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].key, "file:///b");
        assert_eq!((top[0].count, top[0].last), (3, 104));
        assert_eq!(top[1].key, "file:///a");
        // a tie goes to the more recent visit
        f.visit("file:///a", 105);
        let top = f.top(5);
        assert_eq!(top[0].key, "file:///a");
        assert_eq!(top[1].key, "file:///b");
        // n limits the answer
        assert_eq!(f.top(1).len(), 1);
        assert_eq!(f.top(0).len(), 0);
        // invalid keys change nothing
        assert!(!f.visit("", 1));
        assert!(!f.visit("file:///a\nb", 1));
        assert!(!f.visit(&"x".repeat(MAX_KEY + 1), 1));
    }

    #[test]
    fn the_list_and_each_count_have_a_limit() {
        let mut f = Frequent::default();
        for i in 0..MAX_FOLDERS + 50 {
            f.visit(&format!("file:///d{i}"), 1000 + i as i64);
        }
        assert_eq!(f.len(), MAX_FOLDERS);
        // a very visited folder survives a flood of new ones
        let mut f = Frequent::default();
        for _ in 0..5 {
            f.visit("file:///favourite", 10);
        }
        for i in 0..MAX_FOLDERS * 2 {
            f.visit(&format!("file:///n{i}"), 100 + i as i64);
        }
        assert_eq!(f.len(), MAX_FOLDERS);
        assert!(f.top(3).iter().any(|e| e.key == "file:///favourite"));
        // a count stops at its limit
        let mut f = Frequent::default();
        for _ in 0..MAX_COUNT + 20 {
            f.visit("file:///x", 5);
        }
        assert_eq!(f.top(1)[0].count, MAX_COUNT);
    }

    #[test]
    fn an_unused_folder_is_forgotten_after_a_long_time() {
        let mut f = Frequent::default();
        f.visit("file:///old", 0);
        f.visit("file:///old", 1);
        assert_eq!(f.top(5).len(), 1);
        f.visit("file:///new", MAX_AGE + 10);
        assert_eq!(f.len(), 1);
        assert_eq!(f.top(5).len(), 0);
    }

    #[test]
    fn clear_and_forget() {
        let mut f = Frequent::default();
        f.visit("file:///a", 1);
        f.visit("file:///b", 2);
        assert!(f.forget("file:///a"));
        assert!(!f.forget("file:///a"));
        assert_eq!(f.len(), 1);
        f.clear();
        assert!(f.is_empty());
        assert_eq!(f.to_text(), "");
    }

    #[test]
    fn text_round_trips_and_survives_a_hand_edited_file() {
        let mut f = Frequent::default();
        f.visit("file:///a b/c", 7);
        f.visit("file:///a b/c", 9);
        f.visit("sftp://me@nas/x", 8);
        let again = Frequent::parse(&f.to_text());
        assert_eq!(again, f);

        let hostile = "garbage\n\
            0\t5\tfile:///zero\n\
            -3\t5\tfile:///negative\n\
            x\t5\tfile:///nan\n\
            4\tnot-a-time\tfile:///time\n\
            4\t5\t\n\
            99999999\t5\tfile:///big\n\
            4\t5\tfile:///dup\n\
            9\t6\tfile:///dup\n\
            3\t5\tfile:///tab\tin-key\n\
            2\t5\tfile:///ctl\u{7}\n";
        let f = Frequent::parse(hostile);
        let keys: Vec<&str> = f.entries.iter().map(|e| e.key.as_str()).collect();
        assert!(keys.contains(&"file:///big"));
        assert!(keys.contains(&"file:///dup"));
        assert!(!keys.contains(&"file:///zero"));
        assert!(!keys.contains(&"file:///negative"));
        assert!(!keys.contains(&"file:///ctl\u{7}"));
        let big = f.entries.iter().find(|e| e.key == "file:///big").unwrap();
        assert_eq!(big.count, MAX_COUNT);
        let dup = f.entries.iter().find(|e| e.key == "file:///dup").unwrap();
        assert_eq!(dup.count, 4, "the first line of a folder wins");
        // a key with a tab inside (a control character) is dropped
        assert!(!f.entries.iter().any(|e| e.key.contains("tab")));
        // a file with too many lines is cut to the bound
        let many: String = (0..MAX_FOLDERS * 3)
            .map(|i| format!("{}\t5\tfile:///m{i}\n", 1 + i % 7))
            .collect();
        assert_eq!(Frequent::parse(&many).len(), MAX_FOLDERS);
    }
}
