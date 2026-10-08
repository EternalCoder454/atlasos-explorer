//! Quick Look and the preview pane, with no Qt: what a file is for a preview
//! (from its MIME type), how its text is read and made safe to show, and how
//! the details are written. File content is untrusted like a file name is:
//! the reader opens only regular files, reads at most [`TEXT_CAP`] bytes, and
//! shows control, bidi and invisible characters as markers, never as they
//! are. See docs/DESIGN.md, "Quick Look".

use crate::display::push_visible;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Bytes of a text file that are read and shown (256 KiB).
pub const TEXT_CAP: usize = 256 * 1024;
/// Characters of one line that are shown; the rest of the line is cut with `…`.
pub const MAX_LINE_CHARS: usize = 2000;
/// Bytes at the start of a file that decide whether it is text.
const SNIFF_BYTES: usize = 8192;

/// What a preview shows for a file. The numbers are the C++ enum's
/// (`PreviewLoader::Category`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// Nothing is chosen (or the item is gone).
    None = 0,
    Folder = 1,
    /// A picture: a thumbnail from KIO's thumbnailers (images, SVG, RAW, ...).
    Image = 2,
    Pdf = 3,
    Video = 4,
    Audio = 5,
    Font = 6,
    /// Office and e-book documents: a thumbnail, when a thumbnailer has one.
    Document = 7,
    /// Plain text and source code: the first [`TEXT_CAP`] bytes as text.
    Text = 8,
    /// Anything else: its icon and its details.
    Other = 9,
}

/// The category of a MIME type name (`image/png`). A type nothing is known
/// about is [`Category::Other`]; the caller may still find it holds text.
pub fn classify_mime(mime: &str) -> Category {
    let mime = mime.trim().to_ascii_lowercase();
    let (top, sub) = mime.split_once('/').unwrap_or((mime.as_str(), ""));
    // A parameter (`text/plain; charset=utf-8`) is not part of the type.
    let sub = sub.split(';').next().unwrap_or("").trim();
    match top {
        "inode" if sub == "directory" => Category::Folder,
        "image" => Category::Image,
        "video" => Category::Video,
        "audio" => Category::Audio,
        "font" => Category::Font,
        "text" if matches!(sub, "rtf" | "richtext") => Category::Document,
        "text" => Category::Text,
        "application" => classify_application(sub),
        _ => Category::Other,
    }
}

fn classify_application(sub: &str) -> Category {
    if sub == "pdf" || sub == "x-pdf" {
        return Category::Pdf;
    }
    if sub.starts_with("x-font-")
        || sub.starts_with("font-")
        || matches!(
            sub,
            "x-font" | "vnd.ms-fontobject" | "font-woff" | "x-fontobject" | "x-truetype-font"
        )
    {
        return Category::Font;
    }
    // Documents a thumbnailer may know: OpenDocument, Office, e-books, RTF.
    if sub.starts_with("vnd.oasis.opendocument.")
        || sub.starts_with("vnd.openxmlformats-officedocument.")
        || sub.starts_with("vnd.ms-")
        || sub.starts_with("vnd.sun.xml.")
        || sub.starts_with("vnd.stardivision.")
        || matches!(
            sub,
            "msword"
                | "rtf"
                | "epub+zip"
                | "x-mobipocket-ebook"
                | "vnd.amazon.ebook"
                | "x-fictionbook+xml"
                | "x-abiword"
                | "x-kword"
                | "x-krita"
                | "x-kpresenter"
                | "x-kspread"
                | "vnd.comicbook+zip"
                | "vnd.comicbook-rar"
                | "x-cbz"
                | "x-cbr"
        )
    {
        return Category::Document;
    }
    if sub.ends_with("+xml") || sub.ends_with("+json") || sub.ends_with("+yaml") {
        return Category::Text;
    }
    if matches!(
        sub,
        "json"
            | "xml"
            | "javascript"
            | "x-javascript"
            | "ecmascript"
            | "x-ecmascript"
            | "x-ndjson"
            | "x-yaml"
            | "yaml"
            | "toml"
            | "x-toml"
            | "sql"
            | "x-sql"
            | "x-sh"
            | "x-shellscript"
            | "x-csh"
            | "x-awk"
            | "x-perl"
            | "x-php"
            | "x-httpd-php"
            | "x-ruby"
            | "x-python"
            | "x-python-code"
            | "x-lua"
            | "x-tcl"
            | "x-desktop"
            | "x-subrip"
            | "x-wine-extension-ini"
            | "x-java"
            | "x-ms-dos-executable-script"
            | "x-latex"
            | "x-tex"
            | "x-bibtex"
            | "x-markdown"
            | "x-wiki"
            | "x-m4"
            | "x-cue"
            | "mbox"
            | "x-mbox"
            | "x-ipynb+json"
            | "x-gdscript"
            | "x-gettext-translation"
            | "x-theme"
            | "x-kicad-project"
            | "x-meson"
            | "x-cmake"
            | "x-ninja"
            | "x-rust"
            | "x-go"
            | "x-zig"
    ) {
        return Category::Text;
    }
    Category::Other
}

