+++
title = "Bridging a billing engine"
weight = 8
description = "Turn a derived billing document and a header into an EN 16931 invoice in one call — every amount read from the accessor of its business term, nothing recomputed — and render a BT-20 Skonto that BR-DE-18 accepts."
+++

Behind `features = ["billing"]`, `en16931` combines a
[`billing`](https://crates.io/crates/billing) 0.16 document with a header into
one semantic invoice. The division of labour is worth stating even if you use a
different engine, because the same seams appear whatever produced the
arithmetic:

- **The engine owns the arithmetic.** A built `billing::Document` holds its
  inputs and every EN 16931 amount derived from them, once.
- **`en16931` owns what the arithmetic means.** Which business term each number is.
- **The caller owns the header.** Number, dates, BT-3 and the document kind,
  BT-24, parties, payment instructions, notes, periods, BT-20 — a tariff engine
  has no business knowing a buyer's postal address.

```rust
let header = Invoice::builder(
        "urn:cen.eu:en16931:2017", "R-2026-0001", Date::parse("2026-07-31")?, "380", "EUR",
    )
    .seller(seller)
    .buyer(buyer)
    .due_in_days(14)
    .build();

let invoice = billing_adapter::from_billing(&document, header)?;
validate(&invoice).into_result()?;
```

The header is an ordinary `Invoice`. `billing` 0.16 states units as BT-130
codes and carries a VAT treatment on every line, so the call needs nothing else.

## Every amount is the document's

Each amount is the value of the `Document` accessor that names its business
term, narrowed to two decimals **exactly**. Nothing is recomputed and nothing is
rounded — a seam that recomputed a total would be a second arithmetic engine.

```text
BT-131            ← line_net(i)                 line i is BG-25 line i, BT-126 = i + 1
BG-27 / BG-28     ← line_adjustment_amounts(i)
BG-20 / BG-21     ← adjustment_amount(i)        one per VAT group for an apportioned discount
BG-23             ← vat_groups()
BT-106 … BT-115   ← line_total() … payable()
BT-6 + BT-111     ← vat_accounting_total()      together, as BR-53 needs
BT-5              ← currency()
```

Because line *i* of the invoice is line *i* of the document, the terms the
engine has no word for — a line period (BG-26), an order line reference
(BT-132), an item identifier — are set by index after the call.

**Absent is not zero.** BT-107 and BT-108 are stated when an allowance or charge
exists; BT-113 and BT-114 when they are not zero.

A `split` share's VAT can differ by a unit from rounding its own base × rate,
because the engine partitions the group tax exactly across the shares. It
crosses as the engine states it; `BR-CO-17`'s tolerance decides, not the seam.

## The §14c hole

A final invoice deducting advance payments must, in Germany, state *"die auf sie
entfallenden Steuerbeträge"* — the tax contained in each advance (§14 Abs. 5
Satz 2 UStG). Omit it and the issuer owes that tax **a second time** under §14c
Abs. 1.

Core EN 16931 has nowhere to put it: BT-113 is one flat figure. So each itemised
advance becomes an entry in `Extensions` — mirroring ZUGFeRD EXTENDED's
`BG-X-45`, the only standardised home — with its net, tax and VAT category, and
`EN-EXT-01` **warns** when the target profile cannot represent it:

```text
EN 16931 validation (EN 16931-1:2017+A1:2019) — 227 rule(s) checked, 1 finding(s), valid
  warning      [EN-EXT-01] BT-113 — This invoice carries extension data that the
  target profile cannot represent. […] In Germany that is a §14c Abs. 1 UStG liability.
```

Not fatal: the invoice *is* lawful. But not silent either. An advance's
reference and dates are not the engine's; set them after the call.

## What is refused

Each refusal is a typed `SeamError` that names the business term:

| Refusal | Why |
|---|---|
| a header carrying a line, an allowance or charge, a VAT breakdown entry, a total, BT-6, an advance payment, or a BT-5 other than the document's | the document derives it; two values for one term would compete |
| a document at more than two decimals | rounding would break the exact equalities `BR-CO-10` … `BR-CO-16`; build it with `AmountScale::EN16931` |
| a VAT-free document | EN 16931 requires a VAT category on every line (`BR-CO-04`) |
| a credit-note header (BT-3 `381`) over a negative BT-115 | the engine's `negated()` already reversed the signs; a credit note over it reverses the document twice |

Send the original amounts under 381, or the negated ones under 380.

Labels, the residual VAT statement and the credit ledger do not cross: they
have no business term.

## The BT-20 Skonto

Germany's early-payment discount has a micro-syntax inside BT-20:

```text
Zahlbar innerhalb 30 Tagen ohne Abzug.
#SKONTO#TAGE=10#PROZENT=2.00#
⏎
```

`BR-DE-18` has **two** halves, and the second hides inside the same assertion as
the first:

```text
every $line in …tokenize(., '(\r?\n)')[starts-with(normalize-space(.), '#')]
  satisfies matches(normalize-space($line), $XR-SKONTO-REGEX)
        and matches(…tokenize(., '#.+#')[last()], '^\s*\n')
```

Everything after the **last** `#…#` must begin with a newline, and the
percentage has exactly two decimals: `PROZENT=3` is invalid. A string built by
hand gets one of the two wrong sooner or later, and the rule is fatal.

`PaymentTerms` holds the prose and the discounts as values and renders both
rules in. It is part of the model, not of the `billing` feature, so an invoice
built any other way gets the same rendering:

```rust
let terms = PaymentTerms::new("Zahlbar innerhalb 30 Tagen ohne Abzug.")?
    .with_discount(EarlyPaymentDiscount::new(10, Percentage::new(dec!(2)))?);
header.payment_terms = Some(terms.to_string());
```

A zero-day period, a percentage outside 0–100 or with more than two decimals,
and prose with a line starting with `#` are refused with an error naming BT-20.
Every other value renders a field `BR-DE-18` accepts — a property test checks
that against the rule itself. The discount amount is computed by
`billing::EarlyPaymentDiscount::amount`; `PaymentTerms` only renders the terms.

## See also

- **[Validation](@/docs/validation.md)** — what happens to the invoice you built.
- **[Conformance](@/docs/conformance.md)** — how the mapping is held to the artefacts.
