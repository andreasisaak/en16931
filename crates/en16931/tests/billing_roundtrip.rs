//! `billing` document → `from_billing` → validated invoice. Each fixture is
//! compared term by term with the `Document` (`assert_mirrors`) and validated
//! under core and XRechnung (`assert_valid`).

#![cfg(feature = "billing")]

use billing::{
    Adjust, AdjustmentKind, Advance, AmountScale, Basis, Bound, CashRounding, Credit, Currency,
    Document, DocumentBuilder, Exemption, Line, LineAdjustment, Payment, Percent, Price, Quantity,
    Rounding, TaxCategory, UnitPrice, Vat, VatAccounting,
};
use en16931::billing_adapter::{SeamError, from_billing};
use en16931::bt::{BtId, Group, Path};
use en16931::invoice::{
    Code, Contact, CreditTransfer, Delivery, DocumentAllowanceCharge, Party, PaymentInstructions,
    PaymentMeans, PostalAddress, VatBreakdown,
};
use en16931::{
    AdvancePayment, Date, DocumentKind, EarlyPaymentDiscount, Identifier, Invoice, InvoiceAmount,
    InvoiceLine, PaymentTerms, Percentage, profiles, validate,
};
use rust_decimal::{Decimal, dec};

// ── Fixtures ──────────────────────────────────────────────────────────────────

fn vat(p: Decimal) -> Vat {
    Vat::standard(Percent::new(p)).expect("a standard rate")
}

fn builder() -> DocumentBuilder {
    Document::builder(Currency::EUR, AmountScale::EN16931).default_vat(vat(dec!(19)))
}

/// The P1 document: a 19 % and a 7 % line, a 10 % allowance apportioned over
/// both VAT groups, and cash rounding to 0.05.
fn p1() -> Document {
    builder()
        .line(Line::new(
            "Wartung",
            Quantity::new(dec!(3), "HUR"),
            UnitPrice::new(Price::new(dec!(133.33))),
        ))
        .line(Line::fixed("Fachbuch", dec!(100)).with_vat(vat(dec!(7))))
        .apportion(
            AdjustmentKind::Allowance,
            "10 % Rabatt",
            Adjust::Percent(Percent::new(dec!(10))),
        )
        .cash_rounding(CashRounding {
            increment: dec!(0.05),
            rounding: Rounding::HalfAwayFromZero,
        })
        .build()
        .expect("a lawful document")
}

fn simple() -> Document {
    builder()
        .line(Line::fixed("Service", dec!(100)))
        .build()
        .expect("a lawful document")
}

fn party(name: &str, country: &str, city: &str, post_code: &str) -> Party {
    Party {
        name: Some(name.to_owned()),
        address: PostalAddress {
            city: Some(city.to_owned()),
            post_code: Some(post_code.to_owned()),
            country: Some(Code::new(country)),
            ..Default::default()
        },
        electronic_address: Some(Identifier::schemed("4006381333931", "0088")),
        vat_identifier: Some(format!("{country}123456789")),
        ..Default::default()
    }
}

/// What XRechnung needs that `billing` does not derive (plus BT-5, which
/// `Invoice::builder` requires).
fn header() -> Invoice {
    let mut seller = party("Stadtwerke Musterstadt GmbH", "DE", "Musterstadt", "12345");
    seller.contact = Contact {
        name: Some("Frau Muster".to_owned()),
        phone: Some("+49 30 123456".to_owned()),
        email: Some("rechnung@stadtwerke.example".to_owned()),
    };
    let mut inv = Invoice::builder(
        profiles::XRECHNUNG.specification_id,
        "R-2026-0001",
        Date::parse("2026-07-31").unwrap(),
        "380",
        "EUR",
    )
    .seller(seller)
    .buyer(party("Beispiel AG", "DE", "Beispielstadt", "54321"))
    .due_date(Date::parse("2026-08-14").unwrap())
    .build();
    inv.buyer_reference = Some("04011000-12345-34".to_owned());
    inv.business_process = Some("urn:fdc:peppol.eu:2017:poacc:billing:01:1.0".to_owned());
    inv.delivery = Some(Delivery {
        date: Some(Date::parse("2026-07-31").unwrap()),
        ..Default::default()
    });
    inv.payment = Some(PaymentInstructions {
        means_code: Some(Code::new("58")),
        means: Some(PaymentMeans::CreditTransfer(vec![CreditTransfer {
            account_identifier: Some("DE89370400440532013000".to_owned()),
            ..Default::default()
        }])),
        ..Default::default()
    });
    inv
}

