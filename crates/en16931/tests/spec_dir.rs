//! The reference corpus is found at the workspace level, or its absence is
//! reported (and fails when `EN16931_REQUIRE_SPEC` is set).

mod common;

use common::{REQUIRE, spec_root};

#[test]
fn the_artefact_directory_is_found_or_its_absence_is_reported() {
    match spec_root() {
        Some(p) => {
            assert!(
                p.join("eInvoicing-EN16931").is_dir(),
                "{} exists but holds no CEN artefacts — a partial fetch is worse \
                 than none, because the suites would check a subset and pass",
                p.display()
            );
            // The corpus is the workspace's, never a member's.
            assert!(
                !p.starts_with(env!("CARGO_MANIFEST_DIR")),
                "the reference corpus resolved inside the crate ({}) rather than \
                 in the workspace",
                p.display()
            );
        }
        None => {
            assert!(
                std::env::var_os(REQUIRE).is_none(),
                "{REQUIRE} is set but there is no reference corpus — run `cargo xtask fetch`"
            );
        }
    }
}
