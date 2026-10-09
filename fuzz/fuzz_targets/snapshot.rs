#![no_main]
//! The index's snapshot file: read from the user's cache at every start of the
//! service. Anything may be in it; it may not crash the service, allocate
//! more than the limits allow, or give back an index that is not well formed.
use atlas_file_index::index::{MAX_ARENA, MAX_RECORDS};
use atlas_file_index::snapshot;
use libfuzzer_sys::fuzz_target;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    if let Ok((index, _when)) = snapshot::decode(data, &HashMap::new()) {
        assert!(index.len() <= MAX_RECORDS);
        assert!(index.arena().len() <= MAX_ARENA);
        // What it accepted is what it would write again.
        let bytes = snapshot::encode(&index, 0);
        assert!(snapshot::decode(&bytes, &HashMap::new()).is_ok());
    }
});