fn credit_note_header() -> Invoice {
    let mut h = header();
    h.kind = DocumentKind::CreditNote;
    h.type_code = Some(Code::new("381"));
    h
}

// ── Assertions ────────────────────────────────────────────────────────────────

fn amt(a: billing::Amount) -> InvoiceAmount {
    InvoiceAmount::from_decimal_exact(a.to_decimal()).expect("two decimals")
}

/// Every BT-131, every BG-23 amount and every BG-22 term equals the `Document`
/// accessor of the same term.
fn assert_mirrors(doc: &Document, inv: &Invoice) {
    assert_eq!(inv.lines.len(), doc.lines().len(), "one BG-25 per line");
    for (i, line) in inv.lines.iter().enumerate() {
        assert_eq!(line.id, (i + 1).to_string(), "BT-126");
        assert_eq!(line.net_amount, amt(doc.line_net(i)), "BT-131 of line {i}");
        assert_eq!(
            line.item.name.as_deref(),
            Some(doc.lines()[i].description())
        );
    }
    assert_eq!(inv.vat_breakdown.len(), doc.vat_groups().len(), "BG-23");
    for (row, g) in inv.vat_breakdown.iter().zip(doc.vat_groups()) {
        assert_eq!(row.taxable_amount, amt(g.taxable_base()), "BT-116");
        assert_eq!(row.tax_amount, amt(g.tax()), "BT-117");
        assert_eq!(row.category.as_str(), g.category().code(), "BT-118");
        assert_eq!(
            row.rate,
            Some(Percentage::new(g.rate_percent().value())),
            "BT-119"
        );
    }
    let t = &inv.totals;
    let has = |k| doc.adjustments().iter().any(|a| a.kind() == k);
    let opt = |present: bool, a: billing::Amount| present.then(|| amt(a));
    assert_eq!(t.line_total, amt(doc.line_total()), "BT-106");
    assert_eq!(
        t.allowance_total,
        opt(has(AdjustmentKind::Allowance), doc.allowance_total()),
        "BT-107"
    );
    assert_eq!(
        t.charge_total,
        opt(has(AdjustmentKind::Charge), doc.charge_total()),
        "BT-108"
    );
    assert_eq!(t.taxable_total, amt(doc.tax_exclusive_total()), "BT-109");
    assert_eq!(t.vat_total, Some(amt(doc.vat_total())), "BT-110");
    assert_eq!(
        t.vat_total_accounting,
        doc.vat_accounting_total().map(|(_, a)| amt(a)),
        "BT-111"
    );
    assert_eq!(t.gross_total, amt(doc.tax_inclusive_total()), "BT-112");
    assert_eq!(
        t.paid,
        opt(!doc.prepaid().is_zero(), doc.prepaid()),
        "BT-113"
    );
    assert_eq!(
        t.rounding,
        opt(!doc.rounding().is_zero(), doc.rounding()),
        "BT-114"
    );
    assert_eq!(t.due, amt(doc.payable()), "BT-115");
    assert_eq!(inv.currency.as_ref().map(Code::as_str), Some("EUR"), "BT-5");
}

/// No fatal finding under the core or under XRechnung.
fn assert_valid(inv: &Invoice) {
    let core = validate(inv);
    assert!(core.is_valid(), "core:\n{core}");
    let xr = profiles::XRECHNUNG.validate(inv);
    assert!(xr.is_valid(), "XRechnung:\n{xr}");
}

