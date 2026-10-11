//! A derived [`billing::Document`] and a header become one invoice — behind the
//! `billing` feature, against `billing` 0.16.
//!
//! `billing` derives the amounts; the caller supplies the header (number, dates,
//! BT-3, BT-24, parties, payment instructions, notes and so on).
//! [`from_billing`] combines them:
//!
//! ```
//! use billing::{Adjust, AdjustmentKind, AmountScale, Currency, Document, Line, Percent, Vat};
//! use en16931::billing_adapter::from_billing;
//! use en16931::{Date, Invoice};
//! use rust_decimal::dec;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let doc = Document::builder(Currency::EUR, AmountScale::EN16931)
//!     .default_vat(Vat::standard(Percent::new(dec!(19)))?)
//!     .line(Line::fixed("Grundpreis", dec!(400)))
//!     .line(Line::fixed("Bücher", dec!(100)).with_vat(Vat::standard(Percent::new(dec!(7)))?))
//!     .apportion(AdjustmentKind::Allowance, "10 % Rabatt", Adjust::Percent(Percent::new(dec!(10))))
//!     .build()?;
//!
//! // Everything `billing` does not derive. BT-5 may be stated if it agrees.
//! let header = Invoice::builder(
//!     "urn:cen.eu:en16931:2017", "R-2026-0001", Date::parse("2026-07-31")?, "380", "EUR",
//! )
//! .due_in_days(14)
//! .build();
//!
//! let invoice = from_billing(&doc, header)?;
//! assert_eq!(invoice.totals.vat_total.unwrap().to_string(), "74.70"); // 360 × 19 % + 90 × 7 %
//! assert_eq!(invoice.lines[1].id, "2");                                // BT-126 = index + 1
//! # Ok(()) }
//! ```
//!
//! It maps without validating; run [`crate::validate`] or
//! [`Profile::validate`](crate::Profile::validate) on the result.
//!
//! # What crosses
//!
//! Every amount is taken from the `Document` accessor as is, converted exactly
//! to two decimals, never recomputed or rounded.
//!
//! | `billing` | Business term |
//! |---|---|
//! | `currency()` | BT-5 |
//! | `lines()[i]` | BG-25 line *i*, BT-126 = *i* + 1, order kept (bound lines last) |
//! | `line_net(i)` | BT-131 |
//! | `quantity()` value / unit | BT-129 / BT-130 (the unit *is* the code) |
//! | `price()` price / base quantity / unit | BT-146 / BT-149 / BT-150 (BT-149 stated when not 1 or when BT-150 is) |
//! | the line's `Vat` | BT-151 / BT-152 (no BT-152 for `O`) |
//! | `description()` | BT-153 |
//! | line adjustments + `line_adjustment_amounts(i)` | BG-27 / BG-28 |
//! | `adjustments()` + `adjustment_amount(i)` | BG-20 / BG-21 (description → BT-97/104) |
//! | `vat_groups()` | BG-23, BT-116 … BT-121, in `billing`'s order |
//! | `line_total()` … `payable()` | BT-106 … BT-115 |
//! | `vat_accounting_total()` | BT-6 and BT-111, together |
//! | each `Payment::Advance` | one [`crate::AdvancePayment`] (ZUGFeRD EXTENDED's `BG-X-45`) |
//!
//! BT-107 / BT-108 are stated when an allowance / charge exists, BT-113 /
//! BT-114 when non-zero, BT-6 / BT-111 only with VAT accounting.
//!
//! An advance's reference and dates are not in `billing`; set them on
//! `invoice.extensions.advance_payments[k]` afterwards. Its gross is net plus tax.
//!
//! Not mapped (no business term): line labels, `residual_vat()`, the credit
//! ledger (`credits()`, `credit_applications()`), and the itemisation of
//! `Payment::Gross` entries (part of BT-113).
//!
//! # What is refused
//!
//! Each refusal is a [`SeamError`] naming the business term:
//!
//! - a header carrying a term `billing` derives (a line, an allowance or charge,
//!   a BG-23 entry, a BG-22 amount, BT-6, an advance payment, or a different BT-5);
//! - a scale finer than two decimals — build with `AmountScale::EN16931`;
//! - a VAT-free document (`BR-CO-04`);
//! - a credit note (kind [`DocumentKind::CreditNote`] or BT-3 `381`) over a
//!   negative BT-115, which would reverse the document twice.

