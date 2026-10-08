//! What each folder remembers of how it is shown: the view, the sort, the
//! icon size and the grouping. Kept by the app in its own settings file
//! (`telamon-explorerrc`), never in the folder (no `.directory`, no
//! `.DS_Store`), as a bounded list of the most recently changed folders.
//!
//! The list is text, one folder per line, so the settings file stays readable
//! and the app has nothing to parse: [`FolderViews::parse`] takes what was
//! saved (anything can be in a file; bad lines are dropped and numbers are
//! brought into their limits) and [`FolderViews::to_text`] writes it back.

use crate::group::GroupBy;
use crate::zoom;

/// How many folders are remembered; the least recently changed one goes first.
pub const MAX_FOLDERS: usize = 500;
/// The longest folder key kept, in bytes (a longer location is not remembered).
pub const MAX_KEY: usize = 2048;

/// How the folder is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Details = 0,
    Icons = 1,
    Compact = 2,
    Columns = 3,
    Gallery = 4,
}

impl Mode {
    pub fn from_code(code: u32) -> Option<Mode> {
        match code {
            0 => Some(Mode::Details),
            1 => Some(Mode::Icons),
            2 => Some(Mode::Compact),
            3 => Some(Mode::Columns),
            4 => Some(Mode::Gallery),
            _ => None,
        }
    }

    /// The name the window uses (`FolderView.viewMode`).
    pub fn name(self) -> &'static str {
        match self {
            Mode::Details => "details",
            Mode::Icons => "icons",
            Mode::Compact => "compact",
            Mode::Columns => "columns",
            Mode::Gallery => "gallery",
        }
    }

    pub fn from_name(name: &str) -> Option<Mode> {
        [
            Mode::Details,
            Mode::Icons,
            Mode::Compact,
            Mode::Columns,
            Mode::Gallery,
        ]
        .into_iter()
        .find(|m| m.name() == name)
    }
}

/// The sort columns a folder can remember (the core's `sort::Column` codes:
/// Name, Size, Type, Modified, Created, Accessed). Search relevance is never kept.
pub const SORT_COLUMNS: u32 = 6;

/// One folder's way of being shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewPrefs {
    pub mode: Mode,
    pub sort: u32,
    pub descending: bool,
    /// Pixels, within the zoom limits.
    pub icon: i32,
    pub group: GroupBy,
}

impl Default for ViewPrefs {
    fn default() -> Self {
        ViewPrefs {
            mode: Mode::Details,
            sort: 0,
            descending: false,
            icon: zoom::ICON_DEFAULT,
            group: GroupBy::None,
        }
    }
}

impl ViewPrefs {
    /// The same prefs brought into their limits (a sort column that does not
    /// exist is Name; an icon size is clamped).
    pub fn sanitized(self) -> ViewPrefs {
        ViewPrefs {
            sort: if self.sort < SORT_COLUMNS {
                self.sort
            } else {
                0
            },
            icon: zoom::clamp(zoom::Kind::Icon, self.icon),
            ..self
        }
    }
}

/// Whether `key` can be a folder's key: a location written as text, on one
/// line, not too long.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= MAX_KEY && !key.chars().any(|c| c.is_control())
}

/// The remembered folders, the most recently changed first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderViews {
    entries: Vec<(String, ViewPrefs)>,
}