fn convert(doc: &Document, header: Invoice) -> Invoice {
    let inv = from_billing(doc, header).expect("converts");
    assert_mirrors(doc, &inv);
    assert_valid(&inv);
    inv
}

fn a(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).unwrap()
}

// ── One call ──────────────────────────────────────────────────────────────────

#[test]
fn the_p1_document_converts_mirrors_and_validates() {
    let doc = p1();
    let inv = convert(&doc, header());

    assert_eq!(inv.totals.line_total, a("499.99"));
    assert_eq!(inv.totals.allowance_total, Some(a("50.00")));
    assert_eq!(inv.totals.vat_total, Some(a("74.70")));
    assert_eq!(inv.totals.gross_total, a("524.69"));
    assert_eq!(inv.totals.rounding, Some(a("0.01")));
    assert_eq!(inv.totals.due, a("524.70"));
    // One allowance per VAT group, each with its basis and its group's VAT.
    assert_eq!(inv.allowances.len(), 2);
    let rates: Vec<_> = inv.allowances.iter().map(|x| x.vat.rate).collect();
    assert_eq!(
        rates,
        [
            Some(Percentage::new(dec!(19))),
            Some(Percentage::new(dec!(7)))
        ]
    );
    assert_eq!(inv.allowances[0].base_amount, Some(a("399.99")));
    assert_eq!(
        inv.allowances[0].percentage,
        Some(Percentage::new(dec!(10)))
    );
    assert_eq!(inv.allowances[0].reason.as_deref(), Some("10 % Rabatt"));
    // The header's terms are kept unchanged.
    assert_eq!(inv.number.as_deref(), Some("R-2026-0001"));
    assert_eq!(inv.buyer_reference.as_deref(), Some("04011000-12345-34"));
}

#[test]
fn line_i_is_line_i_and_bt_126_is_i_plus_one() {
    let doc = builder()
        .line(Line::fixed("eins", dec!(10)))
        .line(Line::fixed("zwei", dec!(20)).with_vat(vat(dec!(7))))
        .line(Line::fixed("drei", dec!(30)))
        .build()
        .unwrap();
    let mut inv = convert(&doc, header());
    let names: Vec<_> = inv
        .lines
        .iter()
        .map(|l| l.item.name.clone().unwrap())
        .collect();
    assert_eq!(names, ["eins", "zwei", "drei"]);
    // Lines correspond by index, so callers can add terms `billing` lacks.
    inv.lines[1].accounting_reference = Some("4711".to_owned());
    assert_valid(&inv);
}

