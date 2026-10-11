+++
title = "ZUGFeRD and Factur-X"
weight = 6
description = "Extract the CII payload from a ZUGFeRD or Factur-X hybrid PDF, embed a validated invoice into a PDF/A-3 file, compare the PDF's XMP metadata against the payload, and know which profiles are not EN 16931 invoices at all."
+++

ZUGFeRD (Germany) and Factur-X (France) are the same thing under two names: a
**PDF/A-3** document with a CII payload embedded as an attachment. The pages are
for a human, the XML is for a machine, and they are supposed to say the same
thing.

Behind `features = ["zugferd"]`, because the PDF layer takes the dependency graph
from 13 crates to 57 and breaks `wasm32`.

## Extracting

```rust
use en16931_formats::zugferd::{self, Limits};

let got = zugferd::extract(&pdf_bytes, &Limits::default())?;

got.xml;               // the payload, byte-identical to what was sent
got.profile;           // what the payload's BT-24 claims
got.xmp;               // what the PDF's own metadata declares
got.divergence;        // where those two disagree
got.relationship;      // the /AFRelationship value found
```

Extraction is the common direction — receiving is more common than sending — and
the one with no PDF/A risk. To read the payload into the model as well, hand the
whole PDF to `en16931_formats::read`, or to `read_with(&pdf_bytes, &limits)` under
your own `Limits`; to decide whether to process it, to
`en16931_formats::receive`, whose `Policy` carries the `Limits`:

```rust
use en16931_formats::{Policy, Recommendation, zugferd::Limits};

let policy = Policy { limits: Limits::default().with_max_stream_bytes(8 << 20), ..Policy::default() };
let got = en16931_formats::receive(&pdf_bytes, &policy);
match got.recommendation {
    Recommendation::Accept => { /* BASIC, EN 16931, EXTENDED, XRechnung — valid */ }
    Recommendation::Undefined => { /* MINIMUM, BASIC WL: lawful, not an invoice */ }
    Recommendation::Reject => eprintln!("{got}"),
}
```

A PDF over a limit is a rejected document whose container layer says which
limit; the payload is never read.

**A PDF from a stranger is bounded, not trusted.** A PDF amplifies: a 522 KB
file can carry a stream that inflates to 512 MiB, and a 572-byte file can carry
an embedded-files tree whose `/Kids` lists itself. Capping the input does not
bound either, so every call states its `Limits` and the reader enforces them:

| Limit | Default | Bounds |
|---|---|---|
| `max_stream_bytes` | 64 MiB | what any one stream may inflate to — the invoice, the XMP, an object stream |
| `max_total_bytes` | 128 MiB | what one call may inflate in sum |
| `max_objects` | 10 000 | how many objects one walk over the PDF's structure may visit |

The invoice is chosen by name **before** anything is inflated, so a bomb in an
attachment that is not the invoice is never opened. Over a limit, the call
returns `Error::LimitExceeded` naming the stream and the limit; a cyclic, oversized
or malformed name tree is `Error::EmbeddedFilesTree`; a stream whose filter does not
decode is `Error::Undecodable` — never its still-encoded bytes. Input size and
parse time stay the caller's to cap.

## The XMP is not decoration

It is how a receiver discovers an invoice is there and which profile it claims,
*before parsing anything*. A PDF whose metadata says BASIC while the payload says
EN 16931 validates, opens, and is wrong in a way no schema notices: a receiver
routing on the XMP and one routing on BT-24 process the same file differently,
and both behave correctly.

So both are read and compared. `Divergence::NoXmp` says when a counterparty
scanning metadata first will not see an e-invoice at all.

The metadata is read **by namespace**. `DocumentFileName` and `ConformanceLevel`
come only from the ZUGFeRD / Factur-X schemas — Factur-X, ZUGFeRD 2, ZUGFeRD 1
and its release candidate, the newest winning field by field — and the PDF/A
part only from the PDF/A identification schema. The ZUGFeRD community's
additional-data schema uses the same property names for a *further* attachment,
and old release-candidate metadata survives re-stamping; neither is mistaken for
the invoice's. A packet that is not well-formed XML counts as no metadata.

