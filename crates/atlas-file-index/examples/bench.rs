//! Benchmark: builds a 200k-entry tree under `$ATLAS_TEST_DIR` (else the temp
//! dir), scans it, and prints the build time, the snapshot save and load times,
//! the memory held, and p50/p95/max query time over about 30 typical queries.
//!
//!   cargo run --release -p atlas-file-index --example bench [entries]
//!
//! Never touches a real home: everything is generated, and removed at the end.

use atlas_file_index::category::Category;
use atlas_file_index::config::Excludes;
use atlas_file_index::query::{Options, search};
use atlas_file_index::scan::scan_full;
use atlas_file_index::snapshot::{self, CacheDir};
use atlas_file_index::testdir::Scratch;
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, v: &[&'a str]) -> &'a str {
        v[(self.next() % v.len() as u64) as usize]
    }
}

const WORDS: &[&str] = &[
    "report",
    "Budget",
    "invoice",
    "holiday",
    "photo",
    "Résumé",
    "meeting",
    "notes",
    "project",
    "Atlas",
    "draft",
    "final",
    "screenshot",
    "backup",
    "music",
    "movie",
    "thesis",
    "recipe",
    "travel",
    "contract",
    "Köln",
    "café",
    "design",
    "archive",
];
const SEP: &[&str] = &[" ", "-", "_", ".", "", ""];
const EXT: &[&str] = &[
    "txt", "pdf", "png", "jpg", "docx", "xlsx", "md", "rs", "mp3", "mp4", "zip", "odt", "csv",
    "json",
];
const TOP: &[&str] = &[
    "Documents",
    "Downloads",
    "Pictures",
    "Music",
    "Videos",
    "Desktop",
    "Projects",
    "Work",
    "Archive",
    "Misc",
];

fn name(r: &mut Rng) -> String {
    let n = 1 + r.next() % 3;
    let mut s = String::new();
    for i in 0..n {
        if i > 0 {
            s.push_str(r.pick(SEP));
        }
        let w = r.pick(WORDS);
        if r.next().is_multiple_of(5) {
            // camelCase
            let mut c = w.chars();
            s.extend(c.next().map(|f| f.to_uppercase().collect::<String>()));
            s.push_str(c.as_str());
        } else {
            s.push_str(w);
        }
    }
    if r.next().is_multiple_of(3) {
        s.push_str(&format!("{}", r.next() % 2024));
    }
    s
}

fn pct(v: &[Duration], p: f64) -> Duration {
    v[((v.len() as f64 - 1.0) * p) as usize]
}

fn rss_anon_kb() -> u64 {
    fs::read_to_string("/proc/self/smaps_rollup")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Anonymous:"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|n| n.parse().ok()))
        })
        .unwrap_or(0)
}

