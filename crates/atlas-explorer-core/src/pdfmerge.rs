//! Joining PDF files into one ("Combine into PDF"), in the order given.
//!
//! Done in this process with `lopdf`: no program is run, and nothing is
//! trusted in the inputs. Every file is checked before it is read (size,
//! header, not password-protected, at least one page, bounded page and
//! decompression counts), and the page tree of each is flattened into the new
//! one with the properties pages inherit (resources, media box, crop box,
//! rotation) written into each page. What belongs to the document as a whole
//! (outline, named destinations, forms, structure) is left behind: the result
//! is the pages, as they look, and their own annotations.

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The most files one PDF is made of.
pub const MAX_FILES: usize = 500;
/// The most pages in the result.
pub const MAX_PAGES: usize = 5_000;
/// The largest single PDF read.
pub const MAX_FILE_BYTES: u64 = 256 << 20;
/// The most all the inputs hold together.
pub const MAX_TOTAL_BYTES: u64 = 1 << 30;
/// What one compressed stream may inflate to while a file is read.
const MAX_INFLATE: usize = 256 << 20;

/// Why a merge stopped. `input` is the position (from 0) of the file at fault.
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    /// Not a PDF, or damaged beyond reading.
    Unreadable,
    /// Protected with a password: Files does not take protection off.
    Protected,
    /// A PDF with no page.
    NoPages,
    /// Too big (one file, or all together).
    TooBig,
    /// More pages than one result may hold.
    TooManyPages,
    /// The result could not be written.
    CannotWrite,
    /// The user stopped it.
    Cancelled,
    /// Nothing was given.
    NothingToDo,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MergeError {
    pub input: Option<usize>,
    pub failure: Failure,
}

impl MergeError {
    fn at(input: usize, failure: Failure) -> MergeError {
        MergeError {
            input: Some(input),
            failure,
        }
    }
    fn plain(failure: Failure) -> MergeError {
        MergeError {
            input: None,
            failure,
        }
    }

    /// The number the app gets (0 is success).
    pub fn code(&self) -> i32 {
        match self.failure {
            Failure::Unreadable => 1,
            Failure::Protected => 2,
            Failure::NoPages => 3,
            Failure::TooBig => 4,
            Failure::TooManyPages => 5,
            Failure::CannotWrite => 6,
            Failure::Cancelled => 7,
            Failure::NothingToDo => 8,
        }
    }
}

/// What a page may take from the page tree above it.
const INHERITED: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Writes into `page` what it has not itself and an ancestor has.
fn inherit(doc: &Document, page: &mut Dictionary) {
    for key in INHERITED {
        if page.has(key) {
            continue;
        }
        let mut seen = HashSet::new();
        let mut at = page.get(b"Parent").and_then(Object::as_reference).ok();
        while let Some(id) = at {
            if !seen.insert(id) || seen.len() > 64 {
                break;
            }
            let Ok(node) = doc.get_dictionary(id) else {
                break;
            };
            if let Ok(value) = node.get(key) {
                page.set(key.to_vec(), value.clone());
                break;
            }
            at = node.get(b"Parent").and_then(Object::as_reference).ok();
        }
    }
}

/// Whether the start of a file is a PDF's (the header may follow a few bytes
/// of junk, as readers allow).
fn has_header(file: &mut File) -> bool {
    let mut head = [0u8; 1024];
    let mut n = 0;
    while n < head.len() {
        match file.read(&mut head[n..]) {
            Ok(0) | Err(_) => break,
            Ok(k) => n += k,
        }
    }
    head[..n].windows(5).any(|w| w == b"%PDF-")
}

/// Whether the file says it is protected (looked for when it can't be read).
fn says_encrypted(path: &Path) -> bool {
    let Ok(mut f) = File::open(path) else {
        return false;
    };
    let mut buf = Vec::new();
    // The trailer is at the end; a protected file names /Encrypt there.
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len > 4096 {
        use std::io::{Seek, SeekFrom};
        if f.seek(SeekFrom::Start(len - 4096)).is_err() {
            return false;
        }
    }
    let _ = f.take(4096).read_to_end(&mut buf);
    buf.windows(8).any(|w| w == b"/Encrypt")
}

