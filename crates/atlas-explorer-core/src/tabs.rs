//! The tab strip's decisions, with no Qt: which tab is shown after one closes
//! or moves, where a new tab goes, what a saved session may contain. The
//! window keeps the tabs themselves (their folders, histories, views); this
//! module only does the index arithmetic and checks what comes back from the
//! settings file, so it is tested without a display. See docs/DESIGN.md,
//! "Window".

use crate::launch;
use std::path::Path;

/// Most tabs in one window. A launch or a saved session can't open more.
pub const MAX_TABS: usize = 64;
/// Closed tabs kept for "Reopen Closed Tab".
pub const MAX_CLOSED: usize = 10;
/// Entries of a tab's back or forward history kept when the tab is closed.
pub const MAX_HISTORY: usize = 50;

/// The tab to show after the tab at `closed` is removed from a strip of
/// `len` tabs whose current tab is `current`. The tab to the right of the
/// closed one takes its place (the left one when it was the last); closing
/// another tab keeps the current one. `None` when no tab is left, or an index
/// is out of range (the strip is not touched).
pub fn after_close(len: usize, current: usize, closed: usize) -> Option<usize> {
    if len <= 1 || current >= len || closed >= len {
        return None;
    }
    Some(if closed < current {
        current - 1
    } else if closed == current {
        current.min(len - 2)
    } else {
        current
    })
}

/// Where the current tab is after the tab at `from` is moved to `to` (both
/// are positions in a strip of `len` tabs); `current` unchanged when
/// anything is out of range.
pub fn after_move(len: usize, current: usize, from: usize, to: usize) -> usize {
    if from >= len || to >= len || current >= len {
        return current;
    }
    if current == from {
        to
    } else if from < current && current <= to {
        current - 1
    } else if to <= current && current < from {
        current + 1
    } else {
        current
    }
}

/// The tab `step` places from `current`, wrapping around at both ends. A
/// `current` out of range counts as the first tab.
pub fn cycle(len: usize, current: usize, step: i64) -> usize {
    if len == 0 {
        return 0;
    }
    let current = if current >= len { 0 } else { current };
    (current as i128 + step as i128).rem_euclid(len as i128) as usize
}

/// The tab for the shortcut Alt+`n` (1 to 9): the n-th tab, and 9 is always
/// the last one, as in browsers. `None` when there is no such tab.
pub fn jump(len: usize, n: usize) -> Option<usize> {
    match n {
        0 => None,
        9 if len > 0 => Some(len - 1),
        _ if n <= len => Some(n - 1),
        _ => None,
    }
}

/// Where a tab opened from the tab at `opener` goes. `run` is how many tabs
/// were already opened from it in a row, so that three middle-clicks leave
/// the tabs in the order they were clicked, right after the opener.
pub fn insert_after_opener(len: usize, opener: usize, run: usize) -> usize {
    opener.saturating_add(1).saturating_add(run).min(len)
}

/// Where a reopened tab goes: where it was, or the end when the strip has
/// become shorter.
pub fn reopen_index(len: usize, original: usize) -> usize {
    original.min(len)
}

/// Whether one more tab may be opened.
pub fn can_open(len: usize) -> bool {
    len < MAX_TABS
}

/// Tabs saved for the next start, checked: every location went through the
/// launch parser, so a settings file (or whatever wrote to it) can't point
/// Files at a kind of location a launch would be refused.
#[derive(Debug, PartialEq, Eq)]
pub struct Session {
    pub urls: Vec<String>,
    /// Index of the tab to show, within `urls`.
    pub current: usize,
}

