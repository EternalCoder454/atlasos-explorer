//! The engine end to end on generated trees under `$ATLAS_TEST_DIR`: freshness
//! from inotify, snapshots across restarts, cold start, lazy rechecks.

use atlas_file_index::config::Config;
use atlas_file_index::testdir::Scratch;
use atlas_file_index::uri::path_to_uri;
use atlas_file_index::{Engine, EngineConfig, Options, State, Status};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Env {
    _t: Scratch,
    root: PathBuf,
    home: PathBuf,
}

fn env(name: &str) -> Env {
    let t = Scratch::new(name);
    let base = fs::canonicalize(&t.0).unwrap();
    let root = base.join("tree");
    let home = base.join("home");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&home).unwrap();
    Env { _t: t, root, home }
}

fn cfg(e: &Env) -> EngineConfig {
    EngineConfig {
        config: Config {
            roots: vec![e.root.clone()],
            exclude: vec![],
        },
        cache_home: e.home.join(".cache"),
        recent_file: e.home.join("recently-used.xbel"),
        watch_budget: None,
        debounce: Duration::from_millis(40),
        snapshot_delay: Duration::from_millis(50),
        recheck_after: Duration::from_secs(300),
        scan_delay: Duration::ZERO,
        refresh_gap: Duration::from_millis(50),
    }
}

fn start(c: EngineConfig) -> Engine {
    Engine::start(c, Arc::new(|_: &Status| {})).unwrap()
}

fn wait(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    f()
}

fn found(e: &Engine, q: &str) -> Vec<String> {
    e.search(q, 50, &Options::default())
        .iter()
        .map(|h| String::from_utf8_lossy(&h.path).to_string())
        .collect()
}

fn ready(e: &Engine) {
    assert!(
        wait(Duration::from_secs(10), || e.status().state == State::Ready),
        "not ready: {:?}",
        e.status()
    );
}

fn touch(p: &Path) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"x").unwrap();
}

const WITHIN: Duration = Duration::from_secs(2);

#[test]
fn watcher_reflects_create_rename_delete() {
    let e = env("watch");
    touch(&e.root.join("a.txt"));
    touch(&e.root.join("dir/inner.txt"));
    let eng = start(cfg(&e));
    ready(&eng);
    assert_eq!(found(&eng, "inner").len(), 1);

    // create
    touch(&e.root.join("zebra.txt"));
    assert!(
        wait(WITHIN, || found(&eng, "zebra").len() == 1),
        "create not seen"
    );
    // rename
    fs::rename(e.root.join("zebra.txt"), e.root.join("yak.txt")).unwrap();
    assert!(
        wait(WITHIN, || found(&eng, "yak").len() == 1
            && found(&eng, "zebra").is_empty()),
        "rename not seen"
    );
    // delete
    fs::remove_file(e.root.join("yak.txt")).unwrap();
    assert!(
        wait(WITHIN, || found(&eng, "yak").is_empty()),
        "delete not seen"
    );
    // a new folder with a file, then a change inside it (the new folder is watched)
    touch(&e.root.join("newdir/first.txt"));
    assert!(
        wait(WITHIN, || found(&eng, "first").len() == 1),
        "new folder not seen"
    );
    touch(&e.root.join("newdir/second.txt"));
    assert!(
        wait(WITHIN, || found(&eng, "second").len() == 1),
        "change in the new folder not seen"
    );
    // a change in an old folder after the index was rebuilt around it
    touch(&e.root.join("dir/third.txt"));
    assert!(
        wait(WITHIN, || found(&eng, "third").len() == 1),
        "change in an old folder not seen"
    );
    // a moved folder
    fs::rename(e.root.join("newdir"), e.root.join("moved")).unwrap();
    assert!(wait(WITHIN, || found(&eng, "first")
        .iter()
        .all(|p| p.contains("/moved/"))
        && found(&eng, "first").len() == 1));
    fs::remove_dir_all(e.root.join("moved")).unwrap();
    assert!(
        wait(WITHIN, || found(&eng, "second").is_empty()),
        "removed folder not seen"
    );
    eng.shutdown();
}

