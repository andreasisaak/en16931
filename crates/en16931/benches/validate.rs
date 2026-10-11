//! Validation time (`cargo bench`). Target: a 5-line invoice through the core
//! rule set in well under 100 µs.
//!
//! * `validate/core/5`: the headline number.
//! * `profile/<id>/5`: the marginal cost of each profile's extra checks.
//! * `validate/core/{1,100,1000}`: per-line rules must stay linear.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use en16931::invoice::*;
use en16931::{Date, Identifier, InvoiceAmount, Percentage, Quantity, profiles, validate};
use rust_decimal::Decimal;
use std::hint::black_box;

fn amount(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).expect("amount")
}

fn pct(v: i64) -> Percentage {
    Percentage::new(Decimal::from(v))
}

fn party(name: &str, country: &str) -> Party {
    Party {
        name: Some(name.to_owned()),
        vat_identifier: Some(format!("{country}123456789")),
        electronic_address: Some(Identifier::schemed("0088:test", "0088")),
        address: PostalAddress {
            line1: Some("Musterstr. 1".to_owned()),
            city: Some("Berlin".to_owned()),
            post_code: Some("10115".to_owned()),
            country: Some(Code::new(country)),
            ..Default::default()
        },
        contact: Contact {
            name: Some("A. Muster".to_owned()),
            phone: Some("+49 30 1234567".to_owned()),
            email: Some("rechnung@example.de".to_owned()),
        },
        ..Default::default()
    }
}

/// A valid invoice with `n` standard-rated lines of 100.00 each.
fn invoice(n: usize) -> Invoice {
    let net = Decimal::from(100);
    let line_total = amount(&format!("{}.00", 100 * n));
    let vat = amount(&format!("{}.00", 19 * n));
    let gross = amount(&format!("{}.00", 119 * n));

    let lines = (0..n)
        .map(|i| InvoiceLine {
            id: (i + 1).to_string(),
            note: None,
            order_line_reference: None,
            accounting_reference: None,
            object_identifier: None,
            quantity: Quantity::new(Decimal::ONE),
            unit_code: Code::new("C62"),
            net_amount: amount("100.00"),
            period: None,
            allowances: vec![],
            charges: vec![],
            price: PriceDetails {
                net_price: en16931::UnitPriceAmount::new(net),
                ..Default::default()
            },
            vat: LineVat {
                category: Code::new("S"),
                rate: Some(pct(19)),
            },
            item: Item {
                name: Some("Widget".to_owned()),
                ..Default::default()
            },
        })
        .collect();

    let mut inv = Invoice::builder(
        profiles::EN16931.specification_id,
        "INV-2026-001",
        Date::parse("2026-06-30").expect("date"),
        Code::new("380"),
        Code::new("EUR"),
    )
    .seller(party("Seller GmbH", "DE"))
    .buyer(party("Buyer BV", "NL"))
    .buyer_reference("04011000-12345-34")
    .business_process("urn:fdc:peppol.eu:2017:poacc:billing:01:1.0")
    .due_date(Date::parse("2026-07-30").expect("date"))
    .totals(DocumentTotals {
        line_total,
        taxable_total: line_total,
        vat_total: Some(vat),
        gross_total: gross,
        due: gross,
        ..Default::default()
    })
    .build();
    inv.lines = lines;
    inv.vat_breakdown = vec![VatBreakdown {
        taxable_amount: line_total,
        tax_amount: vat,
        category: Code::new("S"),
        rate: Some(pct(19)),
        exemption_reason: None,
        exemption_reason_code: None,
    }];
    inv
}

fn bench(c: &mut Criterion) {
    let five = invoice(5);
    assert!(
        validate(&five).is_valid(),
        "the benchmark must measure the *valid* path — an invalid document \
         short-circuits nothing here, but it would measure the wrong thing"
    );

    let mut group = c.benchmark_group("validate");
    for n in [1usize, 5, 100, 1000] {
        let inv = invoice(n);
        group.bench_with_input(BenchmarkId::new("core", n), &inv, |b, inv| {
            b.iter(|| black_box(validate(black_box(inv))));
        });
    }
    group.finish();

    let mut group = c.benchmark_group("profile");
    let inv = invoice(5);
    for p in profiles::ALL {
        group.bench_with_input(BenchmarkId::new(p.id, 5), &inv, |b, inv| {
            b.iter(|| black_box(p.validate(black_box(inv))));
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
