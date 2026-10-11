#![cfg(any(feature = "ubl", feature = "cii"))]

//! Read → write → read is the identity over the reference corpus (CEN, KoSIT
//! and OpenPeppol documents), in each syntax.
//!
//! Where a syntax cannot carry something (UBL `<CreditNote>` has no
//! `cbc:DueDate`; core forbids the Extension groups), every difference between
//! the two readings must be named in [`Written::entries`]. The writer is
//! [`write_unchecked`]; documents it refuses as unwritable are counted.
//!
//! [`Written::entries`]: en16931_formats::Written::entries
//! [`write_unchecked`]: en16931_formats::write_unchecked

mod common;

use std::path::PathBuf;

fn documents(syntax: en16931_formats::Syntax) -> Vec<(PathBuf, String)> {
    common::authority_documents(syntax, "round-trip fidelity")
}

/// The model with each party's BT-29 sorted, the one order EN 16931 leaves
/// meaningless and CII's `ram:GlobalID` / `ram:ID` split changes.
fn canonical(inv: &en16931::Invoice) -> en16931::Invoice {
    let mut inv = inv.clone();
    for party in [&mut inv.seller, &mut inv.buyer] {
        party
            .identifiers
            .sort_by_key(|i| (i.content().to_owned(), i.scheme().map(str::to_owned)));
    }
    inv
}

/// The same, as JSON, for describing a difference. Comparison is on the model:
/// `Decimal` compares by value but serialises by representation.
fn as_json(inv: &en16931::Invoice) -> serde_json::Value {
    serde_json::to_value(inv).expect("the model serialises")
}

/// Field-by-field differences, naming the term.
fn differences(a: &serde_json::Value, b: &serde_json::Value, at: &str, out: &mut Vec<String>) {
    if a == b {
        return;
    }
    match (a, b) {
        (serde_json::Value::Object(x), serde_json::Value::Object(y)) => {
            let keys: std::collections::BTreeSet<_> = x.keys().chain(y.keys()).collect();
            for k in keys {
                let null = serde_json::Value::Null;
                differences(
                    x.get(k).unwrap_or(&null),
                    y.get(k).unwrap_or(&null),
                    &format!("{at}.{k}"),
                    out,
                );
            }
        }
        (serde_json::Value::Array(x), serde_json::Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                differences(p, q, &format!("{at}[{i}]"), out);
            }
        }
        _ => out.push(format!("{at}: wrote {a}, read back {b}")),
    }
}

macro_rules! syntaxes {
    ($($feature:literal => ($module:ident, $syntax:ident, $min:literal, $refused:literal)),* $(,)?) => {$(
        #[cfg(feature = $feature)]
        #[test]
        fn $module() {
            use en16931_formats::Syntax;

            let docs = documents(Syntax::$syntax);
            if docs.is_empty() {
                println!("SKIPPED: no artefacts — run `cargo xtask fetch`");
                return;
            }
            let (mut checked, mut reported, mut refused) = (0usize, 0usize, 0usize);
            let mut silent: Vec<String> = Vec::new();

            for (path, text) in &docs {
                // A document the reader refuses is `tests/corpus.rs`' business.
                let Ok(first) = en16931_formats::read(text.as_bytes()) else { continue };
                assert_eq!(first.syntax, Syntax::$syntax, "{}", path.display());
                // A model the target schema has no form for is refused; counted.
                let out = match en16931_formats::write_unchecked(&first.invoice, Syntax::$syntax) {
                    Ok(out) => out,
                    Err(e) => {
                        println!("{}: refused — {e}", path.display());
                        refused += 1;
                        continue;
                    }
                };
                if stringify!($module) == "cii" {
                    common::assert_cii_bt8_spelled_for_cii(&path.display().to_string(), &out.xml);
                }
                let second = en16931_formats::read(out.xml.as_bytes()).unwrap_or_else(|e| {
                    panic!("{}: the writer emitted unreadable output: {e}", path.display())
                });
                checked += 1;

                let (before, after) = (canonical(&first.invoice), canonical(&second.invoice));
                if before == after {
                    continue;
                }
                let mut diffs = Vec::new();
                differences(&as_json(&before), &as_json(&after), "", &mut diffs);
                // Something moved: allowed only if the writer said so.
                if out.entries.is_empty() {
                    silent.push(format!(
                        "{}\n    {}",
                        path.display(),
                        diffs.join("\n    ")
                    ));
                } else {
                    reported += 1;
                }
            }

            assert!(
                checked > $min,
                "only {checked} documents — is the corpus complete?"
            );
            // Deliberately invalid corpus documents with no form in the target
            // schema (`processing-valid/ubl005.xml` lacks BT-2). Pinned.
            assert_eq!(refused, $refused, "documents refused as unwritable");
            assert!(
                silent.is_empty(),
                "{} document(s) lost data with nothing in `entries` to say so:\n\n{}",
                silent.len(),
                silent.join("\n\n")
            );
            println!(
                "{}: {checked} documents survive read→write→read; {reported} differ and \
                 every difference is named in `entries`; {refused} refused as unwritable",
                stringify!($module)
            );
        }
    )*};
}