#[test]
fn hidden_files_need_include_hidden() {
    let e = env("hidden");
    touch(&e.root.join(".secret.txt"));
    touch(&e.root.join("open.txt"));
    touch(&e.root.join(".git/config"));
    let eng = start(cfg(&e));
    ready(&eng);
    assert!(found(&eng, "secret").is_empty());
    let o = Options {
        include_hidden: true,
        ..Default::default()
    };
    assert_eq!(eng.search("secret", 10, &o).len(), 1);
    assert!(
        eng.search("config", 10, &o).is_empty(),
        "dot-folders are never entered"
    );
    eng.shutdown();
}

#[test]
fn cold_start_answers_empty_while_scanning() {
    let e = env("cold");
    touch(&e.root.join("report.txt"));
    let mut c = cfg(&e);
    c.scan_delay = Duration::from_millis(600);
    let states: Arc<Mutex<Vec<State>>> = Arc::default();
    let s2 = states.clone();
    let eng = Engine::start(
        c,
        Arc::new(move |s: &Status| s2.lock().unwrap().push(s.state)),
    )
    .unwrap();
    let st = eng.status();
    assert_eq!(st.state, State::Scanning);
    assert_eq!(st.entries, 0);
    assert_eq!(st.roots, vec![path_to_uri(e.root.as_os_str().as_bytes())]);
    assert!(found(&eng, "report").is_empty());
    ready(&eng);
    assert_eq!(found(&eng, "report").len(), 1);
    let seen = states.lock().unwrap().clone();
    assert!(seen.contains(&State::Ready), "{seen:?}");
    eng.shutdown();
}

#[test]
fn snapshot_serves_a_restart_before_any_scan() {
    let e = env("restart");
    for i in 0..50 {
        touch(&e.root.join(format!("d{i}/file{i}.txt")));
    }
    let eng = start(cfg(&e));
    ready(&eng);
    eng.shutdown(); // writes the pending snapshot
    let snap = e.home.join(".cache/telamon-explorer/index/v2.idx");
    assert!(snap.exists());
    // a second start answers from the snapshot straight away, even if the scan is slow
    let mut c = cfg(&e);
    c.scan_delay = Duration::from_secs(30);
    let eng2 = start(c);
    let st = eng2.status();
    assert!(matches!(st.state, State::Stale | State::Ready), "{st:?}");
    assert!(st.entries > 50);
    assert_eq!(found(&eng2, "file7").len(), 1);
    // and it notices what changed while it was down
    eng2.shutdown();
    touch(&e.root.join("d3/while-down.txt"));
    let eng3 = start(cfg(&e));
    assert!(
        wait(WITHIN, || found(&eng3, "while-down").len() == 1),
        "reconcile after start"
    );
    eng3.shutdown();
}

#[test]
fn damaged_snapshots_mean_a_rescan() {
    let e = env("damaged");
    touch(&e.root.join("keep.txt"));
    let eng = start(cfg(&e));
    ready(&eng);
    eng.shutdown();
    let snap = e.home.join(".cache/telamon-explorer/index/v2.idx");
    let good = fs::read(&snap).unwrap();
    for bad in [
        good[..good.len() / 2].to_vec(),
        vec![0u8; 300],
        b"ATLASIDX".to_vec(),
        {
            let mut g = good.clone();
            let n = g.len();
            g[n - 3] ^= 0xFF;
            g
        },
    ] {
        fs::write(&snap, &bad).unwrap();
        let eng = start(cfg(&e));
        ready(&eng);
        assert_eq!(found(&eng, "keep").len(), 1);
        eng.shutdown();
        assert_eq!(
            fs::read(&snap).unwrap().len(),
            good.len(),
            "rewritten whole"
        );
    }
    // a snapshot of other roots is not used either
    let other = env("damaged-other");
    touch(&other.root.join("elsewhere.txt"));
    let mut c = cfg(&other);
    c.cache_home = e.home.join(".cache");
    let eng = start(c);
    ready(&eng);
    assert!(found(&eng, "keep").is_empty());
    assert_eq!(found(&eng, "elsewhere").len(), 1);
    eng.shutdown();
}

