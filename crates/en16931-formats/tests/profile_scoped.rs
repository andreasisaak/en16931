#![cfg(all(feature = "ubl", feature = "cii"))]

//! `write`: no document comes out unless the rules its BT-24 claims have run.
//! Refusal, acceptance and the BT-24 stamping order are asserted.

mod common;

use en16931::profiles::{EN16931, En16931, XRECHNUNG, XRechnung};
use en16931::validation::profile::Validated;
use en16931::{Invoice, validate};
use en16931_formats::Syntax;

/// The maximal fixture, made **valid**.
///
/// `common::maximal` carries both BT-7 and BT-8, which `BR-CO-03` forbids
/// together; BT-8 is removed.
fn valid_fixture() -> Invoice {
    let mut inv = common::maximal();
    inv.vat_point_date_code = None; // BR-CO-03: BT-7 xor BT-8
    // BT-46 is 0..1: UBL carries one buyer identifier and reports the rest.
    inv.buyer.identifiers.truncate(1);
    assert!(
        EN16931.validate(&inv).is_valid(),
        "{}",
        EN16931.validate(&inv)
    );
    inv
}

/// An invalid invoice yields no proof, so there is nothing to `write`.
#[test]
fn an_invalid_invoice_produces_no_document_in_either_syntax() {
    let empty = Invoice::default();
    assert!(!validate(&empty).is_valid(), "the premise");

    let rejected =
        Validated::<XRechnung>::new(empty).expect_err("an empty invoice is not an XRechnung");
    let (back, report) = rejected.into_parts();
    assert_eq!(
        back,
        Invoice::default(),
        "the invoice is handed back unchanged"
    );
    assert_eq!(report.profile(), Some("XRechnung 3.0"));
    assert!(report.has("BR-02"), "the report is the whole report");
    assert!(report.has("BR-DE-15"), "profile rules ran too");
    assert!(
        report.to_string().contains("XRechnung 3.0"),
        "the message names the profile: {report}"
    );
}

/// And the acceptance, with BT-24 stamped from the profile that was run.
#[test]
fn a_valid_invoice_is_written_and_carries_the_profiles_bt_24() {
    let mut inv = valid_fixture();
    // The core profile is the one this fixture is built to satisfy.
    inv.specification_id = Some("something the caller typed".into());
    let proof = Validated::<En16931>::new(inv).expect("valid");

    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let xml = en16931_formats::write(&proof, syntax).xml;
        assert!(
            xml.contains(EN16931.specification_id),
            "{syntax}: BT-24 comes from the profile, not from the caller"
        );
        assert!(
            !xml.contains("something the caller typed"),
            "{syntax}: the caller's claim is replaced, not appended"
        );
    }
}

/// BT-24 cannot disagree with the rules that were run.
///
/// `Validated::new` checks the invoice as the caller hands it, and `write`
/// stamps BT-24 from the proof's profile afterwards. A foreign BT-24 reaches
/// the validator (XRechnung's `BR-DE-21` is a warning) and is replaced on the
/// way out; read back, the document passes that profile's rules.
#[test]
fn bt_24_cannot_disagree_with_the_rules_that_ran() {
    let mut inv = valid_fixture();
    inv.specification_id = Some(EN16931.specification_id.into());
    let report = XRECHNUNG.validate(&inv);
    let Ok(proof) = Validated::<XRechnung>::new(inv) else {
        panic!("the fixture is an XRechnung but for BT-24:\n{report}");
    };

    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let xml = en16931_formats::write(&proof, syntax).xml;
        let back = en16931_formats::read(xml.as_bytes()).expect("reads back");
        assert_eq!(
            back.invoice.specification_id.as_deref(),
            Some(XRECHNUNG.specification_id),
            "{syntax}: the caller's BT-24 is replaced"
        );
        let shipped = XRECHNUNG.validate(&back.invoice);
        assert!(
            !shipped.has("BR-DE-21"),
            "{syntax}: the shipped BT-24 is XRechnung's:\n{shipped}"
        );
        assert!(
            shipped.is_valid(),
            "{syntax}: shipped as XRechnung:\n{shipped}"
        );
    }
}

/// `write` reports what the syntax could not carry, beside the document.
#[test]
fn write_reports_what_the_syntax_could_not_carry() {
    let proof = Validated::<En16931>::new(valid_fixture()).expect("valid");
    let written = en16931_formats::write(&proof, Syntax::Ubl);
    assert!(written.xml.starts_with("<?xml"));
    // An ordinary invoice reports nothing; the field is reachable on this path.
    assert!(written.entries.is_empty(), "{:?}", written.entries);
}

/// The refusal composes with `?` in ordinary code.
#[test]
fn the_refusal_composes_with_the_question_mark_operator() {
    fn submit(inv: Invoice) -> Result<String, Box<dyn std::error::Error>> {
        let proof = Validated::<XRechnung>::new(inv)?;
        Ok(en16931_formats::write(&proof, Syntax::Ubl).xml)
    }
    assert!(submit(Invoice::default()).is_err());
}