use rust_decimal::Decimal;

use crate::bt::{BtId, Group, Path};
use crate::extensions::AdvancePayment;
use crate::invoice::{
    Code, DocumentAllowanceCharge, DocumentKind, DocumentTotals, Invoice, Item,
    LineAllowanceCharge, LineVat, PriceDetails, VatBreakdown,
};
use crate::{InvoiceAmount, InvoiceLine, Percentage, Quantity, UnitPriceAmount};

/// Why a document and a header cannot become one invoice. Every variant names
/// the business term concerned ([`paths`](Self::paths)).
///
/// Not a validation finding: these say the inputs cannot be combined at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SeamError {
    /// The header carries a term `billing` derives; the document's value would
    /// compete with it. Leave it out of the header.
    #[error(
        "the header carries {0}, which the billing document derives; leave it out of the header"
    )]
    DerivedInHeader(Path),

    /// The document's amounts carry more decimals than EN 16931's two.
    #[error(
        "the document is derived at {decimals} decimals; EN 16931 amounts have two — build it \
         with `AmountScale::EN16931` rather than rounding here"
    )]
    ScaleTooFine {
        /// The document's scale.
        decimals: u8,
    },

    /// The document (or this line or adjustment) carries no VAT; EN 16931
    /// requires BT-151 on every line (`BR-CO-04`).
    #[error("{0}: the billing document is VAT-free; EN 16931 requires a VAT category (BR-CO-04)")]
    VatFree(Path),

    /// A credit-note header over a document whose BT-115 is negative: the
    /// document would be reversed twice, once by BT-3 and once by its signs.
    #[error(
        "BT-3 says credit note and BT-115 is {payable}: that reverses the document twice; \
         send the original amounts under 381, or the negated ones under 380"
    )]
    NegatedCreditNote {
        /// The document's BT-115.
        payable: InvoiceAmount,
    },
}

impl SeamError {
    /// The business terms (or groups) the refusal concerns.
    #[must_use]
    pub fn paths(&self) -> Vec<Path> {
        match self {
            Self::DerivedInHeader(p) | Self::VatFree(p) => vec![*p],
            Self::ScaleTooFine { .. } => vec![Path::group(Group::Totals)],
            Self::NegatedCreditNote { .. } => {
                vec![Path::term(BtId(3)), Path::term(BtId(115))]
            }
        }
    }
}

