//! Tags and star ratings as the freedesktop convention keeps them: the
//! xattr `user.xdg.tags` holds comma-separated names (Baloo and Dolphin read
//! the same), and `user.baloo.rating` a number 0 to 10 (two per star). A
//! colour tag is a tag with one of seven well-known names; nothing else is
//! stored for it, so other programs see plain names.
//!
//! Everything here works on text; reading and writing the attributes is
//! `xattr`, and the queue's undo step is `attrs`.

/// The seven colour tags, in the order the menus list them, with the colour
/// of their dot (sRGB, readable on light and dark).
pub const COLOURS: [(&str, &str); 7] = [
    ("Red", "#e5484d"),
    ("Orange", "#f76b15"),
    ("Yellow", "#e5b800"),
    ("Green", "#30a46c"),
    ("Blue", "#3e63dd"),
    ("Purple", "#8e4ec6"),
    ("Gray", "#8b8d98"),
];

/// The longest tag name a person can make.
pub const MAX_NAME_CHARS: usize = 64;
/// The most tags one item can hold.
pub const MAX_TAGS: usize = 32;
/// The longest stored value (the attribute's own limit is `xattr::MAX_VALUE`).
pub const MAX_VALUE: usize = 2048;

/// Case-insensitive equality of two tag names.
pub fn same(a: &str, b: &str) -> bool {
    a == b || a.to_lowercase() == b.to_lowercase()
}

/// The colour of a tag's dot, `None` for a named tag.
pub fn colour_of(name: &str) -> Option<&'static str> {
    COLOURS
        .iter()
        .find(|(n, _)| same(n, name))
        .map(|(_, hex)| *hex)
}

/// The canonical spelling of a colour tag (`red` is `Red`), `None` for others.
pub fn colour_name(name: &str) -> Option<&'static str> {
    COLOURS.iter().find(|(n, _)| same(n, name)).map(|(n, _)| *n)
}

/// Position in the colour list, for ordering.
fn colour_rank(name: &str) -> usize {
    COLOURS
        .iter()
        .position(|(n, _)| same(n, name))
        .unwrap_or(COLOURS.len())
}

/// The tags in an attribute value: split at commas, each trimmed, empty ones
/// dropped, repeats (any case) dropped keeping the first. Bytes that are not
/// UTF-8 are replaced, so the result is for showing; see [`is_clean`].
pub fn parse(raw: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(raw);
    let mut out: Vec<String> = Vec::new();
    for part in text.split(',') {
        let t = part.trim();
        if t.is_empty() || out.iter().any(|o| same(o, t)) {
            continue;
        }
        out.push(t.to_string());
    }
    out
}

/// Whether [`parse`] shows every byte of the value as it is: valid UTF-8.
/// A value that is not is shown, but never rewritten (it would lose bytes).
pub fn is_clean(raw: &[u8]) -> bool {
    std::str::from_utf8(raw).is_ok()
}

/// The attribute value for a list of tags, `None` when there are none (the
/// attribute is then removed).
pub fn encode(tags: &[String]) -> Option<Vec<u8>> {
    if tags.is_empty() {
        None
    } else {
        Some(tags.join(",").into_bytes())
    }
}

/// Why a name typed by a person can't be a tag, in words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameProblem {
    Empty,
    Comma,
    Control,
    TooLong,
}

impl NameProblem {
    pub fn text(self) -> &'static str {
        match self {
            NameProblem::Empty => "Type a name for the tag.",
            NameProblem::Comma => "A tag name can't have a comma in it.",
            NameProblem::Control => "A tag name can't have control characters in it.",
            NameProblem::TooLong => "That tag name is too long.",
        }
    }
}

/// A name a person typed, made into a tag: trimmed, and a colour's name in
/// its own spelling (`red` is `Red`).
pub fn new_name(typed: &str) -> Result<String, NameProblem> {
    let t = typed.trim();
    if t.is_empty() {
        return Err(NameProblem::Empty);
    }
    if t.contains(',') {
        return Err(NameProblem::Comma);
    }
    if t.chars().any(char::is_control) {
        return Err(NameProblem::Control);
    }
    if t.chars().count() > MAX_NAME_CHARS {
        return Err(NameProblem::TooLong);
    }
    Ok(colour_name(t).unwrap_or(t).to_string())
}

/// `tags` with `name` added (when `on`, and not there in any case) or
/// removed (any case). The order of the others is kept.
pub fn toggled(tags: &[String], name: &str, on: bool) -> Vec<String> {
    let present = tags.iter().any(|t| same(t, name));
    if on {
        let mut out = tags.to_vec();
        if !present {
            out.push(name.to_string());
        }
        out
    } else {
        tags.iter().filter(|t| !same(t, name)).cloned().collect()
    }
}