/// The outcome of [`read_text`]. The numbers are the C++ side's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextOutcome {
    Text = 0,
    /// The start of the file is not text (a NUL byte, or mostly controls and
    /// invalid UTF-8).
    Binary = 1,
    /// Not a regular file once opened (a pipe, a device, a socket, a folder).
    NotRegular = 2,
    /// It could not be opened or read.
    Unreadable = 3,
}

/// What [`read_text`] found.
#[derive(Debug, PartialEq, Eq)]
pub struct TextPreview {
    pub outcome: TextOutcome,
    /// The text to show: safe, see [`sanitize_text`]. Empty unless `Text`.
    pub text: String,
    /// The file has more than was read (or than fits in `max_out` bytes).
    pub truncated: bool,
}

impl TextPreview {
    fn none(outcome: TextOutcome) -> Self {
        TextPreview {
            outcome,
            text: String::new(),
            truncated: false,
        }
    }
}

/// Reads at most `cap` bytes from the start of the file at `path` and turns
/// them into text that is safe to show.
///
/// Nothing dangerous is opened: the file is opened without blocking and
/// without becoming a controlling terminal, and when it is open it must be a
/// regular file (a symlink to one is followed; a pipe, a device or a socket is
/// refused, so a read can never wait for a writer or run endlessly). The
/// result is at most `max_out` bytes of text.
pub fn read_text(path: &Path, cap: usize, max_out: usize) -> TextPreview {
    let opened = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path);
    let file = match opened {
        Ok(f) => f,
        Err(_) => return TextPreview::none(TextOutcome::Unreadable),
    };
    match file.metadata() {
        Ok(m) if m.is_file() => {}
        Ok(_) => return TextPreview::none(TextOutcome::NotRegular),
        Err(_) => return TextPreview::none(TextOutcome::Unreadable),
    }
    let mut raw = Vec::with_capacity(cap.min(64 * 1024) + 1);
    // One byte more than the cap says whether there is more.
    if file.take(cap as u64 + 1).read_to_end(&mut raw).is_err() {
        return TextPreview::none(TextOutcome::Unreadable);
    }
    let mut truncated = raw.len() > cap;
    raw.truncate(cap);
    if looks_binary(&raw) {
        return TextPreview::none(TextOutcome::Binary);
    }
    let mut text = sanitize_text(&raw, truncated);
    if text.len() > max_out {
        let mut end = max_out;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        truncated = true;
    }
    TextPreview {
        outcome: TextOutcome::Text,
        text,
        truncated,
    }
}

fn utf16_bom(raw: &[u8]) -> Option<bool> {
    match raw {
        [0xFF, 0xFE, ..] => Some(true),
        [0xFE, 0xFF, ..] => Some(false),
        _ => None,
    }
}

/// Whether the start of a file is not text: a NUL byte (UTF-16 text with its
/// byte order mark excepted), or more than a tenth of the first bytes being
/// controls other than tab, line breaks, form feed and escape, or a third
/// being invalid UTF-8.
pub fn looks_binary(raw: &[u8]) -> bool {
    let sample = &raw[..raw.len().min(SNIFF_BYTES)];
    if sample.is_empty() || utf16_bom(sample).is_some() {
        return false;
    }
    if sample.contains(&0) {
        return true;
    }
    let controls = sample
        .iter()
        .filter(|&&b| (b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0C | 0x1B)) || b == 0x7F)
        .count();
    if controls * 10 > sample.len() {
        return true;
    }
    // Invalid bytes, counted the way `from_utf8` finds them. A cut at the end
    // of the sample is not invalid.
    let mut invalid = 0usize;
    let mut rest = sample;
    while let Err(e) = std::str::from_utf8(rest) {
        let after = e.valid_up_to();
        match e.error_len() {
            Some(n) => {
                invalid += n;
                rest = &rest[after + n..];
            }
            None => break,
        }
    }
    invalid * 3 > sample.len()
}

