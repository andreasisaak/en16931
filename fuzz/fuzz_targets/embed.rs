//! Any bytes as the PDF/A input of `zugferd::embed`, with the EN 16931 example
//! invoice as the payload.
//!
//! Asserts: no panic; a PDF `embed` returns reads back through `extract` with
//! the invoice it was given and no divergence (`embed` promises to refuse
//! rather than return one that does not).
#![no_main]

use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use en16931::Validated;
use en16931::profiles::En16931;
use en16931_formats::zugferd::{EmbedOptions, Limits, embed, extract};
use libfuzzer_sys::fuzz_target;

static PROOF: LazyLock<Validated<En16931>> = LazyLock::new(|| {
    let invoice = en16931::examples::for_profile(&en16931::profiles::EN16931)
        .expect("a shipped profile has an example");
    Validated::new(invoice).expect("the example is valid")
});

fuzz_target!(|data: &[u8]| {
    let options = EmbedOptions {
        modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_640_800)),
        ..EmbedOptions::default()
    };
    if let Ok(out) = embed(data, &PROOF, &options) {
        let got = extract(&out, &Limits::default()).expect("embed's output extracts");
        assert!(got.divergence.is_empty(), "{:?}", got.divergence);
    }
});