/// What [`apply_edit`] can't do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditProblem {
    TooMany,
    TooLong,
}

impl EditProblem {
    pub fn text(self) -> &'static str {
        match self {
            EditProblem::TooMany => "An item can't have more than 32 tags.",
            EditProblem::TooLong => "There is no room to keep more tags on this item.",
        }
    }
}

/// Adds `add`, then takes away `remove` (names compared in any case), or
/// clears all when `clear`.
pub fn apply_edit(
    tags: &[String],
    add: &[String],
    remove: &[String],
    clear: bool,
) -> Result<Vec<String>, EditProblem> {
    let mut out: Vec<String> = if clear { Vec::new() } else { tags.to_vec() };
    for r in remove {
        out.retain(|t| !same(t, r));
    }
    for a in add {
        if !out.iter().any(|t| same(t, a)) {
            out.push(a.clone());
        }
    }
    if out.len() > MAX_TAGS {
        return Err(EditProblem::TooMany);
    }
    if encode(&out).is_some_and(|v| v.len() > MAX_VALUE) {
        return Err(EditProblem::TooLong);
    }
    Ok(out)
}

/// How many of the selected items have a tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Have {
    None,
    Some,
    All,
}

/// Whether none, some or all of `items` (each one's tags) have `name`.
pub fn have(items: &[Vec<String>], name: &str) -> Have {
    let n = items
        .iter()
        .filter(|tags| tags.iter().any(|t| same(t, name)))
        .count();
    if n == 0 {
        Have::None
    } else if n == items.len() {
        Have::All
    } else {
        Have::Some
    }
}