syntaxes! {
    "ubl" => (ubl, Ubl, 250, 1),
    "cii" => (cii, Cii, 120, 0),
}

// ── The Extension groups, which only a profile that declares them may carry ──

/// `write` under a `Validated<XRechnungExtension>` carries BG-DEX-01 and
/// BG-DEX-09; a core write drops them and reports it.
///
/// `UBL-CR-646` and `UBL-CR-470` fence them out of core; the writer reads
/// `Profile::extensions`, the field `EN-EXT-01` reads too.
#[cfg(feature = "ubl")]
#[test]
fn the_extension_groups_travel_only_where_the_profile_can_hold_them() {
    use en16931::profiles::{XRechnung, XRechnungExtension};
    use en16931::validation::profile::Validated;
    use en16931_formats::{Syntax, report::Kind, write, write_unchecked};

    let cites = |entries: &[en16931_formats::report::Entry], rule: &str| {
        entries
            .iter()
            .any(|e| e.kind == Kind::Prohibited && e.rule.as_deref() == Some(rule))
    };

    let mut inv = common::maximal();
    // `BR-CO-03` makes BT-7 and BT-8 exclusive; the fixture carries both.
    inv.vat_point_date_code = None;
    inv.extensions
        .third_party_payments
        .push(en16931::ThirdPartyPayment {
            payment_type: Some("MobilesBezahlen".to_owned()),
            amount: Some(en16931::InvoiceAmount::parse("19.96").expect("amount")),
            description: Some("Fremdleistung".to_owned()),
        });
    // `BR-DEX-09` replaces `BR-CO-16`, so the fixture must balance under it.
    inv.totals.due = inv
        .totals
        .due
        .checked_add(en16931::InvoiceAmount::parse("19.96").expect("amount"))
        .expect("no overflow");

    // Core: dropped, and named.
    let core = write_unchecked(&inv, Syntax::Ubl).expect("UBL has a form for the fixture");
    assert!(
        !core.xml.contains("PrepaidPayment"),
        "core EN 16931 forbids it (UBL-CR-470)"
    );
    assert!(
        cites(&core.entries, "UBL-CR-470"),
        "…and the caller is told: {:?}",
        core.entries
    );

    // The Extension declares the group, so the prohibition is waived.
    let proof = Validated::<XRechnungExtension>::new(inv.clone())
        .unwrap_or_else(|e| panic!("the fixture should satisfy it:\n{e}"));
    let ext = write(&proof, Syntax::Ubl);
    assert!(
        ext.xml.contains("cac:PrepaidPayment"),
        "BG-DEX-09 belongs in an Extension document:\n{}",
        ext.xml
    );
    assert!(
        !cites(&ext.entries, "UBL-CR-470"),
        "…and is not reported as a loss: {:?}",
        ext.entries
    );

    // The plain CIUS does not declare it, so it is dropped there too.
    if let Ok(proof) = Validated::<XRechnung>::new(inv) {
        let w = write(&proof, Syntax::Ubl);
        assert!(!w.xml.contains("PrepaidPayment"));
        assert!(cites(&w.entries, "UBL-CR-470"), "{:?}", w.entries);
    }
}
