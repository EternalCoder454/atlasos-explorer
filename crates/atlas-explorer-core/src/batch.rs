//! Batch rename: what the new names of a set of items would be, and whether
//! they can be used. Pure text in, text out, so the dialog's live list and
//! the Apply that follows it decide from the same code; the files themselves
//! are renamed by the operation queue, as one undoable step.
//!
//! Four ways of making a name from a name: find and replace (plain text or a
//! regular expression, with or without regard to case), a number added before
//! or after the name, a change of case, and text added before or after the
//! name. Number, case and text work on the name without its extension (a
//! folder's whole name is its name); find and replace works on the whole file
//! name, so it can change an extension too.
//!
//! The checks are the single rename's (`names::validate`) and two more that
//! only a set has: two items that would get the same name, and a new name that
//! is the current name of another selected item (a swap or a chain would need
//! an order and a temporary name, so it is refused in words instead). A new
//! name that is already in the folder is refused too. Any refusal blocks
//! Apply; a warning (a hidden character, a leading space) does not.

use crate::names::{self, Invalid, Warning};
use regex::{NoExpand, Regex, RegexBuilder};
use std::collections::{HashMap, HashSet};

/// Most items one batch takes; more is refused, in words, before any work.
pub const MAX_ITEMS: usize = 5000;
/// Longest find pattern, in bytes (a regular expression is compiled from it).
pub const MAX_PATTERN_BYTES: usize = 512;
/// Largest compiled pattern, in bytes. The `regex` engine runs in time
/// linear in the name, so a hostile pattern can only be big, not slow.
const REGEX_SIZE_LIMIT: usize = 1 << 20;
/// Largest number and widest padding that make a name.
pub const MAX_NUMBER: u64 = 999_999_999;
pub const MAX_PADDING: usize = 12;

/// One item to rename: its current name, and whether it is a folder (a
/// folder has no extension to keep).
#[derive(Debug, Clone, Copy)]
pub struct Item<'a> {
    pub name: &'a str,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// Before the name.
    Start,
    /// After the name (before the extension).
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMode {
    Lower,
    Upper,
    /// Each word starts with a capital letter.
    Title,
    /// The first letter is a capital; the rest is lower case.
    Sentence,
}