impl FolderViews {
    /// Reads a saved list. Lines that do not parse are dropped, a folder
    /// listed twice keeps its first (newest) line, and only [`MAX_FOLDERS`]
    /// are kept.
    pub fn parse(text: &str) -> FolderViews {
        let mut entries: Vec<(String, ViewPrefs)> = Vec::new();
        for line in text.lines() {
            if entries.len() >= MAX_FOLDERS {
                break;
            }
            if let Some((key, prefs)) = parse_line(line)
                && !entries.iter().any(|(k, _)| *k == key)
            {
                entries.push((key, prefs));
            }
        }
        FolderViews { entries }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (key, p) in &self.entries {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                p.mode as u32,
                p.sort,
                u32::from(p.descending),
                p.icon,
                p.group as u32,
                key
            ));
        }
        out
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<ViewPrefs> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, p)| *p)
    }

    /// Remembers `prefs` for `key` as the most recent change. A key that
    /// cannot be one is refused (false).
    pub fn set(&mut self, key: &str, prefs: ViewPrefs) -> bool {
        if !valid_key(key) {
            return false;
        }
        self.entries.retain(|(k, _)| k != key);
        self.entries.insert(0, (key.to_string(), prefs.sanitized()));
        self.entries.truncate(MAX_FOLDERS);
        true
    }

    /// Forgets a folder; whether it was remembered.
    pub fn forget(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(k, _)| k != key);
        self.entries.len() != before
    }
}