## What else the read checks

Getting the invoice out is the easy half. The hard half is that a hybrid PDF can
be **structurally wrong in ways nothing complains about**: it opens, it renders,
every viewer shows the attachment — and the counterparty's intake never sees an
e-invoice. Those defects come back as a rejected invoice weeks later, with no
error message to search for.

Every extraction therefore reports how the payload is wired in, as `Divergence`
values:

| | What breaks |
|---|---|
| `NotAssociated` — absent from the catalogue's `/AF` | a PDF/A-3 receiver asking what is associated with this document is told nothing. The commonest defect: every PDF library can *attach* a file, fewer can *associate* one |
| `NotInEmbeddedFiles` — absent from `/Names/EmbeddedFiles` | readers without PDF/A-3 support never find it |
| `NoRelationship` — no `/AFRelationship` | nothing says whether the XML *is* the invoice or accompanies one |
| `NotPdfA3` — `pdfaid:part` is not `3` | parts 1 and 2 of ISO 19005 forbid embedding a file of arbitrary type at all, so the file contradicts itself and veraPDF will say so |
| `Relationship`, `Profile`, `Filename`, `NoXmp` | the metadata and the payload disagree |

None of these stops extraction: the payload still comes back verbatim, which is
what you diagnose with. They are what a **sender** wants to know before the file
leaves, and `en16931 inspect invoice.pdf` prints them.

## Not every ZUGFeRD profile is an EN 16931 invoice

This is the trap in the profile matrix. **MINIMUM and BASIC WL carry no lines**,
so they cannot satisfy `BR-16`. They are accounting aids, not invoices.

```rust
match got.profile.is_en16931_invoice() {
    IsInvoice::Yes     => { /* the 334 rules apply */ }
    IsInvoice::No(why) => println!("not an invoice: {why}"),  // names BR-16
    IsInvoice::Unknown => { /* unrecognised — do not guess */ }
}
```

An unrecognised profile is `Unknown`, never quietly the core model. A type system
that says MINIMUM is an EN 16931 invoice is worse than no type system.

`receive` draws the same line from the payload's BT-24, through
`en16931::profiles::classify`:

| Profile | Rules that run | Recommendation when none is fatal |
|---|---|---|
| MINIMUM | `BR-CO-15` — the one core rule its terms carry | *undefined* |
| BASIC WL | `BR-CO-11` … `BR-CO-16` | *undefined* |
| BASIC (`#compliant#`) | every core rule | *accept* |
| EN 16931 | every core rule | *accept* |
| EXTENDED (`#conformant#`) | every core rule; its logistics service charge counts as a document charge | *accept* |
| XRECHNUNG | XRechnung 3.0 | *accept* |

BASIC and EXTENDED are lawful EN 16931 e-invoices in Germany, recognised in
every spelling their specifications published (Factur-X 1.0, ZUGFeRD 2.0 and
2.1+). FeRD's own BASIC restrictions and EXTENDED rules are not shipped — the
report's rule set says "EN 16931 core rules". Over the 151 PDFs of the ZUGFeRD
community corpus, `receive` recommends 81 accept, 56 reject and 14 undefined;
the rejections are ZUGFeRD 1 documents (not EN 16931), withdrawn XRechnung
identifiers, and genuine findings.

## Writing: `embed`, into a PDF/A-3 file you rendered

```rust
use en16931::profiles::En16931;
use en16931::validation::profile::Validated;
use en16931_formats::zugferd::{self, EmbedOptions};

let proof = Validated::<En16931>::new(invoice)?;
let hybrid = zugferd::embed(&rendered_pdf_a3, &proof, &EmbedOptions::default())?;
```

Or, without writing Rust: `en16931 embed rendered.pdf invoice.xml -o invoice.pdf`.

A ZUGFeRD file is not "a PDF with an attachment". It is a **PDF/A-3** document,
and a file that is no longer valid PDF/A is no longer a valid ZUGFeRD invoice.
So the work splits the way every other implementation splits it — factur-x,
Mustang, drafthorse, ZUGFeRD-csharp:

