//! Read, write and convert timings for both syntaxes.
//!
//! * `read/ubl` vs `read/cii`: the two syntaxes on the same invoice.
//! * `read/*` vs `write/*`: reading should be the slower direction.
//! * `read/ubl/1000`: reading should stay linear in line count.
//! * `convert/ubl-to-cii`: what `en16931 convert` does.
//!
//! Run with `cargo bench -p en16931-formats --all-features`.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use en16931::invoice::*;
use en16931::{Date, InvoiceAmount, Percentage, Quantity};
use en16931_formats::{Loss, Syntax};
use rust_decimal::Decimal;
use std::hint::black_box;

fn party(name: &str) -> Party {
    Party {
        name: Some(name.to_owned()),
        vat_identifier: Some("DE123456789".into()),
        electronic_address: Some(en16931::Identifier::schemed("991-01234-56", "0204")),
        address: PostalAddress {
            city: Some("Musterstadt".into()),
            post_code: Some("12345".into()),
            country: Some(Code::new("DE")),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// An invoice with `n` lines at two VAT rates.
fn invoice(n: usize) -> Invoice {
    // `Invoice` is `#[non_exhaustive]`, so it is filled in, not constructed.
    let mut inv = Invoice::default();
    inv.specification_id = Some("urn:cen.eu:en16931:2017".into());
    inv.number = Some("R-2026-0001".into());
    inv.issue_date = Date::parse("2026-07-31").ok();
    inv.type_code = Some(Code::new("380"));
    inv.currency = Some(Code::new("EUR"));
    inv.seller = party("Stadtwerke Musterstadt GmbH");
    inv.buyer = party("Beispiel AG");
    inv.due_date = Date::parse("2026-08-14").ok();

    for i in 0..n {
        inv.lines.push(InvoiceLine::new(
            (i + 1).to_string(),
            "Netznutzung Arbeitspreis",
            Quantity::new(Decimal::from(1000)),
            "KWH",
            InvoiceAmount::parse("289.00").expect("amount"),
            "S",
            Some(Percentage::new(Decimal::from(if i % 2 == 0 {
                19
            } else {
                7
            }))),
        ));
    }
    // The totals a real document carries.
    let _ = en16931::reconcile(&mut inv);
    inv
}

/// `inv` written in `syntax` — every invoice here has what the schemas require.
fn xml(inv: &Invoice, syntax: Syntax) -> String {
    en16931_formats::write_unchecked(inv, syntax)
        .expect("writable")
        .xml
}

fn benches(c: &mut Criterion) {
    // Reading: the direction a receiver takes.
    let mut group = c.benchmark_group("read");
    for n in [5usize, 100, 1000] {
        let inv = invoice(n);
        for (name, syntax) in [("ubl", Syntax::Ubl), ("cii", Syntax::Cii)] {
            let xml = xml(&inv, syntax);
            group.bench_with_input(BenchmarkId::new(name, n), &xml, |b, xml| {
                b.iter(|| en16931_formats::read(black_box(xml.as_bytes())).expect("readable"));
            });
        }
    }
    group.finish();

    // Writing does no I/O: element-order sorting and string building.
    let mut group = c.benchmark_group("write");
    for n in [5usize, 1000] {
        let inv = invoice(n);
        for (name, syntax) in [("ubl", Syntax::Ubl), ("cii", Syntax::Cii)] {
            group.bench_with_input(BenchmarkId::new(name, n), &inv, |b, inv| {
                b.iter(|| en16931_formats::write_unchecked(black_box(inv), syntax));
            });
        }
    }
    group.finish();

    // `en16931 convert`, end to end: read, write the other syntax, assess the source.
    {
        let mut group = c.benchmark_group("convert");
        let inv = invoice(5);
        for (name, from, to) in [
            ("ubl-to-cii", Syntax::Ubl, Syntax::Cii),
            ("cii-to-ubl", Syntax::Cii, Syntax::Ubl),
        ] {
            let source = xml(&inv, from);
            group.bench_function(name, |b| {
                b.iter(|| {
                    en16931_formats::convert(black_box(source.as_bytes()), to, Loss::Allow)
                        .expect("convertible")
                });
            });
        }
        group.finish();
    }
}

criterion_group!(syntax, benches);
criterion_main!(syntax);
