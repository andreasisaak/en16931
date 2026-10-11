//! A PDF through `zugferd::extract` and `zugferd::embedded_files` under small
//! limits.
//!
//! Asserts: no panic, and results stay within the limits (the invoice within
//! the per-stream bound, all attachments within the per-call total).
#![no_main]

use en16931_formats::zugferd::{Limits, embedded_files, extract};
use libfuzzer_sys::fuzz_target;

const LIMITS: Limits = Limits::new()
    .with_max_stream_bytes(64 * 1024)
    .with_max_total_bytes(256 * 1024)
    .with_max_objects(256);

fuzz_target!(|data: &[u8]| {
    if let Ok(got) = extract(data, &LIMITS) {
        assert!(
            got.xml.len() <= LIMITS.max_stream_bytes(),
            "the invoice is {} bytes, over the {}-byte stream limit",
            got.xml.len(),
            LIMITS.max_stream_bytes()
        );
        for d in &got.divergence {
            let _ = d.to_string();
        }
    }
    if let Ok(files) = embedded_files(data, &LIMITS) {
        let total: usize = files.values().map(Vec::len).sum();
        assert!(
            total <= LIMITS.max_total_bytes(),
            "the attachments are {total} bytes, over the {}-byte call limit",
            LIMITS.max_total_bytes()
        );
        assert!(files.values().all(|f| f.len() <= LIMITS.max_stream_bytes()));
    }
});
