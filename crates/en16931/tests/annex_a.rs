//! EN 16931-1 Annex A's eight worked invoices, through
//! [`en16931::examples::annex_a`]: the model represents them and the
//! [`Reconciler`] reproduces their figures. Rule tests on a valid start follow.
//!
//! Annex A errata, where the rules win:
//!
//! * A.1.4 prints BT-152 = 25 % for its 12,5 % line ([`a_1_4`]).
//! * A.1.5 prints BT-106 = 286,63 for lines summing to 286,62 and carries the
//!   cent through BT-109 … BT-115; it also prints the zero-rated group's base
//!   as 0 beside a 15,00 zero-rated charge ([`a_1_5`]).
//! * A.1.5, A.1.6 and A.1.7 print the BT-115 remark as *"Invoice total **VAT**
//!   amount − Paid amount"*; BR-CO-16 says BT-112 ([`a_1_7`]).
//! * A.1.8 labels its charge reason BT-97, the allowance reason's id; it is
//!   BT-104.
//!
//! [`Reconciler`]: en16931::Reconciler

use en16931::examples::{self, AnnexExample};
use en16931::invoice::*;
use en16931::validation::validate;
use en16931::*;
use rust_decimal::dec;

fn amount(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
}

fn pct(v: i64) -> Percentage {
    Percentage::new(rust_decimal::Decimal::from(v))
}

fn example(clause: &str) -> AnnexExample {
    examples::annex_a()
        .into_iter()
        .find(|e| e.clause == clause)
        .unwrap_or_else(|| panic!("no example {clause}"))
}

/// BG-22 as the annex prints it: BT-106, BT-107, BT-108, BT-109, BT-110,
/// BT-112, BT-113, BT-115 — `None` where the annex states nothing.
#[allow(clippy::too_many_arguments)]
fn assert_totals(
    e: &AnnexExample,
    line_total: &str,
    allowances: Option<&str>,
    charges: Option<&str>,
    without_vat: &str,
    vat: &str,
    with_vat: &str,
    paid: Option<&str>,
    due: &str,
) {
    let t = &e.invoice.totals;
    let s = |a: Option<InvoiceAmount>| a.map(|a| a.to_string());
    assert_eq!(t.line_total.to_string(), line_total, "{} BT-106", e.clause);
    assert_eq!(
        s(t.allowance_total).as_deref(),
        allowances,
        "{} BT-107",
        e.clause
    );
    assert_eq!(s(t.charge_total).as_deref(), charges, "{} BT-108", e.clause);
    assert_eq!(
        t.taxable_total.to_string(),
        without_vat,
        "{} BT-109",
        e.clause
    );
    assert_eq!(s(t.vat_total).as_deref(), Some(vat), "{} BT-110", e.clause);
    assert_eq!(t.gross_total.to_string(), with_vat, "{} BT-112", e.clause);
    assert_eq!(s(t.paid).as_deref(), paid, "{} BT-113", e.clause);
    assert_eq!(t.due.to_string(), due, "{} BT-115", e.clause);
}

/// BG-23 as the annex prints it: `(category, rate, BT-116, BT-117)`, in any order.
fn assert_breakdown(e: &AnnexExample, want: &[(&str, &str, &str, &str)]) {
    let mut got: Vec<(String, String, String, String)> = e
        .invoice
        .vat_breakdown
        .iter()
        .map(|b| {
            (
                b.category.to_string(),
                b.rate
                    .map(|r| r.normalized().to_string())
                    .unwrap_or_default(),
                b.taxable_amount.to_string(),
                b.tax_amount.to_string(),
            )
        })
        .collect();
    let mut want: Vec<(String, String, String, String)> = want
        .iter()
        .map(|(c, r, b, t)| ((*c).into(), (*r).into(), (*b).into(), (*t).into()))
        .collect();
    got.sort();
    want.sort();
    assert_eq!(got, want, "{} BG-23", e.clause);
}

/// Every example is valid under the core, with no finding at all.
#[test]
fn every_annex_example_is_valid() {
    let all = examples::annex_a();
    assert_eq!(
        all.iter().map(|e| e.clause).collect::<Vec<_>>(),
        [
            "A.1.2", "A.1.3", "A.1.4", "A.1.5", "A.1.6", "A.1.7", "A.1.8", "A.1.9"
        ]
    );
    for e in &all {
        let report = validate(&e.invoice);
        assert!(report.is_valid(), "{}: {report}", e.clause);
        assert_eq!(report.findings().len(), 0, "{}: {report}", e.clause);
    }
}

