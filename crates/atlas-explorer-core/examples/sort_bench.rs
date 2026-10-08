//! Prints how long sorting 100k rows takes: `cargo run --release --example sort_bench`.
use atlas_explorer_core::sort::{Column, SortRow, name_key, sort_permutation};
use std::time::Instant;

fn main() {
    let rows: Vec<SortRow> = (0..100_000u64)
        .map(|i| {
            let n = (i.wrapping_mul(2654435761)) % 1_000_003;
            SortRow {
                key: name_key(format!("Photo {n} (copy {}).jpg", i % 17).as_bytes()),
                is_dir: i % 10 == 0,
                size: n,
                mtime: n as i64,
                ctime: 0,
                atime: 0,
                kind: "JPEG image".into(),
                group: Vec::new(),
                origin: Vec::new(),
                deleted: 0,
            }
        })
        .collect();
    for col in [Column::Name, Column::Size, Column::Type] {
        let t = Instant::now();
        let p = sort_permutation(&rows, col, true, true);
        println!("{col:?}: {:?} ({} rows)", t.elapsed(), p.len());
    }
}
