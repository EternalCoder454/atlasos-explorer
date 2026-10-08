//! Search inside files: looking for words in one file's content, with no Qt
//! and no index. Nothing is kept: a file is read once, line by line, as it is
//! asked, and only the first matching line is remembered, as a snippet that is
//! safe to show. Text files are read directly; a PDF's text comes from
//! `pdftotext`, run by argument list (never a shell), killed when it takes too
//! long or the search is stopped. Binary files are recognised by a NUL in
//! their first 8 KiB and left alone. The folders are walked by
//! `atlas_file_index::walk::walk_content`; the limits are here. See
//! docs/DESIGN.md, "Search inside files".

use crate::display::push_visible;
use crate::pattern::{Pattern, PatternError};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A text file larger than this is not searched (it is counted and reported).
pub const MAX_TEXT_BYTES: u64 = 4 << 20;
/// A PDF larger than this is not searched.
pub const MAX_PDF_BYTES: u64 = 50 << 20;
/// How much of a PDF's text is looked at.
pub const MAX_PDF_TEXT: u64 = 4 << 20;
/// How long `pdftotext` may take for one PDF.
pub const PDF_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest line looked at; the rest of a longer line is skipped.
pub const MAX_LINE: usize = 64 << 10;
/// Files with a matching line one search lists at most.
pub const MAX_CONTENT_HITS: usize = 2000;
/// A search ends after reading this much, or after this long.
pub const MAX_TOTAL_BYTES: u64 = 4 << 30;
pub const MAX_SECONDS: u64 = 300;
/// How much of a file decides whether it is text.
const SNIFF: usize = 8192;
/// Most characters of a snippet (before the ellipses).
const SNIPPET_BYTES: usize = 200;
/// Matching lines counted after the first, at most.
const MAX_EXTRA: u32 = 9999;

/// The line shown while Inside Files is on.
pub const NOTE: &str = "Looks inside text files and PDFs for these words. Nothing is stored.";

/// What to look for.
#[derive(Clone, Debug)]
pub struct ContentQuery {
    pat: Pattern,
}

impl ContentQuery {
    /// `text` as typed: the words taken literally, or, with `pattern`, as a
    /// regular expression. Case is ignored. Empty text asks for nothing and
    /// is refused by the caller (this returns an error for it).
    pub fn new(text: &str, pattern: bool) -> Result<ContentQuery, PatternError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(PatternError("Type the words to look for.".into()));
        }
        let pat = if pattern {
            Pattern::new(text)?
        } else {
            Pattern::literal(text)?
        };
        Ok(ContentQuery { pat })
    }
}

/// The first matching line of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Its number, from 1.
    pub line: u32,
    /// The line around the match, safe to show (controls and bidi marks made
    /// visible), at most about 200 bytes.
    pub snippet: String,
    /// Further lines that match (counted to 9999).
    pub more: u32,
}

/// The outcome of looking through one file's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scan {
    Match(Found),
    NoMatch,
    /// A NUL in the first 8 KiB: not text.
    Binary,
    /// The search was stopped while the file was read.
    Stopped,
}

/// Reads `r` as text, line by line, for the query. `sniff` says to decide
/// first whether it is text (a file); a PDF's text from `pdftotext` is text
/// already.
pub fn scan<R: Read>(r: R, q: &ContentQuery, stop: &AtomicBool, sniff: bool) -> io::Result<Scan> {
    let mut r = BufReader::with_capacity(64 << 10, r);
    if sniff {
        let head = r.fill_buf()?;
        if head[..head.len().min(SNIFF)].contains(&0) {
            return Ok(Scan::Binary);
        }
    }
    let mut line: Vec<u8> = Vec::new();
    let mut number: u32 = 0;
    let mut found: Option<Found> = None;
    while read_line(&mut r, &mut line)? {
        number = number.saturating_add(1);
        if number.is_multiple_of(2048) && stop.load(Ordering::Relaxed) {
            return Ok(found.map_or(Scan::Stopped, Scan::Match));
        }
        let text = line.strip_suffix(b"\r").unwrap_or(&line);
        let Some((s, e)) = q.pat.find(text) else {
            continue;
        };
        match &mut found {
            None => {
                found = Some(Found {
                    line: number,
                    snippet: snippet(text, s, e),
                    more: 0,
                });
            }
            Some(f) => {
                f.more = (f.more + 1).min(MAX_EXTRA);
                if f.more >= MAX_EXTRA {
                    break;
                }
            }
        }
    }
    Ok(found.map_or(Scan::NoMatch, Scan::Match))
}

