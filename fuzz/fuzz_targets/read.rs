//! Any bytes through `en16931_formats::read`: a UBL or CII document, or a
//! ZUGFeRD / Factur-X PDF, detected from the bytes.
//!
//! Asserts: no panic, and an accepted document's report renders. A hang or
//! blow-up is a finding; time proportional to input size is not.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(read) = en16931_formats::read(data) {
        let report = read.report();
        let _ = report.to_string();
        let _ = en16931::validate(&read.invoice).to_string();
    }
});
