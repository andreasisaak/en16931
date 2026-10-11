//! Locating the reference corpus.
//!
//! Its path is `EN16931_REFERENCE`, set in the workspace's `.cargo/config.toml`
//! (`cargo xtask fetch` fills it). Without the corpus suites skip, unless
//! `EN16931_REQUIRE_SPEC` is set (as in CI), when [`require`] fails instead.

#![allow(dead_code)] // not every including suite uses every helper

use std::path::PathBuf;

/// The environment variable CI sets to forbid skipping.
pub const REQUIRE: &str = "EN16931_REQUIRE_SPEC";

/// The reference corpus, if it has been fetched (`cargo xtask fetch`).
#[must_use]
pub fn spec_root() -> Option<PathBuf> {
    option_env!("EN16931_REFERENCE")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// `spec_root()`, or `None` — and a failure instead when CI forbids skipping.
///
/// # Panics
/// When the reference corpus is absent and [`REQUIRE`] is set.
#[must_use]
pub fn require(suite: &str) -> Option<PathBuf> {
    match spec_root() {
        Some(p) => Some(p),
        None => {
            assert!(
                std::env::var_os(REQUIRE).is_none(),
                "{suite} needs the artefacts and {REQUIRE} is set, so skipping is \
                 not permitted here. Run `cargo xtask fetch`.\n\
                 This is the check that stops a conformance run reporting green \
                 having validated nothing."
            );
            eprintln!(
                "note: {suite} SKIPPED — no reference corpus. Run `cargo xtask fetch`. \
                 Set {REQUIRE}=1 to make this a failure."
            );
            None
        }
    }
}

pub mod docs;
pub mod peppol;