#[test]
fn every_term_billing_derives_is_refused_in_the_header() {
    let doc = simple();
    let line: InvoiceLine = from_billing(&doc, header()).unwrap().lines[0].clone();
    let allowance = DocumentAllowanceCharge {
        amount: a("1.00"),
        base_amount: None,
        percentage: None,
        vat: line.vat.clone(),
        reason: Some("x".to_owned()),
        reason_code: None,
    };
    let row = VatBreakdown {
        taxable_amount: a("1.00"),
        tax_amount: a("0.19"),
        category: Code::new("S"),
        rate: Some(Percentage::new(dec!(19))),
        exemption_reason: None,
        exemption_reason_code: None,
    };
    type Mutate = Box<dyn Fn(&mut Invoice)>;
    let cases: Vec<(Mutate, Path)> = vec![
        (
            Box::new(move |h| h.lines.push(line.clone())),
            Path::group(Group::Line),
        ),
        (
            Box::new({
                let x = allowance.clone();
                move |h| h.allowances.push(x.clone())
            }),
            Path::group(Group::DocumentAllowance),
        ),
        (
            Box::new(move |h| h.charges.push(allowance.clone())),
            Path::group(Group::DocumentCharge),
        ),
        (
            Box::new(move |h| h.vat_breakdown.push(row.clone())),
            Path::group(Group::VatBreakdown),
        ),
        (
            Box::new(|h| h.currency = Some(Code::new("USD"))),
            Path::term(BtId(5)),
        ),
        (
            Box::new(|h| h.vat_accounting_currency = Some(Code::new("USD"))),
            Path::term(BtId(6)),
        ),
        (
            Box::new(|h| h.totals.line_total = a("1.00")),
            Path::term(BtId(106)),
        ),
        (
            Box::new(|h| h.totals.allowance_total = Some(a("0.00"))),
            Path::term(BtId(107)),
        ),
        (
            Box::new(|h| h.totals.charge_total = Some(a("0.00"))),
            Path::term(BtId(108)),
        ),
        (
            Box::new(|h| h.totals.taxable_total = a("1.00")),
            Path::term(BtId(109)),
        ),
        (
            Box::new(|h| h.totals.vat_total = Some(a("0.00"))),
            Path::term(BtId(110)),
        ),
        (
            Box::new(|h| h.totals.vat_total_accounting = Some(a("0.00"))),
            Path::term(BtId(111)),
        ),
        (
            Box::new(|h| h.totals.gross_total = a("1.00")),
            Path::term(BtId(112)),
        ),
        (
            Box::new(|h| h.totals.paid = Some(a("0.00"))),
            Path::term(BtId(113)),
        ),
        (
            Box::new(|h| h.totals.rounding = Some(a("0.00"))),
            Path::term(BtId(114)),
        ),
        (
            Box::new(|h| h.totals.due = a("1.00")),
            Path::term(BtId(115)),
        ),
        (
            Box::new(|h| {
                h.extensions.advance_payments.push(AdvancePayment {
                    gross: a("1.19"),
                    received_on: None,
                    tax: vec![],
                    reference: None,
                    reference_date: None,
                });
            }),
            Path::term(BtId(113)),
        ),
    ];
    for (mutate, path) in cases {
        let mut h = header();
        mutate(&mut h);
        let err = from_billing(&doc, h).expect_err("a derived term in the header");
        assert_eq!(err, SeamError::DerivedInHeader(path), "{err}");
        assert_eq!(err.paths(), [path]);
        assert!(err.to_string().contains(&path.to_string()), "{err}");
    }
}

#[test]
fn a_header_bt_5_that_agrees_is_kept_and_none_is_filled_in() {
    let doc = simple();
    let mut h = header();
    assert_eq!(h.currency.as_ref().map(Code::as_str), Some("EUR"));
    from_billing(&doc, h.clone()).expect("an agreeing BT-5 does not compete");
    h.currency = None;
    let inv = convert(&doc, h);
    assert_eq!(inv.currency.as_ref().map(Code::as_str), Some("EUR"));
}

#[test]
fn a_vat_free_document_is_refused_naming_bt_151() {
    let doc = Document::builder(Currency::EUR, AmountScale::EN16931)
        .line(Line::fixed("ohne USt", dec!(100)))
        .build()
        .unwrap();
    let err = from_billing(&doc, header()).unwrap_err();
    assert_eq!(
        err,
        SeamError::VatFree(Path::at_term(Group::Line, 0, BtId(151)))
    );
    assert!(err.to_string().contains("BR-CO-04"), "{err}");
}

#[test]
fn a_scale_finer_than_two_decimals_is_refused_naming_the_fix() {
    let scale = AmountScale::new(3, Rounding::HalfAwayFromZero).unwrap();
    let doc = Document::builder(Currency::EUR, scale)
        .default_vat(vat(dec!(19)))
        .line(Line::fixed("fein", dec!(1.005)))
        .build()
        .unwrap();
    let err = from_billing(&doc, header()).unwrap_err();
    assert_eq!(err, SeamError::ScaleTooFine { decimals: 3 });
    assert!(err.to_string().contains("AmountScale::EN16931"), "{err}");
    assert_eq!(err.paths(), [Path::group(Group::Totals)]);
}