/// Reads up to the next newline into `buf` (without it), keeping at most
/// [`MAX_LINE`] bytes of a longer line. False at the end of the input.
fn read_line<R: BufRead>(r: &mut R, buf: &mut Vec<u8>) -> io::Result<bool> {
    buf.clear();
    let mut any = false;
    loop {
        let chunk = r.fill_buf()?;
        if chunk.is_empty() {
            return Ok(any);
        }
        any = true;
        let (used, done) = match memchr(b'\n', chunk) {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        let take = if done { used - 1 } else { used };
        let room = MAX_LINE.saturating_sub(buf.len());
        buf.extend_from_slice(&chunk[..take.min(room)]);
        r.consume(used);
        if done {
            return Ok(true);
        }
    }
}

fn memchr(needle: u8, hay: &[u8]) -> Option<usize> {
    hay.iter().position(|&b| b == needle)
}

/// The text around a match: about 200 bytes with the match in it, cut on
/// character boundaries, surrounding space trimmed, `…` where it was cut, and
/// every control or bidi character made visible.
fn snippet(line: &[u8], start: usize, _end: usize) -> String {
    let mut s = start.saturating_sub(60);
    let mut e = (s + SNIPPET_BYTES).min(line.len());
    // Never in the middle of a UTF-8 sequence.
    while s > 0 && s < line.len() && (line[s] & 0xC0) == 0x80 {
        s -= 1;
    }
    while e < line.len() && (line[e] & 0xC0) == 0x80 {
        e += 1;
    }
    let cut_front = s > 0;
    let cut_back = e < line.len();
    let text = String::from_utf8_lossy(&line[s..e]);
    let mut out = String::with_capacity(text.len() + 6);
    if cut_front {
        out.push('\u{2026}');
    }
    let mut seen_text = false;
    let mut pending_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            pending_space = seen_text;
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        seen_text = true;
        push_visible(&mut out, c);
    }
    if cut_back {
        out.push('\u{2026}');
    }
    out
}

// ---- PDF ----

/// `pdftotext`, when the system has one on its PATH.
pub fn pdftotext() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let path = std::env::var_os("PATH")?;
            std::env::split_paths(&path)
                .filter(|d| d.is_absolute())
                .map(|d| d.join("pdftotext"))
                .find(|p| is_executable_file(p))
        })
        .as_deref()
}

fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The outcome of a PDF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pdf {
    Scanned(Scan),
    /// `pdftotext` couldn't read it (damaged, locked) or couldn't run.
    Failed,
    /// `pdftotext` took longer than [`PDF_TIMEOUT`].
    TimedOut,
    /// This system has no `pdftotext`.
    NoTool,
}

