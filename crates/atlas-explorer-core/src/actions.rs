//! Custom actions: the commands the user adds to "More Actions" in Settings.
//! An action is a name, a program, its arguments, the file types it is for and
//! whether to ask before it runs. This module keeps the list (as text, in
//! Files' settings file, `telamon-explorerrc`, group `[CustomActions]`, key
//! `Items`), checks an action as it is typed, and turns an action and the
//! selected items into **argument lists**.
//!
//! Nothing here, or after it, goes near a shell. The arguments are text typed
//! by the user, split into words by [`split_words`] (single quotes, double
//! quotes and a backslash group characters; nothing is ever expanded), and the
//! placeholders in the words are replaced by the items afterwards: a file name
//! becomes one argument whatever it holds (spaces, quotes, `;`, `$(...)`,
//! a leading `-`, a line break), and is never split, looked at or expanded
//! again. The program is run as `argv[0]` with those arguments by the caller
//! (`QProcess` with a list).
//!
//! Placeholders (KDE's set for service menus):
//!
//! | | |
//! |---|---|
//! | `%f` | the path of one file (the action runs once for each selected item) |
//! | `%F` | the paths of all the selected files, each as an argument of its own |
//! | `%u` | the URL of one item (once for each) |
//! | `%U` | the URLs of all the items, each as an argument of its own |
//! | `%d` | the folder that holds one item (once for each) |
//! | `%%` | a percent sign |
//!
//! Paths and URLs are absolute, so a name never looks like an option. `%F` and
//! `%U` stand alone as a word; the others may sit inside one (`--in=%f`).
//! `%f`, `%u` and `%d` cannot be mixed with `%F` or `%U`.

use crate::display::display_name;
use std::fmt::Write;
use std::path::{Path, PathBuf};

/// Most actions.
pub const MAX_ACTIONS: usize = 30;
/// Longest name, in characters.
pub const MAX_NAME_CHARS: usize = 60;
/// Longest program (a path or a command name) and arguments text, in bytes.
pub const MAX_PROGRAM_BYTES: usize = 1024;
pub const MAX_ARGS_BYTES: usize = 1024;
/// Most file-type patterns, and the longest one.
pub const MAX_TYPES: usize = 24;
pub const MAX_TYPE_BYTES: usize = 100;
/// Most processes one use of an action starts (an action with `%f` runs once
/// for each item).
pub const MAX_RUNS: usize = 20;
/// Most items one use of an action takes.
pub const MAX_ITEMS: usize = 500;

const FIELDS: usize = 6;
const MAX_ID: u32 = 1_000_000;

/// Programs that run a command line they are given, so a file name in the
/// arguments could become a command. They are refused as the program of an
/// action (the same goes for tools whose job is to run another program).
const REFUSED_PROGRAMS: &[&str] = &[
    "sh",
    "bash",
    "dash",
    "zsh",
    "fish",
    "ksh",
    "csh",
    "tcsh",
    "ash",
    "busybox",
    "env",
    "xargs",
    "sudo",
    "su",
    "doas",
    "pkexec",
    "nohup",
    "setsid",
    "timeout",
    "nice",
    "ionice",
    "script",
    "flatpak-spawn",
    "systemd-run",
    "runuser",
    "chroot",
    "unshare",
    "nsenter",
    "flock",
    "watch",
    "strace",
    "ltrace",
    "gdb",
    "exec",
    "eval",
    "command",
    "time",
    "stdbuf",
    "setpriv",
    "capsh",
    "taskset",
    "chrt",
    "parallel",
];

/// Interpreters that run the code they are given on the command line, and the
/// options that take it. A placeholder inside that code would make a file
/// name into a program, so it is refused there (a placeholder as a later
/// argument, which the code reads as data, is fine).
const INTERPRETERS: &[&str] = &[
    "python", "python2", "python3", "perl", "ruby", "node", "nodejs", "deno", "php", "lua",
    "luajit", "awk", "gawk", "mawk", "tclsh", "wish", "pwsh", "bun",
];
const CODE_OPTIONS: &[&str] = &[
    "-c",
    "-e",
    "-E",
    "-r",
    "-p",
    "--eval",
    "--command",
    "--exec",
    "--run",
];

/// One custom action.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Action {
    pub id: u32,
    pub name: String,
    /// An absolute path, or a command name looked for in `PATH`.
    pub program: String,
    /// The arguments as typed, with placeholders.
    pub args: String,
    /// MIME type patterns (`image/*`, `application/pdf`, `inode/directory`,
    /// `all/allfiles`); none means every item.
    pub types: Vec<String>,
    /// Ask before running.
    pub ask: bool,
}

/// What an operation on the list says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// No usable name, program, arguments or types (see [`problem`]).
    Invalid,
    /// There are [`MAX_ACTIONS`] already.
    Full,
    /// No action has that id.
    Missing,
}

/// Why an action cannot be kept or used, in plain words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    NoName,
    NoProgram,
    ProgramTooLong,
    /// A program that runs command lines (`sh`, `env`, `sudo`...).
    ProgramRefused(String),
    /// A placeholder inside the code an interpreter is told to run (`python -c '...%f...'`).
    PlaceholderInCode,
    /// A program name with a slash that is not a full path.
    ProgramNotAbsolute,
    ProgramMissing(String),
    ProgramNotExecutable(String),
    ArgsTooLong,
    /// A quote that is never closed.
    OpenQuote,
    /// A `%x` that is not a placeholder.
    UnknownPlaceholder(char),
    /// A `%` at the end of a word.
    LonePercent,
    /// `%F` or `%U` inside a longer word.
    ListNotAlone,
    /// `%f`, `%u` or `%d` together with `%F` or `%U`.
    MixedPlaceholders,
    BadTypes,
    /// A control character in a field.
    ControlCharacter,
}