#[test]
fn line_adjustments_and_the_price_basis_cross() {
    let doc = builder()
        .line(
            Line::new(
                "Arbeitspreis",
                Quantity::new(dec!(1000), "KWH"),
                UnitPrice::new(Price::new(dec!(30)))
                    .with_base_quantity(dec!(100))
                    .with_unit("KWH"),
            )
            .with_adjustment(
                LineAdjustment::allowance(dec!(15))
                    .with_basis(dec!(300), Percent::new(dec!(5)))
                    .with_reason("Treuebonus"),
            )
            .with_adjustment(LineAdjustment::charge(dec!(2)).with_reason_code("FC")),
        )
        .build()
        .unwrap();
    let inv = convert(&doc, header());
    let line = &inv.lines[0];
    assert_eq!(line.quantity.into_decimal(), dec!(1000));
    assert_eq!(line.unit_code.as_str(), "KWH");
    assert_eq!(line.price.net_price.into_decimal(), dec!(30));
    assert_eq!(
        line.price.base_quantity.map(|q| q.into_decimal()),
        Some(dec!(100))
    );
    assert_eq!(
        line.price.base_quantity_code.as_ref().map(Code::as_str),
        Some("KWH")
    );
    assert_eq!(line.allowances.len(), 1);
    assert_eq!(line.allowances[0].amount, a("15.00"));
    assert_eq!(line.allowances[0].base_amount, Some(a("300.00")));
    assert_eq!(
        line.allowances[0].percentage,
        Some(Percentage::new(dec!(5)))
    );
    assert_eq!(line.allowances[0].reason.as_deref(), Some("Treuebonus"));
    assert_eq!(line.charges[0].amount, a("2.00"));
    assert_eq!(
        line.charges[0].reason_code.as_ref().map(Code::as_str),
        Some("FC")
    );
    assert_eq!(line.net_amount, a("287.00")); // 1000 × 30 / 100 − 15 + 2
}

#[test]
fn optional_totals_are_absent_rather_than_zero() {
    let doc = simple();
    let inv = convert(&doc, header());
    assert_eq!(inv.totals.allowance_total, None, "BT-107");
    assert_eq!(inv.totals.charge_total, None, "BT-108");
    assert_eq!(inv.totals.vat_total_accounting, None, "BT-111");
    assert_eq!(inv.vat_accounting_currency, None, "BT-6");
    assert_eq!(inv.totals.paid, None, "BT-113");
    assert_eq!(inv.totals.rounding, None, "BT-114");
}

// ── BT-20 ─────────────────────────────────────────────────────────────────────

#[test]
fn a_rendered_skonto_passes_br_de_18_and_satisfies_br_co_25() {
    let terms = PaymentTerms::new("Zahlbar innerhalb 30 Tagen ohne Abzug.")
        .unwrap()
        .with_discount(EarlyPaymentDiscount::new(10, Percentage::new(dec!(2))).unwrap());
    let mut h = header();
    h.payment_terms = Some(terms.to_string());
    let doc = p1();
    let inv = convert(&doc, h);
    assert!(!profiles::XRECHNUNG.validate(&inv).has("BR-DE-18"));
    assert_eq!(
        inv.payment_terms.as_deref(),
        Some("Zahlbar innerhalb 30 Tagen ohne Abzug.\n#SKONTO#TAGE=10#PROZENT=2.00#\n")
    );

    // BT-20 is one of the two ways to satisfy BR-CO-25.
    let mut no_due = inv.clone();
    no_due.due_date = None;
    assert!(!validate(&no_due).has("BR-CO-25"));

    // `billing` computes what the discount takes off; the terms only state it.
    let skonto = billing::EarlyPaymentDiscount {
        percent: Percent::new(dec!(2)),
    };
    assert_eq!(skonto.amount(&doc).unwrap().to_string(), "10.49000"); // 2 % of 524.70
}

// ── Credits, advances, corrections ────────────────────────────────────────────