impl CaseMode {
    pub fn from_code(code: u32) -> Option<CaseMode> {
        Some(match code {
            0 => CaseMode::Lower,
            1 => CaseMode::Upper,
            2 => CaseMode::Title,
            3 => CaseMode::Sentence,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op<'a> {
    Replace {
        find: &'a str,
        with: &'a str,
        match_case: bool,
        /// `find` is a regular expression and `with` may use `${1}` and `${name}` (a
        /// bare `$1_` would name a group called "1_").
        regex: bool,
    },
    Number {
        start: u64,
        step: u64,
        /// Digits at least; shorter numbers get leading zeros.
        padding: usize,
        at: Edge,
        /// Between the number and the name.
        separator: &'a str,
    },
    Case(CaseMode),
    AddText {
        text: &'a str,
        at: Edge,
    },
}

/// What is the matter with one new name, or that nothing is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The name does not change; the item is left alone.
    Unchanged,
    Fine,
    /// Usable, but worth a word (the single rename's warnings).
    Warn(Vec<Warning>),
    Bad(Invalid),
    /// Another item in the batch gets the same name.
    Twin,
    /// A name another selected item has now.
    Taken,
    /// A name already in the folder.
    Exists,
}

impl Check {
    /// Blocks Apply.
    pub fn blocks(&self) -> bool {
        matches!(
            self,
            Check::Bad(_) | Check::Twin | Check::Taken | Check::Exists
        )
    }

    /// A code for the bridge (0 unchanged, 1 fine, 2 warning, 3 not a name,
    /// 4 twin, 5 taken by a selected item, 6 already in the folder).
    pub fn code(&self) -> u8 {
        match self {
            Check::Unchanged => 0,
            Check::Fine => 1,
            Check::Warn(_) => 2,
            Check::Bad(_) => 3,
            Check::Twin => 4,
            Check::Taken => 5,
            Check::Exists => 6,
        }
    }

    /// The reason in plain words; empty for a name that is fine.
    pub fn describe(&self) -> String {
        match self {
            Check::Unchanged | Check::Fine => String::new(),
            Check::Warn(w) => w.iter().map(|w| w.describe()).collect::<Vec<_>>().join(" "),
            Check::Bad(i) => i.describe().to_string(),
            Check::Twin => "Another item in the list would get the same name.".to_string(),
            Check::Taken => {
                "Another selected item has this name now. Rename that one to something else first."
                    .to_string()
            }
            Check::Exists => "A file with this name is already here.".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The name the item would get (its own when nothing changes).
    pub new: String,
    pub check: Check,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// One row per item, in the order given.
    pub rows: Vec<Row>,
    /// Why no names could be made at all (a bad pattern, too many items); in
    /// plain words. Empty when the rows are good.
    pub problem: String,
    /// Rows whose name changes.
    pub changed: usize,
    /// Rows that block Apply.
    pub blocked: usize,
}

impl Plan {
    /// Apply can go ahead: something changes, and nothing blocks.
    pub fn can_apply(&self) -> bool {
        self.problem.is_empty() && self.changed > 0 && self.blocked == 0
    }
}

fn refused(items: &[Item], problem: &str) -> Plan {
    Plan {
        rows: items
            .iter()
            .map(|i| Row {
                new: i.name.to_string(),
                check: Check::Unchanged,
            })
            .collect(),
        problem: problem.to_string(),
        changed: 0,
        blocked: 0,
    }
}

/// The pattern of a find and replace, compiled once for every name.
fn finder(find: &str, match_case: bool, regex: bool) -> Result<Regex, &'static str> {
    if find.len() > MAX_PATTERN_BYTES {
        return Err("The text to find is too long.");
    }
    let pattern = if regex {
        find.to_string()
    } else {
        regex::escape(find)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!match_case)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_SIZE_LIMIT)
        .nest_limit(50)
        .build()
        .map_err(|_| "That isn't a valid regular expression.")
}

/// A stem and extension for the operations that keep the extension.
fn parts<'a>(item: &Item<'a>) -> (&'a str, &'a str) {
    if item.is_dir {
        (item.name, "")
    } else {
        names::split_ext(item.name)
    }
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            // An apostrophe is inside a word ("don't"), anything else ends one.
            start = !matches!(c, '\'' | '\u{2019}');
            out.push(c);
        }
    }
    out
}