- **The rendering is yours**, and it must already be PDF/A-3. `embed` reads the
  declaration in the XMP and refuses anything else — PDF/A-1 or -2, no PDF/A at
  all, an encrypted file, a missing or filtered metadata stream, a file that
  already carries an invoice — with a typed `EmbedError`. It never converts a
  PDF to PDF/A and never renders.
- **The rest is `embed`'s.** The CII payload comes from `write`, so only a
  validated invoice gets in. The profile decides the filename and
  `fx:ConformanceLevel` — `factur-x.xml` and `EN 16931`, or `xrechnung.xml` and
  `XRECHNUNG`; a proof for any other profile does not compile. It writes `/AF`,
  `/AFRelationship`, the embedded-files name tree, the Factur-X XMP with its
  PDF/A extension schema, and a title, subject and author derived from the
  invoice ("Seller: Invoice 123", the factur-x convention), in the XMP and the
  Info dictionary alike.
- **It appends; it does not rewrite.** The output is your file followed by one
  incremental update, so your bytes are an unchanged prefix of the result,
  nothing your conformance rests on is touched, and a signature can still follow.
  Every XMP property your renderer wrote is kept byte for byte.

Before it returns, `embed` reads the result back with `extract` and refuses to
hand over a file with any divergence — it cannot produce the defects the reader
reports.

### What says it works: veraPDF

Most of a PDF/A-3 file's conformance is checkable only by a PDF/A validator, not
by a Rust assertion. The test suite runs **veraPDF** (PDF/A-3b, in its official Docker image)
over every file `embed` writes in it: both profiles, into blank PDF/A-3b files
of every shape — with and without an existing extension-schema bag, with a
cross-reference table and a stream — and into every PDF/A-3 sample of the
ZUGFeRD community corpus with its own invoice removed. Any failed rule fails the
build.

### `/AFRelationship`

The relationship decides whether the XML **is** the invoice or merely
accompanies one, and the published guidance disagrees:

| Profile | Guidance |
|---|---|
| MINIMUM, BASIC WL | `Data` — no lines; the pages are the invoice |
| BASIC, EN 16931, EXTENDED | German sources say `Alternative`; PDFlib documents `Source` for Factur-X to non-German recipients |

`embed` writes `Alternative` and offers `Source` (`EmbedOptions::relationship`).
`Data` is not offered: for the profiles it writes, every source agrees it is
wrong. The reader reports what it finds and raises `Divergence::Relationship`
only for `Data` on a profile that carries lines.

## Provenance

The ZUGFeRD and Factur-X specifications are not in the reference corpus: the
profile names, attachment filenames and XMP structure are checked against the
Factur-X reference implementation
([`facturx.py`](https://github.com/akretion/factur-x/blob/master/src/facturx/facturx.py)
and its [XMP extension schema](https://github.com/akretion/factur-x/blob/master/src/facturx/xmp/Factur-X_extension_schema.xmp)),
and the crate marks them ⚠. **`fx:Version` is the Factur-X XMP schema's version
(`1.0`), not ZUGFeRD's.**

## Two details `embed` handles

**The XMP namespace URI** is
`urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#`. The mixed case and the
trailing `#` are both load-bearing, and the sample PDFs in the Factur-X 1.0 info
package spell it in lowercase, which is wrong.

**The Factur-X extension-schema block cannot be pasted into an arbitrary XMP
packet as its own `rdf:Description`.** XMP allows each property once per packet
and `pdfaExtension:schemas` is a property — so a PDF generator that already
writes extension schemas of its own already carries that bag, and a second one
makes Adobe-lineage parsers and veraPDF reject the whole packet, silently
ending the file's PDF/A conformance. Merge the fx schema's `rdf:li` into the
existing bag — which is what `embed` does. The defect is invisible to every XML
parser, which is why the writer is checked against veraPDF.

## See also

- **[The CLI](@/docs/cli.md)** — `en16931 inspect rechnung.pdf`.