/// The tags to list in the sidebar or after the colours in a menu: the names
/// found, once each (a colour in its own spelling), the colours first in their
/// order, then the others sorted without regard to case. `seen` are names of
/// items looked at, `indexed` the file index's (name, count).
pub fn in_use(seen: &[String], indexed: &[(String, u32)]) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    let mut add = |name: &str, count: u32| {
        let key = colour_name(name).unwrap_or(name);
        if let Some(e) = out.iter_mut().find(|(n, _)| same(n, key)) {
            e.1 = e.1.max(count);
        } else {
            out.push((key.to_string(), count));
        }
    };
    for (n, c) in indexed {
        add(n, *c);
    }
    for n in seen {
        add(n, 0);
    }
    out.sort_by(|a, b| {
        colour_rank(&a.0)
            .cmp(&colour_rank(&b.0))
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    out
}

// ---- Ratings ----

/// The rating in an attribute value: 0 to 10, two to a star.
/// `None` when the value is not a number from 0 to 10.
pub fn rating_parse(raw: &[u8]) -> Option<u8> {
    let t = std::str::from_utf8(raw).ok()?.trim();
    if t.is_empty() || t.len() > 3 {
        return None;
    }
    let n: u8 = t.parse().ok()?;
    (n <= 10).then_some(n)
}

/// The attribute value for a rating, `None` for no rating (0): the attribute
/// is then removed.
pub fn rating_encode(rating: u8) -> Option<Vec<u8>> {
    (rating > 0).then(|| rating.min(10).to_string().into_bytes())
}

/// Stars (0 to 5, halves) for a rating of 0 to 10.
pub fn rating_stars(rating: u8) -> f32 {
    f32::from(rating.min(10)) / 2.0
}

/// A rating of 0 to 10 for whole or half stars, clamped.
pub fn rating_from_stars(stars: f32) -> u8 {
    if !stars.is_finite() {
        return 0;
    }
    (stars.clamp(0.0, 5.0) * 2.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_the_freedesktop_value() {
        assert_eq!(parse(b"Red,Work"), v(&["Red", "Work"]));
        assert_eq!(parse(b" Red , ,Work,, "), v(&["Red", "Work"]));
        assert_eq!(parse(b""), Vec::<String>::new());
        assert_eq!(parse(b",,,"), Vec::<String>::new());
        assert_eq!(parse(b"Taxes 2025,red,RED"), v(&["Taxes 2025", "red"]));
        assert_eq!(parse("Caf\u{e9},\u{65e5}\u{672c}".as_bytes()).len(), 2);
    }

    #[test]
    fn bytes_that_are_not_text_are_flagged() {
        assert!(is_clean(b"Red,Work"));
        assert!(!is_clean(b"Re\xffd"));
        assert_eq!(parse(b"Re\xffd"), v(&["Re\u{fffd}d"]));
    }

    #[test]
    fn encodes_back() {
        assert_eq!(encode(&v(&["Red", "Work"])), Some(b"Red,Work".to_vec()));
        assert_eq!(encode(&[]), None);
        let round = parse(&encode(&v(&["Taxes 2025", "Blue"])).unwrap());
        assert_eq!(round, v(&["Taxes 2025", "Blue"]));
    }

    #[test]
    fn colours_by_name_in_any_case() {
        assert_eq!(colour_of("red"), Some("#e5484d"));
        assert_eq!(colour_of("GREEN"), colour_of("Green"));
        assert_eq!(colour_of("Work"), None);
        assert_eq!(colour_name("purple"), Some("Purple"));
        assert_eq!(COLOURS.len(), 7);
    }

    #[test]
    fn names_people_type() {
        assert_eq!(new_name("  Taxes 2025 "), Ok("Taxes 2025".into()));
        assert_eq!(new_name("red"), Ok("Red".into()));
        assert_eq!(new_name("  "), Err(NameProblem::Empty));
        assert_eq!(new_name("a,b"), Err(NameProblem::Comma));
        assert_eq!(new_name("a\nb"), Err(NameProblem::Control));
        assert_eq!(new_name(&"x".repeat(65)), Err(NameProblem::TooLong));
        assert!(new_name(&"x".repeat(64)).is_ok());
        assert!(!NameProblem::Comma.text().is_empty());
    }

    #[test]
    fn toggling() {
        let t = v(&["Red", "Work"]);
        assert_eq!(toggled(&t, "Blue", true), v(&["Red", "Work", "Blue"]));
        assert_eq!(toggled(&t, "red", false), v(&["Work"]));
        assert_eq!(toggled(&t, "RED", true), t);
        assert_eq!(toggled(&[], "Red", false), Vec::<String>::new());
    }

    #[test]
    fn edits_respect_limits() {
        let t = v(&["Red"]);
        assert_eq!(
            apply_edit(&t, &v(&["Blue"]), &v(&["red"]), false),
            Ok(v(&["Blue"]))
        );
        assert_eq!(apply_edit(&t, &[], &[], true), Ok(vec![]));
        let many: Vec<String> = (0..MAX_TAGS).map(|i| format!("t{i}")).collect();
        assert_eq!(
            apply_edit(&many, &v(&["one more"]), &[], false),
            Err(EditProblem::TooMany)
        );
        // Taking one away and adding one stays within the limit.
        assert!(apply_edit(&many, &v(&["one more"]), &v(&["t0"]), false).is_ok());
        // Names of two-byte letters: 20 of them are over the stored limit.
        let long: Vec<String> = (0..20)
            .map(|i| format!("{}{i:0>2}", "\u{e9}".repeat(60)))
            .collect();
        assert_eq!(
            apply_edit(&long, &[], &[], false),
            Err(EditProblem::TooLong)
        );
    }

    #[test]
    fn none_some_all() {
        let items = vec![v(&["Red"]), v(&["Red", "Work"]), v(&["Blue"])];
        assert_eq!(have(&items, "red"), Have::Some);
        assert_eq!(have(&items, "Green"), Have::None);
        assert_eq!(have(&items[..2], "Red"), Have::All);
        assert_eq!(have(&[], "Red"), Have::None);
    }

    #[test]
    fn menu_list_has_colours_first_then_names() {
        let list = in_use(
            &v(&["zeta", "Alpha"]),
            &[("blue".into(), 3), ("Work".into(), 2), ("WORK".into(), 5)],
        );
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Blue", "Alpha", "Work", "zeta"]);
        assert_eq!(list[0].1, 3);
        assert_eq!(list[2].1, 5);
    }

    #[test]
    fn ratings() {
        assert_eq!(rating_parse(b"8"), Some(8));
        assert_eq!(rating_parse(b"10"), Some(10));
        assert_eq!(rating_parse(b"0"), Some(0));
        assert_eq!(rating_parse(b" 3\n"), Some(3));
        assert_eq!(rating_parse(b"11"), None);
        assert_eq!(rating_parse(b"-1"), None);
        assert_eq!(rating_parse(b"x"), None);
        assert_eq!(rating_parse(b""), None);
        assert_eq!(rating_encode(0), None);
        assert_eq!(rating_encode(7), Some(b"7".to_vec()));
        assert_eq!(rating_encode(200), Some(b"10".to_vec()));
        assert_eq!(rating_stars(7), 3.5);
        assert_eq!(rating_from_stars(4.0), 8);
        assert_eq!(rating_from_stars(4.5), 9);
        assert_eq!(rating_from_stars(9.0), 10);
        assert_eq!(rating_from_stars(f32::NAN), 0);
    }
}