/// Combine a derived `billing` document with a header into one invoice.
///
/// `header` is an ordinary [`Invoice`] holding the terms `billing` does not
/// own; they are kept unchanged. Lines, allowances, charges, the VAT breakdown,
/// the totals, BT-5, BT-6 and the advance payments come from `doc`. See the
/// [module docs](self) for the mapping and the refusals.
///
/// # Errors
/// A [`SeamError`] naming the term that cannot cross.
pub fn from_billing(doc: &billing::Document, header: Invoice) -> Result<Invoice, SeamError> {
    check_header(doc, &header)?;
    let decimals = doc.scale().decimals;
    if decimals > 2 {
        return Err(SeamError::ScaleTooFine { decimals });
    }
    if doc.vat_groups().is_empty() {
        let at = if doc.lines().is_empty() {
            Path::term(BtId(151))
        } else {
            Path::at_term(Group::Line, 0, BtId(151))
        };
        return Err(SeamError::VatFree(at));
    }
    let is_credit_note = header.kind == DocumentKind::CreditNote
        || header
            .type_code
            .as_ref()
            .is_some_and(|c| c.as_str() == "381");
    if is_credit_note && doc.payable().is_negative() {
        return Err(SeamError::NegatedCreditNote {
            payable: amount(doc.payable(), decimals)?,
        });
    }

    let mut inv = header;
    inv.currency = Some(Code::new(doc.currency().code()));

    for (i, line) in doc.lines().iter().enumerate() {
        inv.lines.push(map_line(doc, i, line, decimals)?);
    }

    for (i, adj) in doc.adjustments().iter().enumerate() {
        let (group, vat_term) = match adj.kind() {
            billing::AdjustmentKind::Allowance => (Group::DocumentAllowance, BtId(95)),
            billing::AdjustmentKind::Charge => (Group::DocumentCharge, BtId(102)),
        };
        let target = match group {
            Group::DocumentAllowance => &mut inv.allowances,
            _ => &mut inv.charges,
        };
        let vat = adj
            .vat()
            .map(line_vat)
            .ok_or_else(|| SeamError::VatFree(Path::at_term(group, target.len(), vat_term)))?;
        let (base_amount, percentage) = basis(adj.basis(), decimals)?;
        target.push(DocumentAllowanceCharge {
            amount: amount(doc.adjustment_amount(i), decimals)?,
            base_amount,
            percentage,
            vat,
            reason: Some(adj.description().to_owned()).filter(|d| !d.is_empty()),
            reason_code: adj.reason_code().map(Code::new),
        });
    }

    for g in doc.vat_groups() {
        inv.vat_breakdown.push(VatBreakdown {
            taxable_amount: amount(g.taxable_base(), decimals)?,
            tax_amount: amount(g.tax(), decimals)?,
            category: Code::new(g.category().code()),
            // BT-119 is stated for every category, `O` included (0 %).
            rate: Some(Percentage::new(g.rate_percent().value())),
            exemption_reason: g.exemption().and_then(|e| e.text()).map(str::to_owned),
            exemption_reason_code: g.exemption().and_then(|e| e.code()).map(Code::new),
        });
    }

    let accounting = doc
        .vat_accounting_total()
        .map(|(currency, total)| Ok((currency, amount(total, decimals)?)))
        .transpose()?;
    inv.vat_accounting_currency = accounting.map(|(c, _)| Code::new(c.code()));
    let kinds = || {
        doc.adjustments()
            .iter()
            .map(billing::DocumentAdjustment::kind)
    };
    let has = |k: billing::AdjustmentKind| kinds().any(|x| x == k);
    inv.totals = DocumentTotals {
        line_total: amount(doc.line_total(), decimals)?,
        allowance_total: has(billing::AdjustmentKind::Allowance)
            .then(|| amount(doc.allowance_total(), decimals))
            .transpose()?,
        charge_total: has(billing::AdjustmentKind::Charge)
            .then(|| amount(doc.charge_total(), decimals))
            .transpose()?,
        taxable_total: amount(doc.tax_exclusive_total(), decimals)?,
        vat_total: Some(amount(doc.vat_total(), decimals)?),
        vat_total_accounting: accounting.map(|(_, a)| a),
        gross_total: amount(doc.tax_inclusive_total(), decimals)?,
        paid: non_zero(doc.prepaid(), decimals)?,
        rounding: non_zero(doc.rounding(), decimals)?,
        due: amount(doc.payable(), decimals)?,
    };

    for p in doc.payments() {
        let billing::Payment::Advance(a) = p else {
            continue;
        };
        let net = decimal(a.net, decimals)?;
        let tax = decimal(a.tax, decimals)?;
        inv.extensions.advance_payments.push(AdvancePayment {
            // `billing`'s own definition of an advance's gross; exact.
            gross: net
                .checked_add(tax)
                .map_err(|_| SeamError::ScaleTooFine { decimals })?,
            received_on: None,
            tax: vec![VatBreakdown {
                taxable_amount: net,
                tax_amount: tax,
                category: Code::new(a.vat.category().code()),
                rate: Some(Percentage::new(a.vat.rate_percent().value())),
                exemption_reason: a.vat.exemption().and_then(|e| e.text()).map(str::to_owned),
                exemption_reason_code: a.vat.exemption().and_then(|e| e.code()).map(Code::new),
            }],
            reference: None,
            reference_date: None,
        });
    }

    Ok(inv)
}