#[test]
fn itemised_advances_reach_the_advance_payment_extension() {
    let doc = builder()
        .line(Line::fixed("Bauleistung", dec!(1000)))
        .payment(Payment::Advance(Advance {
            vat: vat(dec!(19)),
            net: dec!(300),
            tax: dec!(57),
        }))
        .payment(Payment::Advance(Advance {
            vat: vat(dec!(19)),
            net: dec!(200),
            tax: dec!(38),
        }))
        .build()
        .unwrap();
    let mut inv = convert(&doc, header());
    assert_eq!(inv.totals.paid, Some(a("595.00")));
    let adv = &inv.extensions.advance_payments;
    assert_eq!(adv.len(), 2);
    assert_eq!(adv[0].gross, a("357.00"));
    assert_eq!(adv[0].tax[0].taxable_amount, a("300.00"));
    assert_eq!(adv[0].tax[0].tax_amount, a("57.00"));
    assert_eq!(adv[0].tax[0].category.as_str(), "S");
    assert_eq!(adv[0].tax[0].rate, Some(Percentage::new(dec!(19))));
    // Σ gross of the itemised advances is BT-113 when nothing else was paid.
    let sum = InvoiceAmount::checked_sum(adv.iter().map(|x| x.gross)).unwrap();
    assert_eq!(Some(sum), inv.totals.paid);
    // Reference and dates left `billing`; the caller adds them by index.
    assert_eq!(adv[0].reference, None);
    inv.extensions.advance_payments[0].reference_date = Some(Date::parse("2026-03-01").unwrap());
    assert_valid(&inv);
}

#[test]
fn credits_land_where_billing_placed_them() {
    let before = builder()
        .line(Line::fixed("Abo", dec!(400)))
        .line(Line::fixed("Buch", dec!(100)).with_vat(vat(dec!(7))))
        .credit(Credit::before_tax("Guthaben", dec!(50)))
        .build()
        .unwrap();
    let inv = convert(&before, header());
    assert_eq!(inv.allowances.len(), 2, "one allowance per VAT group");
    assert_eq!(inv.totals.allowance_total, Some(a("50.00")));
    assert_eq!(inv.totals.paid, None);

    let after = builder()
        .line(Line::fixed("Abo", dec!(100)))
        .credit(Credit::after_tax("Wallet", dec!(50)))
        .build()
        .unwrap();
    let inv = convert(&after, header());
    assert!(inv.allowances.is_empty());
    assert_eq!(inv.totals.paid, Some(a("50.00")));
    assert_eq!(inv.totals.due, a("69.00"));
}

#[test]
fn a_cap_is_a_negative_quantity_at_a_positive_price() {
    let doc = builder()
        .line(Line::fixed("Ladevorgang", dec!(300)))
        .bound(Bound::maximum(Basis::Net, dec!(250), "Preisobergrenze"))
        .build()
        .unwrap();
    let inv = convert(&doc, header());
    let cap = inv.lines.last().unwrap();
    assert_eq!(cap.id, "2", "the bound line comes last");
    assert_eq!(cap.quantity.into_decimal(), dec!(-1));
    assert_eq!(cap.price.net_price.into_decimal(), dec!(50));
    assert_eq!(cap.net_amount, a("-50.00"));
    assert_eq!(inv.totals.taxable_total, a("250.00"));
}

#[test]
fn each_split_share_converts_as_billing_states_it() {
    let doc = p1();
    let shares = doc.split(&[dec!(1), dec!(2)]).unwrap();
    assert_eq!(shares.len(), 2);
    for share in &shares {
        let inv = convert(share, header());
        for (row, tax) in inv
            .vat_breakdown
            .iter()
            .zip(share.tax_partition().expect("a split share"))
        {
            assert_eq!(row.tax_amount, amt(*tax), "BT-117 is the partition");
        }
    }
}

#[test]
fn a_negated_document_is_a_negative_invoice_under_380() {
    let doc = p1().negated();
    let inv = convert(&doc, header());
    assert!(inv.totals.due.is_negative());
    assert_eq!(inv.kind, DocumentKind::Invoice);
}