impl Problem {
    pub fn text(&self) -> String {
        match self {
            Problem::NoName => "Type a name for the action.".into(),
            Problem::NoProgram => "Choose the program to run, or type its name.".into(),
            Problem::ProgramTooLong => "The program's name is too long.".into(),
            Problem::ProgramRefused(p) => format!(
                "\u{201c}{p}\u{201d} runs command lines, and a file name could become a command. Choose the program that does the job itself."
            ),
            Problem::PlaceholderInCode => "A file name can't go inside the code a program is told to run, because the name would become part of that code. Pass the file as a separate argument after the code.".into(),
            Problem::ProgramNotAbsolute => {
                "Type the full path of the program (it starts with /), or just its name.".into()
            }
            Problem::ProgramMissing(p) => format!("\u{201c}{p}\u{201d} was not found."),
            Problem::ProgramNotExecutable(p) => {
                format!("\u{201c}{p}\u{201d} can't be run: it is not an executable file.")
            }
            Problem::ArgsTooLong => "The arguments are too long.".into(),
            Problem::OpenQuote => "A quote in the arguments is never closed.".into(),
            Problem::UnknownPlaceholder(c) => format!(
                "%{c} is not a placeholder. Use %f, %F, %u, %U, %d, or %% for a percent sign."
            ),
            Problem::LonePercent => {
                "A % at the end of a word is not a placeholder. Type %% for a percent sign.".into()
            }
            Problem::ListNotAlone => {
                "%F and %U stand alone as a word, because each file becomes an argument of its own."
                    .into()
            }
            Problem::MixedPlaceholders => {
                "Use %f, %u and %d (one file at a time) or %F and %U (all the files at once), not both."
                    .into()
            }
            Problem::BadTypes => {
                "File types look like image/*, application/pdf or inode/directory.".into()
            }
            Problem::ControlCharacter => "A line break or control character is not allowed here.".into(),
        }
    }
}

// ---- The arguments ----

/// The words of an argument text. Words are separated by white space; a
/// single quote keeps everything up to the next one, a double quote keeps
/// everything up to the next one except that a backslash takes the next
/// character as it is, and a backslash outside quotes does the same. An empty
/// pair of quotes is an empty word. Nothing is expanded. `Err` for a quote
/// that is never closed or a backslash at the end.
pub fn split_words(text: &str) -> Result<Vec<String>, Problem> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if have {
                    words.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            '\'' => {
                have = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => cur.push(c),
                        None => return Err(Problem::OpenQuote),
                    }
                }
            }
            '"' => {
                have = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c) => cur.push(c),
                            None => return Err(Problem::OpenQuote),
                        },
                        Some(c) => cur.push(c),
                        None => return Err(Problem::OpenQuote),
                    }
                }
            }
            '\\' => {
                have = true;
                match chars.next() {
                    Some(c) => cur.push(c),
                    None => return Err(Problem::OpenQuote),
                }
            }
            c => {
                have = true;
                cur.push(c);
            }
        }
    }
    if have {
        words.push(cur);
    }
    Ok(words)
}

/// One piece of a word of the arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Text(String),
    /// `f`, `u` or `d`.
    One(char),
}

/// One word: text and one-item placeholders, or a list placeholder alone.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Word {
    Pieces(Vec<Piece>),
    /// `F` or `U`.
    List(char),
}

/// The arguments, split and checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    words: Vec<Word>,
}

impl Template {
    /// Whether the action runs once for each item (`%f`, `%u` or `%d`).
    pub fn per_item(&self) -> bool {
        self.words.iter().any(|w| match w {
            Word::Pieces(p) => p.iter().any(|p| matches!(p, Piece::One(_))),
            Word::List(_) => false,
        })
    }

    /// Whether it needs files on this computer (`%f`, `%F`, `%d`).
    pub fn needs_paths(&self) -> bool {
        self.words.iter().any(|w| match w {
            Word::Pieces(p) => p.iter().any(|p| matches!(p, Piece::One('f' | 'd'))),
            Word::List(c) => *c == 'F',
        })
    }

    /// Whether it names the selected items at all.
    pub fn uses_items(&self) -> bool {
        self.per_item() || self.words.iter().any(|w| matches!(w, Word::List(_)))
    }
}

/// Splits and checks an argument text.
pub fn parse_args(text: &str) -> Result<Template, Problem> {
    if text.len() > MAX_ARGS_BYTES {
        return Err(Problem::ArgsTooLong);
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\t' && c != '\n')
    {
        return Err(Problem::ControlCharacter);
    }
    let mut words = Vec::new();
    let (mut single, mut list) = (false, false);
    for w in split_words(text)? {
        let mut pieces: Vec<Piece> = Vec::new();
        let mut lit = String::new();
        let mut chars = w.chars();
        let mut found_list = None;
        while let Some(c) = chars.next() {
            if c != '%' {
                lit.push(c);
                continue;
            }
            match chars.next() {
                Some('%') => lit.push('%'),
                Some(p @ ('f' | 'u' | 'd')) => {
                    if !lit.is_empty() {
                        pieces.push(Piece::Text(std::mem::take(&mut lit)));
                    }
                    pieces.push(Piece::One(p));
                    single = true;
                }
                Some(p @ ('F' | 'U')) => found_list = Some(p),
                Some(other) => return Err(Problem::UnknownPlaceholder(other)),
                None => return Err(Problem::LonePercent),
            }
        }
        if let Some(p) = found_list {
            // %F and %U are a word of their own: nothing else in it.
            if !pieces.is_empty() || !lit.is_empty() || w.chars().count() != 2 {
                return Err(Problem::ListNotAlone);
            }
            list = true;
            words.push(Word::List(p));
            continue;
        }
        if !lit.is_empty() {
            pieces.push(Piece::Text(lit));
        }
        // A word of quotes only ("") stays as an empty argument.
        words.push(Word::Pieces(pieces));
    }
    if single && list {
        return Err(Problem::MixedPlaceholders);
    }
    Ok(Template { words })
}

// ---- Checking an action ----

/// A name as it is kept: no control or bidi character, runs of space as one,
/// at most [`MAX_NAME_CHARS`]. `None` when nothing is left.
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

/// A file-type pattern in its kept form (lower case), or `None` if it is not
/// one: `type/subtype`, `type/*`, or `*` alone for every item.
pub fn clean_type(pattern: &str) -> Option<String> {
    let p = pattern.trim().to_ascii_lowercase();
    if p.is_empty() || p.len() > MAX_TYPE_BYTES {
        return None;
    }
    if p == "*" {
        return Some(p);
    }
    let (kind, sub) = p.split_once('/')?;
    let ok = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-' | b'_'))
    };
    (ok(kind) && (ok(sub) || sub == "*")).then_some(p)
}

