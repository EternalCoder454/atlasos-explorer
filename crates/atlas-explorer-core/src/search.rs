//! The search field's decisions, with no Qt: what the filter chips mean as
//! `Search1` options, where a search runs (the index or a live walk), what
//! the status chip says, and how a result's folder is written in the Path
//! column. Pure data in, data out. The window asks the index over D-Bus and
//! draws the results (`cpp/kio/SearchController.*`, `qml/SearchBar.qml`); see
//! docs/DESIGN.md, "Search".

use crate::display::display_name;
use crate::location;

/// Hits one search shows at most (the index never returns more).
pub const MAX_HITS: usize = 500;
/// Hits a live walk collects before it stops by itself.
pub const MAX_LIVE_HITS: usize = 5000;
/// Smallest size of a "Medium" file, and one past the largest "Small" one.
pub const SMALL_LIMIT: u64 = 1024 * 1024;
/// Smallest size of a "Large" file.
pub const LARGE_LIMIT: u64 = 100 * 1024 * 1024;

const DAY: i64 = 24 * 60 * 60;

/// The Kind chip. The numbers are the C ABI's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Any = 0,
    Document = 1,
    Image = 2,
    Audio = 3,
    Video = 4,
    Archive = 5,
    Code = 6,
    Folder = 7,
}

impl Kind {
    pub fn from_u32(v: u32) -> Kind {
        match v {
            1 => Kind::Document,
            2 => Kind::Image,
            3 => Kind::Audio,
            4 => Kind::Video,
            5 => Kind::Archive,
            6 => Kind::Code,
            7 => Kind::Folder,
            _ => Kind::Any,
        }
    }

    /// The index's categories (the `kinds` option) a kind stands for. A
    /// Document is anything a person would call one: text, PDF, spreadsheets
    /// and slides too.
    pub fn categories(self) -> &'static [&'static str] {
        match self {
            Kind::Any => &[],
            Kind::Document => &["document", "spreadsheet", "presentation", "pdf", "text"],
            Kind::Image => &["image"],
            Kind::Audio => &["audio"],
            Kind::Video => &["video"],
            Kind::Archive => &["archive"],
            Kind::Code => &["code"],
            Kind::Folder => &["folder"],
        }
    }
}

/// The Modified chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modified {
    Any = 0,
    Today = 1,
    Week = 2,
    Month = 3,
    Year = 4,
}

impl Modified {
    pub fn from_u32(v: u32) -> Modified {
        match v {
            1 => Modified::Today,
            2 => Modified::Week,
            3 => Modified::Month,
            4 => Modified::Year,
            _ => Modified::Any,
        }
    }
}

/// The Size chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeClass {
    Any = 0,
    Small = 1,
    Medium = 2,
    Large = 3,
}

impl SizeClass {
    pub fn from_u32(v: u32) -> SizeClass {
        match v {
            1 => SizeClass::Small,
            2 => SizeClass::Medium,
            3 => SizeClass::Large,
            _ => SizeClass::Any,
        }
    }
}

/// What the chips mean, in the terms of `Search1`'s options (and of the live
/// walk, which filters the same way).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    /// Categories for `kinds`; empty means any.
    pub kinds: Vec<&'static str>,
    /// Only files (`kind` = "file"): a size says nothing about a folder.
    pub files_only: bool,
    /// `modified_after`, seconds since the epoch.
    pub modified_after: Option<i64>,
    pub size_min: Option<u64>,
    pub size_max: Option<u64>,
}

impl Filter {
    /// Is any chip set?
    pub fn is_set(&self) -> bool {
        !self.kinds.is_empty()
            || self.files_only
            || self.modified_after.is_some()
            || self.size_min.is_some()
            || self.size_max.is_some()
    }
}

/// The filter for the chips. `now` is the time in seconds since the epoch and
/// `start_of_today` the local midnight before it: "Today" is since then, the
/// other periods are 7, 30 and 365 days back from now.
pub fn filter(
    kind: Kind,
    modified: Modified,
    size: SizeClass,
    now: i64,
    start_of_today: i64,
) -> Filter {
    let modified_after = match modified {
        Modified::Any => None,
        Modified::Today => Some(start_of_today.min(now)),
        Modified::Week => Some(now.saturating_sub(7 * DAY)),
        Modified::Month => Some(now.saturating_sub(30 * DAY)),
        Modified::Year => Some(now.saturating_sub(365 * DAY)),
    };
    let (size_min, size_max) = match size {
        SizeClass::Any => (None, None),
        SizeClass::Small => (None, Some(SMALL_LIMIT - 1)),
        SizeClass::Medium => (Some(SMALL_LIMIT), Some(LARGE_LIMIT - 1)),
        SizeClass::Large => (Some(LARGE_LIMIT), None),
    };
    Filter {
        kinds: kind.categories().to_vec(),
        files_only: size != SizeClass::Any,
        modified_after,
        size_min,
        size_max,
    }
}

