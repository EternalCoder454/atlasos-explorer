//! Sorting a folder's rows: a natural sort key per name, computed once, and
//! a permutation sorted by one column. Runs on a worker, never the GUI thread.
//!
//! The key is a byte string whose plain byte order is the natural order:
//! the name casefolded, each run of ASCII digits replaced by `0x01`, the
//! count of its significant digits (big-endian `u32`, so a longer number is
//! larger and nothing overflows), and those digits without leading zeros.
//! A `0x00` and the name's original bytes end the key, so names that fold the
//! same (`a`/`A`, `01`/`1`) still have one fixed order.

use std::cmp::Ordering;

/// The natural sort key of a file name (any bytes; invalid UTF-8 is folded
/// as U+FFFD and told apart by the original bytes at the end).
pub fn name_key(name: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(name);
    let mut key = Vec::with_capacity(name.len() * 2 + 8);
    let mut chars = text.chars().peekable();
    let mut buf = [0u8; 4];
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut digits = String::new();
            digits.push(c);
            while let Some(&d) = chars.peek() {
                if !d.is_ascii_digit() {
                    break;
                }
                digits.push(d);
                chars.next();
            }
            let significant = digits.trim_start_matches('0');
            key.push(0x01);
            key.extend_from_slice(&(significant.len().min(u32::MAX as usize) as u32).to_be_bytes());
            key.extend_from_slice(significant.as_bytes());
        } else {
            for lower in c.to_lowercase() {
                key.extend_from_slice(lower.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    key.push(0x00);
    key.extend_from_slice(name);
    key
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Name,
    Size,
    Type,
    Modified,
    Created,
    Accessed,
}

/// What sorting needs to know about one row.
#[derive(Debug, Clone)]
pub struct SortRow {
    /// From [`name_key`].
    pub key: Vec<u8>,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: i64,
    pub ctime: i64,
    pub atime: i64,
    /// The kind shown in the Type column.
    pub kind: String,
}

fn cmp_kind(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|c| c.to_ascii_lowercase())
        .cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
}

/// The row order: indices into `rows`, sorted by `column`. Ties fall to the
/// name key (always ascending, except for the Name column itself), so the
/// order is total and the same every time. With `folders_first`, folders
/// precede files in both directions.
pub fn sort_permutation(
    rows: &[SortRow],
    column: Column,
    descending: bool,
    folders_first: bool,
) -> Vec<u32> {
    let mut perm: Vec<u32> = (0..rows.len().min(u32::MAX as usize) as u32).collect();
    perm.sort_unstable_by(|&ia, &ib| {
        let (a, b) = (&rows[ia as usize], &rows[ib as usize]);
        if folders_first && a.is_dir != b.is_dir {
            return if a.is_dir {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let primary = match column {
            Column::Name => a.key.cmp(&b.key),
            Column::Size => a.size.cmp(&b.size),
            Column::Type => cmp_kind(&a.kind, &b.kind),
            Column::Modified => a.mtime.cmp(&b.mtime),
            Column::Created => a.ctime.cmp(&b.ctime),
            Column::Accessed => a.atime.cmp(&b.atime),
        };
        let primary = if descending {
            primary.reverse()
        } else {
            primary
        };
        primary.then_with(|| a.key.cmp(&b.key))
    });
    perm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, is_dir: bool, size: u64) -> SortRow {
        SortRow {
            key: name_key(name.as_bytes()),
            is_dir,
            size,
            mtime: size as i64,
            ctime: 0,
            atime: 0,
            kind: String::new(),
        }
    }

    fn names<'a>(rows: &'a [(&'a str, bool, u64)], p: &[u32]) -> Vec<&'a str> {
        p.iter().map(|&i| rows[i as usize].0).collect()
    }

    fn sorted(list: &[(&str, bool, u64)], col: Column, desc: bool, ff: bool) -> Vec<String> {
        let rows: Vec<SortRow> = list.iter().map(|&(n, d, s)| row(n, d, s)).collect();
        names(list, &sort_permutation(&rows, col, desc, ff))
            .into_iter()
            .map(String::from)
            .collect()
    }

    fn key_lt(a: &str, b: &str) -> bool {
        name_key(a.as_bytes()) < name_key(b.as_bytes())
    }

    #[test]
    fn digit_runs_compare_as_numbers() {
        assert!(key_lt("file2", "file10"));
        assert!(key_lt("file9.txt", "file10.txt"));
        assert!(key_lt("a1b2", "a1b10"));
        assert!(!key_lt("file10", "file2"));
    }

    #[test]
    fn case_is_folded_and_ties_are_broken_by_bytes() {
        assert!(key_lt("apple", "Banana"));
        assert!(!key_lt("B", "a2"));
        // Equal when folded: the order is still fixed and strict.
        assert!(key_lt("A", "a") ^ key_lt("a", "A"));
        assert!(key_lt("file01", "file1") ^ key_lt("file1", "file01"));
        assert_ne!(name_key(b"x\xff"), name_key(b"x\xfe"));
    }

    #[test]
    fn leading_zeros_and_huge_runs_do_not_overflow() {
        let huge9 = format!("n{}", "9".repeat(5000));
        let huge1 = format!("n1{}", "0".repeat(5000));
        assert!(key_lt(&huge9, &huge1));
        assert!(key_lt(&format!("n{}", "9".repeat(4999)), &huge9));
        assert!(key_lt("n0000", "n1"));
        assert!(key_lt("n007", "n8"));
        assert!(key_lt("n", "n0"));
    }

    #[test]
    fn folders_stay_first_in_both_directions() {
        let list = [
            ("b", false, 1),
            ("a", true, 9),
            ("c", false, 5),
            ("z", true, 2),
        ];
        for desc in [false, true] {
            for col in [Column::Name, Column::Size, Column::Modified] {
                let out = sorted(&list, col, desc, true);
                assert!(out[..2].iter().all(|n| n == "a" || n == "z"), "{out:?}");
            }
        }
        assert_eq!(
            sorted(&list, Column::Name, true, true),
            ["z", "a", "c", "b"]
        );
        assert_eq!(
            sorted(&list, Column::Name, false, false),
            ["a", "b", "c", "z"]
        );
    }

    #[test]
    fn size_ties_fall_to_name_and_descending_reverses() {
        let list = [("b", false, 1), ("a", false, 1), ("c", false, 7)];
        assert_eq!(sorted(&list, Column::Size, false, true), ["a", "b", "c"]);
        assert_eq!(sorted(&list, Column::Size, true, true), ["c", "a", "b"]);
    }

    #[test]
    fn result_is_a_permutation() {
        let list: Vec<(String, bool, u64)> = (0..500)
            .map(|i| (format!("f{}", (i * 37) % 101), i % 3 == 0, (i % 7) as u64))
            .collect();
        let rows: Vec<SortRow> = list.iter().map(|(n, d, s)| row(n, *d, *s)).collect();
        let mut p = sort_permutation(&rows, Column::Size, true, true);
        p.sort_unstable();
        assert!(p.iter().enumerate().all(|(i, &v)| v as usize == i));
    }

    #[test]
    fn type_column_ignores_ascii_case() {
        let mut a = row("a", false, 0);
        let mut b = row("b", false, 0);
        a.kind = "PDF document".into();
        b.kind = "Image".into();
        let p = sort_permutation(&[a, b], Column::Type, false, true);
        assert_eq!(p, [1, 0]);
    }
}
