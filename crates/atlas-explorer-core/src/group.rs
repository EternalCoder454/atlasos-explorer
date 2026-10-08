//! Grouping a folder's rows ("Group by"): which group a row is in, what the
//! group is called, and in what order the groups come. Qt-free; the model
//! asks for it once per row on the sort worker (`telamon_group_of`).
//!
//! A group is told by its **order key**, bytes whose plain order is the order
//! of the groups when the sort is ascending; rows with equal keys are one
//! group. The **label** is what the header says.

use crate::sort::Column;

/// What the rows are grouped by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GroupBy {
    #[default]
    None = 0,
    /// The first letter of the name (digits together, the rest as "#").
    Name = 1,
    /// The kind shown in the Type column.
    Type = 2,
    /// How long ago the file was modified (Today, Yesterday, Earlier This Week, ...).
    Modified = 3,
}

impl GroupBy {
    pub fn from_code(code: u32) -> Option<GroupBy> {
        match code {
            0 => Some(GroupBy::None),
            1 => Some(GroupBy::Name),
            2 => Some(GroupBy::Type),
            3 => Some(GroupBy::Modified),
            _ => None,
        }
    }
}

/// How long ago, newest first: a group's place in the dates, which is also its order key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Age {
    Future,
    Today,
    Yesterday,
    EarlierThisWeek,
    LastWeek,
    EarlierThisMonth,
    LastMonth,
    EarlierThisYear,
    LongAgo,
    Unknown,
}

impl Age {
    pub fn label(self) -> &'static str {
        match self {
            Age::Future => "Later",
            Age::Today => "Today",
            Age::Yesterday => "Yesterday",
            Age::EarlierThisWeek => "Earlier This Week",
            Age::LastWeek => "Last Week",
            Age::EarlierThisMonth => "Earlier This Month",
            Age::LastMonth => "Last Month",
            Age::EarlierThisYear => "Earlier This Year",
            Age::LongAgo => "A Long Time Ago",
            Age::Unknown => "Unknown",
        }
    }
}

/// Days since 1970-01-01 of a Unix time in a zone `tz` seconds east of UTC.
fn day_of(secs: i64, tz: i64) -> i64 {
    (secs.saturating_add(tz)).div_euclid(86_400)
}

/// (year, month 1-12) of a day count (Howard Hinnant's civil_from_days).
fn year_month(day: i64) -> (i64, i64) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m)
}

/// How old a file modified at `mtime` is on the day of `now`, in a zone `tz`
/// seconds east of UTC. `week_start` is the first day of the week, 0 for
/// Monday to 6 for Sunday. A file with no date (`mtime` 0 or less) is Unknown.
pub fn age(mtime: i64, now: i64, tz: i64, week_start: u32) -> Age {
    if mtime <= 0 {
        return Age::Unknown;
    }
    let (d, today) = (day_of(mtime, tz), day_of(now, tz));
    if d > today {
        return Age::Future;
    }
    if d == today {
        return Age::Today;
    }
    if d == today - 1 {
        return Age::Yesterday;
    }
    // 1970-01-01 was a Thursday: day 0 has weekday 3 (Monday is 0).
    let weekday = (today + 3).rem_euclid(7);
    let since_week_start = (weekday - i64::from(week_start % 7)).rem_euclid(7);
    let week_begin = today - since_week_start;
    if d >= week_begin {
        return Age::EarlierThisWeek;
    }
    if d >= week_begin - 7 {
        return Age::LastWeek;
    }
    let (ty, tm) = year_month(today);
    let (dy, dm) = year_month(d);
    if (dy, dm) == (ty, tm) {
        return Age::EarlierThisMonth;
    }
    let (ly, lm) = if tm == 1 { (ty - 1, 12) } else { (ty, tm - 1) };
    if (dy, dm) == (ly, lm) {
        return Age::LastMonth;
    }
    if dy == ty {
        return Age::EarlierThisYear;
    }
    Age::LongAgo
}

/// What the core needs of one row to group it.
pub struct GroupInput<'a> {
    /// From `sort::name_key`.
    pub key: &'a [u8],
    pub is_dir: bool,
    /// The kind shown in the Type column.
    pub kind: &'a str,
    pub mtime: i64,
}

/// When "now" is, for the date groups.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub now: i64,
    /// Seconds east of UTC.
    pub tz: i64,
    /// 0 Monday to 6 Sunday.
    pub week_start: u32,
}