/// The patterns from text typed as a list (separated by spaces, commas or
/// semicolons). `Err` when one is not a pattern or there are too many.
pub fn parse_types(text: &str) -> Result<Vec<String>, Problem> {
    let mut out: Vec<String> = Vec::new();
    for item in text
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|s| !s.is_empty())
    {
        let p = clean_type(item).ok_or(Problem::BadTypes)?;
        if !out.contains(&p) {
            out.push(p);
        }
        if out.len() > MAX_TYPES {
            return Err(Problem::BadTypes);
        }
    }
    Ok(out)
}

/// The file the program names, or why it is no good. A name with a slash must
/// be a full path; a name without one is looked for in `path_env` (the
/// directories of `PATH`, which must be absolute). The file must be a regular
/// file (a link to one is fine) that can be executed.
pub fn resolve_program(program: &str, path_env: &str) -> Result<PathBuf, Problem> {
    let program = program.trim();
    if program.is_empty() {
        return Err(Problem::NoProgram);
    }
    if program.len() > MAX_PROGRAM_BYTES {
        return Err(Problem::ProgramTooLong);
    }
    if program.chars().any(char::is_control) {
        return Err(Problem::ControlCharacter);
    }
    let typed_base = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    if is_refused(typed_base) {
        return Err(Problem::ProgramRefused(typed_base.to_string()));
    }
    let found = if program.contains('/') {
        let p = Path::new(program);
        if !p.is_absolute() {
            return Err(Problem::ProgramNotAbsolute);
        }
        if !p.exists() {
            return Err(Problem::ProgramMissing(program.to_string()));
        }
        p.to_path_buf()
    } else {
        // The first entry that has a program that can run: a file that merely
        // exists and cannot be executed does not hide a good one further on.
        let candidates: Vec<PathBuf> = path_env
            .split(':')
            .filter(|d| Path::new(d).is_absolute())
            .map(|d| Path::new(d).join(program))
            .filter(|p| p.exists())
            .collect();
        candidates
            .iter()
            .find(|p| is_executable(p))
            .or_else(|| candidates.first())
            .cloned()
            .ok_or_else(|| Problem::ProgramMissing(program.to_string()))?
    };
    // A link to `bash` is `bash`.
    if let Some(real) = std::fs::canonicalize(&found)
        .ok()
        .and_then(|r| r.file_name().and_then(|n| n.to_str()).map(str::to_string))
        && is_refused(&real)
    {
        return Err(Problem::ProgramRefused(real));
    }
    if !is_executable(&found) {
        return Err(Problem::ProgramNotExecutable(program.to_string()));
    }
    Ok(found)
}

fn is_refused(base: &str) -> bool {
    let b = base.to_ascii_lowercase();
    REFUSED_PROGRAMS.contains(&b.as_str())
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a valid NUL-terminated path; access reads nothing else.
    unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 }
}

/// Why this action can't be kept (its name, program, arguments or types), or
/// `None` when it can. The program is looked for in `path_env`.
pub fn problem(a: &Action, path_env: &str) -> Option<Problem> {
    if clean_name(&a.name).is_none() {
        return Some(Problem::NoName);
    }
    if a.types.len() > MAX_TYPES || a.types.iter().any(|t| clean_type(t).is_none()) {
        return Some(Problem::BadTypes);
    }
    if let Err(p) = parse_args(&a.args) {
        return Some(p);
    }
    if let Some(p) = placeholder_in_code(&a.program, &a.args) {
        return Some(p);
    }
    resolve_program(&a.program, path_env).err()
}

fn has_placeholder(word: &str) -> bool {
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('%') => {}
                Some('f' | 'F' | 'u' | 'U' | 'd') => return true,
                _ => {}
            }
        }
    }
    false
}

