//! What the operations say, in plain words: the title an undo names ("Move 3
//! Items to Backup"), the line shown while it runs ("Moving 3 items to
//! Backup"), sizes, speeds and times. Names come in already made safe to show
//! (`display_name`); this only shortens them.

use crate::queue::Kind;
use std::time::Duration;

/// Longest name shown inside a sentence, in characters.
const MAX_NAME_CHARS: usize = 40;

/// The name cut to `MAX_NAME_CHARS` characters, in the middle.
pub fn short_name(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= MAX_NAME_CHARS {
        return name.to_string();
    }
    let keep = MAX_NAME_CHARS - 1;
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('\u{2026}');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// What an operation is about: `names` are the display names of the items
/// (the first is named when there is one), `count` how many there are in
/// all, `to` the folder they go to (the Trash, a destination), `new_name` the
/// name a rename gives.
pub struct About<'a> {
    pub kind: Kind,
    pub names: &'a [String],
    pub count: usize,
    pub to: Option<&'a str>,
    pub new_name: Option<&'a str>,
}

fn items(a: &About, title_case: bool) -> String {
    let n = a.count.max(a.names.len());
    if n == 1
        && let Some(first) = a.names.first()
    {
        return short_name(first);
    }
    if title_case {
        format!("{n} Items")
    } else {
        format!("{n} items")
    }
}

/// The title of a finished operation, as Undo and Redo name it: "Copy a.txt
/// to Backup", "Move 3 Items to Trash", "Rename a.txt to b.txt".
pub fn title(a: &About) -> String {
    let what = items(a, true);
    let to = a.to.map(short_name);
    match (a.kind, to) {
        (Kind::Copy, Some(to)) => format!("Copy {what} to {to}"),
        (Kind::Move, Some(to)) => format!("Move {what} to {to}"),
        (Kind::Link, Some(to)) => format!("Link {what} in {to}"),
        (Kind::Trash, _) => format!("Move {what} to Trash"),
        (Kind::Restore, _) => format!("Restore {what} from Trash"),
        (Kind::Delete, _) => format!("Delete {what}"),
        (Kind::EmptyTrash, _) => "Empty Trash".to_string(),
        (Kind::Rename, _) => match a.new_name {
            Some(n) => format!("Rename {what} to {}", short_name(n)),
            None => format!("Rename {what}"),
        },
        (Kind::NewFolder, _) => format!("Create Folder {what}"),
        (Kind::Copy, None) => format!("Copy {what}"),
        (Kind::Move, None) => format!("Move {what}"),
        (Kind::Link, None) => format!("Link {what}"),
        (Kind::External, _) => what,
        (Kind::Attrs, _) => format!("Change {what}"),
    }
}

/// The line of a running operation: "Copying a.txt to Backup".
pub fn running(a: &About) -> String {
    let what = items(a, false);
    let to = a.to.map(short_name);
    match (a.kind, to) {
        (Kind::Copy, Some(to)) => format!("Copying {what} to {to}"),
        (Kind::Move, Some(to)) => format!("Moving {what} to {to}"),
        (Kind::Link, Some(to)) => format!("Linking {what} in {to}"),
        (Kind::Trash, _) => format!("Moving {what} to the Trash"),
        (Kind::Restore, _) => format!("Restoring {what} from the Trash"),
        (Kind::Delete, _) => format!("Deleting {what}"),
        (Kind::EmptyTrash, _) => "Emptying the Trash".to_string(),
        (Kind::Rename, _) => match a.new_name {
            Some(n) => format!("Renaming {what} to {}", short_name(n)),
            None => format!("Renaming {what}"),
        },
        (Kind::NewFolder, _) => format!("Creating folder {what}"),
        (Kind::Copy, None) => format!("Copying {what}"),
        (Kind::Move, None) => format!("Moving {what}"),
        (Kind::Link, None) => format!("Linking {what}"),
        (Kind::External, _) => what,
        (Kind::Attrs, _) => format!("Changing {what}"),
    }
}

const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