fn parse_line(line: &str) -> Option<(String, ViewPrefs)> {
    let mut parts = line.splitn(6, '\t');
    let mode = Mode::from_code(parts.next()?.parse().ok()?)?;
    let sort: u32 = parts.next()?.parse().ok()?;
    let descending = match parts.next()? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let icon: i32 = parts.next()?.parse().ok()?;
    let group = GroupBy::from_code(parts.next()?.parse().ok()?)?;
    let key = parts.next()?;
    if sort >= SORT_COLUMNS || !valid_key(key) {
        return None;
    }
    Some((
        key.to_string(),
        ViewPrefs {
            mode,
            sort,
            descending,
            icon,
            group,
        }
        .sanitized(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs(mode: Mode, sort: u32) -> ViewPrefs {
        ViewPrefs {
            mode,
            sort,
            ..ViewPrefs::default()
        }
    }

    #[test]
    fn a_folder_remembers_what_was_set() {
        let mut v = FolderViews::default();
        assert!(v.get("file:///home/a").is_none());
        let p = ViewPrefs {
            mode: Mode::Gallery,
            sort: 3,
            descending: true,
            icon: 128,
            group: GroupBy::Modified,
        };
        assert!(v.set("file:///home/a", p));
        assert_eq!(v.get("file:///home/a"), Some(p));
        assert!(v.get("file:///home/b").is_none());
    }

    #[test]
    fn text_round_trips() {
        let mut v = FolderViews::default();
        v.set("file:///a", prefs(Mode::Columns, 1));
        v.set(
            "file:///b%20c",
            ViewPrefs {
                descending: true,
                group: GroupBy::Type,
                icon: 160,
                ..prefs(Mode::Icons, 2)
            },
        );
        let again = FolderViews::parse(&v.to_text());
        assert_eq!(again, v);
        // the newest comes first in the text
        assert!(v.to_text().starts_with("1\t2\t1\t160\t2\tfile:///b%20c\n"));
    }

    #[test]
    fn the_list_is_bounded_and_forgets_the_oldest() {
        let mut v = FolderViews::default();
        for i in 0..(MAX_FOLDERS + 40) {
            assert!(v.set(&format!("file:///d/{i}"), prefs(Mode::Icons, 0)));
        }
        assert_eq!(v.len(), MAX_FOLDERS);
        assert!(v.get("file:///d/0").is_none());
        assert!(v.get("file:///d/39").is_none());
        assert!(v.get("file:///d/40").is_some());
        assert!(v.get(&format!("file:///d/{}", MAX_FOLDERS + 39)).is_some());
    }

    #[test]
    fn changing_a_folder_makes_it_the_newest() {
        let mut v = FolderViews::default();
        for i in 0..MAX_FOLDERS {
            v.set(&format!("file:///d/{i}"), prefs(Mode::Icons, 0));
        }
        // The oldest is changed, so it stays when 10 new ones push others out.
        v.set("file:///d/0", prefs(Mode::Gallery, 0));
        for i in 0..10 {
            v.set(&format!("file:///new/{i}"), prefs(Mode::Details, 0));
        }
        assert_eq!(v.len(), MAX_FOLDERS);
        assert_eq!(v.get("file:///d/0").unwrap().mode, Mode::Gallery);
        assert!(v.get("file:///d/1").is_none());
        // and one folder is only listed once
        assert_eq!(v.to_text().matches("file:///d/0\n").count(), 1);
    }

    #[test]
    fn forgetting_a_folder() {
        let mut v = FolderViews::default();
        v.set("file:///a", prefs(Mode::Columns, 0));
        assert!(v.forget("file:///a"));
        assert!(!v.forget("file:///a"));
        assert!(v.get("file:///a").is_none());
        assert!(v.is_empty());
    }

    #[test]
    fn a_file_can_hold_anything() {
        let text = "\
0\t0\t0\t96\t0\tfile:///ok
nonsense
9\t0\t0\t96\t0\tfile:///bad-mode
1\t99\t0\t96\t0\tfile:///bad-sort
1\t0\t2\t96\t0\tfile:///bad-desc
1\t0\t0\t96\t7\tfile:///bad-group
1\t0\t0\tabc\t0\tfile:///bad-icon
1\t0\t0\t96\t0\t
1\t0\t0\t96\t0
3\t1\t1\t100000\t3\tfile:///huge-icon
4\t0\t0\t-5\t0\tfile:///tiny-icon
1\t0\t0\t96\t0\tfile:///ok
2\t5\t0\t96\t1\tfile:///dupe-wins-first-line
2\t5\t0\t96\t1\tfile:///ctl\u{7}
";
        let v = FolderViews::parse(text);
        assert_eq!(v.len(), 4, "{v:?}");
        // the first line of a folder counts
        assert_eq!(v.get("file:///ok").unwrap().mode, Mode::Details);
        let huge = v.get("file:///huge-icon").unwrap();
        assert_eq!(huge.icon, zoom::ICON_MAX);
        assert_eq!(
            (huge.mode, huge.sort, huge.descending, huge.group),
            (Mode::Columns, 1, true, GroupBy::Modified)
        );
        assert_eq!(v.get("file:///tiny-icon").unwrap().icon, zoom::ICON_MIN);
        assert!(v.get("file:///dupe-wins-first-line").is_some());
        // a huge file stops at the limit
        let mut big = String::new();
        for i in 0..(MAX_FOLDERS * 2) {
            big.push_str(&format!("0\t0\t0\t96\t0\tfile:///f/{i}\n"));
        }
        assert_eq!(FolderViews::parse(&big).len(), MAX_FOLDERS);
    }

    #[test]
    fn keys_that_cannot_be_kept_are_refused() {
        let mut v = FolderViews::default();
        assert!(!v.set("", prefs(Mode::Icons, 0)));
        assert!(!v.set("file:///a\nb", prefs(Mode::Icons, 0)));
        assert!(!v.set("file:///a\tb", prefs(Mode::Icons, 0)));
        assert!(!v.set(
            &format!("file:///{}", "x".repeat(MAX_KEY)),
            prefs(Mode::Icons, 0)
        ));
        assert!(v.is_empty());
        assert!(v.set(
            &format!("file:///{}", "x".repeat(MAX_KEY - 8)),
            prefs(Mode::Icons, 0)
        ));
    }

    #[test]
    fn set_brings_numbers_into_their_limits() {
        let mut v = FolderViews::default();
        v.set(
            "file:///a",
            ViewPrefs {
                icon: 5,
                sort: 77,
                ..ViewPrefs::default()
            },
        );
        let p = v.get("file:///a").unwrap();
        assert_eq!((p.icon, p.sort), (zoom::ICON_MIN, 0));
    }

    #[test]
    fn mode_names_round_trip() {
        for c in 0..5 {
            let m = Mode::from_code(c).unwrap();
            assert_eq!(Mode::from_name(m.name()), Some(m));
        }
        assert!(Mode::from_code(5).is_none());
        assert!(Mode::from_name("tiles").is_none());
    }
}
