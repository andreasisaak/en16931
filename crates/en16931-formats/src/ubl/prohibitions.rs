//! UBL prohibitions ("shall not be used" syntax rules), extracted from CEN's
//! Schematron as data.
//!
//! A writer built from the model cannot express most forbidden elements; a
//! reader reports elements outside the subset as
//! [`Unmapped`](crate::report::Kind::Unmapped).
//!
//! Each rule applies only under its Schematron context: `not(cbc:UUID)` forbids
//! `cbc:UUID` on the element the context selects, not everywhere. So
//! [`forbidden_path`] takes a full path from the document element and matches
//! context and relative path.

use super::prohibitions_generated as generated;
pub use generated::{FORBIDDEN_ATTRIBUTES, FORBIDDEN_PATHS, TOTAL_PARAMS, UNEXTRACTED};

/// Does this element path violate a prohibition?
///
/// `path` is a `/`-joined chain of qualified element names starting at the
/// document element — `Invoice/cac:AccountingCustomerParty/cac:Party/cac:Language`.
/// Returns the rule id.
#[must_use]
pub fn forbidden_path(path: &str) -> Option<&'static str> {
    static INDEX: std::sync::OnceLock<crate::xml::ProhibitionIndex> = std::sync::OnceLock::new();
    INDEX
        .get_or_init(|| crate::xml::ProhibitionIndex::build(FORBIDDEN_PATHS))
        .find(FORBIDDEN_PATHS, path)
}

/// Is this attribute forbidden anywhere in a UBL document?
#[must_use]
pub fn forbidden_attribute(name: &str) -> Option<&'static str> {
    FORBIDDEN_ATTRIBUTES
        .iter()
        .find(|(_, a)| *a == name)
        .map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The last-segment index returns the same rule id as a linear scan, for
    /// every table entry's own path and the same path nested one level deeper.
    #[test]
    fn the_index_agrees_with_a_full_scan() {
        fn by_scan(path: &str) -> Option<&'static str> {
            FORBIDDEN_PATHS
                .iter()
                .find(|(_, ctx, rel)| crate::xml::path_matches(path, ctx, rel))
                .map(|(id, _, _)| *id)
        }

        let mut checked = 0usize;
        for (_, ctx, rel) in FORBIDDEN_PATHS {
            let root = ctx.trim_start_matches('/');
            // Nested one level deeper: an anchored context must refuse it, a
            // floating one must still match.
            for path in [
                format!("{root}/{rel}"),
                format!("Invoice/cac:Somewhere/{root}/{rel}"),
            ] {
                assert_eq!(
                    forbidden_path(&path),
                    by_scan(&path),
                    "index and scan disagree on {path:?}"
                );
                checked += 1;
            }
        }

        // Paths that are in no table at all, so both must answer `None`.
        for path in ["", "/", "Invoice", "Invoice/cbc:ID", "a/b/c/d/e"] {
            assert_eq!(forbidden_path(path), by_scan(path), "{path:?}");
            checked += 1;
        }
        assert!(checked > 1_000, "the sweep should be large: {checked}");
    }

    /// The tables are populated. `TOTAL_PARAMS` counts assertions, not rows.
    #[test]
    fn the_tables_are_populated() {
        assert!(FORBIDDEN_PATHS.len() > 1_000, "{}", FORBIDDEN_PATHS.len());
        assert!(FORBIDDEN_ATTRIBUTES.len() > 10);
        const { assert!(TOTAL_PARAMS > 600) };
        // The remainder needs an XPath engine.
        const { assert!(UNEXTRACTED < 40) };
    }

    /// A prohibition anchored at the document element fires there and only there.
    #[test]
    fn a_root_anchored_prohibition_does_not_fire_at_depth() {
        let Some((rule, ctx, rel)) = FORBIDDEN_PATHS
            .iter()
            .find(|(_, c, _)| c.starts_with("/ubl:"))
        else {
            panic!("expected a root-anchored prohibition");
        };
        let root = ctx
            .trim_start_matches('/')
            .split(':')
            .next_back()
            .expect("root");
        assert_eq!(forbidden_path(&format!("{root}/{rel}")), Some(*rule));
        // Nested one level deeper is a different element.
        assert_eq!(forbidden_path(&format!("{root}/cac:Elsewhere/{rel}")), None);
    }

    /// Terms EN 16931 *does* use must never be reported.
    #[test]
    fn legitimate_elements_are_not_forbidden() {
        for path in [
            "Invoice/cbc:ID",
            "Invoice/cbc:IssueDate",
            "Invoice/cac:AccountingSupplierParty/cac:Party/cac:PartyName/cbc:Name",
            "Invoice/cac:LegalMonetaryTotal/cbc:PayableAmount",
            "Invoice/cac:InvoiceLine/cac:Item/cbc:Name",
        ] {
            assert_eq!(forbidden_path(path), None, "{path} must be allowed");
        }
    }

    #[test]
    fn language_id_is_forbidden_everywhere() {
        assert!(forbidden_attribute("languageID").is_some());
        assert_eq!(forbidden_attribute("currencyID"), None);
    }
}