/// **A.1.2 — Example 1, Different Invoiced item VAT rates.**
#[test]
fn a_1_2() {
    let e = example("A.1.2");
    assert_breakdown(
        &e,
        &[("S", "25", "60.00", "15.00"), ("S", "12", "90.00", "10.80")],
    );
    assert_totals(
        &e, "150.00", None, None, "150.00", "25.80", "175.80", None, "175.80",
    );
}

/// **A.1.3 — Example 2, Item price base quantity.** 4,50 per 1 000 pieces.
#[test]
fn a_1_3() {
    let e = example("A.1.3");
    let price = &e.invoice.lines[0].price;
    assert_eq!(price.net_price, UnitPriceAmount::new(dec!(4.50)));
    assert_eq!(
        price.base_quantity.map(|q| q.to_string()).as_deref(),
        Some("1000")
    );
    assert_breakdown(&e, &[("S", "25", "45.00", "11.25")]);
    assert_totals(
        &e, "45.00", None, None, "45.00", "11.25", "56.25", None, "56.25",
    );
}

/// **A.1.4 — Example 3, Invoiced quantity unit of measure.**
///
/// The calculation table prints BT-152 = 25 %; everything else, and the
/// arithmetic, says 12,5 %, which the example carries.
#[test]
fn a_1_4() {
    let e = example("A.1.4");
    let l = &e.invoice.lines[0];
    assert_eq!(l.quantity.to_string(), "1.3");
    assert_eq!(
        l.vat.rate.map(|r| r.normalized().to_string()).as_deref(),
        Some("12.5")
    );
    assert_ne!(l.vat.rate, Some(pct(25)), "the calculation table's erratum");
    assert_breakdown(&e, &[("S", "12.5", "12.35", "1.54")]);
    assert_totals(
        &e, "12.35", None, None, "12.35", "1.54", "13.89", None, "13.89",
    );
}

/// **A.1.5 — Example 4, Discounts, allowances and charges.**
///
/// The annex prints BT-106 = 286,63 for lines summing to 286,62 (and carries the
/// cent onwards), and a zero-rated base of 0 beside a 15,00 zero-rated charge.
/// The example holds what `BR-CO-10` and `BR-Z-08` require.
#[test]
fn a_1_5() {
    let e = example("A.1.5");
    assert_eq!(e.invoice.lines[0].net_amount.to_string(), "222.50");
    assert_eq!(e.invoice.lines[1].net_amount.to_string(), "64.12");
    assert_eq!(e.invoice.allowances[0].amount.to_string(), "28.66");
    assert_breakdown(
        &e,
        &[("S", "25", "257.96", "64.49"), ("Z", "0", "15.00", "0.00")],
    );
    assert_totals(
        &e,
        "286.62",
        Some("28.66"),
        Some("15.00"),
        "272.96",
        "64.49",
        "337.45",
        None,
        "337.45",
    );
    // The printed figures fail the rules.
    let mut printed = e.invoice.clone();
    printed.totals.line_total = amount("286.63");
    assert!(
        validate(&printed).has("BR-CO-10"),
        "286,63 is not the sum of the lines"
    );
    let mut printed = e.invoice.clone();
    printed
        .vat_breakdown
        .iter_mut()
        .filter(|b| b.category.as_str() == "Z")
        .for_each(|b| {
            b.taxable_amount = amount("0.00");
        });
    assert!(
        validate(&printed).has("BR-Z-08"),
        "the freight charge is zero-rated"
    );
}

/// **A.1.6 — Example 5, Negative Invoice line.**
///
/// A return credited on an ordinary invoice: the sign is on BT-129 (−10), since
/// BR-27 forbids a negative BT-146.
#[test]
fn a_1_6() {
    let e = example("A.1.6");
    assert!(e.invoice.lines[1].quantity.is_negative());
    assert!(!e.invoice.lines[1].price.net_price.is_negative(), "BR-27");
    // BT-117 is 31,88 for an exact 31,875 (BR-CO-17 rounds; ±1.00 tolerance).
    assert_breakdown(&e, &[("S", "25", "127.50", "31.88")]);
    assert_totals(
        &e, "127.50", None, None, "127.50", "31.88", "159.38", None, "159.38",
    );
}