/// Looks through a PDF's text. `pdftotext` is run as
/// `pdftotext -q -enc UTF-8 -nopgbrk <absolute path> -` with no shell, no
/// input, no environment and no error output; its text is read through a cap
/// and it is killed at once when the search is stopped, the time is up or a
/// match has been found.
pub fn scan_pdf(path: &Path, q: &ContentQuery, stop: &AtomicBool) -> Pdf {
    let Some(exe) = pdftotext() else {
        return Pdf::NoTool;
    };
    // An absolute path never reads as an option.
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(d) => d.join(path),
            Err(_) => return Pdf::Failed,
        }
    };
    let mut cmd = Command::new(exe);
    cmd.args(["-q", "-enc", "UTF-8", "-nopgbrk"])
        .arg(&abs)
        .arg("-")
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let Ok(mut child) = cmd.spawn() else {
        return Pdf::Failed;
    };
    let Some(out) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Pdf::Failed;
    };
    let child = Mutex::new(child);
    let done = AtomicBool::new(false);
    let timed_out = AtomicBool::new(false);
    let (result, status) = std::thread::scope(|scope| {
        // The watchdog: ends the child when told to stop or when time is up.
        scope.spawn(|| {
            let started = Instant::now();
            while !done.load(Ordering::Relaxed) {
                let stopped = stop.load(Ordering::Relaxed);
                if stopped || started.elapsed() >= PDF_TIMEOUT {
                    timed_out.store(!stopped, Ordering::Relaxed);
                    if let Ok(mut c) = child.lock() {
                        let _ = c.kill();
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let result = scan(out.take(MAX_PDF_TEXT), q, stop, false);
        done.store(true, Ordering::Relaxed);
        // A match, a cap or a stop: the rest of the text is not wanted.
        let status = match child.lock() {
            Ok(mut c) => {
                let _ = c.kill();
                c.wait().ok()
            }
            Err(_) => None,
        };
        (result, status)
    });
    if timed_out.load(Ordering::Relaxed) {
        return Pdf::TimedOut;
    }
    match result {
        Ok(Scan::Match(f)) => Pdf::Scanned(Scan::Match(f)),
        Ok(Scan::Stopped) => Pdf::Scanned(Scan::Stopped),
        Ok(Scan::NoMatch) => {
            // pdftotext ends at once with an error for a file it can't read;
            // killed by us after a normal end, the status is a signal's.
            let failed = status.is_some_and(|s| s.code().is_some_and(|c| c != 0));
            if failed {
                Pdf::Failed
            } else {
                Pdf::Scanned(Scan::NoMatch)
            }
        }
        Ok(Scan::Binary) | Err(_) => Pdf::Failed,
    }
}

// ---- The summary line ----

/// What one search looked at and left out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Files whose content was read.
    pub searched: u32,
    /// Files that are not text.
    pub binary: u32,
    /// Text files over [`MAX_TEXT_BYTES`] and PDFs over [`MAX_PDF_BYTES`].
    pub too_large: u32,
    /// PDFs, with no `pdftotext` to read them.
    pub pdf_no_tool: u32,
    /// PDFs that took too long.
    pub pdf_timeout: u32,
    /// Files that couldn't be opened or read, and PDFs `pdftotext` refused.
    pub unreadable: u32,
}

/// How a search ended, for the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    Running,
    Done,
    Stopped,
    /// [`MAX_CONTENT_HITS`] files were listed.
    TooMany,
    /// [`MAX_TOTAL_BYTES`] or [`MAX_SECONDS`] was reached.
    Limit,
    /// The folder couldn't be read.
    Unreadable,
}

fn files(n: u32, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The line under the search: how many files hold the words, and what was
/// left out and why.
pub fn summary(found: usize, ending: Ending, stats: &Stats) -> String {
    let head = match (ending, found) {
        (Ending::Running, 0) => "Searching inside files, nothing found yet".to_string(),
        (Ending::Running, n) => format!("Searching inside files, {n} found"),
        (Ending::Stopped, n) => format!("Stopped, {n} found"),
        (Ending::TooMany, n) => format!("The first {n} files (type more to narrow them)"),
        (Ending::Limit, n) => format!("Stopped at the time or size limit, {n} found"),
        (Ending::Unreadable, _) => "Can't look through this folder".to_string(),
        (Ending::Done, 0) => "No file contains it".to_string(),
        (Ending::Done, 1) => "1 file contains it".to_string(),
        (Ending::Done, n) => format!("{n} files contain it"),
    };
    let mut left: Vec<String> = Vec::new();
    if stats.too_large > 0 {
        left.push(format!(
            "{} over {} MiB",
            files(stats.too_large, "file", "files"),
            MAX_TEXT_BYTES >> 20
        ));
    }
    if stats.pdf_no_tool > 0 {
        left.push(format!(
            "{} (pdftotext isn't installed)",
            files(stats.pdf_no_tool, "PDF", "PDFs")
        ));
    }
    if stats.pdf_timeout > 0 {
        left.push(format!(
            "{} that took too long",
            files(stats.pdf_timeout, "PDF", "PDFs")
        ));
    }
    if stats.unreadable > 0 {
        left.push(format!(
            "{} that couldn't be read",
            files(stats.unreadable, "file", "files")
        ));
    }
    if stats.binary > 0 {
        left.push(files(stats.binary, "binary file", "binary files"));
    }
    if left.is_empty() || ending == Ending::Running {
        head
    } else {
        format!("{head}. Left out: {}", left.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn q(text: &str) -> ContentQuery {
        ContentQuery::new(text, false).unwrap()
    }
    fn run(data: &[u8], query: &ContentQuery) -> Scan {
        scan(
            Cursor::new(data.to_vec()),
            query,
            &AtomicBool::new(false),
            true,
        )
        .unwrap()
    }

    #[test]
    fn finds_the_first_line_and_counts_the_rest() {
        let data = b"alpha\nthe Quick brown fox\nbeta\nquick again\nQUICK\n";
        let Scan::Match(f) = run(data, &q("quick")) else {
            panic!()
        };
        assert_eq!((f.line, f.more), (2, 2));
        assert_eq!(f.snippet, "the Quick brown fox");
        assert_eq!(run(data, &q("slow")), Scan::NoMatch);
        // The words are taken literally, case ignored.
        let Scan::Match(f) = run(b"price (10+5) [x]\n", &q("(10+5)")) else {
            panic!()
        };
        assert_eq!(f.line, 1);
    }

    #[test]
    fn a_pattern_matches_lines() {
        let query = ContentQuery::new(r"fo[xz]\s+jumps", true).unwrap();
        assert!(matches!(run(b"a\nthe fox  jumps\n", &query), Scan::Match(f) if f.line == 2));
        assert_eq!(run(b"the fix jumps\n", &query), Scan::NoMatch);
        assert!(ContentQuery::new("(oops", true).is_err());
        assert!(ContentQuery::new("   ", false).is_err());
    }

    #[test]
    fn binary_files_are_left_alone() {
        let mut data = b"looks like text then".to_vec();
        data.push(0);
        data.extend_from_slice(b"needle");
        assert_eq!(run(&data, &q("needle")), Scan::Binary);
        // Not sniffed (a PDF's text): the same bytes are read.
        let r = scan(
            Cursor::new(data),
            &q("needle"),
            &AtomicBool::new(false),
            false,
        )
        .unwrap();
        assert!(matches!(r, Scan::Match(_)));
    }

    #[test]
    fn crlf_and_missing_final_newline() {
        let Scan::Match(f) = run(b"one\r\ntwo needle\r\nthree", &q("needle")) else {
            panic!()
        };
        assert_eq!((f.line, f.snippet.as_str()), (2, "two needle"));
        assert!(matches!(run(b"first\nlast needle", &q("needle")), Scan::Match(f) if f.line == 2));
        assert_eq!(run(b"", &q("x")), Scan::NoMatch);
    }

    #[test]
    fn snippets_are_safe_and_short() {
        let hostile = "\u{1b}[31mred\u{7} needle \u{202E}evil\u{202C}\t<b>bold</b> &amp;\u{200B}";
        let Scan::Match(f) = run(hostile.as_bytes(), &q("needle")) else {
            panic!()
        };
        assert!(!f.snippet.chars().any(char::is_control), "{:?}", f.snippet);
        assert!(!f.snippet.contains('\u{202E}') && !f.snippet.contains('\u{200B}'));
        // Markup stays text.
        assert!(f.snippet.contains("<b>bold</b> &amp;"));
        // A long line is cut around the match, with ellipses.
        let long = format!("{}needle{}", "x ".repeat(5000), "y ".repeat(5000));
        let Scan::Match(f) = run(long.as_bytes(), &q("needle")) else {
            panic!()
        };
        assert!(f.snippet.starts_with('\u{2026}') && f.snippet.ends_with('\u{2026}'));
        assert!(
            f.snippet.contains("needle") && f.snippet.chars().count() < 260,
            "{}",
            f.snippet.chars().count()
        );
        // Multi-byte text is cut on character boundaries.
        let wide = format!("{}needle{}", "é".repeat(300), "ü".repeat(300));
        let Scan::Match(f) = run(wide.as_bytes(), &q("needle")) else {
            panic!()
        };
        assert!(f.snippet.contains("needle") && !f.snippet.contains('\u{FFFD}'));
        // Invalid UTF-8 is shown as the replacement character, not trusted.
        let Scan::Match(f) = run(b"bad \xFF\xFE needle", &q("needle")) else {
            panic!()
        };
        assert!(f.snippet.ends_with("needle"));
    }

    #[test]
    fn an_enormous_line_is_cut_not_stored() {
        // 3 MB on one line: only the first 64 KiB is looked at, so a match
        // beyond it is not found, and the next line still is.
        let mut data = vec![b'x'; 3_000_000];
        data.extend_from_slice(b" needle\nsecond needle line\n");
        let Scan::Match(f) = run(&data, &q("needle")) else {
            panic!()
        };
        assert_eq!(f.line, 2);
        // A match inside the first 64 KiB of a long line is found.
        let mut data = b"needle ".to_vec();
        data.extend(std::iter::repeat_n(b'x', 3_000_000));
        assert!(matches!(run(&data, &q("needle")), Scan::Match(f) if f.line == 1));
    }

    #[test]
    fn the_summary_says_what_was_left_out() {
        let st = Stats {
            too_large: 1,
            binary: 12,
            pdf_no_tool: 2,
            ..Stats::default()
        };
        let t = summary(3, Ending::Done, &st);
        assert!(
            t.starts_with("3 files contain it. Left out: 1 file over 4 MiB, 2 PDFs"),
            "{t}"
        );
        assert!(t.ends_with("12 binary files"), "{t}");
        assert_eq!(
            summary(0, Ending::Done, &Stats::default()),
            "No file contains it"
        );
        assert_eq!(
            summary(1, Ending::Done, &Stats::default()),
            "1 file contains it"
        );
        assert_eq!(
            summary(0, Ending::Running, &st),
            "Searching inside files, nothing found yet"
        );
        assert_eq!(
            summary(4, Ending::Stopped, &Stats::default()),
            "Stopped, 4 found"
        );
        assert!(summary(2000, Ending::TooMany, &Stats::default()).starts_with("The first 2000"));
    }

    #[test]
    fn stop_ends_a_long_read() {
        let data = "line\n".repeat(100_000);
        let stop = AtomicBool::new(true);
        let r = scan(Cursor::new(data.into_bytes()), &q("zzz"), &stop, true).unwrap();
        assert_eq!(r, Scan::Stopped);
    }

    #[test]
    fn extra_matches_are_counted_up_to_a_cap() {
        let data = "needle\n".repeat(20_000);
        let Scan::Match(f) = run(data.as_bytes(), &q("needle")) else {
            panic!()
        };
        assert_eq!((f.line, f.more), (1, MAX_EXTRA));
    }

    /// A one-page PDF with real text and a correct cross-reference table.
    fn pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT /F1 24 Tf 40 150 Td ({text}) Tj ET");
        let objs: Vec<String> = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".into(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offs = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offs.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let x = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes(),
        );
        for o in offs {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn pdf_text_is_searched_with_pdftotext_when_there_is_one() {
        let dir = std::env::temp_dir().join(format!("telamon-content-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("doc.pdf");
        std::fs::write(&file, pdf("Telamon invoice number 4711")).unwrap();
        let stop = AtomicBool::new(false);
        match pdftotext() {
            None => {
                assert_eq!(scan_pdf(&file, &q("invoice"), &stop), Pdf::NoTool);
                eprintln!("pdftotext is not installed: PDF text not exercised");
            }
            Some(_) => {
                let Pdf::Scanned(Scan::Match(f)) = scan_pdf(&file, &q("INVOICE number"), &stop)
                else {
                    panic!("no match in the PDF")
                };
                assert!(f.snippet.contains("invoice number 4711"), "{}", f.snippet);
                assert_eq!(
                    scan_pdf(&file, &q("nothing like this"), &stop),
                    Pdf::Scanned(Scan::NoMatch)
                );
                // A file that is not a PDF is a failure, not a hang or a panic.
                let bad = dir.join("bad.pdf");
                std::fs::write(&bad, b"not a pdf at all").unwrap();
                assert_eq!(scan_pdf(&bad, &q("x"), &stop), Pdf::Failed);
                // A name that looks like an option is a path.
                let dash = dir.join("-q.pdf");
                std::fs::write(&dash, pdf("dash needle")).unwrap();
                assert!(matches!(
                    scan_pdf(&dash, &q("needle"), &stop),
                    Pdf::Scanned(Scan::Match(_))
                ));
            }
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
