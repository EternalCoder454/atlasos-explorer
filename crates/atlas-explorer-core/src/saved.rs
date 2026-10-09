//! Saved searches: the sidebar's "Saved Searches". A saved search is a name
//! and what the search field and its row held when it was saved: the words,
//! where to look (This Folder, with the folder, or Everywhere), the Kind,
//! Modified and Size chips, a tag, and the two switches (Use pattern, Search
//! inside files). Nothing else: no results, no file names. The list is kept
//! in Files' own settings file as text, one search a line; this module reads
//! it as untrusted input (a hand-edited or damaged file gives the searches
//! that are fine and drops the rest), writes it, and adds, renames and
//! removes. The settings file is `telamon-explorerrc`, group
//! `[SavedSearches]`, key `Items` (`cpp/kio/SavedLogic.*`). See
//! docs/DESIGN.md, "Saved searches".
//!
//! A line is the fields separated by tabs, each one percent-encoded where it
//! holds `%` or a control character (anything `%XX` decodes, so a window may
//! encode every character): `id name query scope folder kind modified size
//! tag pattern contents`.

use crate::display::display_name;
use std::fmt::Write;

/// Most saved searches.
pub const MAX_SAVED: usize = 50;
/// Longest name, in characters.
pub const MAX_NAME_CHARS: usize = 80;
/// Longest words, folder and tag, in bytes.
pub const MAX_QUERY_BYTES: usize = 512;
pub const MAX_FOLDER_BYTES: usize = 2048;
pub const MAX_TAG_BYTES: usize = 200;

const FIELDS: usize = 11;
/// The largest id a line may carry.
const MAX_ID: u32 = 1_000_000;

/// One saved search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub id: u32,
    pub name: String,
    pub query: String,
    /// 0 This Folder, 1 Everywhere.
    pub scope: u8,
    /// The folder of a This Folder search, as a URL; empty for Everywhere.
    pub folder: String,
    /// The chips (0 is "any"): `search::Kind`, `Modified`, `SizeClass`.
    pub kind: u8,
    pub modified: u8,
    pub size: u8,
    pub tag: String,
    pub pattern: bool,
    pub contents: bool,
}

/// What `add` and `rename` say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// No name, or nothing to search for.
    Invalid,
    /// There are [`MAX_SAVED`] already.
    Full,
    /// No saved search has that id.
    Missing,
}

/// The list, in the order shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedList {
    items: Vec<Saved>,
}

impl SavedList {
    pub fn items(&self) -> &[Saved] {
        &self.items
    }

    pub fn get(&self, id: u32) -> Option<&Saved> {
        self.items.iter().find(|s| s.id == id)
    }

    /// Reads the settings text; whatever is wrong with a line drops that line.
    pub fn parse(text: &str) -> SavedList {
        let mut list = SavedList::default();
        for line in text.lines() {
            if list.items.len() >= MAX_SAVED {
                break;
            }
            let Some(item) = parse_line(line) else {
                continue;
            };
            if list.items.iter().any(|s| s.id == item.id) {
                continue;
            }
            list.items.push(item);
        }
        list
    }

    /// The text to keep.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for s in &self.items {
            out.push_str(&line_of(s));
            out.push('\n');
        }
        out
    }

    /// Adds a search at the end; its id is a number no other saved search has.
    /// `s.id` is ignored. Returns the id.
    pub fn add(&mut self, s: Saved) -> Result<u32, Outcome> {
        if self.items.len() >= MAX_SAVED {
            return Err(Outcome::Full);
        }
        let Some(mut item) = clean(s) else {
            return Err(Outcome::Invalid);
        };
        let id = self.items.iter().map(|s| s.id).max().map_or(1, |m| m + 1);
        // A number past what the text accepts again would be dropped on the
        // next read, with everything saved after it.
        if id > MAX_ID {
            return Err(Outcome::Full);
        }
        item.id = id;
        self.items.push(item);
        Ok(id)
    }

    pub fn rename(&mut self, id: u32, name: &str) -> Outcome {
        let Some(name) = clean_name(name) else {
            return Outcome::Invalid;
        };
        match self.items.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                s.name = name;
                Outcome::Done
            }
            None => Outcome::Missing,
        }
    }

    pub fn remove(&mut self, id: u32) -> Outcome {
        match self.items.iter().position(|s| s.id == id) {
            Some(i) => {
                self.items.remove(i);
                Outcome::Done
            }
            None => Outcome::Missing,
        }
    }
}