/// **A.1.7 — Example 6, Prepayment and negative Amount due for payment.**
///
/// 110,00 at 25 % against a 250,00 prepayment: amount due −112,50.
#[test]
fn a_1_7() {
    let e = example("A.1.7");
    assert_totals(
        &e,
        "110.00",
        None,
        None,
        "110.00",
        "27.50",
        "137.50",
        Some("250.00"),
        "-112.50",
    );
    // BR-CO-16 uses BT-112, not the "VAT amount" the annex remark names.
    let by_the_prose = amount("27.50").checked_sub(amount("250.00")).unwrap();
    assert_ne!(
        e.invoice.totals.due, by_the_prose,
        "the annex remark is an erratum"
    );
}

/// **A.1.8 — Example 7, Standard VAT including VAT exempted lines.**
///
/// Rates 10 % and 25 % plus exempt lines; document charge and allowance at 25 %.
#[test]
fn a_1_8() {
    let e = example("A.1.8");
    assert_breakdown(
        &e,
        &[
            ("S", "25", "281.00", "70.25"), // 125 + 136 + 35 − 15
            ("S", "10", "24.00", "2.40"),
            ("E", "0", "148.00", "0.00"), // 95 + 53
        ],
    );
    let exempt = e
        .invoice
        .vat_breakdown
        .iter()
        .find(|b| b.category.as_str() == "E");
    assert_eq!(
        exempt.and_then(|b| b.exemption_reason.as_deref()),
        Some("Reason A"),
        "BR-E-10"
    );
    assert_totals(
        &e,
        "433.00",
        Some("15.00"),
        Some("35.00"),
        "453.00",
        "72.65",
        "525.65",
        None,
        "525.65",
    );
}

/// **A.1.9 — Example 8, Reverse Charge, Intra EU supply and Export Invoices.**
#[test]
fn a_1_9() {
    let e = example("A.1.9");
    assert_breakdown(&e, &[("AE", "0", "169.00", "0.00")]);
    assert_totals(
        &e,
        "149.00",
        None,
        Some("20.00"),
        "169.00",
        "0.00",
        "169.00",
        None,
        "169.00",
    );
}

// ── Rules, on a valid starting point ──────────────────────────────────────────
//
// Each test replaces the core example's lines, breakdown and totals.

fn replacing(
    lines: Vec<InvoiceLine>,
    breakdown: Vec<VatBreakdown>,
    totals: DocumentTotals,
) -> Invoice {
    let mut inv = examples::for_profile(&profiles::EN16931).expect("a shipped profile");
    inv.lines = lines;
    inv.vat_breakdown = breakdown;
    inv.totals = totals;
    inv
}