#[test]
fn folders_without_watches_are_rechecked_on_a_query() {
    let e = env("lazy");
    touch(&e.root.join("sub/a.txt"));
    let mut c = cfg(&e);
    c.watch_budget = Some(0);
    c.recheck_after = Duration::ZERO;
    let eng = start(c);
    ready(&eng);
    touch(&e.root.join("sub/lazy-new.txt"));
    // each query may start the background recheck; the answer comes from the current index
    assert!(
        wait(Duration::from_secs(5), || found(&eng, "lazy-new").len()
            == 1),
        "recheck did not find it"
    );
    eng.shutdown();
}

#[test]
fn no_recheck_when_the_last_one_is_recent() {
    let e = env("lazy-recent");
    touch(&e.root.join("sub/a.txt"));
    let mut c = cfg(&e);
    c.watch_budget = Some(0);
    c.recheck_after = Duration::from_secs(3600);
    let eng = start(c);
    ready(&eng);
    touch(&e.root.join("sub/not-yet.txt"));
    for _ in 0..5 {
        assert!(found(&eng, "not-yet").is_empty());
        std::thread::sleep(Duration::from_millis(50));
    }
    eng.shutdown();
}

#[test]
fn notify_changed_and_refresh() {
    let e = env("notify");
    touch(&e.root.join("sub/a.txt"));
    let mut c = cfg(&e);
    c.watch_budget = Some(0); // nothing but hints and refresh can find changes
    c.recheck_after = Duration::from_secs(3600);
    let eng = start(c);
    ready(&eng);
    touch(&e.root.join("sub/hinted.txt"));
    let uri = path_to_uri(e.root.join("sub").as_os_str().as_bytes());
    assert_eq!(eng.notify_changed(&[uri]), 1);
    assert!(
        wait(WITHIN, || found(&eng, "hinted").len() == 1),
        "NotifyChanged not applied"
    );
    // a hint for a new folder reaches the deepest known folder
    touch(&e.root.join("brandnew/deep/x.txt"));
    let uri = path_to_uri(e.root.join("brandnew/deep").as_os_str().as_bytes());
    assert_eq!(eng.notify_changed(&[uri]), 1);
    assert!(
        wait(WITHIN, || found(&eng, "x.txt").len() == 1),
        "hint for a new folder"
    );
    // refresh rescans everything
    touch(&e.root.join("other/refreshed.txt"));
    eng.refresh();
    assert!(
        wait(WITHIN, || found(&eng, "refreshed").len() == 1),
        "Refresh not applied"
    );
    // limits: only file:// URIs, at most 256
    assert_eq!(
        eng.notify_changed(&[
            "http://x/y".to_string(),
            "/plain/path".to_string(),
            "file://host/x".to_string()
        ]),
        0
    );
    let many: Vec<String> = (0..400).map(|i| format!("file:///nowhere/{i}")).collect();
    assert_eq!(eng.notify_changed(&many), 256);
    eng.shutdown();
}

#[test]
fn missing_and_disabled_roots() {
    let e = env("states");
    let mut c = cfg(&e);
    c.config.roots = vec![e.root.join("nope")];
    let eng = start(c);
    assert!(wait(Duration::from_secs(10), || eng.status().state
        == State::Error));
    assert!(!eng.status().error.is_empty());
    eng.shutdown();
    let mut c = cfg(&e);
    c.config.roots = vec![];
    let eng = start(c);
    assert_eq!(eng.status().state, State::Disabled);
    assert!(found(&eng, "x").is_empty());
    eng.refresh();
    eng.shutdown();
}