/// A check that catches the usual ways (it is not a parser for every
/// interpreter): `Some` when `program` is an interpreter and a placeholder
/// sits in the word it is told to run as code (`-c`, `-e`... then the next word, or the same
/// word for `--eval=...` and `-ecode`).
fn placeholder_in_code(program: &str, args: &str) -> Option<Problem> {
    let base = Path::new(program.trim())
        .file_name()
        .and_then(|n| n.to_str())?
        .to_ascii_lowercase();
    let stem = base.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    if !INTERPRETERS.contains(&stem) && !INTERPRETERS.contains(&base.as_str()) {
        return None;
    }
    let words = split_words(args).ok()?;
    // awk's program is its first word that is not an option.
    if matches!(stem, "awk" | "gawk" | "mawk")
        && words
            .iter()
            .find(|w| !w.starts_with('-'))
            .is_some_and(|w| has_placeholder(w))
    {
        return Some(Problem::PlaceholderInCode);
    }
    let mut code_next = false;
    for w in &words {
        // `-lane`, `-Sc`: a cluster of short options ending in the one that
        // takes the code makes the next word code
        let cluster_code = w.len() > 2
            && w.starts_with('-')
            && !w.starts_with("--")
            && w[1..].bytes().all(|b| b.is_ascii_alphabetic())
            && matches!(w.as_bytes()[w.len() - 1], b'c' | b'e' | b'E' | b'r');
        if cluster_code {
            code_next = true;
            continue;
        }
        if code_next && has_placeholder(w) {
            return Some(Problem::PlaceholderInCode);
        }
        code_next = false;
        if CODE_OPTIONS.contains(&w.as_str()) {
            code_next = true;
        } else if w.starts_with('-')
            && has_placeholder(w)
            && (CODE_OPTIONS.iter().any(|o| w.starts_with(o)))
        {
            return Some(Problem::PlaceholderInCode);
        }
    }
    None
}

// ---- Using an action ----

/// An item the action is used on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The path, for an item on this computer.
    pub path: Option<String>,
    /// The URL (`file:///...` for a local item).
    pub url: String,
}

/// Why an action can't be used on these items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The action names items and none are selected.
    NoItems,
    TooManyItems,
    /// The action needs paths and an item is on a server.
    NotLocal,
    /// Starting one process for each item would start too many.
    TooManyRuns,
}

impl Refusal {
    pub fn text(&self) -> String {
        match self {
            Refusal::NoItems => "Select a file first.".into(),
            Refusal::TooManyItems => {
                format!("That is too many items for one action (at most {MAX_ITEMS}).")
            }
            Refusal::NotLocal => {
                "This action needs files on this computer, and some of these are on a server."
                    .into()
            }
            Refusal::TooManyRuns => format!(
                "This action runs once for each item, and that is too many programs at once (at most {MAX_RUNS}). Select fewer items."
            ),
        }
    }
}

/// The argument lists to run (each without `argv[0]`, the program): one for
/// each item when the action has `%f`, `%u` or `%d`, else one. A file name
/// is one argument, as it is.
pub fn expand(args: &str, items: &[Item]) -> Result<Vec<Vec<String>>, Refusal> {
    let tpl = match parse_args(args) {
        Ok(t) => t,
        // A template that was fine when it was kept; if the file was edited, run nothing.
        Err(_) => return Ok(Vec::new()),
    };
    if items.len() > MAX_ITEMS {
        return Err(Refusal::TooManyItems);
    }
    if tpl.uses_items() && items.is_empty() {
        return Err(Refusal::NoItems);
    }
    if tpl.needs_paths() && items.iter().any(|i| i.path.is_none()) {
        return Err(Refusal::NotLocal);
    }
    if tpl.per_item() {
        if items.len() > MAX_RUNS {
            return Err(Refusal::TooManyRuns);
        }
        return Ok(items
            .iter()
            .map(|i| one_run(&tpl, std::slice::from_ref(i)))
            .collect());
    }
    Ok(vec![one_run(&tpl, items)])
}

fn parent_of(path: &str) -> String {
    match Path::new(path).parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_string_lossy().into_owned(),
        _ => "/".to_string(),
    }
}

fn one_run(tpl: &Template, items: &[Item]) -> Vec<String> {
    let mut argv = Vec::new();
    for w in &tpl.words {
        match w {
            Word::List('F') => {
                for i in items {
                    if let Some(p) = &i.path {
                        argv.push(p.clone());
                    }
                }
            }
            Word::List(_) => {
                for i in items {
                    argv.push(i.url.clone());
                }
            }
            Word::Pieces(pieces) => {
                let mut arg = String::new();
                for p in pieces {
                    match p {
                        Piece::Text(t) => arg.push_str(t),
                        Piece::One(c) => {
                            let item = items.first();
                            match (c, item) {
                                ('f', Some(i)) => arg.push_str(i.path.as_deref().unwrap_or("")),
                                ('u', Some(i)) => arg.push_str(&i.url),
                                ('d', Some(i)) => {
                                    arg.push_str(&parent_of(i.path.as_deref().unwrap_or("")))
                                }
                                _ => {}
                            }
                        }
                    }
                }
                argv.push(arg);
            }
        }
    }
    argv
}

// ---- The file types ----

/// Whether one item is one the action is for. `names` are the item's MIME
/// type and the types it inherits from (a folder also has `inode/directory`).
pub fn type_matches(types: &[String], names: &[&str]) -> bool {
    if types.is_empty() {
        return true;
    }
    let is_dir = names.contains(&"inode/directory");
    types.iter().any(|t| match t.as_str() {
        "*" | "all/all" => true,
        "all/allfiles" => !is_dir,
        t => match t.strip_suffix("/*") {
            Some(kind) => names.iter().any(|n| {
                n.split_once('/')
                    .is_some_and(|(k, _)| k.eq_ignore_ascii_case(kind))
            }),
            None => names.iter().any(|n| n.eq_ignore_ascii_case(t)),
        },
    })
}

// ---- The list ----

/// The list, in the order shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActionList {
    items: Vec<Action>,
}

impl ActionList {
    pub fn items(&self) -> &[Action] {
        &self.items
    }

    pub fn get(&self, id: u32) -> Option<&Action> {
        self.items.iter().find(|a| a.id == id)
    }

    /// Reads the settings text; whatever is wrong with a line drops that line.
    /// The program is not looked for (it may be on a disk that is not there
    /// now); it is checked when the action is used.
    pub fn parse(text: &str) -> ActionList {
        let mut list = ActionList::default();
        for line in text.lines() {
            if list.items.len() >= MAX_ACTIONS {
                break;
            }
            let Some(a) = parse_line(line) else {
                continue;
            };
            if list.items.iter().any(|x| x.id == a.id) {
                continue;
            }
            list.items.push(a);
        }
        list
    }

