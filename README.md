# en16931

**The European e-invoice in Rust** — the EN 16931 semantic data model and its
business rules, and the UBL / CII / ZUGFeRD bindings that carry them on the wire.

[**Documentation**](https://hupe1980.github.io/en16931) ·
[API reference](https://docs.rs/en16931) ·
[crates.io](https://crates.io/crates/en16931) ·
[Changelog](CHANGELOG.md)

| Crate | What | |
|---|---|---|
| [`en16931`](crates/en16931) | the semantic model, 334 business rules, the typed proof of validity | 2 dependencies · no XML · no I/O · `wasm32` |
| [`en16931-formats`](crates/en16931-formats) | UBL 2.1 and UN/CEFACT CII in both directions, ZUGFeRD / Factur-X | business rules from `en16931` |
| [`en16931-cli`](crates/en16931-cli) | all of it as one command | exit codes for CI |

Start with `en16931`: if the invoice already exists as data, it gives the
verdict and the proof. Add `en16931-formats` when a document has to cross a wire.

## Five tasks

**Build an invoice** — mandatory terms are arguments, the totals are computed,
and a proof is a type:

```rust
let invoice = Invoice::builder(profiles::EN16931.specification_id, "R-1", date, "380", "EUR")
    .seller(seller).buyer(buyer).due_in_days(14)
    .line(InvoiceLine::new("1", "Arbeitspreis", Quantity::new(dec!(1000)), "KWH",
        InvoiceAmount::parse("289.00")?, "S", Some(Percentage::new(dec!(19)))))
    .build_reconciled()?;                                   // BG-22 and BG-23 computed
let proof = Validated::<En16931>::new(invoice)?;             // Err carries the report
```

**Validate and show what is wrong** — findings name business terms:

```rust
let report = profiles::XRECHNUNG.validate(&invoice);
println!("{report}");   // fatal [BR-DE-15] BT-10 — Buyer reference (BT-10) shall be present
```

**Receive bytes and decide** — UBL, CII or a ZUGFeRD PDF; accept, reject or
undefined, as KoSIT's validator answers:

```rust
let policy = Policy { require: Some(&profiles::XRECHNUNG), ..Policy::default() };
let got = en16931_formats::receive(&bytes, &policy);
```

**Convert between syntaxes** — through the model, refusing to lose anything
unless told:

```rust
let converted = en16931_formats::convert(&bytes, Syntax::Cii, Loss::Refuse)?;
```

**Extract or embed a hybrid PDF's payload** — under limits, and into a PDF/A-3
file you rendered:

```rust
let got = en16931_formats::zugferd::extract(&pdf, &Limits::default())?;
let hybrid = en16931_formats::zugferd::embed(&rendered_pdf_a3, &proof, &EmbedOptions::default())?;
```

## The command

```sh
cargo install en16931-cli

en16931 validate   rechnung.xml                # accept, reject or undefined; BT-24 picks the rules
en16931 validate   rechnung.xml -r xrechnung   # …and require a profile
en16931 convert    rechnung.xml --to cii       # through the model; refuses to lose anything
en16931 extract    rechnung.pdf -o payload.xml # the embedded XML, verbatim
en16931 embed      gerendert.pdf rechnung.xml -o rechnung.pdf  # a ZUGFeRD hybrid
en16931 diff       ours.xml theirs.xml         # as invoices, not as XML
en16931 inspect    rechnung.pdf                # what is this file?
en16931 explain    BR-CO-14                    # what does this rule say, and who runs it?
en16931 profiles                               # the profiles and their artefact releases
en16931 categories S O                         # may these VAT categories share one invoice?
en16931 rules --format json                    # the whole rule catalogue
en16931 generate zsh                           # shell completions, or `man`
```

Exit `0` accepted, `1` rejected or undefined, `2` a file that could not be
opened. See [the command line](https://hupe1980.github.io/en16931/docs/cli/).

## Development

[`just`](https://just.systems) is the task runner; `just` alone lists every
recipe.

```sh
just references      # fetch the CEN / KoSIT / Peppol artefacts (the reference corpus)
just ci              # everything CI runs, locally
```

The reference corpus is not committed (the CEN artefacts are EUPL-1.2). The
suites that need it skip without it; CI sets `EN16931_REQUIRE_SPEC=1`, which
turns a skip into a failure. See
[contributing](https://hupe1980.github.io/en16931/docs/contributing/).

## Attribution

These crates implement the semantic data model of EN 16931-1 and the two
mandatory syntaxes listed in CEN/TS 16931-2. EN 16931-1 and CEN/TS 16931-2 are
made available free of charge by CEN and the European Commission under their
2018 licence agreement, which permits derivative use on condition that
derivative applications carry a statement to this effect. Copyright in the
standard remains with CEN.

## Licence

`MIT OR Apache-2.0`, at your option. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