#[test]
fn recently_used_files_rank_higher() {
    let e = env("recent");
    touch(&e.root.join("proj-old.txt"));
    touch(&e.root.join("proj-used.txt"));
    // both files are brand new; push one into the past, then record a use of it
    let old = std::time::SystemTime::now() - Duration::from_secs(400 * 86_400);
    for n in ["proj-old.txt", "proj-used.txt"] {
        fs::File::options()
            .write(true)
            .open(e.root.join(n))
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    let uri = path_to_uri(e.root.join("proj-used.txt").as_os_str().as_bytes());
    let xbel = format!(
        "<?xml version=\"1.0\"?><xbel><bookmark href=\"{uri}\" modified=\"2099-01-01T00:00:00Z\"></bookmark></xbel>"
    );
    fs::write(e.home.join("recently-used.xbel"), xbel).unwrap();
    let eng = start(cfg(&e));
    ready(&eng);
    let r = found(&eng, "proj");
    assert_eq!(r.len(), 2);
    assert!(r[0].ends_with("proj-used.txt"), "{r:?}");
    eng.shutdown();
}

#[test]
fn a_missing_root_comes_back_when_it_appears() {
    let e = env("rootback");
    let late = e.root.join("late");
    let mut c = cfg(&e);
    c.config.roots = vec![late.clone()];
    c.recheck_after = Duration::ZERO;
    let eng = start(c);
    assert!(wait(WITHIN, || eng.status().state == State::Error));
    touch(&late.join("sub/arrived.txt"));
    // a query rechecks, sees the root, and scans it
    assert!(
        wait(Duration::from_secs(10), || found(&eng, "arrived").len()
            == 1),
        "root never came back"
    );
    eng.shutdown();
}

#[test]
fn changed_exclusion_rules_mean_a_rescan() {
    let e = env("exclhash");
    touch(&e.root.join("keep/a.txt"));
    touch(&e.root.join("skipme/b.txt"));
    let eng = start(cfg(&e));
    ready(&eng);
    assert_eq!(found(&eng, "b.txt").len(), 1);
    eng.shutdown();
    // the snapshot has b.txt; with the new rule it must not be used as it is
    let mut c = cfg(&e);
    c.config.exclude = vec!["skipme".into()];
    c.scan_delay = Duration::from_secs(30);
    let eng2 = start(c);
    assert_eq!(eng2.status().state, State::Scanning, "snapshot was used");
    assert!(found(&eng2, "b.txt").is_empty());
    eng2.shutdown();
}

#[test]
fn refresh_calls_are_merged() {
    let e = env("refreshmerge");
    touch(&e.root.join("a.txt"));
    let eng = start(cfg(&e));
    ready(&eng);
    for _ in 0..1000 {
        eng.refresh();
    }
    touch(&e.root.join("after-refresh.txt"));
    eng.refresh();
    assert!(wait(WITHIN, || found(&eng, "after-refresh").len() == 1));
    eng.shutdown();
}

// ---- tags (user.xdg.tags) ----

fn set_tags(p: &Path, v: &str) {
    atlas_file_index::testdir::set_tags(p, v).unwrap();
}

/// Paths of the entries with the tag, found by `Search("", .., {tag})`.
fn tagged(e: &Engine, tag: &str) -> Vec<String> {
    let o = Options {
        tag: Some(tag.to_string()),
        ..Default::default()
    };
    let mut v: Vec<String> = e
        .search("", 50, &o)
        .iter()
        .map(|h| String::from_utf8_lossy(&h.path).to_string())
        .collect();
    v.sort();
    v
}

fn tag_count(e: &Engine, tag: &str) -> u32 {
    e.tags()
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(tag))
        .map_or(0, |(_, c)| *c)
}

