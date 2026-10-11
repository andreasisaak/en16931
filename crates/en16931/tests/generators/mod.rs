//! Invoice generators, shared by `tests/robustness.rs` and the `roundtrip` fuzz
//! target (which includes this file by path).
//!
//! Output is structurally valid but semantically absurd: `i64`-boundary amounts,
//! multi-byte and control characters, empty codes, negative quantities.

#![allow(dead_code)]

use en16931::invoice::*;
use en16931::{InvoiceAmount, Percentage, Quantity, UnitPriceAmount};
use proptest::prelude::*;
use rust_decimal::Decimal;

/// Amounts clustered at the `i64` boundaries.
pub fn any_amount() -> impl Strategy<Value = InvoiceAmount> {
    prop_oneof![
        Just(InvoiceAmount::from_minor_units(i64::MAX)),
        Just(InvoiceAmount::from_minor_units(i64::MIN)),
        Just(InvoiceAmount::ZERO),
        any::<i64>().prop_map(InvoiceAmount::from_minor_units),
        (-1_000_000i64..1_000_000).prop_map(InvoiceAmount::from_minor_units),
    ]
}

pub fn any_code() -> impl Strategy<Value = Code> {
    prop_oneof![
        Just(Code::new("")),
        Just(Code::new("S")),
        Just(Code::new("Z")),
        Just(Code::new("O")),
        Just(Code::new("EUR")),
        "[A-Za-z0-9 ]{0,8}".prop_map(Code::new),
        any_text().prop_map(|t| Code::new(&t)),
    ]
}

/// Text as European invoices carry it, plus hostile input: multi-byte
/// characters of every width, combining marks, a right-to-left mark, an
/// astral-plane character, control characters and the empty string.
pub fn any_text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("Müller".to_owned()),
        Just("Straße".to_owned()),
        Just("BR-Ié".to_owned()),
        // Combining acute: two chars, three bytes, one grapheme.
        Just("e\u{301}".to_owned()),
        Just("\u{200F}".to_owned()),  // right-to-left mark
        Just("\u{1F9FE}".to_owned()), // astral plane — four bytes
        Just("\u{0}".to_owned()),
        Just("\u{7}".to_owned()),
        ".{0,24}",
        // Long, for code that indexes rather than iterates.
        "\\PC{200,240}",
    ]
    .prop_map(|s: String| s)
}

pub fn any_percentage() -> impl Strategy<Value = Percentage> {
    prop_oneof![
        Just(Percentage::ZERO),
        Just(Percentage::new(Decimal::MAX)),
        Just(Percentage::new(Decimal::MIN)),
        (-100i64..1000).prop_map(|v| Percentage::new(Decimal::from(v))),
    ]
}

pub fn any_quantity() -> impl Strategy<Value = Quantity> {
    prop_oneof![
        Just(Quantity::new(Decimal::ZERO)),
        Just(Quantity::new(Decimal::MAX)),
        Just(Quantity::new(Decimal::MIN)),
        (-1000i64..1000).prop_map(|v| Quantity::new(Decimal::from(v))),
    ]
}

pub fn any_line() -> impl Strategy<Value = InvoiceLine> {
    (
        any_amount(),
        any_quantity(),
        any_code(),
        any_code(),
        any_percentage(),
        any::<bool>(),
        any_text(),
    )
        .prop_map(|(net, qty, unit, cat, rate, has_rate, text)| InvoiceLine {
            id: text.clone(),
            note: Some(text.clone()),
            order_line_reference: None,
            accounting_reference: None,
            object_identifier: None,
            quantity: qty,
            unit_code: unit,
            net_amount: net,
            period: None,
            allowances: vec![],
            charges: vec![],
            price: PriceDetails {
                // May be zero: `R120` must not divide by it.
                base_quantity: Some(qty),
                net_price: UnitPriceAmount::new(Decimal::from(2)),
                ..Default::default()
            },
            vat: LineVat {
                category: cat,
                rate: has_rate.then_some(rate),
            },
            item: Item {
                name: Some(text.clone()),
                description: Some(text),
                ..Item::default()
            },
        })
}

pub fn any_breakdown() -> impl Strategy<Value = VatBreakdown> {
    (any_amount(), any_amount(), any_code(), any_percentage()).prop_map(
        |(taxable, tax, cat, rate)| VatBreakdown {
            taxable_amount: taxable,
            tax_amount: tax,
            category: cat,
            rate: Some(rate),
            exemption_reason: None,
            exemption_reason_code: None,
        },
    )
}

prop_compose! {
    /// A document that is structurally valid and semantically arbitrary.
    pub fn any_invoice()(
        lines in prop::collection::vec(any_line(), 0..12),
        breakdown in prop::collection::vec(any_breakdown(), 0..6),
        line_total in any_amount(),
        taxable in any_amount(),
        vat in proptest::option::of(any_amount()),
        gross in any_amount(),
        due in any_amount(),
        currency in proptest::option::of(any_code()),
        type_code in proptest::option::of(any_code()),
        credit_note in any::<bool>(),
    ) -> Invoice {
        let mut inv = Invoice::default();
        inv.kind = if credit_note { DocumentKind::CreditNote } else { DocumentKind::Invoice };
        inv.currency = currency;
        inv.type_code = type_code;
        inv.lines = lines;
        inv.vat_breakdown = breakdown;
        inv.totals = DocumentTotals {
            line_total,
            taxable_total: taxable,
            vat_total: vat,
            gross_total: gross,
            due,
            ..Default::default()
        };
        inv
    }
}