/// Reads a saved session. Entries the launch parser refuses (a relative path,
/// an unknown scheme, control characters) are left out, at most `MAX_TABS`
/// are kept, and `current` follows its tab: when that tab itself was dropped,
/// the nearest kept tab after it (else before it) is shown. `None` when
/// nothing usable is left.
pub fn restore(saved: &[String], current: usize) -> Option<Session> {
    let mut urls = Vec::new();
    let mut shown = None;
    for (i, entry) in saved.iter().enumerate() {
        if urls.len() == MAX_TABS {
            break;
        }
        // `--` first: an entry is never read as an option.
        let args = ["--".to_string(), entry.clone()];
        let parsed = launch::parse(&args, Path::new(""));
        if !parsed.refused.is_empty() || parsed.locations.len() != 1 {
            continue;
        }
        if shown.is_none() && i >= current {
            shown = Some(urls.len());
        }
        urls.extend(parsed.locations);
    }
    if urls.is_empty() {
        return None;
    }
    let current = shown.unwrap_or(urls.len() - 1);
    Some(Session { urls, current })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_keeps_the_view_sensible() {
        // [a b c d], showing c (2).
        assert_eq!(after_close(4, 2, 0), Some(1), "a left of c: c moves left");
        assert_eq!(after_close(4, 2, 1), Some(1));
        assert_eq!(after_close(4, 2, 2), Some(2), "c closed: d takes its place");
        assert_eq!(after_close(4, 2, 3), Some(2), "d closed: c stays");
        // The last tab closed while shown: the one before it.
        assert_eq!(after_close(4, 3, 3), Some(2));
        assert_eq!(after_close(2, 1, 1), Some(0));
        assert_eq!(after_close(2, 0, 0), Some(0));
    }

    #[test]
    fn closing_the_only_tab_or_a_bad_index_changes_nothing() {
        assert_eq!(after_close(1, 0, 0), None);
        assert_eq!(after_close(0, 0, 0), None);
        assert_eq!(after_close(3, 0, 3), None);
        assert_eq!(after_close(3, 5, 0), None);
    }

    #[test]
    fn closing_tabs_one_by_one_always_leaves_a_valid_index() {
        for len in 2..8usize {
            for current in 0..len {
                for closed in 0..len {
                    let new = after_close(len, current, closed).unwrap();
                    assert!(new < len - 1, "{len} {current} {closed}");
                }
            }
        }
    }

    #[test]
    fn moving_keeps_the_same_tab_current() {
        // Simulate the strip as a Vec and check the current tab is the same one.
        for len in 1..7usize {
            for current in 0..len {
                for from in 0..len {
                    for to in 0..len {
                        let mut strip: Vec<usize> = (0..len).collect();
                        let tab = strip.remove(from);
                        strip.insert(to, tab);
                        let now = after_move(len, current, from, to);
                        assert_eq!(strip[now], current, "{len} {current} {from} {to}");
                    }
                }
            }
        }
        assert_eq!(after_move(3, 1, 9, 0), 1, "out of range changes nothing");
        assert_eq!(after_move(3, 9, 0, 1), 9);
    }

    #[test]
    fn cycling_wraps_both_ways() {
        assert_eq!(cycle(3, 2, 1), 0);
        assert_eq!(cycle(3, 0, -1), 2);
        assert_eq!(cycle(3, 1, 1), 2);
        assert_eq!(cycle(1, 0, 1), 0);
        assert_eq!(cycle(3, 0, -7), 2);
        assert_eq!(cycle(0, 0, 1), 0);
        assert_eq!(cycle(3, 9, 1), 1, "a bad current counts as the first");
        assert!(cycle(4, 1, i64::MIN) < 4);
        assert!(cycle(4, 1, i64::MAX) < 4);
    }

    #[test]
    fn jump_goes_to_the_nth_tab_and_nine_to_the_last() {
        assert_eq!(jump(5, 1), Some(0));
        assert_eq!(jump(5, 5), Some(4));
        assert_eq!(jump(5, 6), None);
        assert_eq!(jump(5, 9), Some(4));
        assert_eq!(jump(12, 9), Some(11));
        assert_eq!(jump(9, 9), Some(8));
        assert_eq!(jump(2, 3), None);
        assert_eq!(jump(0, 9), None);
        assert_eq!(jump(3, 0), None);
    }

    #[test]
    fn tabs_opened_from_a_tab_stay_in_click_order_beside_it() {
        // [a b c], a opens x, y, z in the background.
        assert_eq!(insert_after_opener(3, 0, 0), 1);
        assert_eq!(insert_after_opener(4, 0, 1), 2);
        assert_eq!(insert_after_opener(5, 0, 2), 3);
        // Never past the end.
        assert_eq!(insert_after_opener(3, 2, 0), 3);
        assert_eq!(insert_after_opener(3, 2, 9), 3);
        assert_eq!(insert_after_opener(3, usize::MAX, usize::MAX), 3);
    }

    #[test]
    fn a_reopened_tab_goes_back_where_it_was() {
        assert_eq!(reopen_index(5, 2), 2);
        assert_eq!(reopen_index(2, 6), 2);
        assert_eq!(reopen_index(0, 0), 0);
    }

    #[test]
    fn the_strip_has_a_limit() {
        assert!(can_open(MAX_TABS - 1));
        assert!(!can_open(MAX_TABS));
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_session_comes_back_through_the_launch_parser() {
        let r = restore(
            &s(&[
                "file:///home/u/docs",
                "smb://nas/share",
                "trash:/",
                "file:///tmp/a%20b",
            ]),
            2,
        )
        .unwrap();
        assert_eq!(r.urls.len(), 4);
        assert_eq!(r.urls[0], "file:///home/u/docs");
        assert_eq!(r.current, 2);
    }

    #[test]
    fn a_session_drops_what_a_launch_would_refuse() {
        let r = restore(
            &s(&[
                "http://example.com/",
                "relative/path",
                "file://evil/etc",
                "/tmp/a\u{202E}gpj",
                "--new-window",
                "",
                "file:///ok",
                "exec:/bin/sh",
            ]),
            0,
        )
        .unwrap();
        assert_eq!(r.urls, ["file:///ok"]);
        assert_eq!(r.current, 0);
    }

    #[test]
    fn a_session_with_nothing_usable_is_nothing() {
        assert_eq!(restore(&[], 0), None);
        assert_eq!(restore(&s(&["http://x/", "rel"]), 0), None);
    }

    #[test]
    fn the_shown_tab_follows_its_entry_when_others_are_dropped() {
        // [bad, a, bad, b, c], showing b (3): kept [a b c], b is 1.
        let r = restore(
            &s(&["rel", "file:///a", "rel", "file:///b", "file:///c"]),
            3,
        )
        .unwrap();
        assert_eq!(r.urls, ["file:///a", "file:///b", "file:///c"]);
        assert_eq!(r.current, 1);
        // The shown entry itself is bad: the next kept one.
        let r = restore(&s(&["file:///a", "rel", "file:///c"]), 1).unwrap();
        assert_eq!(r.current, 1);
        assert_eq!(r.urls[r.current], "file:///c");
        // Nothing kept after it: the last kept one. A huge index too.
        let r = restore(&s(&["file:///a", "file:///b", "rel"]), 2).unwrap();
        assert_eq!(r.current, 1);
        let r = restore(&s(&["file:///a", "file:///b"]), usize::MAX).unwrap();
        assert_eq!(r.current, 1);
    }

    #[test]
    fn a_session_is_capped() {
        let many: Vec<String> = (0..200).map(|i| format!("file:///d{i}")).collect();
        let r = restore(&many, 150).unwrap();
        assert_eq!(r.urls.len(), MAX_TABS);
        assert_eq!(r.current, MAX_TABS - 1, "its tab was cut: the last kept");
    }
}