fn main() {
    let target: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(200_000);
    let t = Scratch::new("bench");
    let root = fs::canonicalize(&t.0).expect("test dir").join("home");
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);

    // generate: ~5000 folders in three levels, the rest files
    let t0 = Instant::now();
    let mut count = 1;
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    fs::create_dir_all(&root).unwrap();
    for top in TOP {
        for a in 0..25 {
            for b in 0..20 {
                let d = root
                    .join(top)
                    .join(format!("{} {a}", r.pick(WORDS)))
                    .join(format!("{}{b}", r.pick(WORDS)));
                fs::create_dir_all(&d).unwrap();
                dirs.push(d);
                count += 1;
            }
        }
    }
    count += TOP.len() * 26;
    let per = (target.saturating_sub(count)) / dirs.len() + 1;
    'fill: for d in &dirs {
        for _ in 0..per {
            if count >= target {
                break 'fill;
            }
            let n = format!("{}.{}", name(&mut r), r.pick(EXT));
            if fs::write(d.join(&n), b"").is_ok() {
                count += 1;
            }
        }
    }
    println!(
        "generated {count} entries in {:.1} s",
        t0.elapsed().as_secs_f64()
    );

    // build
    let anon0 = rss_anon_kb();
    let excl = Excludes::new(&[]);
    let stop = AtomicBool::new(false);
    let t0 = Instant::now();
    let res = scan_full(std::slice::from_ref(&root), &excl, &stop, &HashMap::new());
    let build = t0.elapsed();
    let index = res.index;
    println!(
        "build: {} entries in {:.2} s ({} errors)",
        index.len(),
        build.as_secs_f64(),
        res.errors.len()
    );
    println!(
        "memory held by the index: {:.1} MB anonymous (+{:.1} MB since start)",
        rss_anon_kb() as f64 / 1024.0,
        (rss_anon_kb() - anon0.min(rss_anon_kb())) as f64 / 1024.0
    );

    // snapshot
    let cache = t.0.join("cache");
    let cd = CacheDir::open(&cache, true).unwrap().unwrap();
    let t0 = Instant::now();
    let bytes = snapshot::encode(&index, 0);
    let enc = t0.elapsed();
    let t0 = Instant::now();
    cd.write(&bytes).unwrap();
    let wr = t0.elapsed();
    let mut load = Vec::new();
    for _ in 0..5 {
        let t0 = Instant::now();
        let cd = CacheDir::open(&cache, false).unwrap().unwrap();
        let b = cd.read().unwrap().unwrap();
        let (ix, _) = snapshot::decode(&b, &HashMap::new()).unwrap();
        load.push(t0.elapsed());
        assert_eq!(ix.len(), index.len());
    }
    load.sort();
    println!(
        "snapshot: {:.1} MB; encode {:.0} ms, write+fsync {:.0} ms, load (open+read+validate+decode) median {:.0} ms, max {:.0} ms",
        bytes.len() as f64 / 1e6,
        enc.as_secs_f64() * 1e3,
        wr.as_secs_f64() * 1e3,
        pct(&load, 0.5).as_secs_f64() * 1e3,
        load.last().unwrap().as_secs_f64() * 1e3,
    );

    // queries
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let plain = Options::default();
    let path = Options {
        path_match: true,
        ..Default::default()
    };
    let imgs = Options {
        kinds: Some(Category::Image.bit() | Category::Pdf.bit()),
        ..Default::default()
    };
    let recent = Options {
        modified_after: Some(now - 86_400),
        ..Default::default()
    };
    let root_dl = Options {
        root: Some([root.as_os_str().as_encoded_bytes(), b"/Downloads"].concat()),
        ..Default::default()
    };
    let hidden = Options {
        include_hidden: true,
        kind: Some(atlas_file_index::KindFilter::File),
        ..Default::default()
    };
    let queries: Vec<(&str, &Options)> = vec![
        ("a", &plain),
        ("e", &plain),
        ("re", &plain),
        ("rep", &plain),
        ("report", &plain),
        ("resume", &plain),
        ("résumé", &plain),
        ("inv", &plain),
        ("invoice 2023", &plain),
        ("big", &plain),
        ("mbr", &plain),
        ("bdg", &plain),
        ("holiday photo", &plain),
        ("final draft", &plain),
        ("notes", &plain),
        ("cafe", &plain),
        ("koln", &plain),
        ("zzzz", &plain),
        ("x", &plain),
        ("thesis", &imgs),
        ("photo", &imgs),
        ("", &recent),
        ("screen", &recent),
        ("doc", &root_dl),
        ("report", &root_dl),
        ("budget", &hidden),
        ("music", &hidden),
        ("documents report", &path),
        ("pictures holiday", &path),
        ("archive/travel", &path),
        ("tr", &plain),
        ("contract final", &plain),
    ];
    // warm up, then measure
    for (q, o) in &queries {
        let _ = search(&index, q, 50, o, now);
    }
    let mut all: Vec<Duration> = Vec::new();
    let mut worst = (Duration::ZERO, "");
    let mut hits_total = 0;
    for _ in 0..20 {
        for (q, o) in &queries {
            let t0 = Instant::now();
            let h = search(&index, q, 50, o, now);
            let d = t0.elapsed();
            hits_total += h.len();
            if d > worst.0 {
                worst = (d, q);
            }
            all.push(d);
        }
    }
    all.sort();
    println!(
        "query ({} runs, {} queries, {} hits): p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms (slowest: '{}')",
        all.len(),
        queries.len(),
        hits_total,
        pct(&all, 0.5).as_secs_f64() * 1e3,
        pct(&all, 0.95).as_secs_f64() * 1e3,
        all.last().unwrap().as_secs_f64() * 1e3,
        worst.1
    );
}