/// Where a search for the typed text runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The index, everything it holds.
    IndexEverywhere = 0,
    /// The index, below the folder shown.
    IndexFolder = 1,
    /// A walk of the folder shown, on this computer.
    LiveFolder = 2,
    /// A walk of the home folder (the index is turned off).
    LiveHome = 3,
    /// A walk of a folder on a server, an archive, the Trash and so on, by KIO.
    LiveRemote = 4,
}

/// Which scope the user chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    ThisFolder = 0,
    Everywhere = 1,
}

impl Scope {
    pub fn from_u32(v: u32) -> Scope {
        if v == 1 {
            Scope::Everywhere
        } else {
            Scope::ThisFolder
        }
    }
}

/// Where to search. `local` says the folder shown is on this computer,
/// `indexed` that the index covers it (it is below an indexed root, on the
/// same disk, and not in a folder the index leaves out), and `index_on` that
/// there is an index to ask (it is not turned off).
pub fn route(scope: Scope, local: bool, indexed: bool, index_on: bool) -> Route {
    match scope {
        Scope::Everywhere => {
            if index_on {
                Route::IndexEverywhere
            } else {
                Route::LiveHome
            }
        }
        Scope::ThisFolder => {
            if !local {
                Route::LiveRemote
            } else if indexed && index_on {
                Route::IndexFolder
            } else {
                Route::LiveFolder
            }
        }
    }
}

/// Where a search runs when the index cannot do it: a name pattern (the index
/// matches words, not patterns) and words inside files (the index holds no
/// content) are always a walk of the folders.
pub fn route_live(scope: Scope, local: bool) -> Route {
    match scope {
        Scope::Everywhere => Route::LiveHome,
        Scope::ThisFolder if local => Route::LiveFolder,
        Scope::ThisFolder => Route::LiveRemote,
    }
}

/// How the index's `Status` state reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexState {
    /// Nothing is known yet.
    Unknown,
    Ready,
    /// "scanning" or "stale": results may be missing.
    Updating,
    /// "disabled": no roots, indexing is off.
    Off,
    /// "error".
    Problem,
    /// The service did not answer.
    Unavailable,
}

impl IndexState {
    pub fn from_status(state: &str) -> IndexState {
        match state {
            "ready" => IndexState::Ready,
            "scanning" | "stale" => IndexState::Updating,
            "disabled" => IndexState::Off,
            "error" => IndexState::Problem,
            _ => IndexState::Unknown,
        }
    }

    /// Is there an index to ask? Off means "search is a live walk".
    pub fn is_on(self) -> bool {
        self != IndexState::Off
    }

    /// The status chip: how it is drawn (0 none, 1 good, 2 warning, 3 error)
    /// and what it says. `error` is the service's plain-words reason.
    pub fn chip(self, error: &str) -> (u32, String) {
        match self {
            IndexState::Unknown => (0, String::new()),
            IndexState::Ready => (1, "The search index is up to date".into()),
            IndexState::Updating => (
                2,
                "Updating the search index, results may be missing".into(),
            ),
            IndexState::Off => (
                0,
                "The search index is turned off, so searches look through the folders".into(),
            ),
            IndexState::Problem => {
                let why = display_name(error);
                if why.is_empty() {
                    (
                        2,
                        "The search index has a problem, results may be missing".into(),
                    )
                } else {
                    (
                        2,
                        format!("The search index has a problem, results may be missing: {why}"),
                    )
                }
            }
            IndexState::Unavailable => (3, UNAVAILABLE_LINE.into()),
        }
    }
}

/// The reason shown in place of results when the index cannot be asked.
pub const UNAVAILABLE_TITLE: &str = "Search Isn't Available";
pub const UNAVAILABLE_TEXT: &str = "Files couldn't reach the search index. Try again in a moment.";
/// The same in a line (the chip and the status line).
pub const UNAVAILABLE_LINE: &str = "Search isn't available";

/// The line for the number of results.
pub fn count_text(n: usize, capped: bool) -> String {
    match n {
        0 => "No results".into(),
        1 => "1 result".into(),
        n if capped => format!("The first {n} results (type more to narrow them)"),
        n => format!("{n} results"),
    }
}