/// A size in binary units with one decimal where it helps: "512 B",
/// "4.2 GiB", "12 MiB".
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    // 1023.96 KiB rounds up to 1024.0: the next unit says it better.
    if (value * 10.0).round() >= 10240.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 || (value - value.round()).abs() < 0.05 {
        format!("{} {}", value.round() as u64, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// "35 s", "2 min", "1 h 5 min": rounded for reading, never "0 s".
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs_f64().ceil().max(1.0) as u64;
    if secs < 60 {
        format!("{secs} s")
    } else if secs < 3600 {
        let m = secs.div_ceil(60);
        if m >= 60 {
            "1 h".to_string()
        } else {
            format!("{m} min")
        }
    } else {
        let h = secs / 3600;
        let m = (secs % 3600).div_ceil(60);
        match m {
            0 => format!("{h} h"),
            60 => format!("{} h", h + 1),
            _ => format!("{h} h {m} min"),
        }
    }
}

/// What the numbers of a running operation say, "1.2 GiB of 4 GiB, 85 MiB/s,
/// 35 s left", or the item count when there are no bytes. Parts that aren't
/// known are left out.
pub fn progress_line(
    bytes_done: u64,
    bytes_total: u64,
    items_done: u64,
    items_total: u64,
    speed: Option<f64>,
    time_left: Option<Duration>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if bytes_total > 0 {
        parts.push(format!(
            "{} of {}",
            format_size(bytes_done.min(bytes_total)),
            format_size(bytes_total)
        ));
    } else if items_total > 1 {
        parts.push(format!(
            "{} of {} items",
            items_done.min(items_total),
            items_total
        ));
    }
    if let Some(s) = speed
        && s > 0.0
    {
        parts.push(format!("{}/s", format_size(s as u64)));
    }
    if let Some(t) = time_left {
        parts.push(format!("{} left", format_duration(t)));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn about<'a>(kind: Kind, names: &'a [String], count: usize, to: Option<&'a str>) -> About<'a> {
        About {
            kind,
            names,
            count,
            to,
            new_name: None,
        }
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn titles_name_what_an_undo_does() {
        let one = names(&["report.txt"]);
        let three = names(&["a", "b", "c"]);
        assert_eq!(
            title(&about(Kind::Move, &three, 3, Some("Backup"))),
            "Move 3 Items to Backup"
        );
        assert_eq!(
            title(&about(Kind::Copy, &one, 1, Some("Backup"))),
            "Copy report.txt to Backup"
        );
        assert_eq!(
            title(&about(Kind::Trash, &three, 3, None)),
            "Move 3 Items to Trash"
        );
        assert_eq!(
            title(&about(Kind::Link, &one, 1, Some("Docs"))),
            "Link report.txt in Docs"
        );
        assert_eq!(
            title(&about(Kind::Delete, &three, 3, None)),
            "Delete 3 Items"
        );
        let mut r = about(Kind::Rename, &one, 1, None);
        r.new_name = Some("final.txt");
        assert_eq!(title(&r), "Rename report.txt to final.txt");
        let f = names(&["New Folder"]);
        assert_eq!(
            title(&about(Kind::NewFolder, &f, 1, None)),
            "Create Folder New Folder"
        );
        // More items than names (only the first few are passed in).
        assert_eq!(
            title(&about(Kind::Move, &one, 1204, Some("Backup"))),
            "Move 1204 Items to Backup"
        );
    }

    #[test]
    fn running_lines_are_in_the_present() {
        let three = names(&["a", "b", "c"]);
        assert_eq!(
            running(&about(Kind::Copy, &three, 3, Some("Backup"))),
            "Copying 3 items to Backup"
        );
        assert_eq!(
            running(&about(Kind::Trash, &names(&["x"]), 1, None)),
            "Moving x to the Trash"
        );
        assert_eq!(
            running(&about(Kind::Delete, &three, 3, None)),
            "Deleting 3 items"
        );
    }

    #[test]
    fn long_names_are_cut_in_the_middle() {
        assert_eq!(short_name("short.txt"), "short.txt");
        let long = format!("{}.txt", "x".repeat(100));
        let s = short_name(&long);
        assert_eq!(s.chars().count(), MAX_NAME_CHARS);
        assert!(s.contains('\u{2026}'));
        assert!(s.ends_with(".txt"));
        assert_eq!(short_name(&"é".repeat(60)).chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn sizes_read_like_the_size_column() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(
            format_size(1024 * 1024 * 1024 * 4 + 1024 * 1024 * 200),
            "4.2 GiB"
        );
        assert_eq!(format_size(1024 * 1024 * 100), "100 MiB");
        assert_eq!(format_size(1024 * 1024 - 1), "1 MiB");
        assert_eq!(format_size(u64::MAX), "16384 PiB");
    }

    #[test]
    fn durations_round_up_and_stay_short() {
        let s = Duration::from_secs;
        assert_eq!(format_duration(Duration::from_millis(10)), "1 s");
        assert_eq!(format_duration(s(35)), "35 s");
        assert_eq!(format_duration(s(60)), "1 min");
        assert_eq!(format_duration(s(61)), "2 min");
        assert_eq!(format_duration(s(3599)), "1 h");
        assert_eq!(format_duration(s(3600)), "1 h");
        assert_eq!(format_duration(s(3600 + 65)), "1 h 2 min");
        assert_eq!(format_duration(s(7200 - 5)), "2 h");
    }

    #[test]
    fn progress_lines_leave_out_what_is_unknown() {
        assert_eq!(
            progress_line(
                1_288_490_189,
                4_294_967_296,
                0,
                0,
                Some(89_128_960.0),
                Some(Duration::from_secs(35))
            ),
            "1.2 GiB of 4 GiB, 85 MiB/s, 35 s left"
        );
        assert_eq!(progress_line(0, 0, 3, 10, None, None), "3 of 10 items");
        assert_eq!(progress_line(0, 0, 0, 1, None, None), "");
        assert_eq!(progress_line(10, 5, 0, 0, Some(0.0), None), "5 B of 5 B");
    }
}