    /// The text to keep.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for a in &self.items {
            out.push_str(&line_of(a));
            out.push('\n');
        }
        out
    }

    /// Adds an action at the end after checking it against `path_env`. Its id
    /// is a number no other action has. Returns the id.
    pub fn add(&mut self, a: Action, path_env: &str) -> Result<u32, Outcome> {
        if self.items.len() >= MAX_ACTIONS {
            return Err(Outcome::Full);
        }
        let mut a = a;
        if !fit(&mut a, path_env) {
            return Err(Outcome::Invalid);
        }
        let id = self.items.iter().map(|x| x.id).max().map_or(1, |m| m + 1);
        // A number past what the text accepts again would be dropped on the
        // next read, with everything added after it.
        if id > MAX_ID {
            return Err(Outcome::Full);
        }
        a.id = id;
        self.items.push(a);
        Ok(id)
    }

    /// Replaces the action with `id`, in the same place.
    pub fn update(&mut self, id: u32, a: Action, path_env: &str) -> Outcome {
        let mut a = a;
        if !fit(&mut a, path_env) {
            return Outcome::Invalid;
        }
        match self.items.iter_mut().find(|x| x.id == id) {
            Some(slot) => {
                a.id = id;
                *slot = a;
                Outcome::Done
            }
            None => Outcome::Missing,
        }
    }

    pub fn remove(&mut self, id: u32) -> Outcome {
        match self.items.iter().position(|a| a.id == id) {
            Some(i) => {
                self.items.remove(i);
                Outcome::Done
            }
            None => Outcome::Missing,
        }
    }
}

/// Makes `a` fit to keep (clean name, program trimmed); false when it cannot be.
fn fit(a: &mut Action, path_env: &str) -> bool {
    let Some(name) = clean_name(&a.name) else {
        return false;
    };
    a.name = name;
    a.program = a.program.trim().to_string();
    a.args = a.args.trim().to_string();
    let mut types = Vec::new();
    for t in &a.types {
        match clean_type(t) {
            Some(t) if !types.contains(&t) => types.push(t),
            Some(_) => {}
            None => return false,
        }
    }
    a.types = types;
    problem(a, path_env).is_none()
}

// ---- The text ----