/// Joins the PDFs `inputs` into `output`, in that order; returns the number of
/// pages. `cancel` is looked at between files. The output is created (or
/// replaced) by this call: the caller names a new file.
pub fn merge(inputs: &[PathBuf], output: &Path, cancel: &AtomicBool) -> Result<usize, MergeError> {
    if inputs.is_empty() {
        return Err(MergeError::plain(Failure::NothingToDo));
    }
    if inputs.len() > MAX_FILES {
        return Err(MergeError::plain(Failure::TooBig));
    }
    // Sizes first: nothing is read if the total is already too much.
    let mut total = 0u64;
    for (n, p) in inputs.iter().enumerate() {
        let meta = std::fs::metadata(p).map_err(|_| MergeError::at(n, Failure::Unreadable))?;
        if !meta.is_file() {
            return Err(MergeError::at(n, Failure::Unreadable));
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(MergeError::at(n, Failure::TooBig));
        }
        total += meta.len();
    }
    if total > MAX_TOTAL_BYTES {
        return Err(MergeError::plain(Failure::TooBig));
    }

    let mut max_id = 1u32;
    let mut kids: Vec<ObjectId> = Vec::new();
    let mut pages: BTreeMap<ObjectId, Dictionary> = BTreeMap::new();
    let mut rest: BTreeMap<ObjectId, Object> = BTreeMap::new();

    for (n, path) in inputs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(MergeError::plain(Failure::Cancelled));
        }
        let mut file = File::open(path).map_err(|_| MergeError::at(n, Failure::Unreadable))?;
        if !has_header(&mut file) {
            return Err(MergeError::at(n, Failure::Unreadable));
        }
        drop(file);
        let options = LoadOptions {
            max_decompressed_size: Some(MAX_INFLATE),
            ..LoadOptions::default()
        };
        let mut doc = match Document::load_with_options(path, options) {
            Ok(d) => d,
            Err(_) if says_encrypted(path) => return Err(MergeError::at(n, Failure::Protected)),
            Err(_) => return Err(MergeError::at(n, Failure::Unreadable)),
        };
        if doc.is_encrypted() || doc.was_encrypted() {
            return Err(MergeError::at(n, Failure::Protected));
        }
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;

        let ids: Vec<ObjectId> = doc.page_iter().take(MAX_PAGES + 1).collect();
        if ids.is_empty() {
            return Err(MergeError::at(n, Failure::NoPages));
        }
        if kids.len() + ids.len() > MAX_PAGES {
            return Err(MergeError::at(n, Failure::TooManyPages));
        }
        for id in &ids {
            let Ok(dict) = doc.get_dictionary(*id) else {
                return Err(MergeError::at(n, Failure::Unreadable));
            };
            let mut page = dict.clone();
            inherit(&doc, &mut page);
            page.set("Type", "Page");
            // A page can be named twice by a damaged tree; it is one page here.
            if pages.insert(*id, page).is_none() {
                kids.push(*id);
            }
        }
        for (id, object) in std::mem::take(&mut doc.objects) {
            // The document's own structure is not carried over.
            match object.type_name().unwrap_or(b"") {
                b"Catalog" | b"Pages" | b"Outlines" | b"Outline" | b"Page" => {}
                _ => {
                    rest.insert(id, object);
                }
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(MergeError::plain(Failure::Cancelled));
    }

    let mut out = Document::with_version("1.5");
    out.objects = rest;
    let pages_id: ObjectId = (max_id, 0);
    let catalog_id: ObjectId = (max_id + 1, 0);
    out.max_id = max_id + 1;
    for (id, mut page) in pages {
        page.set("Parent", pages_id);
        out.objects.insert(id, Object::Dictionary(page));
    }
    let count = kids.len();
    let mut tree = Dictionary::new();
    tree.set("Type", "Pages");
    tree.set(
        "Kids",
        kids.into_iter().map(Object::Reference).collect::<Vec<_>>(),
    );
    tree.set("Count", count as i64);
    out.objects.insert(pages_id, Object::Dictionary(tree));
    let mut catalog = Dictionary::new();
    catalog.set("Type", "Catalog");
    catalog.set("Pages", pages_id);
    out.objects.insert(catalog_id, Object::Dictionary(catalog));
    out.trailer.set("Root", catalog_id);
    // Objects that only the left-behind structure pointed to.
    out.prune_objects();

    let mut write = || -> std::io::Result<()> {
        let file = File::create(output)?;
        let mut w = BufWriter::new(file);
        out.save_to(&mut w)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        w.flush()?;
        w.into_inner().map_err(|e| e.into_error())?.sync_all()
    };
    write().map_err(|_| MergeError::plain(Failure::CannotWrite))?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::content::{Content, Operation};
    use lopdf::{Stream, dictionary};

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("telamon-pdfmerge-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn content_of(text: &str) -> Vec<u8> {
        Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 24.into()]),
                Operation::new("Td", vec![50.into(), 100.into()]),
                Operation::new("Tj", vec![Object::string_literal(text)]),
                Operation::new("ET", vec![]),
            ],
        }
        .encode()
        .unwrap()
    }

    /// A PDF with one page per text. `nested`: the pages hang under an
    /// intermediate node that holds the resources and the media box, as many
    /// writers make them.
    fn make(path: &Path, texts: &[&str], nested: bool, media: [i64; 4]) {
        let mut doc = Document::with_version("1.5");
        let root = doc.new_object_id();
        let mid = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
        });
        let res = dictionary! { "Font" => dictionary! { "F1" => font } };
        let mut kid_ids = Vec::new();
        for t in texts {
            let c = doc.add_object(Stream::new(Dictionary::new(), content_of(t)));
            let mut page = dictionary! {
                "Type" => "Page",
                "Parent" => if nested { mid } else { root },
                "Contents" => c,
            };
            if !nested {
                page.set("Resources", res.clone());
                page.set(
                    "MediaBox",
                    media
                        .iter()
                        .map(|v| Object::Integer(*v))
                        .collect::<Vec<_>>(),
                );
            }
            kid_ids.push(doc.add_object(page));
        }
        if nested {
            let mut node = dictionary! {
                "Type" => "Pages", "Parent" => root,
                "Kids" => kid_ids.iter().map(|k| Object::Reference(*k)).collect::<Vec<_>>(),
                "Count" => kid_ids.len() as i64,
            };
            node.set("Resources", res);
            node.set(
                "MediaBox",
                media
                    .iter()
                    .map(|v| Object::Integer(*v))
                    .collect::<Vec<_>>(),
            );
            doc.objects.insert(mid, Object::Dictionary(node));
            doc.objects.insert(
                root,
                Object::Dictionary(dictionary! {
                    "Type" => "Pages", "Kids" => vec![Object::Reference(mid)], "Count" => kid_ids.len() as i64,
                }),
            );
        } else {
            doc.objects.insert(
                root,
                Object::Dictionary(dictionary! {
                    "Type" => "Pages",
                    "Kids" => kid_ids.iter().map(|k| Object::Reference(*k)).collect::<Vec<_>>(),
                    "Count" => kid_ids.len() as i64,
                }),
            );
        }
        let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        doc.trailer.set("Root", cat);
        doc.save(path).unwrap();
    }

    /// The texts of the pages of a PDF, in page order.
    fn texts_of(path: &Path) -> Vec<String> {
        let doc = Document::load(path).unwrap();
        doc.page_iter()
            .map(|id| {
                let bytes = doc.get_page_content(id);
                let c = Content::decode(&bytes).unwrap();
                let mut s = String::new();
                for op in c.operations {
                    if op.operator == "Tj"
                        && let Some(Object::String(b, _)) = op.operands.first()
                    {
                        s.push_str(&String::from_utf8_lossy(b));
                    }
                }
                s
            })
            .collect()
    }

    #[test]
    fn pages_come_out_in_the_order_given_with_what_they_inherit() {
        let d = scratch("order");
        make(&d.join("a.pdf"), &["a1", "a2"], true, [0, 0, 300, 200]);
        make(&d.join("b.pdf"), &["b1"], false, [0, 0, 612, 792]);
        make(
            &d.join("c.pdf"),
            &["c1", "c2", "c3"],
            true,
            [0, 0, 100, 100],
        );
        let out = d.join("out.pdf");
        let no = AtomicBool::new(false);
        // b first, then a, then c: the order is the one given, not the names'.
        let n = merge(
            &[d.join("b.pdf"), d.join("a.pdf"), d.join("c.pdf")],
            &out,
            &no,
        )
        .unwrap();
        assert_eq!(n, 6);
        assert_eq!(texts_of(&out), ["b1", "a1", "a2", "c1", "c2", "c3"]);
        // The media box and resources of a page that took them from its parent are its own now.
        let doc = Document::load(&out).unwrap();
        let sizes: Vec<Vec<i64>> = doc
            .page_iter()
            .map(|id| {
                let p = doc.get_dictionary(id).unwrap();
                assert!(p.has(b"Resources"));
                p.get(b"MediaBox")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|o| o.as_i64().unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(sizes[0], [0, 0, 612, 792]);
        assert_eq!(sizes[1], [0, 0, 300, 200]);
        assert_eq!(sizes[3], [0, 0, 100, 100]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_same_file_twice_is_two_copies_of_its_pages() {
        let d = scratch("twice");
        make(&d.join("a.pdf"), &["x", "y"], false, [0, 0, 200, 200]);
        let out = d.join("out.pdf");
        let a = d.join("a.pdf");
        let n = merge(&[a.clone(), a], &out, &AtomicBool::new(false)).unwrap();
        assert_eq!(n, 4);
        assert_eq!(texts_of(&out), ["x", "y", "x", "y"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn bad_inputs_are_refused_in_words_and_nothing_is_written() {
        let d = scratch("bad");
        make(&d.join("good.pdf"), &["g"], false, [0, 0, 100, 100]);
        std::fs::write(d.join("text.pdf"), b"hello, not a pdf\n").unwrap();
        std::fs::write(d.join("empty.pdf"), b"").unwrap();
        let mut cut = std::fs::read(d.join("good.pdf")).unwrap();
        cut.truncate(cut.len() / 2);
        std::fs::write(d.join("cut.pdf"), &cut).unwrap();
        let out = d.join("out.pdf");
        let no = AtomicBool::new(false);
        let good = d.join("good.pdf");
        for (name, want) in [
            ("text.pdf", Failure::Unreadable),
            ("empty.pdf", Failure::Unreadable),
            ("cut.pdf", Failure::Unreadable),
            ("missing.pdf", Failure::Unreadable),
        ] {
            let e = merge(&[good.clone(), d.join(name)], &out, &no).unwrap_err();
            assert_eq!(e, MergeError::at(1, want), "{name}");
            assert!(!out.exists(), "{name} left a file");
        }
        // A folder is not a file.
        assert_eq!(
            merge(std::slice::from_ref(&d), &out, &no)
                .unwrap_err()
                .failure,
            Failure::Unreadable
        );
        assert_eq!(
            merge(&[], &out, &no).unwrap_err().failure,
            Failure::NothingToDo
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_protected_pdf_is_named_as_such() {
        let d = scratch("enc");
        make(&d.join("good.pdf"), &["g"], false, [0, 0, 100, 100]);
        // Real protection from lopdf itself.
        let mut doc = Document::load(d.join("good.pdf")).unwrap();
        doc.trailer.set(
            "ID",
            vec![
                Object::string_literal("0123456789abcdef"),
                Object::string_literal("0123456789abcdef"),
            ],
        );
        let version = lopdf::EncryptionVersion::V1 {
            document: &doc,
            owner_password: "owner",
            user_password: "user",
            permissions: lopdf::Permissions::all(),
        };
        let state = lopdf::EncryptionState::try_from(version).unwrap();
        doc.encrypt(&state).unwrap();
        doc.save(d.join("locked.pdf")).unwrap();
        let out = d.join("out.pdf");
        let e = merge(
            &[d.join("good.pdf"), d.join("locked.pdf")],
            &out,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(e, MergeError::at(1, Failure::Protected));
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn stopping_stops_before_anything_is_written() {
        let d = scratch("stop");
        make(&d.join("a.pdf"), &["a"], false, [0, 0, 100, 100]);
        let out = d.join("out.pdf");
        let e = merge(&[d.join("a.pdf")], &out, &AtomicBool::new(true)).unwrap_err();
        assert_eq!(e.failure, Failure::Cancelled);
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_cyclic_page_tree_does_not_hang() {
        let d = scratch("cycle");
        // Pages node that lists itself as a kid, and a parent loop.
        let mut doc = Document::with_version("1.5");
        let root = doc.new_object_id();
        let c = doc.add_object(Stream::new(Dictionary::new(), content_of("p")));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => root, "Contents" => c,
            "MediaBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
        });
        doc.objects.insert(
            root,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Parent" => root,
                "Kids" => vec![Object::Reference(root), Object::Reference(page)], "Count" => 1,
            }),
        );
        let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        doc.trailer.set("Root", cat);
        doc.save(d.join("loop.pdf")).unwrap();
        let out = d.join("out.pdf");
        let r = merge(&[d.join("loop.pdf")], &out, &AtomicBool::new(false));
        // Either a clean result of the one real page, or a plain refusal; never a hang or a panic.
        if let Ok(n) = r {
            assert_eq!(n, 1);
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
