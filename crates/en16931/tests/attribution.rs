//! The CEN attribution notice, a condition of the 2018 CEN–EC licence agreement,
//! appears in every validation report and in `README.md` (the crate docs).

use en16931::{ATTRIBUTION, Invoice, validate};

/// Collapses runs of whitespace, so a markdown line wrap still matches.
fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Report, README and its Attribution section, against [`ATTRIBUTION`].
#[test]
fn attribution_is_carried_everywhere_it_is_required() {
    // 1. Every validation report.
    let rendered = validate(&Invoice::default()).to_string();
    assert!(
        flat(&rendered).contains(&flat(ATTRIBUTION)),
        "a validation report must carry the notice:\n{rendered}"
    );

    // 2. `README.md`, which `lib.rs` includes as the crate docs.
    let readme = include_str!("../README.md");
    assert!(
        flat(readme).contains(&flat(ATTRIBUTION)),
        "README.md must carry the notice verbatim, not a paraphrase"
    );

    // 3. The dedicated section.
    assert!(
        readme.contains("### Attribution"),
        "README.md must keep its Attribution section"
    );
}

/// The constant itself is canonical: single spaces, no newlines, no outer whitespace.
#[test]
fn the_notice_is_canonical_not_merely_present() {
    assert_eq!(
        ATTRIBUTION,
        flat(ATTRIBUTION),
        "the source of truth must be canonical — copies may wrap, this may not"
    );
    assert!(!ATTRIBUTION.contains('\n'));
    assert_eq!(ATTRIBUTION.trim(), ATTRIBUTION);
}

/// The notice names the work as an implementation of the semantic data model.
#[test]
fn the_notice_states_what_the_agreement_requires() {
    assert!(ATTRIBUTION.contains("implementation of the EN 16931-1 semantic data model"));
    assert!(ATTRIBUTION.contains("© CEN"));
    assert!(ATTRIBUTION.contains("2018 CEN–EC licence agreement"));
}

/// A report with no findings still carries the notice.
#[test]
fn even_a_clean_report_carries_it() {
    let report = validate(&Invoice::default());
    assert!(!report.is_valid(), "sanity: the empty invoice has findings");

    let empty = en16931::validation::validate_with(&Invoice::default(), &[]);
    assert_eq!(empty.findings().len(), 0);
    assert!(
        flat(&empty.to_string()).contains(&flat(ATTRIBUTION)),
        "a report with no findings must still carry the notice:\n{empty}"
    );
}

/// No user-visible string (notice, rule texts, finding messages) contains three
/// consecutive spaces, the mark of a multi-line literal missing its `\` continuation.
#[test]
fn no_user_visible_string_carries_source_indentation() {
    const SIGNATURE: &str = "   ";

    assert!(
        !en16931::ATTRIBUTION.contains(SIGNATURE),
        "the attribution notice carries source indentation: {:?}",
        en16931::ATTRIBUTION
    );

    let mut checked = 0usize;
    for rule in en16931::validation::rules::all() {
        assert!(
            !rule.text.contains(SIGNATURE),
            "{}'s text carries source indentation — a multi-line literal is \
             missing its `\\` continuation: {:?}",
            rule.id.as_str(),
            rule.text
        );
        checked += 1;
    }

    // Finding messages are built at runtime, so scan them as produced.
    for profile in en16931::profiles::ALL {
        for finding in profile.validate(&en16931::Invoice::default()).findings() {
            assert!(
                !finding.message.contains(SIGNATURE),
                "a finding of {} carries source indentation: {:?}",
                profile.id,
                finding.message
            );
        }
    }

    assert!(checked > 300, "the registry should be whole: {checked}");
}