fn encode(out: &mut String, field: &str) {
    for c in field.chars() {
        if c == '%' || c == '\t' || c.is_control() || c == '\u{2028}' || c == '\u{2029}' {
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

fn line_of(a: &Action) -> String {
    let mut out = String::new();
    let _ = write!(out, "{}\t", a.id);
    encode(&mut out, &a.name);
    out.push('\t');
    encode(&mut out, &a.program);
    out.push('\t');
    encode(&mut out, &a.args);
    out.push('\t');
    encode(&mut out, &a.types.join(" "));
    let _ = write!(out, "\t{}", u8::from(a.ask));
    out
}

fn parse_line(line: &str) -> Option<Action> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() < FIELDS {
        return None;
    }
    let id: u32 = f[0].parse().ok().filter(|&n| n > 0 && n <= MAX_ID)?;
    let types = parse_types(&decode(f[4])?).ok()?;
    let mut a = Action {
        id,
        name: clean_name(&decode(f[1])?)?,
        program: decode(f[2])?.trim().to_string(),
        args: decode(f[3])?.trim().to_string(),
        types,
        ask: f[5].trim() == "1",
    };
    if a.program.is_empty()
        || a.program.len() > MAX_PROGRAM_BYTES
        || a.program.chars().any(char::is_control)
        || parse_args(&a.args).is_err()
    {
        return None;
    }
    a.id = id;
    Some(a)
}

/// An action that is about to be kept, as the window hands it over: the line
/// without its id (`name program args types ask`, tab-separated, each field
/// percent-encoded). Not yet checked: [`ActionList::add`] does that.
pub fn parse_record(record: &str) -> Option<Action> {
    let f: Vec<&str> = record.split('\t').collect();
    if f.len() < FIELDS - 1 {
        return None;
    }
    Some(Action {
        id: 0,
        name: decode(f[0])?,
        program: decode(f[1])?,
        args: decode(f[2])?,
        types: parse_types(&decode(f[3])?).ok()?,
        ask: f[4].trim() == "1",
    })
}

/// The command as a person reads it, for "ask first" and the list: the
/// program, then the arguments as typed.
pub fn command_text(a: &Action) -> String {
    // All of it: this is what a person reads before saying yes, and a
    // length cap would hide the end of the arguments.
    let mut s = String::new();
    for c in a.program.chars() {
        crate::display::push_visible(&mut s, c);
    }
    if !a.args.is_empty() {
        s.push(' ');
        for c in a.args.chars() {
            crate::display::push_visible(&mut s, c);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn item(path: &str) -> Item {
        Item {
            path: Some(path.to_string()),
            url: format!("file://{path}"),
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("telamon-actions-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_exec(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, "#!/bin/true\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn more_programs_that_run_other_programs_are_refused() {
        for p in [
            "flatpak-spawn",
            "systemd-run",
            "runuser",
            "chroot",
            "unshare",
            "nsenter",
            "flock",
            "strace",
            "gdb",
            "setpriv",
            "taskset",
            "parallel",
            "stdbuf",
            "time",
        ] {
            assert!(is_refused(p), "{p}");
        }
    }

    #[test]
    fn a_file_name_can_not_become_the_code_an_interpreter_runs() {
        for (prog, args) in [
            ("python3", "-c 'import os; os.remove(\"%f\")'"),
            ("/usr/bin/perl", "-e 'print \"%f\"'"),
            ("ruby", "-e %f"),
            ("node", "--eval=%f"),
            ("php", "-r%u"),
            ("python3.12", "-c %d"),
            ("pwsh", "--command %F"),
        ] {
            assert_eq!(
                placeholder_in_code(prog, args),
                Some(Problem::PlaceholderInCode),
                "{prog} {args}"
            );
        }
        // the file as data, after the code, is the way
        assert_eq!(
            placeholder_in_code("python3", "-c 'import sys; print(sys.argv)' %f"),
            None
        );
        assert_eq!(placeholder_in_code("python3", "script.py %F"), None);
        for (prog, args) in [
            ("perl", "-lane 'system(\"%f\")'"),
            ("python3", "-Sc 'x=\"%f\"'"),
            ("gawk", "'{ system(\"%f\") }'"),
            ("awk", "-F: '{print \"%f\"}'"),
        ] {
            assert_eq!(
                placeholder_in_code(prog, args),
                Some(Problem::PlaceholderInCode),
                "{prog} {args}"
            );
        }
        assert_eq!(placeholder_in_code("awk", "'{print $1}' %f"), None);
        assert_eq!(placeholder_in_code("python3", "-c '100%%'"), None);
        assert_eq!(placeholder_in_code("convert", "-c %f"), None);
    }

    #[test]
    fn the_prompt_shows_the_whole_command() {
        let a = Action {
            program: "/usr/bin/tool".into(),
            args: format!("{} --evil", "x ".repeat(400)),
            ..Action::default()
        };
        let t = command_text(&a);
        assert!(
            t.ends_with("--evil"),
            "{}",
            &t[t.len().saturating_sub(40)..]
        );
        assert!(t.len() > 800);
        // and still not raw control characters
        let a = Action {
            program: "/p".into(),
            args: "a\u{202E}b".into(),
            ..Action::default()
        };
        assert!(!command_text(&a).contains('\u{202E}'));
    }

    #[test]
    fn an_id_the_text_would_refuse_is_not_handed_out() {
        let d = tmp("ids");
        let prog = make_exec(&d, "tool");
        let mk = |id: u32| Action {
            id,
            name: "Tool".into(),
            program: prog.to_string_lossy().into_owned(),
            args: "%f".into(),
            types: vec![],
            ask: true,
        };
        let mut l = ActionList::default();
        l.items.push(mk(MAX_ID));
        assert_eq!(l.add(mk(0), ""), Err(Outcome::Full));
        assert_eq!(ActionList::parse(&l.to_text()).items().len(), 1);
    }

    #[test]
    fn words_split_like_a_person_reads_them() {
        assert_eq!(
            split_words("a b  c").unwrap(),
            vec!["a".to_string(), "b".into(), "c".into()]
        );
        assert_eq!(
            split_words(r#"one "two words" 'three  words' four\ five"#).unwrap(),
            vec!["one", "two words", "three  words", "four five"]
        );
        // Quotes hide nothing: no variable, no command, no glob is expanded.
        assert_eq!(
            split_words(r#""$HOME" '`id`' $(id) * ;"#).unwrap(),
            vec!["$HOME", "`id`", "$(id)", "*", ";"]
        );
        assert_eq!(split_words("''").unwrap(), vec![""]);
        assert_eq!(split_words("").unwrap(), Vec::<String>::new());
        assert_eq!(split_words("a 'b").unwrap_err(), Problem::OpenQuote);
        assert_eq!(split_words(r#"a "b"#).unwrap_err(), Problem::OpenQuote);
        assert_eq!(split_words("a\\").unwrap_err(), Problem::OpenQuote);
        assert_eq!(split_words(r#""a\"b""#).unwrap(), vec!["a\"b"]);
    }

    #[test]
    fn placeholders_are_checked() {
        assert!(parse_args("--in %f --out %d/x.png").is_ok());
        assert!(parse_args("--all %F").is_ok());
        assert!(parse_args("%U").is_ok());
        assert_eq!(
            parse_args("%x").unwrap_err(),
            Problem::UnknownPlaceholder('x')
        );
        assert_eq!(parse_args("100%").unwrap_err(), Problem::LonePercent);
        assert_eq!(parse_args("--files=%F").unwrap_err(), Problem::ListNotAlone);
        assert_eq!(parse_args("%F%F").unwrap_err(), Problem::ListNotAlone);
        assert_eq!(parse_args("%f %F").unwrap_err(), Problem::MixedPlaceholders);
        assert_eq!(parse_args("%u %U").unwrap_err(), Problem::MixedPlaceholders);
        assert_eq!(parse_args("'unclosed").unwrap_err(), Problem::OpenQuote);
        assert!(parse_args(&"a ".repeat(600)).is_err());
        assert!(parse_args("100%% sure").is_ok());
        assert_eq!(
            parse_args("a\u{0}b").unwrap_err(),
            Problem::ControlCharacter
        );
    }

    #[test]
    fn a_hostile_name_is_one_argument_and_nothing_more() {
        let names = [
            "/tmp/a b.txt",
            "/tmp/; rm -rf ~",
            "/tmp/$(touch pwned)",
            "/tmp/`id`",
            "/tmp/it's \"quoted\"",
            "/tmp/-rf",
            "/tmp/--version",
            "/tmp/line\nbreak",
            "/tmp/100%s%n %f %F",
            "/tmp/*",
            "/tmp/\u{202e}evil.txt",
            "/tmp/with\ttab",
            "/tmp/ ",
            "/tmp/\u{1F600} unicode",
        ];
        let items: Vec<Item> = names.iter().map(|n| item(n)).collect();
        // %F: every name is an argument of its own, exactly as it is.
        let runs = expand("--files %F --done", &items).unwrap();
        assert_eq!(runs.len(), 1);
        let mut want: Vec<String> = vec!["--files".into()];
        want.extend(names.iter().map(|s| s.to_string()));
        want.push("--done".into());
        assert_eq!(runs[0], want);
        // %f: one run for each name, and the name is one argument (also inside a word).
        let runs = expand("--in=%f -- %f", &items[..3]).unwrap();
        assert_eq!(runs.len(), 3);
        for (r, n) in runs.iter().zip(&names) {
            assert_eq!(
                r,
                &vec![format!("--in={n}"), "--".to_string(), n.to_string()]
            );
        }
        // A placeholder in a name is not expanded again.
        let runs = expand("%f", &[item("/tmp/%F %u")]).unwrap();
        assert_eq!(runs, vec![vec!["/tmp/%F %u".to_string()]]);
        // URLs.
        let urls = vec![Item {
            path: None,
            url: "sftp://host/a%20b;c".into(),
        }];
        assert_eq!(
            expand("%U", &urls).unwrap(),
            vec![vec!["sftp://host/a%20b;c".to_string()]]
        );
    }

    #[test]
    fn expansion_counts_runs_and_refuses_what_it_cannot_do() {
        let a = item("/h/a");
        let b = item("/h/sub/b");
        assert_eq!(
            expand("%d", &[a.clone(), b.clone()]).unwrap(),
            vec![vec!["/h".to_string()], vec!["/h/sub".to_string()]]
        );
        assert_eq!(
            expand("%d", &[item("/x")]).unwrap(),
            vec![vec!["/".to_string()]]
        );
        // No placeholder: one run, with or without items, arguments as typed.
        assert_eq!(
            expand("-v 'a b' \"\"", &[]).unwrap(),
            vec![vec!["-v".to_string(), "a b".into(), "".into()]]
        );
        assert_eq!(
            expand("-v", &[a.clone(), b.clone()]).unwrap(),
            vec![vec!["-v".to_string()]]
        );
        assert_eq!(expand("%f", &[]).unwrap_err(), Refusal::NoItems);
        assert_eq!(expand("%F", &[]).unwrap_err(), Refusal::NoItems);
        let remote = Item {
            path: None,
            url: "smb://s/x".into(),
        };
        assert_eq!(
            expand("%f", std::slice::from_ref(&remote)).unwrap_err(),
            Refusal::NotLocal
        );
        assert_eq!(
            expand("%F", &[a.clone(), remote.clone()]).unwrap_err(),
            Refusal::NotLocal
        );
        assert_eq!(expand("%u", &[remote]).unwrap().len(), 1);
        let many: Vec<Item> = (0..=MAX_RUNS).map(|i| item(&format!("/t/{i}"))).collect();
        assert_eq!(expand("%f", &many).unwrap_err(), Refusal::TooManyRuns);
        assert_eq!(expand("%F", &many).unwrap().len(), 1);
        let huge: Vec<Item> = (0..=MAX_ITEMS).map(|i| item(&format!("/t/{i}"))).collect();
        assert_eq!(expand("%F", &huge).unwrap_err(), Refusal::TooManyItems);
        // A damaged template runs nothing.
        assert_eq!(expand("%q", &[a]).unwrap(), Vec::<Vec<String>>::new());
    }

    #[test]
    fn the_program_is_looked_for_and_checked() {
        let d = tmp("prog");
        let tool = make_exec(&d, "mytool");
        let path_env = format!("relative:{}", d.display());
        // By full path and by name from PATH.
        assert_eq!(resolve_program(tool.to_str().unwrap(), "").unwrap(), tool);
        assert_eq!(
            resolve_program("mytool", &path_env).unwrap(),
            d.join("mytool")
        );
        // A file that is not executable earlier in PATH does not hide a good one later.
        let d2 = tmp("prog2");
        fs::write(d2.join("mytool"), "x").unwrap();
        let both = format!("{}:{}", d2.display(), d.display());
        assert_eq!(resolve_program("mytool", &both).unwrap(), d.join("mytool"));
        let _ = fs::remove_dir_all(&d2);
        // A relative PATH entry is never used.
        assert!(matches!(
            resolve_program("mytool", "relative"),
            Err(Problem::ProgramMissing(_))
        ));
        assert_eq!(
            resolve_program("./mytool", &path_env).unwrap_err(),
            Problem::ProgramNotAbsolute
        );
        assert_eq!(
            resolve_program("sub/mytool", &path_env).unwrap_err(),
            Problem::ProgramNotAbsolute
        );
        assert!(matches!(
            resolve_program("/nonexistent/prog", ""),
            Err(Problem::ProgramMissing(_))
        ));
        // A folder, and a file that is not executable.
        assert!(matches!(
            resolve_program(d.to_str().unwrap(), ""),
            Err(Problem::ProgramNotExecutable(_))
        ));
        let plain = d.join("plain");
        fs::write(&plain, "x").unwrap();
        assert!(matches!(
            resolve_program(plain.to_str().unwrap(), ""),
            Err(Problem::ProgramNotExecutable(_))
        ));
        assert_eq!(resolve_program("  ", "").unwrap_err(), Problem::NoProgram);
        assert!(matches!(
            resolve_program(&"x".repeat(2000), ""),
            Err(Problem::ProgramTooLong)
        ));
        // Shells and tools that run a command line, by name and through a link.
        for p in [
            "sh",
            "/usr/bin/bash",
            "env",
            "sudo",
            "pkexec",
            "BASH",
            "xargs",
        ] {
            assert!(
                matches!(
                    resolve_program(p, "/usr/bin:/bin"),
                    Err(Problem::ProgramRefused(_))
                ),
                "{p}"
            );
        }
        let link = d.join("innocent");
        std::os::unix::fs::symlink("/bin/sh", &link).unwrap();
        if Path::new("/bin/sh").exists() {
            assert!(matches!(
                resolve_program(link.to_str().unwrap(), ""),
                Err(Problem::ProgramRefused(_))
            ));
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn file_types_match_with_their_parents() {
        let t = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let png = ["image/png", "application/octet-stream"];
        let dir = ["inode/directory"];
        assert!(type_matches(&[], &png));
        assert!(type_matches(&t(&["*"]), &dir));
        assert!(type_matches(&t(&["image/*"]), &png));
        assert!(!type_matches(&t(&["image/*"]), &dir));
        assert!(type_matches(&t(&["image/png"]), &png));
        assert!(type_matches(&t(&["IMAGE/PNG"]), &png));
        assert!(!type_matches(&t(&["image/jpeg"]), &png));
        assert!(type_matches(&t(&["inode/directory"]), &dir));
        assert!(type_matches(&t(&["all/allfiles"]), &png));
        assert!(!type_matches(&t(&["all/allfiles"]), &dir));
        assert!(type_matches(&t(&["all/all"]), &dir));
        // An inherited type counts.
        assert!(type_matches(
            &t(&["text/plain"]),
            &["text/x-rust", "text/plain"]
        ));
        assert_eq!(clean_type(" Image/* ").as_deref(), Some("image/*"));
        assert_eq!(clean_type("image"), None);
        assert_eq!(clean_type("image/png;rm"), None);
        assert_eq!(clean_type("/png"), None);
        assert_eq!(
            parse_types("image/* , application/pdf;text/plain")
                .unwrap()
                .len(),
            3
        );
        assert_eq!(parse_types("image/* image/*").unwrap(), vec!["image/*"]);
        assert!(parse_types("nonsense").is_err());
        assert!(parse_types("").unwrap().is_empty());
    }

    #[test]
    fn the_list_is_kept_checked_and_read_back() {
        let d = tmp("list");
        make_exec(&d, "tool");
        let path_env = d.display().to_string();
        let mk = |name: &str| Action {
            name: name.into(),
            program: "tool".into(),
            args: "--in %f".into(),
            types: vec!["image/*".into()],
            ask: true,
            ..Action::default()
        };
        let mut l = ActionList::default();
        let a = l.add(mk("  Resize\u{202e}  me "), &path_env).unwrap();
        let b = l.add(mk("Second"), &path_env).unwrap();
        assert_eq!((a, b), (1, 2));
        // The name is made safe to show.
        assert!(!l.get(a).unwrap().name.contains('\u{202e}'));
        // Refusals.
        let mut bad = mk("x");
        bad.program = "nonexistent-tool".into();
        assert_eq!(l.add(bad, &path_env), Err(Outcome::Invalid));
        let mut bad = mk("");
        bad.name = "  ".into();
        assert_eq!(l.add(bad, &path_env), Err(Outcome::Invalid));
        let mut bad = mk("x");
        bad.args = "%q".into();
        assert_eq!(l.add(bad, &path_env), Err(Outcome::Invalid));
        let mut bad = mk("x");
        bad.types = vec!["not a type".into()];
        assert_eq!(l.add(bad, &path_env), Err(Outcome::Invalid));
        // Update and remove.
        let mut changed = mk("Renamed");
        changed.ask = false;
        assert_eq!(l.update(a, changed, &path_env), Outcome::Done);
        assert_eq!(l.get(a).unwrap().name, "Renamed");
        assert!(!l.get(a).unwrap().ask);
        assert_eq!(l.update(99, mk("x"), &path_env), Outcome::Missing);
        // The text round-trips, hostile text included.
        let mut hostile = mk("Tab\there");
        hostile.args = "'it' 100%% %f".to_string();
        let h = l.add(hostile, &path_env).unwrap();
        let text = l.to_text();
        assert_eq!(text.lines().count(), 3);
        let back = ActionList::parse(&text);
        assert_eq!(back, l);
        assert_eq!(back.get(h).unwrap().args, l.get(h).unwrap().args);
        // A damaged or edited file: bad lines go, good lines stay.
        let damaged = format!(
            "{text}garbage\n0\tx\ty\tz\t\t0\n9\tN\t\t\t\t0\n10\tN\tp\t%q\t\t0\n11\tN\tp\t\tnot-a-type\t0\n1\tdup\tp\t\t\t0\n"
        );
        assert_eq!(ActionList::parse(&damaged), l);
        assert_eq!(l.remove(b), Outcome::Done);
        assert_eq!(l.remove(b), Outcome::Missing);
        for i in 0..MAX_ACTIONS {
            let _ = l.add(mk(&format!("A{i}")), &path_env);
        }
        assert_eq!(l.items().len(), MAX_ACTIONS);
        assert_eq!(l.add(mk("one more"), &path_env), Err(Outcome::Full));
        assert_eq!(ActionList::parse(&l.to_text()).items().len(), MAX_ACTIONS);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_record_from_the_window_is_read() {
        let r = "My%20Tool\t/usr/bin/tool\t--x%09%25f\timage/* text/plain\t1";
        let a = parse_record(r).unwrap();
        assert_eq!(a.name, "My Tool");
        assert_eq!(a.args, "--x\t%f");
        assert_eq!(a.types, vec!["image/*", "text/plain"]);
        assert!(a.ask);
        assert!(parse_record("too\tfew").is_none());
        assert!(parse_record("n\tp\ta\tbad-type\t0").is_none());
    }

    #[test]
    fn the_command_is_shown_safely() {
        let a = Action {
            program: "tool".into(),
            args: "--x \u{202e}%f".into(),
            ..Action::default()
        };
        assert!(!command_text(&a).contains('\u{202e}'));
        assert!(command_text(&a).starts_with("tool --x"));
    }
}