/// The group of a row: (order key, label). Rows of one group have the same
/// order key.
pub fn group_of(by: GroupBy, row: &GroupInput, clock: Clock) -> (Vec<u8>, String) {
    match by {
        GroupBy::None => (Vec::new(), String::new()),
        GroupBy::Name => name_group(row.key),
        GroupBy::Type => {
            let label = if row.kind.is_empty() {
                "Other"
            } else {
                row.kind
            };
            // Folders first, then the kinds alphabetically (ASCII case folded, as the Type column).
            let mut order = vec![if row.is_dir { 1 } else { 2 }];
            order.extend(label.bytes().map(|c| c.to_ascii_lowercase()));
            (order, label.to_string())
        }
        GroupBy::Modified => {
            let a = age(row.mtime, clock.now, clock.tz, clock.week_start);
            (vec![a as u8 + 1], a.label().to_string())
        }
    }
}

/// The first letter of a name, from its sort key (which starts with the
/// name folded to lower case; a run of digits is 0x01 and a count).
fn name_group(key: &[u8]) -> (Vec<u8>, String) {
    let first = match key.first() {
        Some(&b) => b,
        None => return (vec![3], "#".to_string()),
    };
    if first == 0x01 {
        return (vec![1], "0\u{2013}9".to_string());
    }
    // The key ends the name at a 0x00; a lossy name can hold U+FFFD, no letter.
    let end = key.iter().position(|&b| b == 0).unwrap_or(key.len());
    let text = String::from_utf8_lossy(&key[..end]);
    match text.chars().next() {
        Some(c) if c.is_alphabetic() => {
            let upper: String = c.to_uppercase().collect();
            let mut order = vec![2];
            order.extend_from_slice(upper.as_bytes());
            (order, upper)
        }
        _ => (vec![3], "#".to_string()),
    }
}