/// The line while a walk runs or after it was stopped.
pub fn live_text(found: usize, running: bool, stopped: bool, capped: bool) -> String {
    let n = match found {
        0 => "nothing found yet".to_string(),
        1 => "1 found".into(),
        n => format!("{n} found"),
    };
    if running {
        format!("Searching, {n}")
    } else if stopped {
        format!("Stopped, {n}")
    } else if capped {
        format!("Stopped at {found} results, {n}. Type more to narrow them")
    } else if found == 0 {
        "No results".into()
    } else {
        count_text(found, false)
    }
}

/// A result's folder as the Path column writes it: below the home folder
/// `~/Documents/Reports`, elsewhere on this computer the plain path, on a
/// server `host/path`, in the Trash `Trash/folder`. Every part is made safe
/// to show. `parent` is the folder's URL (as KIO writes it), `home` the home
/// folder as a plain path.
pub fn path_text(parent: &str, home: &str) -> String {
    let segs = location::segments(parent, home);
    let local = segs.first().is_some_and(|s| s.url.starts_with("file://"));
    let mut out = String::new();
    for (i, s) in segs.iter().enumerate() {
        match (i, s.label.as_str()) {
            (0, "Home") if local => out.push('~'),
            (0, "Root") if local => {}
            (0, label) => out.push_str(label),
            (_, label) => {
                out.push('/');
                out.push_str(label);
            }
        }
    }
    if out.is_empty() { "/".into() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const MIDNIGHT: i64 = NOW - 5 * 3600;

    #[test]
    fn kinds_map_to_the_indexs_categories() {
        assert!(Kind::Any.categories().is_empty());
        assert_eq!(Kind::Image.categories(), ["image"]);
        assert_eq!(Kind::Folder.categories(), ["folder"]);
        // A Document is more than the "document" category.
        let d = Kind::Document.categories();
        for c in ["document", "spreadsheet", "presentation", "pdf", "text"] {
            assert!(d.contains(&c), "{c}");
        }
        // Every name is one the service knows.
        for k in 1..=7 {
            for c in Kind::from_u32(k).categories() {
                assert!(
                    [
                        "folder",
                        "document",
                        "spreadsheet",
                        "presentation",
                        "pdf",
                        "image",
                        "audio",
                        "video",
                        "archive",
                        "code",
                        "text"
                    ]
                    .contains(c),
                    "{c}"
                );
            }
        }
        assert_eq!(Kind::from_u32(99), Kind::Any);
    }

    #[test]
    fn modified_periods() {
        let f = |m| filter(Kind::Any, m, SizeClass::Any, NOW, MIDNIGHT).modified_after;
        assert_eq!(f(Modified::Any), None);
        assert_eq!(f(Modified::Today), Some(MIDNIGHT));
        assert_eq!(f(Modified::Week), Some(NOW - 7 * 86_400));
        assert_eq!(f(Modified::Month), Some(NOW - 30 * 86_400));
        assert_eq!(f(Modified::Year), Some(NOW - 365 * 86_400));
        // A clock that is behind the local midnight never asks for the future.
        assert_eq!(
            filter(Kind::Any, Modified::Today, SizeClass::Any, NOW, NOW + 10).modified_after,
            Some(NOW)
        );
    }

    #[test]
    fn size_classes_do_not_overlap_or_leave_gaps() {
        let r = |s| {
            let f = filter(Kind::Any, Modified::Any, s, NOW, MIDNIGHT);
            (f.size_min.unwrap_or(0), f.size_max.unwrap_or(u64::MAX))
        };
        let (a, b, c) = (
            r(SizeClass::Small),
            r(SizeClass::Medium),
            r(SizeClass::Large),
        );
        assert_eq!(a.0, 0);
        assert_eq!(a.1 + 1, b.0);
        assert_eq!(b.1 + 1, c.0);
        assert_eq!(c.1, u64::MAX);
        assert_eq!(r(SizeClass::Any), (0, u64::MAX));
    }

    #[test]
    fn a_size_leaves_folders_out() {
        let f = filter(Kind::Any, Modified::Any, SizeClass::Large, NOW, MIDNIGHT);
        assert!(f.files_only && f.is_set());
        let none = filter(Kind::Any, Modified::Any, SizeClass::Any, NOW, MIDNIGHT);
        assert!(!none.files_only && !none.is_set());
        assert_eq!(none, Filter::default());
    }

    #[test]
    fn chips_combine() {
        let f = filter(
            Kind::Image,
            Modified::Week,
            SizeClass::Medium,
            NOW,
            MIDNIGHT,
        );
        assert_eq!(f.kinds, ["image"]);
        assert_eq!(f.modified_after, Some(NOW - 7 * 86_400));
        assert_eq!(f.size_min, Some(SMALL_LIMIT));
        assert_eq!(f.size_max, Some(LARGE_LIMIT - 1));
    }

    #[test]
    fn routes() {
        use Route::*;
        use Scope::*;
        // This Folder: the index when it covers the folder, else a walk.
        assert_eq!(route(ThisFolder, true, true, true), IndexFolder);
        assert_eq!(route(ThisFolder, true, false, true), LiveFolder);
        assert_eq!(route(ThisFolder, false, false, true), LiveRemote);
        // A local folder the index is off for is walked.
        assert_eq!(route(ThisFolder, true, true, false), LiveFolder);
        // Everywhere is the index, or the home folder walked without one.
        assert_eq!(route(Everywhere, true, false, true), IndexEverywhere);
        assert_eq!(route(Everywhere, false, false, true), IndexEverywhere);
        assert_eq!(route(Everywhere, true, true, false), LiveHome);
    }

    #[test]
    fn patterns_and_content_always_walk() {
        use Route::*;
        use Scope::*;
        assert_eq!(route_live(ThisFolder, true), LiveFolder);
        assert_eq!(route_live(ThisFolder, false), LiveRemote);
        assert_eq!(route_live(Everywhere, true), LiveHome);
        assert_eq!(route_live(Everywhere, false), LiveHome);
    }

    #[test]
    fn index_states_read_in_plain_words() {
        assert_eq!(IndexState::from_status("ready"), IndexState::Ready);
        assert_eq!(IndexState::from_status("scanning"), IndexState::Updating);
        assert_eq!(IndexState::from_status("stale"), IndexState::Updating);
        assert_eq!(IndexState::from_status("disabled"), IndexState::Off);
        assert_eq!(IndexState::from_status("error"), IndexState::Problem);
        assert_eq!(IndexState::from_status("what"), IndexState::Unknown);
        assert!(!IndexState::Off.is_on());
        assert!(IndexState::Updating.is_on() && IndexState::Unknown.is_on());
        assert_eq!(
            IndexState::Updating.chip(""),
            (
                2,
                "Updating the search index, results may be missing".to_string()
            )
        );
        assert_eq!(IndexState::Unavailable.chip("").1, "Search isn't available");
        assert_eq!(IndexState::Unavailable.chip("").0, 3);
        assert_eq!(IndexState::Unknown.chip("").0, 0);
        // The service's reason is shown, made safe.
        let (_, t) = IndexState::Problem.chip("cannot read\u{7} /home");
        assert!(
            t.contains("cannot read") && !t.chars().any(char::is_control),
            "{t}"
        );
        assert!(IndexState::Problem.chip("").1.contains("a problem"));
    }

    #[test]
    fn counts_and_walk_lines() {
        assert_eq!(count_text(0, false), "No results");
        assert_eq!(count_text(1, false), "1 result");
        assert_eq!(count_text(42, false), "42 results");
        assert!(count_text(500, true).contains("first 500"));
        assert_eq!(
            live_text(0, true, false, false),
            "Searching, nothing found yet"
        );
        assert_eq!(live_text(3, true, false, false), "Searching, 3 found");
        assert_eq!(live_text(3, false, true, false), "Stopped, 3 found");
        assert_eq!(live_text(0, false, false, false), "No results");
        assert_eq!(live_text(7, false, false, false), "7 results");
        assert!(live_text(5000, false, false, true).starts_with("Stopped at 5000"));
    }

    #[test]
    fn the_path_column() {
        let home = "/home/u";
        assert_eq!(path_text("file:///home/u", home), "~");
        assert_eq!(
            path_text("file:///home/u/Documents/a%20b", home),
            "~/Documents/a b"
        );
        assert_eq!(
            path_text("file:///media/STICK/DCIM", home),
            "/media/STICK/DCIM"
        );
        assert_eq!(path_text("file:///", home), "/");
        assert_eq!(path_text("trash:/trashed-dir", home), "Trash/trashed-dir");
        assert_eq!(path_text("sftp://me@host/srv/x", home), "host/srv/x");
        // A name with a control character is made visible, not obeyed.
        let t = path_text("file:///home/u/a%0Ab", home);
        assert!(!t.contains('\n'), "{t:?}");
        assert_eq!(path_text("", home), "/");
    }
}
