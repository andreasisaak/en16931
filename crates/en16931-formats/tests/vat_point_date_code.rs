#![cfg(all(feature = "ubl", feature = "cii"))]

//! BT-8 is one model value with two spellings: UBL uses UNTDID 2005
//! (`3 35 432`), CII UNTDID 2475 (`5 29 72`). The model holds the UNTDID 2005
//! code, so `BR-CL-06` is one rule, and the CII binding translates both ways.
//! Both CEN lists are read from the artefacts.

mod common;

use en16931::codes::generated::VAT_POINT_DATE_CODES;
use en16931::invoice::*;
use en16931::{Date, Identifier, Invoice, InvoiceAmount, Percentage, Quantity};
use en16931_formats::report::{Entry, Kind};
use en16931_formats::{Read, Syntax, Written};

/// CEN's CII code-list Schematron, relative to the reference corpus.
const CII_CODES_SCH: &str = "eInvoicing-EN16931/cii/schematron/codelist/EN16931-CII-codes.sch";
/// CEN's own CII test document carrying BT-8 = `29` on every tax breakdown.
const CEN_BR_CO_17: &str = "eInvoicing-EN16931/test/cii/BR-CO-17.xml";

/// The pairs this binding implements, model first (independent of the table
/// under test).
const PAIRS: [(&str, &str); 3] = [("3", "5"), ("35", "29"), ("432", "72")];