/// An environment on a filesystem with user xattrs, or `None` (skipped).
fn tag_env(name: &str) -> Option<Env> {
    let e = env(name);
    if atlas_file_index::testdir::skip_without_xattrs(&e.root) {
        return None;
    }
    Some(e)
}

#[test]
fn tagged_entries_are_found_and_counted() {
    let Some(e) = tag_env("tags") else { return };
    touch(&e.root.join("a.txt"));
    touch(&e.root.join("docs/b.txt"));
    touch(&e.root.join("docs/c.txt"));
    touch(&e.root.join("plain.txt"));
    set_tags(&e.root.join("a.txt"), "Red,Work");
    set_tags(&e.root.join("docs/b.txt"), "red");
    set_tags(&e.root.join("docs"), "Work");
    let eng = start(cfg(&e));
    ready(&eng);
    let p = |rel: &str| e.root.join(rel).to_string_lossy().to_string();
    assert_eq!(tagged(&eng, "Red"), [p("a.txt"), p("docs/b.txt")]);
    assert_eq!(tagged(&eng, "RED"), tagged(&eng, "red"));
    assert_eq!(tagged(&eng, "work"), [p("a.txt"), p("docs")]);
    assert!(tagged(&eng, "blue").is_empty());
    // the untagged do not appear
    assert!(!tagged(&eng, "red").contains(&p("plain.txt")));
    assert_eq!(
        eng.tags(),
        vec![("Red".to_string(), 2), ("Work".to_string(), 2)]
    );
    // a tag and a name together
    let o = Options {
        tag: Some("red".into()),
        ..Default::default()
    };
    assert_eq!(eng.search("b.txt", 10, &o).len(), 1);
    assert!(eng.search("c.txt", 10, &o).is_empty());
    eng.shutdown();
}

#[test]
fn a_tag_change_is_seen_by_the_watcher() {
    let Some(e) = tag_env("tags-watch") else {
        return;
    };
    touch(&e.root.join("a.txt"));
    touch(&e.root.join("sub/b.txt"));
    touch(&e.root.join("sub/inner/c.txt"));
    set_tags(&e.root.join("a.txt"), "Old");
    let eng = start(cfg(&e));
    ready(&eng);
    assert_eq!(tag_count(&eng, "Old"), 1);
    // changed
    set_tags(&e.root.join("a.txt"), "New");
    assert!(
        wait(WITHIN, || tagged(&eng, "new").len() == 1
            && tagged(&eng, "old").is_empty()),
        "a changed tag not seen"
    );
    // added to a file in a subfolder
    set_tags(&e.root.join("sub/b.txt"), "Fresh,New");
    assert!(
        wait(WITHIN, || tagged(&eng, "new").len() == 2
            && tagged(&eng, "fresh").len() == 1),
        "a tag on a file in a subfolder not seen"
    );
    // on a folder itself
    set_tags(&e.root.join("sub/inner"), "Folder Tag");
    assert!(
        wait(WITHIN, || tagged(&eng, "folder tag").len() == 1),
        "a tag on a folder not seen"
    );
    set_tags(&e.root.join("sub"), "Folder Tag");
    assert!(
        wait(WITHIN, || tagged(&eng, "folder tag").len() == 2),
        "a tag on a folder that has a watched folder below it"
    );
    // a new file that is tagged right after it is made
    touch(&e.root.join("sub/inner/d.txt"));
    set_tags(&e.root.join("sub/inner/d.txt"), "Late");
    assert!(
        wait(WITHIN, || tagged(&eng, "late").len() == 1),
        "a new tagged file not seen"
    );
    // cleared
    set_tags(&e.root.join("a.txt"), "");
    assert!(
        wait(WITHIN, || tagged(&eng, "new").len() == 1),
        "a cleared tag not seen"
    );
    assert_eq!(tag_count(&eng, "Fresh"), 1);
    eng.shutdown();
}