/// Bytes as text that is safe to show: UTF-8 (or UTF-16 with a byte order
/// mark), `\r\n` as a line break, tabs kept, every other control, bidi or
/// invisible character as a visible marker (the ones file names get), invalid
/// bytes as U+FFFD, and lines cut at [`MAX_LINE_CHARS`] with `…`. With
/// `cut` the input ended where a cap stopped it, so a character cut in half
/// at the end is dropped.
pub fn sanitize_text(raw: &[u8], cut: bool) -> String {
    let decoded: String = match utf16_bom(raw) {
        Some(little) => {
            let units = raw[2..].chunks_exact(2).map(|p| {
                if little {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            });
            char::decode_utf16(units)
                .map(|r| r.unwrap_or('\u{FFFD}'))
                .collect()
        }
        None => decode_utf8(
            raw.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(raw),
            cut,
        ),
    };
    let mut out = String::with_capacity(decoded.len());
    let mut line_chars = 0usize;
    let mut skipping = false;
    let mut chars = decoded.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => {
                out.push('\n');
                line_chars = 0;
                skipping = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            _ if skipping => {}
            _ => {
                if line_chars >= MAX_LINE_CHARS {
                    out.push('\u{2026}');
                    skipping = true;
                    continue;
                }
                if c == '\t' {
                    out.push('\t');
                } else {
                    push_visible(&mut out, c);
                }
                line_chars += 1;
            }
        }
    }
    out
}

/// UTF-8 with invalid bytes as U+FFFD; with `cut`, an unfinished character at
/// the very end is left out.
fn decode_utf8(raw: &[u8], cut: bool) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&rest[..valid]).unwrap_or(""));
                match e.error_len() {
                    Some(n) => {
                        out.push('\u{FFFD}');
                        rest = &rest[valid + n..];
                    }
                    None => {
                        if !cut {
                            out.push('\u{FFFD}');
                        }
                        return out;
                    }
                }
            }
        }
    }
}