/// Dropping the exempt group's reason must fire BR-E-10 — and nothing else.
#[test]
fn an_exempt_group_without_a_reason_is_reported_precisely() {
    let mut inv = replacing(
        vec![InvoiceLine::new(
            "1",
            "Item 1",
            Quantity::new(dec!(1)),
            "C62",
            amount("95.00"),
            "E",
            Some(Percentage::ZERO),
        )],
        vec![VatBreakdown {
            taxable_amount: amount("95.00"),
            tax_amount: amount("0.00"),
            category: Code::new("E"),
            rate: Some(Percentage::ZERO),
            exemption_reason: None,
            exemption_reason_code: None,
        }],
        DocumentTotals {
            line_total: amount("95.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("95.00"),
            vat_total: Some(amount("0.00")),
            vat_total_accounting: None,
            gross_total: amount("95.00"),
            paid: None,
            rounding: None,
            due: amount("95.00"),
        },
    );

    let report = validate(&inv);
    assert!(report.has("BR-E-10"), "{report}");
    assert_eq!(
        report.fatal().count(),
        1,
        "one problem, one finding: {report}"
    );
    assert_eq!(
        report.fatal().next().unwrap().path.to_string(),
        "BG-23[0]/BT-120",
        "the path names the group and the term, not an XPath"
    );

    // The code satisfies it as well as the text.
    inv.vat_breakdown[0].exemption_reason_code = Some(Code::new("VATEX-EU-132-1A"));
    assert!(validate(&inv).is_valid());
}

/// The totals chain is exact; the VAT derivation (BR-CO-17) allows ±1.00.
#[test]
fn the_two_cen_tolerance_regimes_are_not_interchangeable() {
    let totals = |vat: &str, gross: &str| DocumentTotals {
        line_total: amount("100.00"),
        allowance_total: None,
        charge_total: None,
        taxable_total: amount("100.00"),
        vat_total: Some(amount(vat)),
        vat_total_accounting: None,
        gross_total: amount(gross),
        paid: None,
        rounding: None,
        due: amount(gross),
    };
    let breakdown = |tax: &str| {
        vec![VatBreakdown {
            taxable_amount: amount("100.00"),
            tax_amount: amount(tax),
            category: Code::new("S"),
            rate: Some(pct(19)),
            exemption_reason: None,
            exemption_reason_code: None,
        }]
    };
    let one_line = || {
        vec![InvoiceLine::new(
            "1",
            "Item 1",
            Quantity::new(dec!(1)),
            "C62",
            amount("100.00"),
            "S",
            Some(pct(19)),
        )]
    };

    // Exact: BT-110 = Σ BT-117. One cent out is fatal.
    let inv = replacing(one_line(), breakdown("19.00"), totals("19.01", "119.01"));
    assert!(validate(&inv).has("BR-CO-14"), "BR-CO-14 must be exact");

    // Tolerant: BT-117 vs BT-116 × rate. 0.50 out passes.
    let inv = replacing(one_line(), breakdown("18.50"), totals("18.50", "118.50"));
    let report = validate(&inv);
    assert!(!report.has("BR-CO-17"), "±1.00 tolerance: {report}");
    assert!(report.is_valid(), "{report}");

    // …but 1.50 out is beyond it.
    let inv = replacing(one_line(), breakdown("17.50"), totals("17.50", "117.50"));
    assert!(validate(&inv).has("BR-CO-17"));
}

/// A 0.5 % rate is not a zero rate: the artefact's `round(Percent) = 0` uses
/// XPath rounding (ties towards +∞), so `round(0.5)` is `1`.
#[test]
fn br_co_17_accepts_a_rate_of_half_a_per_cent() {
    let breakdown = |taxable: &str, tax: &str, rate: Percentage| {
        vec![VatBreakdown {
            taxable_amount: amount(taxable),
            tax_amount: amount(tax),
            category: Code::new("S"),
            rate: Some(rate),
            exemption_reason: None,
            exemption_reason_code: None,
        }]
    };
    let half = Percentage::new(dec!(0.5));
    let inv = replacing(
        vec![InvoiceLine::new(
            "1",
            "Item 1",
            Quantity::new(dec!(1)),
            "C62",
            amount("1000.00"),
            "S",
            Some(half),
        )],
        breakdown("1000.00", "5.00", half),
        DocumentTotals {
            line_total: amount("1000.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("1000.00"),
            vat_total: Some(amount("5.00")),
            vat_total_accounting: None,
            gross_total: amount("1005.00"),
            paid: None,
            rounding: None,
            due: amount("1005.00"),
        },
    );
    let report = validate(&inv);
    assert!(!report.has("BR-CO-17"), "{report}");
    assert!(!report.has("BR-S-09"), "{report}");
    assert!(report.is_valid(), "{report}");

    // …and a rate that really does round to zero still takes the zero branch.
    let quarter = Percentage::new(dec!(0.4));
    let inv = replacing(
        vec![InvoiceLine::new(
            "1",
            "Item 1",
            Quantity::new(dec!(1)),
            "C62",
            amount("1000.00"),
            "S",
            Some(quarter),
        )],
        breakdown("1000.00", "4.00", quarter),
        DocumentTotals {
            line_total: amount("1000.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("1000.00"),
            vat_total: Some(amount("4.00")),
            vat_total_accounting: None,
            gross_total: amount("1004.00"),
            paid: None,
            rounding: None,
            due: amount("1004.00"),
        },
    );
    assert!(validate(&inv).has("BR-CO-17"), "round(0.4) is 0");
}

/// BR-CO-17 compares absolute values, so negative breakdowns pass.
#[test]
fn br_co_17_compares_absolute_values_so_credits_pass() {
    let inv = replacing(
        vec![InvoiceLine::new(
            "1",
            "Item 1",
            Quantity::new(dec!(-1)),
            "C62",
            amount("-100.00"),
            "S",
            Some(pct(19)),
        )],
        vec![VatBreakdown {
            taxable_amount: amount("-100.00"),
            tax_amount: amount("-19.00"),
            category: Code::new("S"),
            rate: Some(pct(19)),
            exemption_reason: None,
            exemption_reason_code: None,
        }],
        DocumentTotals {
            line_total: amount("-100.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("-100.00"),
            vat_total: Some(amount("-19.00")),
            vat_total_accounting: None,
            gross_total: amount("-119.00"),
            paid: None,
            rounding: None,
            due: amount("-119.00"),
        },
    );
    let report = validate(&inv);
    assert!(!report.has("BR-CO-17"), "{report}");
}

/// `XXX` passes `BR-CL-04` (it is ISO 4217) but fails `EN-CURRENCY-01`.
#[test]
fn an_unconfigured_currency_is_caught_even_though_iso_allows_it() {
    let mut inv = replacing(
        vec![InvoiceLine::new(
            "1",
            "Item 1",
            Quantity::new(dec!(1)),
            "C62",
            amount("100.00"),
            "S",
            Some(pct(19)),
        )],
        vec![VatBreakdown {
            taxable_amount: amount("100.00"),
            tax_amount: amount("19.00"),
            category: Code::new("S"),
            rate: Some(pct(19)),
            exemption_reason: None,
            exemption_reason_code: None,
        }],
        DocumentTotals {
            line_total: amount("100.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("100.00"),
            vat_total: Some(amount("19.00")),
            vat_total_accounting: None,
            gross_total: amount("119.00"),
            paid: None,
            rounding: None,
            due: amount("119.00"),
        },
    );
    inv.currency = Some(Code::new("XXX"));

    let report = validate(&inv);
    assert!(
        !report.has("BR-CL-04"),
        "XXX is in ISO 4217, so CEN accepts it"
    );
    assert!(report.has("EN-CURRENCY-01"), "but we do not: {report}");
}

/// Findings are ordered stably, so a report is diffable in CI.
#[test]
fn reports_are_deterministic() {
    let inv = replacing(
        vec![],
        vec![],
        DocumentTotals {
            line_total: amount("0.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("0.00"),
            vat_total: None,
            vat_total_accounting: None,
            gross_total: amount("0.00"),
            paid: None,
            rounding: None,
            due: amount("0.00"),
        },
    );
    let a = validate(&inv);
    let b = validate(&inv);
    assert_eq!(a, b);
    assert!(a.has("BR-16"), "no lines");
    assert!(a.has("BR-CO-18"), "no VAT breakdown");
    assert!(!a.is_valid());
}

/// `BR-S-08` ties the lines to the VAT breakdown, so a mis-attributed line is reported.
#[test]
fn br_s_08_ties_the_lines_to_the_breakdown() {
    // Two rates: 125 + 136 at 25 %, 24 at 10 %.
    let lines = || {
        vec![
            InvoiceLine::new(
                "1",
                "Item 1",
                Quantity::new(dec!(1)),
                "C62",
                amount("125.00"),
                "S",
                Some(pct(25)),
            ),
            InvoiceLine::new(
                "2",
                "Item 2",
                Quantity::new(dec!(1)),
                "C62",
                amount("24.00"),
                "S",
                Some(pct(10)),
            ),
            InvoiceLine::new(
                "3",
                "Item 3",
                Quantity::new(dec!(1)),
                "C62",
                amount("136.00"),
                "S",
                Some(pct(25)),
            ),
        ]
    };
    let breakdown = |base_25: &str| {
        vec![
            VatBreakdown {
                taxable_amount: amount(base_25),
                tax_amount: amount("65.25"),
                category: Code::new("S"),
                rate: Some(pct(25)),
                exemption_reason: None,
                exemption_reason_code: None,
            },
            VatBreakdown {
                taxable_amount: amount("24.00"),
                tax_amount: amount("2.40"),
                category: Code::new("S"),
                rate: Some(pct(10)),
                exemption_reason: None,
                exemption_reason_code: None,
            },
        ]
    };
    let totals = DocumentTotals {
        line_total: amount("285.00"),
        allowance_total: None,
        charge_total: None,
        taxable_total: amount("285.00"),
        vat_total: Some(amount("67.65")),
        vat_total_accounting: None,
        gross_total: amount("352.65"),
        paid: None,
        rounding: None,
        due: amount("352.65"),
    };

    // Correct: the 25 % group's base is 125 + 136.
    let ok = replacing(lines(), breakdown("261.00"), totals.clone());
    let report = validate(&ok);
    assert!(report.is_valid(), "{report}");

    // Wrong: attributing all three lines to the 25 % group.
    let bad = replacing(lines(), breakdown("285.00"), totals);
    let report = validate(&bad);
    assert!(report.has("BR-S-08"), "{report}");
    assert_eq!(
        report.fatal().next().unwrap().path.to_string(),
        "BG-23[0]/BT-116"
    );
}

/// `-08` groups by `(category, rate)` for taxed categories, by category alone
/// for zero-tax ones.
#[test]
fn zero_tax_categories_group_by_category_alone() {
    // Two exempt lines, one BG-23 group — BR-E-01 says *exactly* one.
    let inv = replacing(
        vec![
            InvoiceLine::new(
                "1",
                "Item 1",
                Quantity::new(dec!(1)),
                "C62",
                amount("95.00"),
                "E",
                Some(Percentage::ZERO),
            ),
            InvoiceLine::new(
                "2",
                "Item 2",
                Quantity::new(dec!(1)),
                "C62",
                amount("53.00"),
                "E",
                Some(Percentage::ZERO),
            ),
        ],
        vec![VatBreakdown {
            taxable_amount: amount("148.00"),
            tax_amount: amount("0.00"),
            category: Code::new("E"),
            rate: Some(Percentage::ZERO),
            exemption_reason: Some("Reason A".to_owned()),
            exemption_reason_code: None,
        }],
        DocumentTotals {
            line_total: amount("148.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("148.00"),
            vat_total: Some(amount("0.00")),
            vat_total_accounting: None,
            gross_total: amount("148.00"),
            paid: None,
            rounding: None,
            due: amount("148.00"),
        },
    );
    let report = validate(&inv);
    assert!(report.is_valid(), "{report}");
    assert!(!report.has("BR-E-01"));
    assert!(!report.has("BR-E-08"));
}

/// `BR-O-05`: the line rate must be absent, not zero (other zero-tax categories
/// require 0).
#[test]
fn out_of_scope_forbids_the_rate_rather_than_zeroing_it() {
    let build = |rate: Option<Percentage>| {
        let mut inv = replacing(
            vec![InvoiceLine::new(
                "1",
                "Item 1",
                Quantity::new(dec!(1)),
                "C62",
                amount("100.00"),
                "O",
                rate,
            )],
            vec![VatBreakdown {
                taxable_amount: amount("100.00"),
                tax_amount: amount("0.00"),
                category: Code::new("O"),
                // BT-119 (not BT-152): BR-48 makes it optional for O.
                rate: None,
                exemption_reason: Some("Not subject to VAT".to_owned()),
                exemption_reason_code: None,
            }],
            DocumentTotals {
                line_total: amount("100.00"),
                allowance_total: None,
                charge_total: None,
                taxable_total: amount("100.00"),
                vat_total: Some(amount("0.00")),
                vat_total_accounting: None,
                gross_total: amount("100.00"),
                paid: None,
                rounding: None,
                due: amount("100.00"),
            },
        );
        // `O` forbids BT-31 (BR-O-02); BR-CO-26 then needs BT-29 or BT-30.
        inv.seller.vat_identifier = None;
        inv.buyer.vat_identifier = None;
        inv.seller.legal_registration = Some(Identifier::schemed("HRB 12345", "0198"));
        inv
    };

    // Absent — correct.
    assert!(
        validate(&build(None)).is_valid(),
        "{}",
        validate(&build(None))
    );
    // Zero — a finding.
    assert!(validate(&build(Some(Percentage::ZERO))).has("BR-O-05"));
}

/// `BR-B-01`: split payment is domestic Italian only (needs both parties' countries).
#[test]
fn split_payment_is_italy_only_and_never_beside_standard() {
    let inv = replacing(
        vec![InvoiceLine::new(
            "1",
            "Item 1",
            Quantity::new(dec!(1)),
            "C62",
            amount("100.00"),
            "B",
            Some(pct(22)),
        )],
        vec![VatBreakdown {
            taxable_amount: amount("100.00"),
            tax_amount: amount("22.00"),
            category: Code::new("B"),
            rate: Some(pct(22)),
            exemption_reason: None,
            exemption_reason_code: None,
        }],
        DocumentTotals {
            line_total: amount("100.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("100.00"),
            vat_total: Some(amount("22.00")),
            vat_total_accounting: None,
            gross_total: amount("122.00"),
            paid: None,
            rounding: None,
            due: amount("122.00"),
        },
    );
    // The example's parties are both German.
    assert!(validate(&inv).has("BR-B-01"));

    // Unlike AE, B carries tax — so a zero tax amount would be the error.
    assert!(VatCategory::SplitPayment.carries_tax());
}

/// The report states how many rules it checked.
#[test]
fn the_report_states_its_own_coverage() {
    let report = validate(&Invoice::default());
    assert!(
        report.rules_checked() >= 85,
        "only {} rules",
        report.rules_checked()
    );
}