#[test]
fn notify_changed_picks_up_tag_changes_without_watches() {
    let Some(e) = tag_env("tags-notify") else {
        return;
    };
    touch(&e.root.join("sub/a.txt"));
    touch(&e.root.join("sub/deep/b.txt"));
    set_tags(&e.root.join("sub/a.txt"), "One");
    let mut c = cfg(&e);
    c.watch_budget = Some(0); // only hints and refresh can find changes
    c.recheck_after = Duration::from_secs(3600);
    let eng = start(c);
    ready(&eng);
    assert_eq!(tagged(&eng, "one").len(), 1);
    set_tags(&e.root.join("sub/a.txt"), "Two");
    set_tags(&e.root.join("sub/deep"), "Dir");
    // no event, no mtime change: nothing sees it by itself
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(tagged(&eng, "one").len(), 1, "stale until hinted");
    let uri = path_to_uri(e.root.join("sub/a.txt").as_os_str().as_bytes());
    assert_eq!(eng.notify_changed(&[uri]), 1);
    assert!(
        wait(WITHIN, || tagged(&eng, "two").len() == 1
            && tagged(&eng, "one").is_empty()),
        "NotifyChanged did not refresh the tags"
    );
    // the folder was listed again, so its subfolder's own tags came with it
    assert_eq!(tagged(&eng, "dir").len(), 1);
    // a hint for a folder re-reads that folder's own tags
    set_tags(&e.root.join("sub/deep"), "Dir2");
    let uri = path_to_uri(e.root.join("sub/deep").as_os_str().as_bytes());
    assert_eq!(eng.notify_changed(&[uri]), 1);
    assert!(
        wait(WITHIN, || tagged(&eng, "dir2").len() == 1),
        "a hint for a tagged folder"
    );
    // and Refresh sees everything
    set_tags(&e.root.join("sub/deep/b.txt"), "Everything");
    eng.refresh();
    assert!(wait(WITHIN, || tagged(&eng, "everything").len() == 1));
    eng.shutdown();
}

#[test]
fn tags_survive_a_restart_through_the_snapshot() {
    let Some(e) = tag_env("tags-snap") else {
        return;
    };
    for i in 0..20 {
        touch(&e.root.join(format!("d{i}/f{i}.txt")));
    }
    set_tags(&e.root.join("d3/f3.txt"), "Kept,Also Kept");
    set_tags(&e.root.join("d4"), "Kept");
    let eng = start(cfg(&e));
    ready(&eng);
    let before = eng.tags();
    assert_eq!(before.first(), Some(&("Kept".to_string(), 2)));
    eng.shutdown(); // writes the snapshot
    assert!(e.home.join(".cache/telamon-explorer/index/v2.idx").exists());
    // the second start answers from the snapshot, before any scan
    let mut c = cfg(&e);
    c.scan_delay = Duration::from_secs(30);
    let eng2 = start(c);
    assert!(matches!(eng2.status().state, State::Stale | State::Ready));
    assert_eq!(eng2.tags(), before);
    assert_eq!(tagged(&eng2, "kept").len(), 2);
    assert_eq!(tagged(&eng2, "ALSO KEPT").len(), 1);
    eng2.shutdown();
}

#[test]
fn a_snapshot_of_the_version_before_is_ignored_and_removed() {
    let Some(e) = tag_env("tags-v1") else { return };
    touch(&e.root.join("a.txt"));
    set_tags(&e.root.join("a.txt"), "Now");
    let idx = e.home.join(".cache/telamon-explorer/index");
    fs::create_dir_all(&idx).unwrap();
    fs::set_permissions(&idx, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    fs::write(idx.join("v1.idx"), b"ATLASIDX not a real snapshot").unwrap();
    let eng = start(cfg(&e));
    ready(&eng);
    assert_eq!(tagged(&eng, "now").len(), 1);
    eng.shutdown();
    assert!(idx.join("v2.idx").exists());
    assert!(!idx.join("v1.idx").exists(), "the old file is removed");
}