/// A length of time for the details: `0:07`, `3:25`, `1:02:03`. Whole
/// seconds, rounded down.
pub fn format_duration(ms: u64) -> String {
    let total = ms / 1000;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Pixel dimensions for the details: `4000 × 3000`. Empty when either is not
/// known (zero).
pub fn format_dimensions(width: u32, height: u32) -> String {
    if width == 0 || height == 0 {
        String::new()
    } else {
        format!("{width} \u{00D7} {height}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            let d = std::env::temp_dir().join(format!(
                "telamon-preview-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).unwrap();
            Dir(d)
        }
        fn file(&self, name: &str, data: &[u8]) -> PathBuf {
            let p = self.0.join(name);
            fs::write(&p, data).unwrap();
            p
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn classifies_by_mime_type() {
        use Category::*;
        let cases = [
            ("inode/directory", Folder),
            ("image/png", Image),
            ("image/svg+xml", Image),
            ("image/x-xcf", Image),
            ("application/pdf", Pdf),
            ("video/mp4", Video),
            ("audio/flac", Audio),
            ("font/ttf", Font),
            ("application/x-font-ttf", Font),
            ("application/vnd.ms-fontobject", Font),
            ("application/vnd.oasis.opendocument.text", Document),
            (
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                Document,
            ),
            ("application/msword", Document),
            ("application/rtf", Document),
            ("text/rtf", Document),
            ("application/epub+zip", Document),
            ("text/plain", Text),
            ("text/x-rust", Text),
            ("text/html", Text),
            ("application/json", Text),
            ("application/x-shellscript", Text),
            ("application/xhtml+xml", Text),
            ("application/octet-stream", Other),
            ("application/zip", Other),
            ("application/x-executable", Other),
            ("", Other),
            ("nonsense", Other),
        ];
        for (mime, want) in cases {
            assert_eq!(classify_mime(mime), want, "{mime}");
        }
        assert_eq!(classify_mime("TEXT/Plain; charset=utf-8"), Text);
    }

    #[test]
    fn reads_plain_text_and_stops_at_the_cap() {
        let d = Dir::new("cap");
        let p = d.file("a.txt", b"hello\nworld\n");
        let t = read_text(&p, TEXT_CAP, 1 << 20);
        assert_eq!(t.outcome, TextOutcome::Text);
        assert_eq!(t.text, "hello\nworld\n");
        assert!(!t.truncated);

        // Exactly at the cap is not truncated; one byte more is.
        let at = d.file("at.txt", &vec![b'x'; TEXT_CAP]);
        let t = read_text(&at, TEXT_CAP, 1 << 20);
        // one long line: cut to the line limit, plus the 3-byte ellipsis
        assert_eq!((t.text.len(), t.truncated), (MAX_LINE_CHARS + 3, false));
        let over = d.file("over.txt", &vec![b'x'; TEXT_CAP + 1]);
        let t = read_text(&over, TEXT_CAP, 1 << 20);
        assert!(t.truncated);
        assert_eq!(t.outcome, TextOutcome::Text);

        // A big file is read only as far as the cap.
        let many: Vec<u8> = b"line\n"
            .iter()
            .copied()
            .cycle()
            .take(5 * TEXT_CAP)
            .collect();
        let big = d.file("big.txt", &many);
        let t = read_text(&big, TEXT_CAP, 1 << 20);
        assert!(t.truncated);
        assert!(t.text.len() <= TEXT_CAP);
        assert!(t.text.starts_with("line\nline\n"));
    }

    #[test]
    fn output_is_limited_to_what_the_caller_has_room_for() {
        let d = Dir::new("maxout");
        let p = d.file("a.txt", "ä".repeat(100).as_bytes());
        let t = read_text(&p, TEXT_CAP, 11);
        assert_eq!(t.text, "ä".repeat(5));
        assert!(t.truncated);
        assert!(t.text.len() <= 11);
    }

    #[test]
    fn a_cut_in_the_middle_of_a_character_is_dropped() {
        let d = Dir::new("cut");
        let p = d.file("a.txt", "abc\u{20AC}".as_bytes());
        // cap 5 ends inside the three-byte euro sign
        let t = read_text(&p, 5, 100);
        assert_eq!(t.text, "abc");
        assert!(t.truncated);
        // but the same bytes at the true end of a file are an error mark
        assert_eq!(sanitize_text(&[b'a', 0xE2, 0x82], false), "a\u{FFFD}");
    }

    #[test]
    fn binary_files_are_refused() {
        let d = Dir::new("bin");
        let mut with_nul = b"MZ\x90\x00\x03".to_vec();
        with_nul.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let p = d.file("a.exe", &with_nul);
        assert_eq!(
            read_text(&p, TEXT_CAP, 1 << 20).outcome,
            TextOutcome::Binary
        );
        // many controls, no NUL
        let ctl = d.file(
            "ctl",
            &[
                1u8, 2, 3, 4, 5, 6, 7, 8, b'a', 1, 2, 3, 4, 5, 6, 7, 8, 9, 14,
            ],
        );
        assert_eq!(
            read_text(&ctl, TEXT_CAP, 1 << 20).outcome,
            TextOutcome::Binary
        );
        // mostly invalid UTF-8
        let junk = d.file(
            "junk",
            &[0xC3, 0x28, 0xA0, 0xA1, 0xE2, 0x28, 0xA1, 0xFF, 0xFE, 0xFD],
        );
        assert_eq!(
            read_text(&junk, TEXT_CAP, 1 << 20).outcome,
            TextOutcome::Binary
        );
        // the empty file is empty text
        let empty = d.file("empty", b"");
        let t = read_text(&empty, TEXT_CAP, 1 << 20);
        assert_eq!((t.outcome, t.text.as_str()), (TextOutcome::Text, ""));
        // a little Latin-1 in otherwise good text is still text
        let mixed = d.file(
            "mixed",
            b"caf\xE9 au lait, with enough plain text around the one bad byte\n",
        );
        assert_eq!(
            read_text(&mixed, TEXT_CAP, 1 << 20).outcome,
            TextOutcome::Text
        );
    }

    #[test]
    fn utf16_with_a_byte_order_mark_is_text() {
        let mut le = vec![0xFF, 0xFE];
        for u in "h\u{E9}llo\r\nw".encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        let d = Dir::new("u16");
        let p = d.file("le.txt", &le);
        let t = read_text(&p, TEXT_CAP, 1 << 20);
        assert_eq!(
            (t.outcome, t.text.as_str()),
            (TextOutcome::Text, "h\u{E9}llo\nw")
        );
        let mut be = vec![0xFE, 0xFF];
        for u in "ok".encode_utf16() {
            be.extend_from_slice(&u.to_be_bytes());
        }
        assert_eq!(sanitize_text(&be, false), "ok");
    }

    #[test]
    fn unsafe_characters_are_never_in_the_text() {
        let s = sanitize_text(
            "a\u{202E}b\u{200B}c\x1b[31m\u{7}d\u{85}e\u{FEFF}f\0g".as_bytes(),
            false,
        );
        for bad in [
            '\u{202E}', '\u{200B}', '\x1b', '\u{7}', '\u{85}', '\u{FEFF}', '\0',
        ] {
            assert!(!s.contains(bad), "{:04X} in {s:?}", bad as u32);
        }
        assert!(s.contains("U+202E") && s.contains("U+200B") && s.contains("U+0085"));
        assert!(s.contains('\u{241B}') && s.contains('\u{2407}'));
        // tabs and line breaks stay, a CR before LF goes, a lone CR is marked
        assert_eq!(
            sanitize_text(b"a\tb\r\nc\rd\n", false),
            "a\tb\nc\u{240D}d\n"
        );
        // a byte order mark at the start is not text
        assert_eq!(sanitize_text(b"\xEF\xBB\xBFhi", false), "hi");
        // markup is just characters
        assert_eq!(
            sanitize_text(b"<b>bold</b> &amp;", false),
            "<b>bold</b> &amp;"
        );
    }

    #[test]
    fn long_lines_are_cut() {
        let line = "x".repeat(MAX_LINE_CHARS + 500);
        let s = sanitize_text(format!("{line}\nnext\n").as_bytes(), false);
        let first = s.lines().next().unwrap();
        assert_eq!(first.chars().count(), MAX_LINE_CHARS + 1);
        assert!(first.ends_with('\u{2026}'));
        assert!(s.ends_with("\nnext\n"));
        // exactly the limit is kept whole
        let exact = "y".repeat(MAX_LINE_CHARS);
        assert_eq!(sanitize_text(exact.as_bytes(), false), exact);
    }

    #[test]
    fn only_regular_files_are_read() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let d = Dir::new("special");
        // a folder
        assert_eq!(
            read_text(&d.0, TEXT_CAP, 1 << 20).outcome,
            TextOutcome::NotRegular
        );
        // a named pipe with nobody writing: refused at once, not waited for
        let fifo = d.0.join("pipe");
        let c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `c` is a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert_eq!(
            read_text(&fifo, TEXT_CAP, 1 << 20).outcome,
            TextOutcome::NotRegular
        );
        // a device that never ends
        assert_eq!(
            read_text(Path::new("/dev/zero"), TEXT_CAP, 1 << 20).outcome,
            TextOutcome::NotRegular
        );
        // a symlink to a regular file is followed, one to a device is not
        let target = d.file("t.txt", b"target\n");
        std::os::unix::fs::symlink(&target, d.0.join("link")).unwrap();
        let t = read_text(&d.0.join("link"), TEXT_CAP, 1 << 20);
        assert_eq!(t.text, "target\n");
        std::os::unix::fs::symlink("/dev/zero", d.0.join("zero")).unwrap();
        assert_eq!(
            read_text(&d.0.join("zero"), TEXT_CAP, 1 << 20).outcome,
            TextOutcome::NotRegular
        );
        // a missing file
        assert_eq!(
            read_text(&d.0.join("none"), TEXT_CAP, 1 << 20).outcome,
            TextOutcome::Unreadable
        );
    }

    #[test]
    fn durations_and_dimensions_are_written_for_people() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(7_999), "0:07");
        assert_eq!(format_duration(205_000), "3:25");
        assert_eq!(format_duration(3_723_000), "1:02:03");
        assert_eq!(format_dimensions(4000, 3000), "4000 \u{00D7} 3000");
        assert_eq!(format_dimensions(0, 3000), "");
    }
}