/// The first term the header carries that `billing` derives.
fn check_header(doc: &billing::Document, h: &Invoice) -> Result<(), SeamError> {
    let refuse = |p: Path| Err(SeamError::DerivedInHeader(p));
    if h.currency
        .as_ref()
        .is_some_and(|c| c.as_str() != doc.currency().code())
    {
        return refuse(Path::term(BtId(5)));
    }
    if h.vat_accounting_currency.is_some() {
        return refuse(Path::term(BtId(6)));
    }
    if !h.allowances.is_empty() {
        return refuse(Path::group(Group::DocumentAllowance));
    }
    if !h.charges.is_empty() {
        return refuse(Path::group(Group::DocumentCharge));
    }
    let t = &h.totals;
    let zero = InvoiceAmount::default();
    let totals = [
        (t.line_total != zero, 106),
        (t.allowance_total.is_some(), 107),
        (t.charge_total.is_some(), 108),
        (t.taxable_total != zero, 109),
        (t.vat_total.is_some(), 110),
        (t.vat_total_accounting.is_some(), 111),
        (t.gross_total != zero, 112),
        (t.paid.is_some(), 113),
        (t.rounding.is_some(), 114),
        (t.due != zero, 115),
    ];
    if let Some((_, bt)) = totals.into_iter().find(|(set, _)| *set) {
        return refuse(Path::term(BtId(bt)));
    }
    if !h.extensions.advance_payments.is_empty() {
        return refuse(Path::term(BtId(113)));
    }
    if !h.vat_breakdown.is_empty() {
        return refuse(Path::group(Group::VatBreakdown));
    }
    if !h.lines.is_empty() {
        return refuse(Path::group(Group::Line));
    }
    Ok(())
}

fn map_line(
    doc: &billing::Document,
    i: usize,
    line: &billing::Line,
    decimals: u8,
) -> Result<InvoiceLine, SeamError> {
    let vat = line
        .vat()
        .map(line_vat)
        .ok_or_else(|| SeamError::VatFree(Path::at_term(Group::Line, i, BtId(151))))?;
    let mut allowances = Vec::new();
    let mut charges = Vec::new();
    for (adj, reduced) in line
        .adjustments()
        .iter()
        .zip(doc.line_adjustment_amounts(i))
    {
        let (base_amount, percentage) = basis(adj.basis(), decimals)?;
        let mapped = LineAllowanceCharge {
            amount: amount(*reduced, decimals)?,
            base_amount,
            percentage,
            reason: adj.reason().map(str::to_owned),
            reason_code: adj.reason_code().map(Code::new),
        };
        match adj.kind() {
            billing::AdjustmentKind::Allowance => allowances.push(mapped),
            billing::AdjustmentKind::Charge => charges.push(mapped),
        }
    }
    let price = line.price();
    let base = price.base_quantity();
    Ok(InvoiceLine {
        id: (i + 1).to_string(),
        note: None,
        order_line_reference: None,
        accounting_reference: None,
        object_identifier: None,
        quantity: Quantity::new(line.quantity().value()),
        unit_code: Code::new(line.quantity().unit()),
        net_amount: amount(doc.line_net(i), decimals)?,
        period: None,
        allowances,
        charges,
        price: PriceDetails {
            net_price: UnitPriceAmount::new(price.price().value()),
            price_discount: None,
            gross_price: None,
            base_quantity: (base != Decimal::ONE || price.unit().is_some())
                .then(|| Quantity::new(base)),
            base_quantity_code: price.unit().map(Code::new),
        },
        vat,
        item: Item {
            name: Some(line.description().to_owned()),
            ..Default::default()
        },
    })
}

fn line_vat(v: &billing::Vat) -> LineVat {
    LineVat {
        category: Code::new(v.category().code()),
        // `None` for `O`: BR-O-05/06/07 say the rate "shall not" be stated.
        rate: v.stated_rate().map(|p| Percentage::new(p.value())),
    }
}

fn basis(
    b: Option<(Decimal, billing::Percent)>,
    decimals: u8,
) -> Result<(Option<InvoiceAmount>, Option<Percentage>), SeamError> {
    match b {
        Some((base, pct)) => Ok((
            Some(decimal(base, decimals)?),
            Some(Percentage::new(pct.value())),
        )),
        None => Ok((None, None)),
    }
}

fn amount(a: billing::Amount, decimals: u8) -> Result<InvoiceAmount, SeamError> {
    decimal(a.to_decimal(), decimals)
}

fn decimal(d: Decimal, decimals: u8) -> Result<InvoiceAmount, SeamError> {
    InvoiceAmount::from_decimal_exact(d).map_err(|_| SeamError::ScaleTooFine { decimals })
}

fn non_zero(a: billing::Amount, decimals: u8) -> Result<Option<InvoiceAmount>, SeamError> {
    if a.is_zero() {
        Ok(None)
    } else {
        amount(a, decimals).map(Some)
    }
}