/// A name as it is kept: no control or bidi character, runs of space as one,
/// at most [`MAX_NAME_CHARS`]; `None` when nothing is left.
pub fn clean_name(name: &str) -> Option<String> {
    let shown: String = name
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    // Runs of space are one space before the name is made safe to show (a
    // long run would be marked there).
    let collapsed: String = shown
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let shown = display_name(collapsed.as_str());
    let mut out = String::new();
    let mut space = false;
    for c in shown.chars() {
        if c == ' ' {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    let out: String = out.chars().take(MAX_NAME_CHARS).collect();
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// A saved search made fit to keep. `None` when it has no name, or looks for
/// nothing (no words, no chip, no tag).
fn clean(mut s: Saved) -> Option<Saved> {
    s.name = clean_name(&s.name)?;
    // Too long to keep whole is refused: a cut folder or words would search somewhere else.
    s.query = s.query.trim().to_string();
    s.folder = s.folder.trim().to_string();
    s.tag = s.tag.trim().to_string();
    if s.query.len() > MAX_QUERY_BYTES
        || s.folder.len() > MAX_FOLDER_BYTES
        || s.tag.len() > MAX_TAG_BYTES
    {
        return None;
    }
    s.scope = u8::from(s.scope == 1);
    s.kind = s.kind.min(7);
    s.modified = s.modified.min(4);
    s.size = s.size.min(3);
    if s.scope != 0 {
        s.folder.clear();
    }
    s.query.retain(|c| c != '\0');
    // This Folder needs its folder.
    if s.scope == 0 && s.folder.is_empty() {
        return None;
    }
    // The folder is a location, read under the launch rules (the settings
    // file is untrusted): a scheme Files opens, no password, no control or
    // direction characters, even percent-encoded.
    if !s.folder.is_empty() {
        let l = crate::launch::parse(std::slice::from_ref(&s.folder), std::path::Path::new("/"));
        match (l.locations.as_slice(), l.refused.is_empty(), l.dropped) {
            ([one], true, 0) => s.folder = one.clone(),
            _ => return None,
        }
    }
    let asks =
        !s.query.is_empty() || s.kind != 0 || s.modified != 0 || s.size != 0 || !s.tag.is_empty();
    asks.then_some(s)
}

// ---- The text ----

fn encode(out: &mut String, field: &str) {
    for c in field.chars() {
        if c == '%' || c.is_control() || c == '\u{2028}' || c == '\u{2029}' {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                let _ = write!(out, "%{b:02X}");
            }
        } else {
            out.push(c);
        }
    }
}

fn decode(field: &str) -> Option<String> {
    let b = field.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = field.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn line_of(s: &Saved) -> String {
    let mut out = String::new();
    let _ = write!(out, "{}\t", s.id);
    encode(&mut out, &s.name);
    out.push('\t');
    encode(&mut out, &s.query);
    let _ = write!(out, "\t{}\t", s.scope);
    encode(&mut out, &s.folder);
    let _ = write!(out, "\t{}\t{}\t{}\t", s.kind, s.modified, s.size);
    encode(&mut out, &s.tag);
    let _ = write!(out, "\t{}\t{}", u8::from(s.pattern), u8::from(s.contents));
    out
}

/// The ten fields after the id, decoded but not yet checked.
fn from_fields(f: &[&str], id: u32) -> Option<Saved> {
    let num = |s: &str| s.parse::<u8>().ok();
    Some(Saved {
        id,
        name: decode(f[0])?,
        query: decode(f[1])?,
        scope: num(f[2])?,
        folder: decode(f[3])?,
        kind: num(f[4])?,
        modified: num(f[5])?,
        size: num(f[6])?,
        tag: decode(f[7])?,
        pattern: num(f[8])? == 1,
        contents: num(f[9])? == 1,
    })
}

fn parse_line(line: &str) -> Option<Saved> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() < FIELDS {
        return None;
    }
    // Ids are small numbers; a huge one (a damaged file) would overflow the next.
    let id: u32 = f[0].parse().ok().filter(|&n| n > 0 && n <= MAX_ID)?;
    clean(from_fields(&f[1..FIELDS], id)?).map(|mut s| {
        s.id = id;
        s
    })
}

/// A search that is about to be saved, as the window hands it over: the line
/// without its id (`name query scope folder kind modified size tag pattern
/// contents`, tab-separated, each field percent-encoded). Not yet checked:
/// `add` does that.
pub fn parse_record(record: &str) -> Option<Saved> {
    let f: Vec<&str> = record.split('\t').collect();
    if f.len() < FIELDS - 1 {
        return None;
    }
    from_fields(&f[..FIELDS - 1], 0)
}

// ---- Texts ----

/// A name to offer for a search that is being saved: the words, or the chips
/// when there are none.
pub fn default_name(s: &Saved) -> String {
    let kinds = [
        "",
        "Documents",
        "Images",
        "Audio",
        "Video",
        "Archives",
        "Code",
        "Folders",
    ];
    let periods = ["", "Today", "Past 7 Days", "Past Month", "Past Year"];
    let sizes = ["", "Small", "Medium", "Large"];
    let mut parts: Vec<String> = Vec::new();
    let q = s.query.trim();
    if !q.is_empty() {
        parts.push(if s.contents {
            format!("Inside files: {q}")
        } else {
            q.to_string()
        });
    }
    if let Some(k) = kinds.get(usize::from(s.kind)).filter(|k| !k.is_empty()) {
        parts.push((*k).to_string());
    }
    if let Some(m) = periods
        .get(usize::from(s.modified))
        .filter(|k| !k.is_empty())
    {
        parts.push((*m).to_string());
    }
    if let Some(z) = sizes.get(usize::from(s.size)).filter(|k| !k.is_empty()) {
        parts.push((*z).to_string());
    }
    if !s.tag.is_empty() {
        parts.push(format!("Tag {}", s.tag));
    }
    clean_name(&parts.join(" \u{B7} ")).unwrap_or_else(|| "Saved Search".into())
}

/// What a saved search does, in a line for its tooltip. `folder_label` is the
/// saved folder as the window writes places.
pub fn describe(s: &Saved, folder_label: &str) -> String {
    let kinds = [
        "",
        "Documents",
        "Images",
        "Audio",
        "Video",
        "Archives",
        "Code",
        "Folders",
    ];
    let periods = ["", "Today", "Past 7 days", "Past month", "Past year"];
    let sizes = ["", "Small files", "Medium files", "Large files"];
    let mut parts: Vec<String> = Vec::new();
    let q = display_name(s.query.trim());
    if !q.is_empty() {
        let how = match (s.contents, s.pattern) {
            (true, true) => "Pattern inside files",
            (true, false) => "Inside files",
            (false, true) => "Pattern",
            (false, false) => "Words",
        };
        parts.push(format!("{how}: {q}"));
    }
    for (list, v) in [
        (&kinds[..], s.kind),
        (&periods[..], s.modified),
        (&sizes[..], s.size),
    ] {
        if let Some(x) = list.get(usize::from(v)).filter(|x| !x.is_empty()) {
            parts.push((*x).to_string());
        }
    }
    if !s.tag.is_empty() {
        parts.push(format!("Tag: {}", display_name(&s.tag)));
    }
    parts.push(if s.scope == 1 {
        "Everywhere".into()
    } else {
        format!("In {}", display_name(folder_label))
    });
    parts.join(" \u{B7} ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, query: &str) -> Saved {
        Saved {
            name: name.into(),
            query: query.into(),
            scope: 1,
            ..Saved::default()
        }
    }

    #[test]
    fn add_rename_remove_and_the_ids_stay_put() {
        let mut l = SavedList::default();
        let a = l.add(s("Reports", "report")).unwrap();
        let b = l.add(s("Photos", "")).unwrap_err();
        assert_eq!(b, Outcome::Invalid, "a search for nothing is refused");
        let mut photos = s("Photos", "");
        photos.kind = 2;
        let b = l.add(photos).unwrap();
        assert_ne!(a, b);
        assert_eq!(l.rename(a, "  Old   reports  "), Outcome::Done);
        assert_eq!(l.get(a).unwrap().name, "Old reports");
        assert_eq!(l.rename(a, "   "), Outcome::Invalid);
        assert_eq!(l.rename(99, "x"), Outcome::Missing);
        assert_eq!(l.remove(a), Outcome::Done);
        assert_eq!(l.remove(a), Outcome::Missing);
        // A removed id is not reused while a larger one exists.
        let c = l.add(s("Again", "again")).unwrap();
        assert!(c > b);
        assert_eq!(l.items().len(), 2);
    }

    #[test]
    fn the_folder_of_a_saved_search_is_a_location_files_opens() {
        let mk = |folder: &str| {
            let mut x = s("In a folder", "word");
            x.scope = 0;
            x.folder = folder.into();
            x
        };
        let mut l = SavedList::default();
        assert!(l.add(mk("file:///home/u/Docs")).is_ok());
        assert_eq!(l.items()[0].folder, "file:///home/u/Docs");
        assert!(l.add(mk("smb://nas/share")).is_ok());
        for bad in [
            "admin:///",
            "smb://u:p@h/",
            "file:///a%0Ab",
            "file:///a\u{202E}b",
            "sftp://-oProxyCommand=x@h/",
            "http://example.com/",
        ] {
            let r = l.add(mk(bad));
            // a password is taken out, the rest is refused (a relative name has no folder to read it from)
            if bad == "smb://u:p@h/" {
                assert!(r.is_ok());
                assert_eq!(l.items().last().unwrap().folder, "smb://u@h/");
            } else {
                assert_eq!(r, Err(Outcome::Invalid), "{bad}");
            }
        }
    }

    #[test]
    fn an_id_the_text_would_refuse_is_not_handed_out() {
        let mut l = SavedList::default();
        let mut x = s("Last", "one");
        x.id = MAX_ID;
        l.items.push(x);
        assert_eq!(l.add(s("Next", "two")), Err(Outcome::Full));
        assert_eq!(SavedList::parse(&l.to_text()).items().len(), 1);
    }

    #[test]
    fn the_list_survives_being_written_and_read() {
        let mut l = SavedList::default();
        let mut x = s("Invoices", "total due");
        x.scope = 0;
        x.folder = "file:///home/u/Documents/My%20Folder".into();
        x.kind = 1;
        x.modified = 3;
        x.size = 2;
        x.tag = "Work".into();
        x.pattern = true;
        x.contents = true;
        l.add(x.clone()).unwrap();
        l.add(s("Pics", "holiday")).unwrap();
        let text = l.to_text();
        assert_eq!(text.lines().count(), 2);
        let back = SavedList::parse(&text);
        assert_eq!(back, l);
        x.id = 1;
        assert_eq!(back.get(1).unwrap(), &x);
        // Written again, the same text.
        assert_eq!(back.to_text(), text);
    }

    #[test]
    fn hostile_text_is_kept_as_text_and_cannot_break_the_format() {
        let mut l = SavedList::default();
        let mut x = s(
            "na\tme\nwith\u{202E}marks %41",
            "q\tuery\r\nline2 %41 \u{7}",
        );
        x.tag = "t\ta\ng".into();
        let id = l.add(x).unwrap();
        let name = l.get(id).unwrap().name.clone();
        assert!(
            !name.chars().any(|c| c.is_control() || c == '\u{202E}'),
            "{name:?}"
        );
        let text = l.to_text();
        assert_eq!(text.lines().count(), 1, "one line a search: {text:?}");
        assert_eq!(
            text.lines().next().unwrap().matches('\t').count(),
            FIELDS - 1
        );
        let back = SavedList::parse(&text);
        assert_eq!(back.items().len(), 1);
        // The words come back as they were, controls included (they are the user's).
        assert_eq!(back.items()[0].query, l.items()[0].query);
    }

    #[test]
    fn a_damaged_file_gives_the_good_searches() {
        let good = "1\tOne\tone\t1\t\t0\t0\t0\t\t0\t0\n";
        let text = format!(
            "{good}garbage\n\n2\tonly\ttwo\tfields\n3\t\tq\t1\t\t0\t0\t0\t\t0\t0\n\
             1\tDuplicate id\tdup\t1\t\t0\t0\t0\t\t0\t0\n\
             4\tBad escape\t%ZZ\t1\t\t0\t0\t0\t\t0\t0\n\
             5\tNo folder\tx\t0\t\t0\t0\t0\t\t0\t0\n\
             6\tFine\tsix\t1\t\t99\t99\t99\t\t1\t1\n\
             x\tBad id\tq\t1\t\t0\t0\t0\t\t0\t0\n\
             0\tZero id\tq\t1\t\t0\t0\t0\t\t0\t0\n"
        );
        let l = SavedList::parse(&text);
        let ids: Vec<u32> = l.items().iter().map(|s| s.id).collect();
        assert_eq!(ids, [1, 6]);
        assert_eq!(l.get(1).unwrap().name, "One");
        // Out-of-range chips are clamped, not trusted.
        let six = l.get(6).unwrap();
        assert_eq!((six.kind, six.modified, six.size), (7, 4, 3));
        assert!(SavedList::parse("").items().is_empty());
        assert!(SavedList::parse("\u{0}\u{1}\u{2}").items().is_empty());
        // Invalid UTF-8 never gets this far (the settings file is text), but
        // an escape that decodes to it is dropped.
        assert!(
            SavedList::parse("1\tN\t%FF\t1\t\t0\t0\t0\t\t0\t0\n")
                .items()
                .is_empty()
        );
    }

    #[test]
    fn there_is_a_limit() {
        let mut l = SavedList::default();
        for i in 0..MAX_SAVED {
            l.add(s(&format!("S{i}"), &format!("q{i}"))).unwrap();
        }
        assert_eq!(l.add(s("One more", "x")), Err(Outcome::Full));
        // A file with more than the limit is cut.
        let mut text = l.to_text();
        text.push_str("999\tExtra\tx\t1\t\t0\t0\t0\t\t0\t0\n");
        assert_eq!(SavedList::parse(&text).items().len(), MAX_SAVED);
        // Long names and words are cut.
        let mut m = SavedList::default();
        let id = m
            .add(s(&"n".repeat(500), &"w".repeat(MAX_QUERY_BYTES)))
            .unwrap();
        assert_eq!(m.get(id).unwrap().name.chars().count(), MAX_NAME_CHARS);
        assert_eq!(m.get(id).unwrap().query, "w".repeat(MAX_QUERY_BYTES));
        // Words or a folder too long to keep whole are refused, not cut.
        assert_eq!(
            m.add(s("Long", &"w".repeat(MAX_QUERY_BYTES + 1))),
            Err(Outcome::Invalid)
        );
        let mut far = s("Far", "x");
        far.scope = 0;
        far.folder = format!("file:///{}", "d".repeat(MAX_FOLDER_BYTES));
        assert_eq!(m.add(far), Err(Outcome::Invalid));
        // A damaged id cannot overflow the next one.
        let huge = SavedList::parse("4294967295\tHuge\tq\t1\t\t0\t0\t0\t\t0\t0\n");
        assert!(huge.items().is_empty());
    }

    #[test]
    fn this_folder_needs_its_folder() {
        let mut x = s("Here", "x");
        x.scope = 0;
        assert_eq!(SavedList::default().add(x.clone()), Err(Outcome::Invalid));
        x.folder = "file:///home/u".into();
        assert!(SavedList::default().add(x.clone()).is_ok());
        // Everywhere has no folder.
        x.scope = 1;
        let mut l = SavedList::default();
        let id = l.add(x).unwrap();
        assert!(l.get(id).unwrap().folder.is_empty());
    }

    #[test]
    fn a_record_from_the_window_is_read_as_given() {
        let r = "Big%20pics\tholiday%09x\t0\tfile%3A%2F%2F%2Fhome%2Fu\t2\t3\t1\tWork\t1\t0";
        let x = parse_record(r).unwrap();
        assert_eq!(x.name, "Big pics");
        assert_eq!(x.query, "holiday\tx");
        assert_eq!((x.scope, x.kind, x.modified, x.size), (0, 2, 3, 1));
        assert_eq!(x.folder, "file:///home/u");
        assert!(x.pattern && !x.contents);
        let mut l = SavedList::default();
        let id = l.add(x).unwrap();
        assert_eq!(l.get(id).unwrap().query, "holiday\tx");
        assert!(parse_record("too\tfew").is_none());
        assert!(
            parse_record("n\tq\t0\tf\tx\t0\t0\t\t0\t0").is_none(),
            "a chip that is not a number"
        );
    }

    #[test]
    fn names_and_descriptions() {
        let mut x = s("", "invoice");
        x.kind = 1;
        x.modified = 2;
        assert_eq!(
            default_name(&x),
            "invoice \u{B7} Documents \u{B7} Past 7 Days"
        );
        x.contents = true;
        assert!(default_name(&x).starts_with("Inside files: invoice"));
        let only_chips = Saved {
            kind: 2,
            size: 3,
            scope: 1,
            ..Saved::default()
        };
        assert_eq!(default_name(&only_chips), "Images \u{B7} Large");
        assert_eq!(default_name(&Saved::default()), "Saved Search");
        x.scope = 0;
        x.folder = "file:///h".into();
        let d = describe(&x, "~/Documents");
        assert!(
            d.contains("Inside files: invoice") && d.ends_with("In ~/Documents"),
            "{d}"
        );
        assert!(describe(&s("n", "a"), "").ends_with("Everywhere"));
    }
}
