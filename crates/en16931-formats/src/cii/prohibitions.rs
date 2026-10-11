//! CII prohibitions, extracted from CEN's Schematron as data.
//!
//! Each rule applies only under its Schematron context: `CII-DT-076`
//! (`not(ram:ID)`) forbids `ram:ID` on the element the context selects, not
//! everywhere. So [`forbidden_path`] matches context and relative path.
//!
//! CII has no global attribute prohibitions; [`FORBIDDEN_ATTRIBUTES`] is empty
//! so both syntaxes share one interface.

use super::prohibitions_generated as generated;
pub use generated::{FORBIDDEN_ATTRIBUTES, FORBIDDEN_PATHS, TOTAL_PARAMS, UNEXTRACTED};

/// Does this element path violate a prohibition?
///
/// `path` is a `/`-joined chain of qualified element names starting at the
/// document element. Returns the rule id.
#[must_use]
pub fn forbidden_path(path: &str) -> Option<&'static str> {
    static INDEX: std::sync::OnceLock<crate::xml::ProhibitionIndex> = std::sync::OnceLock::new();
    INDEX
        .get_or_init(|| crate::xml::ProhibitionIndex::build(FORBIDDEN_PATHS))
        .find(FORBIDDEN_PATHS, path)
}

/// Is this attribute forbidden anywhere in a CII document?
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
                format!("rsm:CrossIndustryInvoice/ram:Somewhere/{root}/{rel}"),
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
        for path in [
            "",
            "/",
            "rsm:CrossIndustryInvoice",
            "rsm:CrossIndustryInvoice/ram:ID",
            "a/b/c/d/e",
        ] {
            assert_eq!(forbidden_path(path), by_scan(path), "{path:?}");
            checked += 1;
        }
        assert!(checked > 500, "the sweep should be large: {checked}");
    }

    #[test]
    fn the_table_is_populated() {
        assert!(FORBIDDEN_PATHS.len() > 300, "{}", FORBIDDEN_PATHS.len());
        const { assert!(TOTAL_PARAMS > 400) };
        const { assert!(UNEXTRACTED < 80) };
    }

    /// `ram:ID` is legitimate almost everywhere; only context-specific rules forbid it.
    #[test]
    fn ram_id_is_not_forbidden_outright() {
        for path in [
            "rsm:CrossIndustryInvoice/rsm:ExchangedDocument/ram:ID",
            "rsm:CrossIndustryInvoice/rsm:SupplyChainTradeTransaction/\
             ram:ApplicableHeaderTradeAgreement/ram:SellerTradeParty/ram:ID",
        ] {
            assert_eq!(forbidden_path(path), None, "{path} must be allowed");
        }
    }

    #[test]
    fn legitimate_elements_are_not_forbidden() {
        for path in [
            "rsm:CrossIndustryInvoice/rsm:ExchangedDocument/ram:TypeCode",
            "rsm:CrossIndustryInvoice/rsm:SupplyChainTradeTransaction/\
             ram:ApplicableHeaderTradeSettlement/\
             ram:SpecifiedTradeSettlementHeaderMonetarySummation/ram:DuePayableAmount",
        ] {
            assert_eq!(forbidden_path(path), None, "{path} must be allowed");
        }
    }

    /// A prohibition still fires where it applies.
    #[test]
    fn a_real_prohibition_still_fires() {
        let (rule, ctx, rel) = FORBIDDEN_PATHS[0];
        let stem = ctx.trim_start_matches('/');
        assert_eq!(forbidden_path(&format!("{stem}/{rel}")), Some(rule));
    }
}
