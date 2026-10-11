//! Pull the invoice out of a ZUGFeRD / Factur-X PDF.
//!
//! ```sh
//! cargo run --example zugferd_extract --features zugferd -- invoice.pdf
//! ```
//!
//! With no argument, a small PDF is built in-process. `en16931_formats::read`
//! recognises the PDF, returns the model with the verbatim payload beside it,
//! and compares the XMP's claimed profile against the payload's. MINIMUM and
//! BASIC WL are not EN 16931 invoices (no lines, so `BR-16` fails).

use en16931_formats::Error;
use en16931_formats::zugferd::{self, IsInvoice};
use lopdf::dictionary;

fn main() {
    let pdf = match std::env::args().nth(1) {
        Some(path) => std::fs::read(&path).unwrap_or_else(|e| {
            eprintln!("cannot read {path}: {e}");
            std::process::exit(1);
        }),
        None => {
            println!("(no PDF given — using a generated one)\n");
            built_in_pdf()
        }
    };

    let read = match en16931_formats::read(&pdf) {
        Ok(read) => read,
        Err(Error::Pdf(zugferd::Error::NoInvoice { found, .. })) => {
            // A PDF with no invoice: the error lists what it did contain.
            println!("no embedded invoice. attachments present: {found:?}");
            return;
        }
        Err(e) => {
            // A broken PDF, or a payload that is not an invoice.
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let got = read
        .pdf
        .as_ref()
        .expect("a PDF was read, so its container is here");

    println!("embedded file:  {}", got.filename);
    println!("payload:        {} bytes", got.xml.len());
    println!("claims BT-24:   {:?}", got.specification_id);
    println!("profile:        {}", got.profile);
    println!("XMP declares:   {:?}", got.xmp.conformance_level);

    match got.profile.is_en16931_invoice() {
        IsInvoice::Yes => println!("EN 16931 invoice: yes — the 317 rules apply"),
        IsInvoice::No(why) => println!("EN 16931 invoice: NO — {why}"),
        IsInvoice::Unknown => println!("EN 16931 invoice: unknown profile; not guessing"),
    }

    // Each divergence prints via `Display`; `Divergence` is `#[non_exhaustive]`.
    for d in &got.divergence {
        println!("\n⚠ {d}");
    }

    // What the reader reported about the payload, one line each.
    for entry in &read.entries {
        println!("  {entry}");
    }

    println!("\nBT-1: {:?}", read.invoice.number);
    let report = en16931::validate(&read.invoice);
    // The specimen is not a complete invoice; reading yields a model the
    // rules can run on.
    println!(
        "valid: {} ({} findings — the demonstration payload is deliberately bare)",
        report.is_valid(),
        report.findings().len()
    );
}

/// The document-level XMP of a conforming hybrid invoice.
///
/// `pdfaid` names the ISO 19005 part, the `fx:` block the invoice and its
/// profile. Not a complete PDF/A-3 packet: it lacks the `pdfaExtension:schemas`
/// bag.
const XMP: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
  xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
   pdfaid:part="3" pdfaid:conformance="B"/>
 <rdf:Description xmlns:fx="urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#">
  <fx:DocumentType>INVOICE</fx:DocumentType>
  <fx:DocumentFileName>factur-x.xml</fx:DocumentFileName>
  <fx:Version>1.0</fx:Version>
  <fx:ConformanceLevel>EN 16931</fx:ConformanceLevel>
 </rdf:Description>
</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;

/// A minimal PDF/A-3-shaped file carrying an invoice.
fn built_in_pdf() -> Vec<u8> {
    let invoice = {
        let mut i = en16931::Invoice::default();
        i.number = Some("RE-2026-0042".into());
        i.issue_date = Some(en16931::Date::new(2026, 1, 15).expect("a real date"));
        i.specification_id = Some("urn:cen.eu:en16931:2017".into());
        i
    };
    // Unvalidated, so `write_unchecked`; BT-1 and BT-2 give D16B a form.
    let payload = en16931_formats::write_unchecked(&invoice, en16931_formats::Syntax::Cii)
        .expect("BT-1 and BT-2 are all D16B requires")
        .xml;

    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
    doc.objects.insert(
        pages_id,
        lopdf::Object::Dictionary(dictionary! {
            "Type" => "Pages", "Count" => 1, "Kids" => vec![page_id.into()],
        }),
    );
    let stream = doc.add_object(lopdf::Stream::new(
        dictionary! { "Type" => "EmbeddedFile", "Subtype" => "text/xml" },
        payload.into_bytes(),
    ));
    let spec = doc.add_object(dictionary! {
        "Type" => "Filespec",
        "F"  => lopdf::Object::string_literal("factur-x.xml"),
        "UF" => lopdf::Object::string_literal("factur-x.xml"),
        "AFRelationship" => "Alternative",
        "EF" => dictionary! { "F" => stream, "UF" => stream },
    });
    let names = doc.add_object(dictionary! {
        "Names" => vec![lopdf::Object::string_literal("factur-x.xml"), spec.into()],
    });
    // `pdfaid:part` makes this PDF/A-3; the `fx:` block lets a receiver find
    // the invoice before parsing.
    let metadata = doc.add_object(lopdf::Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        XMP.as_bytes().to_vec(),
    ));
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "Names" => dictionary! { "EmbeddedFiles" => names },
        // `/AF` makes the attachment an associated file.
        "AF" => vec![lopdf::Object::Reference(spec)],
        "Metadata" => metadata,
    });
    doc.trailer.set("Root", catalog);

    let mut out = Vec::new();
    doc.save_to(&mut out).expect("write the demonstration PDF");
    out
}