/// Whether the groups come in the opposite order of the order keys. The
/// groups follow the sort's direction only when the rows are sorted by what
/// they are grouped by (Name by Name, Type by Type, dates by Modified); in
/// any other sort, names and kinds run A to Z and dates run newest first.
pub fn reversed(by: GroupBy, column: Column, descending: bool) -> bool {
    match by {
        GroupBy::None => false,
        GroupBy::Name => column == Column::Name && descending,
        GroupBy::Type => column == Column::Type && descending,
        // The order key is the age, newest first; an ascending date sort wants the oldest first.
        GroupBy::Modified => column == Column::Modified && !descending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sort::name_key;

    // 2026-10-07 (a Wednesday), 15:00 UTC.
    const NOW: i64 = 1_791_385_200;
    const DAY: i64 = 86_400;
    const CLOCK: Clock = Clock {
        now: NOW,
        tz: 0,
        week_start: 0,
    };

    fn ago(days: i64) -> i64 {
        NOW - days * DAY
    }

    #[test]
    fn the_fixture_day_is_a_wednesday() {
        // Monday is 0, so Wednesday is 2.
        assert_eq!((day_of(NOW, 0) + 3).rem_euclid(7), 2);
        assert_eq!(year_month(day_of(NOW, 0)), (2026, 10));
    }

    #[test]
    fn ages_by_calendar_day() {
        let a = |d: i64| age(ago(d), NOW, 0, 0);
        assert_eq!(a(0), Age::Today);
        assert_eq!(a(1), Age::Yesterday);
        // Wednesday: Monday (2 days ago) is this week, the one before is not.
        assert_eq!(a(2), Age::EarlierThisWeek);
        assert_eq!(a(3), Age::LastWeek);
        assert_eq!(a(9), Age::LastWeek);
        // 2026-09-27 (a Sunday) is before last week, in September.
        assert_eq!(a(10), Age::LastMonth);
        // 2026-10-01 is last week even though it is still October.
        assert_eq!(a(6), Age::LastWeek);
        assert_eq!(a(14), Age::LastMonth);
        assert_eq!(a(40), Age::EarlierThisYear);
        assert_eq!(a(400), Age::LongAgo);
        assert_eq!(age(0, NOW, 0, 0), Age::Unknown);
        assert_eq!(age(-5, NOW, 0, 0), Age::Unknown);
        assert_eq!(age(NOW + 3 * DAY, NOW, 0, 0), Age::Future);
    }

    #[test]
    fn this_month_and_last_month_across_a_year() {
        // 2026-01-15 12:00 UTC
        let now = 1_768_478_400;
        assert_eq!(year_month(day_of(now, 0)), (2026, 1));
        // 2025-12-20 is last month, 2025-06-01 is a long time ago, 2026-01-02 earlier this month.
        let at = |y: i64, m: i64, d: i64| {
            // days from civil, inverse of year_month for the test
            let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
            let era = y2.div_euclid(400);
            let yoe = y2.rem_euclid(400);
            let doy = (153 * m2 + 2) / 5 + d - 1;
            let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
            (era * 146_097 + doe - 719_468) * DAY + 12 * 3600
        };
        assert_eq!(age(at(2025, 12, 20), now, 0, 0), Age::LastMonth);
        assert_eq!(age(at(2025, 6, 1), now, 0, 0), Age::LongAgo);
        assert_eq!(age(at(2026, 1, 2), now, 0, 0), Age::EarlierThisMonth);
    }

    #[test]
    fn the_zone_moves_midnight() {
        // 23:30 UTC the day before is "Today" for a zone 2 hours east.
        let now = day_of(NOW, 0) * DAY + 30 * 60;
        let before = now - 3600;
        assert_eq!(age(before, now, 0, 0), Age::Yesterday);
        assert_eq!(age(before, now, 7200, 0), Age::Today);
    }

    #[test]
    fn the_week_starts_where_asked() {
        // Sunday 2026-10-04 is 3 days before Wednesday: last week for Monday starts,
        // this week for Sunday starts.
        assert_eq!(age(ago(3), NOW, 0, 0), Age::LastWeek);
        assert_eq!(age(ago(3), NOW, 0, 6), Age::EarlierThisWeek);
    }

    fn input<'a>(key: &'a [u8], kind: &'a str, is_dir: bool, mtime: i64) -> GroupInput<'a> {
        GroupInput {
            key,
            is_dir,
            kind,
            mtime,
        }
    }

    #[test]
    fn names_group_by_first_letter() {
        let g = |n: &str| {
            let key = name_key(n.as_bytes());
            group_of(GroupBy::Name, &input(&key, "", false, 0), CLOCK)
        };
        assert_eq!(g("apple").1, "A");
        assert_eq!(g("Avocado").1, "A");
        assert_eq!(g("apple").0, g("Avocado").0);
        assert_eq!(g("zebra").1, "Z");
        assert_eq!(g("\u{e9}clair").1, "\u{c9}");
        assert_eq!(g("2024 notes").1, "0\u{2013}9");
        assert_eq!(g("2024 notes").0, g("7zip").0);
        assert_eq!(g(".config").1, "#");
        assert_eq!(g("_x").1, "#");
        assert_eq!(g("").1, "#");
        // digits, then letters, then the rest
        assert!(g("1").0 < g("a").0);
        assert!(g("a").0 < g("b").0);
        assert!(g("z").0 < g("_").0);
    }

    #[test]
    fn types_group_by_kind_folders_first() {
        let g =
            |kind: &str, dir: bool| group_of(GroupBy::Type, &input(b"x\0x", kind, dir, 0), CLOCK);
        assert!(g("Folder", true).0 < g("Audio file", false).0);
        assert!(g("Audio file", false).0 < g("PNG image", false).0);
        assert_eq!(g("PNG image", false).0, g("png Image", false).0);
        assert_eq!(g("PNG image", false).1, "PNG image");
        assert_eq!(g("", false).1, "Other");
    }

    #[test]
    fn dates_group_newest_first() {
        let g = |d: i64| group_of(GroupBy::Modified, &input(b"x\0x", "", false, ago(d)), CLOCK);
        assert_eq!(g(0).1, "Today");
        assert!(g(0).0 < g(1).0);
        assert!(g(1).0 < g(2).0);
        assert!(g(14).0 < g(400).0);
        assert_eq!(g(400).1, "A Long Time Ago");
        let unknown = group_of(GroupBy::Modified, &input(b"x\0x", "", false, 0), CLOCK);
        assert!(g(400).0 < unknown.0);
    }

    #[test]
    fn none_has_no_groups() {
        let (order, label) = group_of(GroupBy::None, &input(b"a\0a", "x", false, 5), CLOCK);
        assert!(order.is_empty() && label.is_empty());
    }

    #[test]
    fn the_group_order_follows_the_sort_only_for_its_own_column() {
        use Column::*;
        for desc in [false, true] {
            assert!(!reversed(GroupBy::None, Name, desc));
            assert_eq!(reversed(GroupBy::Name, Name, desc), desc);
            assert!(!reversed(GroupBy::Name, Size, desc));
            assert_eq!(reversed(GroupBy::Type, Type, desc), desc);
            assert!(!reversed(GroupBy::Type, Name, desc));
            // Modified: newest first unless the dates are sorted ascending.
            assert_eq!(reversed(GroupBy::Modified, Modified, desc), !desc);
            assert!(!reversed(GroupBy::Modified, Name, desc));
        }
    }

    #[test]
    fn codes_round_trip() {
        for c in 0..4 {
            assert_eq!(GroupBy::from_code(c).unwrap() as u32, c);
        }
        assert!(GroupBy::from_code(4).is_none());
    }
}
