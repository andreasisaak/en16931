//! The UBL 2.1 binding.
//!
//! UBL has two document elements, `Invoice` and `CreditNote`, in different
//! namespaces; [`en16931::DocumentKind`] selects between them.
//!
//! Reading and writing go through the crate's façade with [`crate::Syntax::Ubl`].
//! This module holds the element [`order`], the [`prohibitions`], the
//! single-occurrence set ([`once`]) and the namespaces ([`ns`]).

pub mod once;
pub mod order;
pub mod prohibitions;
pub(crate) mod read;
pub(crate) mod write;

/// The single-occurrence table, compiled once.
pub(crate) fn once_table() -> &'static crate::xml::Once {
    static TABLE: std::sync::OnceLock<crate::xml::Once> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| crate::xml::Once::new(once::ONCE))
}

#[doc(hidden)]
pub mod prohibitions_generated;

/// The core prohibitions a profile's declared extension groups need waived.
///
/// `UBL-CR-470` (`cac:PrepaidPayment`, BG-DEX-09) and `UBL-CR-646`
/// (`cac:SubInvoiceLine`, BG-DEX-01) are waived when [`Profile::extensions`]
/// declares the matching group, the same field `EN-EXT-01` reads.
///
/// [`Profile::extensions`]: en16931::validation::profile::Profile::extensions
pub(crate) fn waivers(
    profile: &'static en16931::validation::profile::Profile,
) -> &'static [&'static str] {
    let subs = profile
        .extensions
        .contains(&en16931::extensions::SUB_INVOICE_LINES);
    let third = profile
        .extensions
        .contains(&en16931::extensions::THIRD_PARTY_PAYMENTS);
    match (subs, third) {
        (true, true) => &["UBL-CR-646", "UBL-CR-470"],
        (true, false) => &["UBL-CR-646"],
        (false, true) => &["UBL-CR-470"],
        (false, false) => &[],
    }
}

/// The UBL namespaces, for a caller that needs to recognise a document before
/// handing it over.
pub mod ns {
    /// `oasis:…:Invoice-2`.
    pub const INVOICE: &str = "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2";
    /// `oasis:…:CreditNote-2`.
    pub const CREDIT_NOTE: &str = "urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2";
}