/// A valid invoice with BT-8 set (and BT-7 absent: `BR-CO-03`).
fn invoice(bt8: &str) -> Invoice {
    let party = |name: &str, country: &str| Party {
        name: Some(name.to_owned()),
        vat_identifier: Some(format!("{country}123456789")),
        electronic_address: Some(Identifier::schemed(name, "0088")),
        address: PostalAddress {
            city: Some("Musterstadt".to_owned()),
            post_code: Some("12345".to_owned()),
            country: Some(Code::new(country)),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut inv = Invoice::builder(
        en16931::profiles::EN16931.specification_id,
        "BT8-1",
        Date::parse("2026-07-31").expect("date"),
        "380",
        "EUR",
    )
    .seller(party("Seller GmbH", "DE"))
    .buyer(party("Buyer BV", "NL"))
    .due_in_days(14)
    .line(InvoiceLine::new(
        "1",
        "Widget",
        Quantity::ONE,
        "C62",
        InvoiceAmount::parse("100.00").expect("amount"),
        "S",
        Some(Percentage::new(rust_decimal::Decimal::from(19))),
    ))
    .build_reconciled()
    .expect("reconciles");
    inv.vat_point_date_code = Some(Code::new(bt8));
    inv
}

/// Write a model nobody validated in `syntax`; every fixture here has the
/// elements both schemas require.
fn write(inv: &Invoice, syntax: Syntax) -> Written {
    en16931_formats::write_unchecked(inv, syntax).expect("writable")
}

/// Read a document from its text, whatever its syntax.
fn parse(xml: &str) -> Read {
    en16931_formats::read(xml.as_bytes()).expect("readable")
}

/// The reader's `Malformed` entries.
fn malformed(read: &Read) -> Vec<&Entry> {
    read.entries_of(Kind::Malformed).collect()
}

/// Every `ram:DueDateTypeCode` value in a CII document, in order.
fn due_date_type_codes(xml: &str) -> Vec<String> {
    let doc = roxmltree::Document::parse(xml).expect("well-formed");
    doc.descendants()
        .filter(|n| n.tag_name().name() == "DueDateTypeCode")
        .map(|n| n.text().unwrap_or_default().trim().to_owned())
        .collect()
}

/// A CII document as another producer would send it: the invoice written as
/// CII, with every `ram:DueDateTypeCode` replaced by `code`.
fn cii_with(code: &str) -> String {
    let xml = write(&invoice("3"), Syntax::Cii).xml;
    let doc = roxmltree::Document::parse(&xml).expect("well-formed");
    let mut out = String::new();
    let mut at = 0;
    for n in doc
        .descendants()
        .filter(|n| n.tag_name().name() == "DueDateTypeCode")
    {
        let text = n.first_child().expect("has text").range();
        out.push_str(&xml[at..text.start]);
        out.push_str(code);
        at = text.end;
    }
    assert!(at > 0, "the writer emitted no ram:DueDateTypeCode");
    out.push_str(&xml[at..]);
    out
}

/// The `BR-CL-06` list in CEN's CII code-list Schematron.
fn cen_cii_list() -> Option<Vec<String>> {
    let root = common::require("BT-8 binding against CEN's CII code list")?;
    let sch = std::fs::read_to_string(root.join(CII_CODES_SCH)).expect("CII code-list Schematron");
    let at = sch
        .find("id=\"BR-CL-06\"")
        .expect("BR-CL-06 in the CII codes");
    let head = &sch[..at];
    let open = head.rfind("contains('").expect("a contains() literal") + "contains('".len();
    let close = open + sch[open..].find('\'').expect("closing quote");
    Some(
        sch[open..close]
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
    )
}

// ── A lawful CII invoice validates ───────────────────────────────────────────

/// A CII invoice with `ram:DueDateTypeCode` `5` validates.
#[test]
fn issue_1_a_cii_invoice_with_code_5_validates() {
    let read = parse(&cii_with("5"));
    assert!(malformed(&read).is_empty(), "{:?}", malformed(&read));
    assert_eq!(read.invoice.vat_point_date_code, Some(Code::new("3")));
    let report = en16931::validate(&read.invoice);
    assert!(!report.has("BR-CL-06"), "{report}");
    assert!(report.is_valid(), "{report}");
}

#[test]
fn each_cii_code_reads_as_its_en16931_code() {
    for (model, cii_code) in PAIRS {
        let read = parse(&cii_with(cii_code));
        assert_eq!(
            read.invoice.vat_point_date_code,
            Some(Code::new(model)),
            "CII {cii_code}"
        );
        assert!(
            malformed(&read).is_empty(),
            "CII {cii_code}: {:?}",
            malformed(&read)
        );
        assert!(
            !en16931::validate(&read.invoice).has("BR-CL-06"),
            "CII {cii_code}"
        );
    }
}

/// CEN's own CII test file carries `29` in each of its five invoices.
#[test]
fn cens_br_co_17_document_raises_no_br_cl_06() {
    let Some(root) = common::require("CEN's BR-CO-17 CII test document") else {
        return;
    };
    let set = std::fs::read_to_string(root.join(CEN_BR_CO_17)).expect("BR-CO-17.xml");
    // A Difi `testSet`: each `<test>` wraps one self-contained CII fragment.
    let doc = roxmltree::Document::parse(&set).expect("well-formed");
    let invoices: Vec<&str> = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "CrossIndustryInvoice")
        .map(|n| &set[n.range()])
        .collect();
    assert_eq!(
        invoices.len(),
        5,
        "CEN's BR-CO-17 CII test set changed shape"
    );
    for xml in invoices {
        assert_eq!(due_date_type_codes(xml), ["29"]);
        let read = parse(xml);
        assert!(malformed(&read).is_empty(), "{:?}", malformed(&read));
        assert_eq!(read.invoice.vat_point_date_code, Some(Code::new("35")));
        assert!(!en16931::validate(&read.invoice).has("BR-CL-06"));
    }
}

/// The model's list is UNTDID 2005 and stays so: a CII code set directly on the
/// model is not one of its values.
#[test]
fn a_cii_code_on_the_model_still_fails_br_cl_06() {
    assert!(en16931::validate(&invoice("5")).has("BR-CL-06"));
}

// ── Conversion writes each syntax's own code ─────────────────────────────────

#[test]
fn each_en16931_code_is_written_as_its_cii_code() {
    for (model, cii_code) in PAIRS {
        let w = write(&invoice(model), Syntax::Cii);
        assert!(w.entries.is_empty(), "{model}: {:?}", w.entries);
        let codes = due_date_type_codes(&w.xml);
        assert!(!codes.is_empty(), "{model}: nothing written");
        assert!(codes.iter().all(|c| c == cii_code), "{model}: {codes:?}");
        let back = parse(&w.xml).invoice;
        assert_eq!(back.vat_point_date_code, Some(Code::new(model)));
    }
}

/// UBL `35` → CII `29` → UBL `35`, nothing reported lost.
#[test]
fn ubl_to_cii_to_ubl_keeps_bt_8() {
    let ubl_xml = write(&invoice("35"), Syntax::Ubl).xml;
    let from_ubl = parse(&ubl_xml).invoice;
    let to_cii = write(&from_ubl, Syntax::Cii);
    assert!(to_cii.entries.is_empty(), "{:?}", to_cii.entries);
    assert!(due_date_type_codes(&to_cii.xml).iter().all(|c| c == "29"));
    let from_cii = parse(&to_cii.xml).invoice;
    let to_ubl = write(&from_cii, Syntax::Ubl);
    assert!(to_ubl.entries.is_empty(), "{:?}", to_ubl.entries);
    assert!(
        to_ubl
            .xml
            .contains("<cbc:DescriptionCode>35</cbc:DescriptionCode>")
    );
    assert_eq!(from_cii.vat_point_date_code, from_ubl.vat_point_date_code);
}

/// CII `5` → UBL `3` → CII `5`, each in its own artefact's list.
#[test]
fn issue_1_converts_cii_to_ubl_to_cii() {
    let from_cii = parse(&cii_with("5")).invoice;
    let to_ubl = write(&from_cii, Syntax::Ubl);
    assert!(to_ubl.entries.is_empty(), "{:?}", to_ubl.entries);
    assert!(
        to_ubl
            .xml
            .contains("<cbc:DescriptionCode>3</cbc:DescriptionCode>")
    );
    let from_ubl = parse(&to_ubl.xml).invoice;
    let to_cii = write(&from_ubl, Syntax::Cii);
    assert!(to_cii.entries.is_empty(), "{:?}", to_cii.entries);
    let codes = due_date_type_codes(&to_cii.xml);
    assert!(
        !codes.is_empty() && codes.iter().all(|c| c == "5"),
        "{codes:?}"
    );
}

/// A model value with no CII spelling is not written, and the writer says so.
#[test]
fn an_unmappable_model_value_is_dropped_and_reported() {
    let w = write(&invoice("99"), Syntax::Cii);
    assert!(due_date_type_codes(&w.xml).is_empty(), "written anyway");
    let [note] = &w.entries[..] else {
        panic!("expected one dropped entry, got {:?}", w.entries);
    };
    assert_eq!(note.kind, Kind::Dropped, "{note}");
    assert_eq!(note.term.as_deref(), Some("BT-8"), "{note}");
    assert_eq!(note.value.as_deref(), Some("99"), "{note}");
    assert!(note.path.ends_with("DueDateTypeCode"), "{note}");
}

/// Both sides of the mapping are CEN's two `BR-CL-06` lists, and the mapping
/// is a bijection between them.
#[test]
fn the_mapping_is_a_bijection_between_cens_two_lists() {
    let Some(cen_cii) = cen_cii_list() else {
        return;
    };
    assert_eq!(
        cen_cii,
        common::CII_VAT_POINT_DATE_CODES,
        "the suites' CII list (tests/common) is not CEN's"
    );
    assert_eq!(cen_cii.len(), VAT_POINT_DATE_CODES.len());
    assert_eq!(PAIRS.len(), VAT_POINT_DATE_CODES.len());

    let mut seen_model = std::collections::BTreeSet::new();
    for code in &cen_cii {
        let read = parse(&cii_with(code));
        assert!(
            malformed(&read).is_empty(),
            "CII {code}: {:?}",
            malformed(&read)
        );
        let model = read
            .invoice
            .vat_point_date_code
            .unwrap_or_else(|| panic!("CII {code} read as nothing"));
        assert!(
            VAT_POINT_DATE_CODES.contains(&model.as_str()),
            "CII {code} read as {model:?}, outside VAT_POINT_DATE_CODES"
        );
        assert!(
            seen_model.insert(model.as_str().to_owned()),
            "two CII codes read as {model:?}"
        );
    }
    for model in VAT_POINT_DATE_CODES {
        let codes = due_date_type_codes(&write(&invoice(model), Syntax::Cii).xml);
        assert!(
            codes.iter().all(|c| cen_cii.contains(c)) && !codes.is_empty(),
            "{model} written as {codes:?}, outside CEN's CII list {cen_cii:?}"
        );
    }
}

// ── Values outside the CII list are reported ─────────────────────────────────

/// `3` is in the UBL list and would pass `BR-CL-06` if copied into the model;
/// `99` is in neither list.
#[test]
fn a_cii_value_outside_the_list_is_malformed_not_passed_on() {
    for code in ["3", "99"] {
        let read = parse(&cii_with(code));
        assert_eq!(read.invoice.vat_point_date_code, None, "CII {code}");
        let [note] = &malformed(&read)[..] else {
            panic!(
                "CII {code}: expected one malformed entry, got {:?}",
                malformed(&read)
            );
        };
        assert_eq!(note.path, "ApplicableTradeTax/DueDateTypeCode", "{note}");
        assert_eq!(note.term.as_deref(), Some("BT-8"), "{note}");
        assert_eq!(note.value.as_deref(), Some(code), "{note}");
    }
}