fn sentence_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut first = true;
    for c in s.chars() {
        if first && c.is_alphanumeric() {
            out.extend(c.to_uppercase());
            first = false;
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

fn with_edge(stem: &str, ext: &str, add: &str, at: Edge, between: &str) -> String {
    match at {
        Edge::Start => format!("{add}{between}{stem}{ext}"),
        Edge::End => format!("{stem}{between}{add}{ext}"),
    }
}

/// The new names, and what is the matter with each. `exists` says whether a
/// name is already taken in the folder (by something other than the items in
/// this batch, which are checked against each other here).
pub fn plan(items: &[Item], op: &Op, exists: impl Fn(&str) -> bool) -> Plan {
    if items.len() > MAX_ITEMS {
        return refused(items, "That is too many items to rename at once.");
    }
    let finder = match op {
        Op::Replace {
            find,
            match_case,
            regex,
            ..
        } if !find.is_empty() => match finder(find, *match_case, *regex) {
            Ok(f) => Some(f),
            Err(why) => return refused(items, why),
        },
        _ => None,
    };
    if let Op::Number {
        start,
        step,
        padding,
        ..
    } = op
        && (*start > MAX_NUMBER || *step > MAX_NUMBER || *padding > MAX_PADDING)
    {
        return refused(items, "That number is too large.");
    }

    let news: Vec<String> = items
        .iter()
        .enumerate()
        .map(|(i, item)| match op {
            Op::Replace { with, regex, .. } => match &finder {
                Some(f) if *regex => f.replace_all(item.name, *with).into_owned(),
                Some(f) => f.replace_all(item.name, NoExpand(with)).into_owned(),
                None => item.name.to_string(),
            },
            Op::Number {
                start,
                step,
                padding,
                at,
                separator,
            } => {
                let (stem, ext) = parts(item);
                let n = start.saturating_add(step.saturating_mul(i as u64));
                let number = format!("{n:0width$}", width = *padding);
                with_edge(stem, ext, &number, *at, separator)
            }
            Op::Case(mode) => {
                let (stem, ext) = parts(item);
                let changed = match mode {
                    CaseMode::Lower => stem.to_lowercase(),
                    CaseMode::Upper => stem.to_uppercase(),
                    CaseMode::Title => title_case(stem),
                    CaseMode::Sentence => sentence_case(stem),
                };
                format!("{changed}{ext}")
            }
            Op::AddText { text, at } => {
                let (stem, ext) = parts(item);
                with_edge(stem, ext, text, *at, "")
            }
        })
        .collect();

    // How many items end with each name, and the names now.
    let mut finals: HashMap<&str, usize> = HashMap::new();
    for n in &news {
        *finals.entry(n.as_str()).or_default() += 1;
    }
    let olds: HashSet<&str> = items.iter().map(|i| i.name).collect();

    let mut rows = Vec::with_capacity(items.len());
    let (mut changed, mut blocked) = (0, 0);
    for (item, new) in items.iter().zip(news.iter()) {
        let check = if new == item.name {
            Check::Unchanged
        } else {
            match names::validate(new) {
                Err(bad) => Check::Bad(bad),
                Ok(warnings) => {
                    if finals.get(new.as_str()).copied().unwrap_or(0) > 1 {
                        Check::Twin
                    } else if olds.contains(new.as_str()) {
                        Check::Taken
                    } else if exists(new) {
                        Check::Exists
                    } else if warnings.is_empty() {
                        Check::Fine
                    } else {
                        Check::Warn(warnings)
                    }
                }
            }
        };
        if check != Check::Unchanged {
            changed += 1;
        }
        if check.blocks() {
            blocked += 1;
        }
        rows.push(Row {
            new: new.clone(),
            check,
        });
    }
    Plan {
        rows,
        problem: String::new(),
        changed,
        blocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> Vec<Item<'static>> {
        // Leaked: the tests are short-lived and this keeps the call sites plain.
        names
            .iter()
            .map(|n| Item {
                name: Box::leak(n.to_string().into_boxed_str()),
                is_dir: false,
            })
            .collect()
    }
    fn run(names: &[&str], op: &Op) -> Vec<String> {
        plan(&files(names), op, |_| false)
            .rows
            .into_iter()
            .map(|r| r.new)
            .collect()
    }
    fn replace<'a>(find: &'a str, with: &'a str, match_case: bool, regex: bool) -> Op<'a> {
        Op::Replace {
            find,
            with,
            match_case,
            regex,
        }
    }

    #[test]
    fn find_and_replace_plain_text() {
        let op = replace("IMG", "Photo", true, false);
        assert_eq!(
            run(&["IMG_1.jpg", "img_2.jpg", "x IMG IMG.png"], &op),
            ["Photo_1.jpg", "img_2.jpg", "x Photo Photo.png"]
        );
        let op = replace("IMG", "Photo", false, false);
        assert_eq!(
            run(&["IMG_1.jpg", "img_2.jpg"], &op),
            ["Photo_1.jpg", "Photo_2.jpg"]
        );
    }

    #[test]
    fn plain_text_is_not_a_pattern_and_the_replacement_is_not_expanded() {
        let op = replace("a.b", "$0", true, false);
        assert_eq!(run(&["a.b", "axb"], &op), ["$0", "axb"]);
        let op = replace("(", "[", true, false);
        assert_eq!(run(&["f(1).txt"], &op), ["f[1).txt"]);
    }

    #[test]
    fn find_and_replace_with_a_pattern() {
        let op = replace(r"(\d+)-(\d+)", "${2}_${1}", true, true);
        assert_eq!(run(&["12-34.txt", "no.txt"], &op), ["34_12.txt", "no.txt"]);
        let op = replace(r"\s+", "_", true, true);
        assert_eq!(run(&["a  b   c.txt"], &op), ["a_b_c.txt"]);
        let op = replace("^", "new-", true, true);
        assert_eq!(run(&["a.txt"], &op), ["new-a.txt"]);
        // Case follows the switch.
        let op = replace("[a-c]+", "-", false, true);
        assert_eq!(run(&["xABCy"], &op), ["x-y"]);
        let op = replace("[a-c]+", "-", true, true);
        assert_eq!(run(&["xABCy"], &op), ["xABCy"]);
    }

    #[test]
    fn a_bad_pattern_is_a_problem_not_a_panic() {
        let items = files(&["a.txt"]);
        let p = plan(&items, &replace("(", "", true, true), |_| false);
        assert_eq!(p.problem, "That isn't a valid regular expression.");
        assert!(!p.can_apply());
        assert_eq!(p.rows[0].check, Check::Unchanged);
        let long = "a".repeat(MAX_PATTERN_BYTES + 1);
        let p = plan(&items, &replace(&long, "", true, false), |_| false);
        assert_eq!(p.problem, "The text to find is too long.");
        // A huge repetition is refused by the size limit.
        let p = plan(&items, &replace(r"(a{1000}){1000}", "", true, true), |_| {
            false
        });
        assert!(!p.problem.is_empty());
    }

    #[test]
    fn an_empty_find_changes_nothing() {
        let p = plan(&files(&["a.txt"]), &replace("", "x", true, true), |_| false);
        assert!(p.problem.is_empty());
        assert_eq!(p.changed, 0);
        assert!(!p.can_apply());
    }

    #[test]
    fn replace_can_change_the_extension() {
        let op = replace(".jpeg", ".jpg", false, false);
        assert_eq!(run(&["a.JPEG", "b.jpeg"], &op), ["a.jpg", "b.jpg"]);
    }

    #[test]
    fn numbers_before_and_after_keep_the_extension() {
        let after = Op::Number {
            start: 1,
            step: 1,
            padding: 3,
            at: Edge::End,
            separator: " ",
        };
        assert_eq!(
            run(&["a.txt", "b.tar.gz", "c"], &after),
            ["a 001.txt", "b 002.tar.gz", "c 003"]
        );
        let before = Op::Number {
            start: 10,
            step: 5,
            padding: 0,
            at: Edge::Start,
            separator: "_",
        };
        assert_eq!(run(&["a.txt", "b.txt"], &before), ["10_a.txt", "15_b.txt"]);
        let none = Op::Number {
            start: 0,
            step: 1,
            padding: 2,
            at: Edge::End,
            separator: "",
        };
        assert_eq!(run(&["a.txt", ".bashrc"], &none), ["a00.txt", ".bashrc01"]);
    }

    #[test]
    fn a_folder_has_no_extension() {
        let items = [
            Item {
                name: "photos.2024",
                is_dir: true,
            },
            Item {
                name: "photos.2024",
                is_dir: false,
            },
        ];
        let op = Op::AddText {
            text: "!",
            at: Edge::End,
        };
        let p = plan(&items, &op, |_| false);
        assert_eq!(p.rows[0].new, "photos.2024!");
        assert_eq!(p.rows[1].new, "photos!.2024");
    }

    #[test]
    fn numbers_stay_in_bounds() {
        let items = files(&["a", "b"]);
        let big = Op::Number {
            start: MAX_NUMBER + 1,
            step: 1,
            padding: 0,
            at: Edge::End,
            separator: "",
        };
        assert!(!plan(&items, &big, |_| false).problem.is_empty());
        let wide = Op::Number {
            start: 0,
            step: 1,
            padding: MAX_PADDING + 1,
            at: Edge::End,
            separator: "",
        };
        assert!(!plan(&items, &wide, |_| false).problem.is_empty());
        let top = Op::Number {
            start: MAX_NUMBER,
            step: MAX_NUMBER,
            padding: 0,
            at: Edge::End,
            separator: "",
        };
        assert!(plan(&items, &top, |_| false).problem.is_empty());
    }

    #[test]
    fn change_case() {
        let names = ["my FILE_name-v2 (don't).TXT", "ÉCOLE élève.md", "x"];
        assert_eq!(
            run(&names, &Op::Case(CaseMode::Lower)),
            ["my file_name-v2 (don't).TXT", "école élève.md", "x"]
        );
        assert_eq!(
            run(&names, &Op::Case(CaseMode::Upper)),
            ["MY FILE_NAME-V2 (DON'T).TXT", "ÉCOLE ÉLÈVE.md", "X"]
        );
        assert_eq!(
            run(&names, &Op::Case(CaseMode::Title)),
            ["My File_Name-V2 (Don't).TXT", "École Élève.md", "X"]
        );
        assert_eq!(
            run(&names, &Op::Case(CaseMode::Sentence)),
            ["My file_name-v2 (don't).TXT", "École élève.md", "X"]
        );
    }

    #[test]
    fn sentence_case_starts_at_the_first_letter() {
        assert_eq!(sentence_case("2nd PLACE"), "2nd place");
        assert_eq!(sentence_case("  ab CD"), "  Ab cd");
        assert_eq!(sentence_case(""), "");
    }

    #[test]
    fn add_text_keeps_the_extension() {
        let before = Op::AddText {
            text: "old-",
            at: Edge::Start,
        };
        assert_eq!(
            run(&["a.txt", "b.tar.gz"], &before),
            ["old-a.txt", "old-b.tar.gz"]
        );
        let after = Op::AddText {
            text: "-v2",
            at: Edge::End,
        };
        assert_eq!(
            run(&["a.txt", "b.tar.gz", "c"], &after),
            ["a-v2.txt", "b-v2.tar.gz", "c-v2"]
        );
    }

    #[test]
    fn unchanged_rows_are_left_alone() {
        let items = files(&["a.txt", "B.txt"]);
        let p = plan(&items, &Op::Case(CaseMode::Lower), |_| false);
        assert_eq!(p.rows[0].check, Check::Unchanged);
        assert_eq!(p.rows[1].check, Check::Fine);
        assert_eq!((p.changed, p.blocked), (1, 0));
        assert!(p.can_apply());
        let p = plan(&files(&["a.txt"]), &Op::Case(CaseMode::Lower), |_| false);
        assert!(!p.can_apply(), "nothing changes");
    }

    #[test]
    fn bad_names_block() {
        let items = files(&["a.txt", "b.txt"]);
        let p = plan(&items, &replace("a.txt", "x/y", true, false), |_| false);
        assert_eq!(p.rows[0].check, Check::Bad(Invalid::Slash));
        assert_eq!(p.blocked, 1);
        assert!(!p.can_apply());
        let p = plan(&items, &replace("a.txt", "", true, false), |_| false);
        assert_eq!(p.rows[0].check, Check::Bad(Invalid::Empty));
        let p = plan(&files(&["..a"]), &replace("..a", "..", true, false), |_| {
            false
        });
        assert_eq!(p.rows[0].check, Check::Bad(Invalid::DotOrDotDot));
        let p = plan(&items, &replace("a", &"b".repeat(300), true, false), |_| {
            false
        });
        assert_eq!(p.rows[0].check, Check::Bad(Invalid::TooLong));
    }

    #[test]
    fn warnings_do_not_block() {
        let items = files(&["a.txt"]);
        let p = plan(&items, &replace("a", "\u{202E}a", true, false), |_| false);
        assert_eq!(
            p.rows[0].check,
            Check::Warn(vec![Warning::HiddenCharacters])
        );
        assert_eq!(p.blocked, 0);
        assert!(p.can_apply());
        assert!(p.rows[0].check.describe().contains("text-direction"));
    }

    #[test]
    fn two_items_with_one_name_both_block() {
        let items = files(&["a1.txt", "b1.txt", "c.txt"]);
        let p = plan(&items, &replace("[ab]", "x", true, true), |_| false);
        assert_eq!(p.rows[0].check, Check::Twin);
        assert_eq!(p.rows[1].check, Check::Twin);
        assert_eq!(p.rows[2].check, Check::Unchanged);
        assert_eq!((p.changed, p.blocked), (2, 2));
        assert!(!p.can_apply());
        assert_eq!(
            Check::Twin.describe(),
            "Another item in the list would get the same name."
        );
    }

    #[test]
    fn a_name_an_unchanged_item_keeps_is_a_twin() {
        // b is not touched, and a would become b.
        let items = files(&["a", "b"]);
        let p = plan(&items, &replace("a", "b", true, false), |_| false);
        assert_eq!(p.rows[0].check, Check::Twin);
        assert_eq!(p.rows[1].check, Check::Unchanged);
    }

    #[test]
    fn a_name_a_selected_item_has_now_blocks() {
        // "aa" becomes "aaaa" and "a" becomes "aa": the second would need the
        // first out of the way, and that needs an order, so it is refused.
        let items = files(&["aa", "a"]);
        let p = plan(&items, &replace("a", "aa", true, false), |_| false);
        assert_eq!(p.rows[0].new, "aaaa");
        assert_eq!(p.rows[0].check, Check::Fine);
        assert_eq!(p.rows[1].check, Check::Taken);
        assert!(!p.can_apply());
        // A rename that gives every item its own name again changes nothing.
        let items = files(&["a", "b"]);
        let same = replace("(a|b)", "$1", true, true);
        assert_eq!(plan(&items, &same, |_| false).changed, 0);
    }

    #[test]
    fn a_name_in_the_folder_blocks() {
        let items = files(&["a.txt"]);
        let p = plan(&items, &replace("a", "b", true, false), |n| n == "b.txt");
        assert_eq!(p.rows[0].check, Check::Exists);
        assert!(!p.can_apply());
        assert_eq!(
            Check::Exists.describe(),
            "A file with this name is already here."
        );
    }

    #[test]
    fn too_many_items_are_refused() {
        let names: Vec<String> = (0..=MAX_ITEMS).map(|i| format!("f{i}")).collect();
        let items: Vec<Item> = names
            .iter()
            .map(|n| Item {
                name: n,
                is_dir: false,
            })
            .collect();
        let p = plan(&items, &Op::Case(CaseMode::Upper), |_| false);
        assert!(!p.problem.is_empty());
        assert!(!p.can_apply());
        assert_eq!(p.rows.len(), items.len());
    }

    #[test]
    fn five_thousand_names_plan_quickly() {
        let names: Vec<String> = (0..MAX_ITEMS).map(|i| format!("IMG_{i:05}.jpg")).collect();
        let items: Vec<Item> = names
            .iter()
            .map(|n| Item {
                name: n,
                is_dir: false,
            })
            .collect();
        let started = std::time::Instant::now();
        let p = plan(
            &items,
            &replace(r"IMG_(\d+)", "photo-$1", true, true),
            |_| false,
        );
        assert!(p.can_apply());
        assert!(started.elapsed().as_secs() < 2);
    }

    #[test]
    fn codes_and_texts() {
        let all = [
            Check::Unchanged,
            Check::Fine,
            Check::Warn(vec![Warning::LeadingDash]),
            Check::Bad(Invalid::Empty),
            Check::Twin,
            Check::Taken,
            Check::Exists,
        ];
        let codes: Vec<u8> = all.iter().map(Check::code).collect();
        assert_eq!(codes, [0, 1, 2, 3, 4, 5, 6]);
        for c in &all[3..] {
            assert!(!c.describe().is_empty());
            assert!(c.blocks());
        }
        assert!(all[2].describe().contains("option"));
        assert!(!all[2].blocks());
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        fn check_plan(names: &[String], op: &Op) -> Result<(), TestCaseError> {
            let items: Vec<Item> = names
                .iter()
                .map(|n| Item {
                    name: n,
                    is_dir: false,
                })
                .collect();
            let p = plan(&items, op, |_| false);
            prop_assert_eq!(p.rows.len(), items.len());
            for (row, item) in p.rows.iter().zip(&items) {
                if row.check == Check::Unchanged || row.check.blocks() {
                    continue;
                }
                let n = row.new.as_str();
                prop_assert!(
                    !n.contains('/') && !n.contains('\0'),
                    "{:?} from {:?}",
                    n,
                    item.name
                );
                prop_assert!(!n.is_empty() && n != "." && n != "..", "{:?}", n);
                prop_assert!(n.len() <= 255, "{}", n.len());
            }
            Ok(())
        }

        proptest! {
            #[test]
            fn plans_never_panic_and_new_names_are_names(
                names in prop::collection::vec(".{0,40}", 1..6),
                text in ".{0,20}",
                find in ".{0,6}",
                start in 0u64..2000,
                step in 0u64..10,
                padding in 0usize..14,
                mode in 0u32..4,
                end in any::<bool>(),
                match_case in any::<bool>(),
                regex in any::<bool>(),
            ) {
                let at = if end { Edge::End } else { Edge::Start };
                check_plan(&names, &Op::Number { start, step, padding, at, separator: &text })?;
                check_plan(&names, &Op::Case(CaseMode::from_code(mode).unwrap()))?;
                check_plan(&names, &Op::AddText { text: &text, at })?;
                check_plan(&names, &Op::Replace { find: &find, with: &text, match_case, regex })?;
            }
        }
    }
}