#[test]
fn a_credit_note_over_a_negated_document_is_refused_naming_bt_3_and_bt_115() {
    let doc = p1().negated();
    for h in [
        credit_note_header(),
        {
            let mut h = header();
            h.kind = DocumentKind::CreditNote;
            h
        },
        {
            let mut h = header();
            h.type_code = Some(Code::new("381"));
            h
        },
    ] {
        let err = from_billing(&doc, h).unwrap_err();
        assert_eq!(
            err,
            SeamError::NegatedCreditNote {
                payable: a("-524.70")
            }
        );
        assert_eq!(err.paths(), [Path::term(BtId(3)), Path::term(BtId(115))]);
        let msg = err.to_string();
        assert!(msg.contains("BT-3") && msg.contains("BT-115"), "{msg}");
    }
    // The original amounts under 381 are the credit note.
    let inv = convert(&p1(), credit_note_header());
    assert_eq!(inv.kind, DocumentKind::CreditNote);
    assert_eq!(inv.totals.due, a("524.70"));
}

#[test]
fn category_o_states_no_line_rate_and_a_zero_breakdown_rate() {
    let o = Vat::exempt(
        TaxCategory::OutOfScope,
        Exemption::Text("Nicht steuerbar".to_owned()),
    )
    .unwrap();
    let doc = Document::builder(Currency::EUR, AmountScale::EN16931)
        .default_vat(o)
        .line(Line::fixed("Schadensersatz", dec!(100)))
        .build()
        .unwrap();
    // BR-O-02: no VAT identifiers beside category O; BR-CO-26 then wants the
    // seller identified otherwise (BT-30).
    let mut h = header();
    h.seller.vat_identifier = None;
    h.seller.legal_registration = Some(Identifier::new("HRB 12345"));
    h.buyer.vat_identifier = None;
    let inv = convert(&doc, h);
    assert_eq!(inv.lines[0].vat.category.as_str(), "O");
    assert_eq!(inv.lines[0].vat.rate, None, "BT-152 absent (BR-O-05)");
    assert_eq!(inv.vat_breakdown[0].rate, Some(Percentage::ZERO), "BT-119");
    assert_eq!(
        inv.vat_breakdown[0].exemption_reason.as_deref(),
        Some("Nicht steuerbar")
    );
}

#[test]
fn an_exemption_crosses_as_bt_120_and_bt_121() {
    let e = Vat::exempt(
        TaxCategory::Exempt,
        Exemption::Both {
            text: "Steuerfreie Heilbehandlung".to_owned(),
            code: "VATEX-EU-132".to_owned(),
        },
    )
    .unwrap();
    let doc = builder()
        .line(Line::fixed("Behandlung", dec!(80)).with_vat(e))
        .line(Line::fixed("Material", dec!(20)))
        .build()
        .unwrap();
    let inv = convert(&doc, header());
    let row = inv
        .vat_breakdown
        .iter()
        .find(|r| r.category.as_str() == "E")
        .unwrap();
    assert_eq!(
        row.exemption_reason.as_deref(),
        Some("Steuerfreie Heilbehandlung")
    );
    assert_eq!(
        row.exemption_reason_code.as_ref().map(Code::as_str),
        Some("VATEX-EU-132")
    );
}

#[test]
fn vat_accounting_crosses_as_bt_6_and_bt_111_together() {
    let doc = builder()
        .line(Line::fixed("Service", dec!(100)))
        .vat_accounting(VatAccounting {
            currency: Currency::new("DKK").unwrap(),
            rate: dec!(7.4589),
        })
        .build()
        .unwrap();
    let inv = convert(&doc, header());
    assert_eq!(
        inv.vat_accounting_currency.as_ref().map(Code::as_str),
        Some("DKK")
    );
    assert_eq!(inv.totals.vat_total_accounting, Some(a("141.72"))); // 19.00 × 7.4589
}

#[test]
fn cash_rounding_is_bt_114() {
    let inv = convert(&p1(), header());
    assert_eq!(inv.totals.rounding, Some(a("0.01")));
    assert_eq!(
        inv.totals.due,
        inv.totals
            .gross_total
            .checked_add(inv.totals.rounding.unwrap())
            .unwrap()
    );
}
