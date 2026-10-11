#![cfg(feature = "zugferd")]

//! ZUGFeRD / Factur-X extraction, against PDFs built in the test rather than
//! checked in, so each case's structure is visible in the source.

mod common;

use en16931_formats::zugferd::{Divergence, IsInvoice, Limits};
use lopdf::{Object, Stream, dictionary};

const INVOICE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rsm:CrossIndustryInvoice xmlns:rsm="urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100"
                          xmlns:ram="urn:un:unece:uncefact:data:standard:ReusableAggregateBusinessInformationEntity:100">
  <rsm:ExchangedDocumentContext>
    <ram:GuidelineSpecifiedDocumentContextParameter>
      <ram:ID>urn:cen.eu:en16931:2017</ram:ID>
    </ram:GuidelineSpecifiedDocumentContextParameter>
  </rsm:ExchangedDocumentContext>
</rsm:CrossIndustryInvoice>"#;

/// An XMP packet declaring a ZUGFeRD invoice in a PDF/A-3 file.
///
/// PDF/A-3 is what permits an arbitrary embedded file; `pdfaid` is written as
/// attributes, the `fx:` block as elements, as most producers do.
fn xmp(level: &str, filename: &str) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
  xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description rdf:about=""
   xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
   pdfaid:part="3" pdfaid:conformance="B"/>
 <rdf:Description xmlns:fx="urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#">
  <fx:DocumentType>INVOICE</fx:DocumentType>
  <fx:DocumentFileName>{filename}</fx:DocumentFileName>
  <fx:Version>1.0</fx:Version>
  <fx:ConformanceLevel>{level}</fx:ConformanceLevel>
 </rdf:Description>
</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    )
}

/// What a fixture PDF is allowed to get wrong.
///
/// Every field defaults to conforming; a defect test names exactly its defect.
#[derive(Clone, Copy, Default)]
struct Defects {
    /// Leave the file specification out of the catalogue's `/AF` array.
    not_associated: bool,
    /// Leave it out of `/Names/EmbeddedFiles`, reachable only through `/AF`.
    not_in_name_tree: bool,
    /// Write no `/AFRelationship` on the file specification.
    no_relationship: bool,
    /// Write `/UF` and the name-tree key as UTF-16BE text strings (the common case).
    utf16_names: bool,
    /// Put `/AF`, the name tree's `/Names` array and `/UF` behind indirect
    /// references.
    indirect: bool,
}

/// `name` as a UTF-16BE PDF text string, byte-order mark first.
fn utf16be(name: &str) -> Object {
    Object::String(
        [0xFE, 0xFF]
            .into_iter()
            .chain(name.encode_utf16().flat_map(u16::to_be_bytes))
            .collect(),
        lopdf::StringFormat::Hexadecimal,
    )
}

/// A conforming hybrid PDF carrying `attachments`.
/// A conforming hybrid PDF carrying `attachments`; the metadata names the
/// first attachment, if any.
fn pdf_with(attachments: &[(&str, &[u8])]) -> Vec<u8> {
    let packet = attachments.first().map(|(name, _)| xmp("EN 16931", name));
    build(attachments, packet.as_deref())
}

/// A PDF whose attachment declares `/AFRelationship` `relationship`.
fn pdf_with_relationship(attachments: &[(&str, &[u8])], relationship: &str) -> Vec<u8> {
    let packet = xmp("EN 16931", attachments[0].0);
    build_full(attachments, Some(&packet), relationship, Defects::default())
}

/// A conforming PDF but for `defects`.
fn pdf_with_defects(attachments: &[(&str, &[u8])], defects: Defects) -> Vec<u8> {
    let packet = xmp("EN 16931", attachments[0].0);
    build_full(attachments, Some(&packet), "Alternative", defects)
}

/// A PDF carrying `attachments` both in `/AF` and in `/Names/EmbeddedFiles`,
/// optionally with an XMP metadata packet.
fn build(attachments: &[(&str, &[u8])], metadata: Option<&str>) -> Vec<u8> {
    build_full(attachments, metadata, "Alternative", Defects::default())
}

fn build_full(
    attachments: &[(&str, &[u8])],
    metadata: Option<&str>,
    relationship: &str,
    defects: Defects,
) -> Vec<u8> {
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Count" => 1, "Kids" => vec![page_id.into()],
        }),
    );

    let mut names = Vec::new();
    let mut associated = Vec::new();
    for (name, bytes) in attachments {
        let stream = doc.add_object(Stream::new(
            dictionary! { "Type" => "EmbeddedFile", "Subtype" => "text/xml" },
            bytes.to_vec(),
        ));
        let unicode = || {
            if defects.utf16_names {
                utf16be(name)
            } else {
                Object::string_literal(*name)
            }
        };
        let mut filespec = dictionary! {
            "Type" => "Filespec",
            "F" => Object::string_literal(*name),
            "UF" => if defects.indirect {
                Object::Reference(doc.add_object(unicode()))
            } else {
                unicode()
            },
            "EF" => dictionary! { "F" => stream, "UF" => stream },
        };
        if !defects.no_relationship {
            filespec.set("AFRelationship", Object::Name(relationship.into()));
        }
        let spec = doc.add_object(filespec);
        associated.push(Object::Reference(spec));
        names.push(unicode());
        names.push(spec.into());
    }

    let names = if defects.indirect {
        Object::Reference(doc.add_object(names))
    } else {
        Object::Array(names)
    };
    let embedded = doc.add_object(dictionary! { "Names" => names });
    let mut catalog_dict = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    };
    if !defects.not_in_name_tree {
        catalog_dict.set(
            "Names",
            Object::Dictionary(dictionary! { "EmbeddedFiles" => embedded }),
        );
    }
    if !defects.not_associated {
        let associated = if defects.indirect {
            Object::Reference(doc.add_object(associated))
        } else {
            Object::Array(associated)
        };
        catalog_dict.set("AF", associated);
    }
    if let Some(packet) = metadata {
        let meta = doc.add_object(Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            packet.as_bytes().to_vec(),
        ));
        catalog_dict.set("Metadata", meta);
    }
    let catalog = doc.add_object(catalog_dict);
    doc.trailer.set("Root", catalog);

    let mut out = Vec::new();
    doc.save_to(&mut out).expect("write the fixture PDF");
    out
}

#[test]
fn the_invoice_comes_back_verbatim() {
    let pdf = pdf_with(&[("factur-x.xml", INVOICE.as_bytes())]);
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");

    assert_eq!(
        got.xml,
        INVOICE.as_bytes(),
        "the payload must be byte-identical"
    );
    assert_eq!(got.filename, "factur-x.xml");
    assert_eq!(got.profile, en16931_formats::zugferd::Profile::En16931);
    assert_eq!(
        got.specification_id.as_deref(),
        Some("urn:cen.eu:en16931:2017")
    );
}

/// `read_with` is `read` under the caller's limits: a 1 MiB stream cap refuses
/// a PDF whose 1.5 MiB invoice `read` accepts.
#[test]
fn read_with_applies_the_callers_limits() {
    const MIB: usize = 1024 * 1024;
    let invoice = format!("{INVOICE}{}", " ".repeat(MIB + MIB / 2));
    let pdf = pdf_with(&[("factur-x.xml", invoice.as_bytes())]);

    let read = en16931_formats::read(&pdf).expect("within the default limits");
    assert!(read.pdf.is_some());

    let capped = Limits::new().with_max_stream_bytes(MIB);
    let err = en16931_formats::read_with(&pdf, &capped).expect_err("over the cap");
    assert!(
        matches!(
            err,
            en16931_formats::Error::Pdf(en16931_formats::zugferd::Error::LimitExceeded {
                limit: en16931_formats::zugferd::Limit::StreamBytes(MIB),
                ..
            })
        ),
        "{err:?}"
    );
    assert!(en16931_formats::read_with(&pdf, &Limits::default()).is_ok());
}

/// ZUGFeRD 2.0's filename is accepted too.
#[test]
fn the_older_filename_is_accepted_too() {
    let pdf = pdf_with(&[("zugferd-invoice.xml", INVOICE.as_bytes())]);
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.filename, "zugferd-invoice.xml");
}

/// Filename casing is not significant.
#[test]
fn the_filename_match_is_case_insensitive() {
    let pdf = pdf_with(&[("ZUGFeRD-invoice.XML", INVOICE.as_bytes())]);
    assert!(en16931_formats::zugferd::extract(&pdf, &Limits::default()).is_ok());
}

/// With two payloads, the newer convention wins regardless of order.
#[test]
fn preference_order_decides_when_a_pdf_carries_two() {
    let pdf = pdf_with(&[
        ("zugferd-invoice.xml", b"<old/>"),
        ("factur-x.xml", INVOICE.as_bytes()),
    ]);
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.filename, "factur-x.xml", "the newer convention wins");
    assert_eq!(got.xml, INVOICE.as_bytes());
}

/// A PDF with no invoice gets its own message, listing what it did carry.
#[test]
fn a_pdf_without_an_invoice_says_what_it_has() {
    let pdf = pdf_with(&[("terms-and-conditions.pdf", b"%PDF-1.7")]);
    let err = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect_err("no invoice");
    let en16931_formats::zugferd::Error::NoInvoice { found, looked_for } = &err else {
        panic!("expected NoInvoice, got {err:?}");
    };
    assert_eq!(found, &["terms-and-conditions.pdf".to_owned()]);
    assert!(looked_for.contains(&"factur-x.xml"));
    assert!(
        err.to_string().contains("terms-and-conditions.pdf"),
        "{err}"
    );
}

#[test]
fn a_pdf_with_no_attachments_at_all_is_not_a_parse_failure() {
    let pdf = pdf_with(&[]);
    assert!(matches!(
        en16931_formats::zugferd::extract(&pdf, &Limits::default()),
        Err(en16931_formats::zugferd::Error::NoInvoice { .. })
    ));
}

/// Every attachment is listed, not just invoice-like ones.
#[test]
fn every_attachment_is_enumerable() {
    let pdf = pdf_with(&[
        ("factur-x.xml", INVOICE.as_bytes()),
        ("timesheet.csv", b"a,b,c"),
    ]);
    let files =
        en16931_formats::zugferd::embedded_files(&pdf, &Limits::default()).expect("enumerate");
    assert_eq!(files.len(), 2);
    assert_eq!(files["timesheet.csv"], b"a,b,c");
}

/// A payload that is not UTF-8 is reported, not lossily decoded.
#[test]
fn a_non_utf8_payload_is_reported() {
    let pdf = pdf_with(&[("factur-x.xml", &[0xff, 0xfe, 0x00])]);
    assert!(matches!(
        en16931_formats::zugferd::extract(&pdf, &Limits::default()),
        Err(en16931_formats::zugferd::Error::Encoding { .. })
    ));
}

/// The profile is what the document claims; MINIMUM is not an EN 16931 invoice.
#[test]
fn a_minimum_profile_is_extracted_but_not_an_invoice() {
    let xml = INVOICE.replace("urn:cen.eu:en16931:2017", "urn:factur-x.eu:1p0:minimum");
    let pdf = pdf_with(&[("factur-x.xml", xml.as_bytes())]);
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.profile, en16931_formats::zugferd::Profile::Minimum);
    let en16931_formats::zugferd::IsInvoice::No(why) = got.profile.is_en16931_invoice() else {
        panic!("MINIMUM must not be typed as an EN 16931 invoice");
    };
    assert!(why.contains("BR-16"), "{why}");
}

// ── XMP: how a receiver finds the invoice before parsing anything ──────────────

#[test]
fn the_xmp_is_read_and_agrees_with_the_payload() {
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some(&xmp("EN 16931", "factur-x.xml")),
    );
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");

    assert_eq!(got.xmp.document_type.as_deref(), Some("INVOICE"));
    assert_eq!(got.xmp.conformance_level.as_deref(), Some("EN 16931"));
    assert_eq!(got.xmp.document_filename.as_deref(), Some("factur-x.xml"));
    assert_eq!(got.xmp.version.as_deref(), Some("1.0"));
    assert!(got.divergence.is_empty(), "{:?}", got.divergence);
}

/// The XMP writes `EN 16931` and BT-24 a URN, so profiles are compared, not literals.
#[test]
fn different_spellings_of_the_same_profile_are_not_a_divergence() {
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some(&xmp("EN 16931", "factur-x.xml")),
    );
    assert!(
        en16931_formats::zugferd::extract(&pdf, &Limits::default())
            .expect("extract")
            .divergence
            .is_empty()
    );
}

/// Metadata claiming a full invoice over a payload with no lines diverges.
#[test]
fn en16931_metadata_over_a_minimum_payload_is_reported() {
    const MINIMUM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rsm:CrossIndustryInvoice xmlns:rsm="urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100"
                          xmlns:ram="urn:un:unece:uncefact:data:standard:ReusableAggregateBusinessInformationEntity:100">
  <rsm:ExchangedDocumentContext>
    <ram:GuidelineSpecifiedDocumentContextParameter>
      <ram:ID>urn:factur-x.eu:1p0:minimum</ram:ID>
    </ram:GuidelineSpecifiedDocumentContextParameter>
  </rsm:ExchangedDocumentContext>
</rsm:CrossIndustryInvoice>"#;
    let pdf = build(
        &[("factur-x.xml", MINIMUM.as_bytes())],
        Some(&xmp("EN 16931", "factur-x.xml")),
    );
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.profile, en16931_formats::zugferd::Profile::Minimum);
    assert!(
        got.divergence
            .iter()
            .any(|d| matches!(d, Divergence::Profile { .. })),
        "{:?}",
        got.divergence
    );
}

/// XMP says BASIC while the payload says EN 16931: a divergence, though both
/// halves are valid.
#[test]
fn a_profile_mismatch_is_reported() {
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some(&xmp("BASIC", "factur-x.xml")),
    );
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(
        got.profile,
        en16931_formats::zugferd::Profile::En16931,
        "the payload wins"
    );
    let [en16931_formats::zugferd::Divergence::Profile { xmp, payload }] = &got.divergence[..]
    else {
        panic!("expected one profile divergence, got {:?}", got.divergence);
    };
    assert_eq!(xmp, "BASIC");
    assert_eq!(payload, "urn:cen.eu:en16931:2017");
}

#[test]
fn a_filename_mismatch_is_reported() {
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some(&xmp("EN 16931", "zugferd-invoice.xml")),
    );
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    let [en16931_formats::zugferd::Divergence::Filename { xmp, actual }] = &got.divergence[..]
    else {
        panic!("expected one filename divergence, got {:?}", got.divergence);
    };
    assert_eq!(xmp, "zugferd-invoice.xml");
    assert_eq!(actual, "factur-x.xml");
}

/// An XMP packet with the given `rdf:Description`s, PDF/A-3 declared first.
fn xmp_with(descriptions: &str) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
  xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description rdf:about=""
   xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
   pdfaid:part="3" pdfaid:conformance="B"/>
{descriptions}
</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    )
}

const FACTUR_X: &str = r#" <rdf:Description rdf:about=""
   xmlns:fx="urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#">
  <fx:DocumentType>INVOICE</fx:DocumentType>
  <fx:DocumentFileName>factur-x.xml</fx:DocumentFileName>
  <fx:Version>1.0</fx:Version>
  <fx:ConformanceLevel>EN 16931</fx:ConformanceLevel>
 </rdf:Description>"#;

/// The additional-data schema names a further attachment with the same local
/// names; it must not be taken for the invoice's filename.
#[test]
fn xmp_additional_data_placed_first_is_not_the_invoice() {
    let adt = r#" <rdf:Description rdf:about=""
   xmlns:adt="https://www.zugferd-community.net/additional-data/1.0#">
  <adt:DocumentFileName>additional_data-logistics-invoice-1.0-unique.xml</adt:DocumentFileName>
  <adt:DocumentType>INVOICE_ADDITIONAL_DATA</adt:DocumentType>
  <adt:Version>1.0</adt:Version>
  <adt:ConformanceLevel>BASIC</adt:ConformanceLevel>
 </rdf:Description>"#;
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some(&xmp_with(&format!("{adt}\n{FACTUR_X}"))),
    );
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.xmp.document_filename.as_deref(), Some("factur-x.xml"));
    assert_eq!(got.xmp.document_type.as_deref(), Some("INVOICE"));
    assert_eq!(got.xmp.conformance_level.as_deref(), Some("EN 16931"));
    assert!(got.divergence.is_empty(), "{:?}", got.divergence);
}

/// A ZUGFeRD 1.0 release-candidate description (`urn:ferd:pdfa:invoice:rc#`)
/// beside a newer one: the newest family wins, wherever it stands.
#[test]
fn xmp_stale_rc_metadata_placed_first_does_not_win() {
    let rc = r#" <rdf:Description rdf:about="" xmlns:zf="urn:ferd:pdfa:invoice:rc#">
  <zf:DocumentType>INVOICE</zf:DocumentType>
  <zf:DocumentFileName>ZUGFeRD-invoice.xml</zf:DocumentFileName>
  <zf:Version>RC</zf:Version>
  <zf:ConformanceLevel>BASIC</zf:ConformanceLevel>
 </rdf:Description>"#;
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some(&xmp_with(&format!("{rc}\n{FACTUR_X}"))),
    );
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.xmp.document_filename.as_deref(), Some("factur-x.xml"));
    assert_eq!(got.xmp.conformance_level.as_deref(), Some("EN 16931"));
    assert_eq!(got.xmp.version.as_deref(), Some("1.0"));
    assert!(got.divergence.is_empty(), "{:?}", got.divergence);
}

/// An RC-only file still reads its own metadata.
#[test]
fn xmp_rc_metadata_alone_is_still_read() {
    let rc = r#" <rdf:Description rdf:about="" xmlns:zf="urn:ferd:pdfa:invoice:rc#"
   zf:ConformanceLevel="EN 16931">
  <zf:DocumentFileName>factur-x.xml</zf:DocumentFileName>
 </rdf:Description>"#;
    let pdf = build(&[("factur-x.xml", INVOICE.as_bytes())], Some(&xmp_with(rc)));
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.xmp.document_filename.as_deref(), Some("factur-x.xml"));
    assert_eq!(got.xmp.conformance_level.as_deref(), Some("EN 16931"));
    assert!(got.divergence.is_empty(), "{:?}", got.divergence);
}

/// Only `pdfaid:part` decides PDF/A-3, not a `part` lookalike placed first.
#[test]
fn xmp_pdfa_part_comes_from_the_pdfa_namespace_only() {
    let packet = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
  xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description rdf:about="" xmlns:stRef="http://ns.adobe.com/xap/1.0/sType/ResourceRef#">
  <stRef:part>1</stRef:part>
 </rdf:Description>
 <rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">
  <pdfaid:part>3</pdfaid:part><pdfaid:conformance>B</pdfaid:conformance>
 </rdf:Description>
{FACTUR_X}
</rdf:RDF></x:xmpmeta>"#
    );
    let pdf = build(&[("factur-x.xml", INVOICE.as_bytes())], Some(&packet));
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.xmp.pdfa_part.as_deref(), Some("3"));
    assert_eq!(got.xmp.pdfa_conformance.as_deref(), Some("B"));
    assert!(got.divergence.is_empty(), "{:?}", got.divergence);
}

/// Found by name but not declared in the metadata: extracted, and reported.
#[test]
fn a_pdf_with_no_xmp_says_so() {
    let pdf = build(&[("factur-x.xml", INVOICE.as_bytes())], None);
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(
        got.divergence,
        vec![en16931_formats::zugferd::Divergence::NoXmp]
    );
    assert_eq!(got.xmp, en16931_formats::zugferd::Xmp::default());
}

/// A conforming specimen reports nothing at all.
///
/// The baseline for the defect tests below.
#[test]
fn a_conforming_hybrid_pdf_has_no_divergence() {
    let pdf = pdf_with(&[("factur-x.xml", INVOICE.as_bytes())]);
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert!(got.divergence.is_empty(), "{:?}", got.divergence);
    assert_eq!(got.xmp.pdfa_part.as_deref(), Some("3"));
    assert_eq!(got.xmp.pdfa_conformance.as_deref(), Some("B"));
}

/// An attachment that is not an *associated* file.
///
/// The invoice is still extractable, but a PDF/A-3 receiver asking what is
/// associated with the document is told nothing.
#[test]
fn an_attachment_outside_the_af_array_is_reported() {
    let pdf = pdf_with_defects(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Defects {
            not_associated: true,
            ..Defects::default()
        },
    );
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.divergence, vec![Divergence::NotAssociated]);
    assert_eq!(got.xml, INVOICE.as_bytes(), "still extractable");
}

/// An associated file that older readers cannot reach.
#[test]
fn an_attachment_outside_the_embedded_files_tree_is_reported() {
    let pdf = pdf_with_defects(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Defects {
            not_in_name_tree: true,
            ..Defects::default()
        },
    );
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.divergence, vec![Divergence::NotInEmbeddedFiles]);
}

/// `/UF` and the name-tree key in UTF-16BE, as PDF text strings usually are.
#[test]
fn utf16_file_names_are_decoded() {
    let pdf = pdf_with_defects(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Defects {
            utf16_names: true,
            ..Defects::default()
        },
    );
    let files = en16931_formats::zugferd::embedded_files(&pdf, &Limits::default()).expect("parse");
    assert_eq!(files.keys().collect::<Vec<_>>(), ["factur-x.xml"]);
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.xml, INVOICE.as_bytes());
    assert_eq!(
        got.divergence,
        vec![],
        "in the name tree, associated, related"
    );
}

/// `/AF 21 0 R` rather than an inline array.
#[test]
fn indirect_arrays_and_names_are_followed() {
    for utf16_names in [false, true] {
        let pdf = pdf_with_defects(
            &[("factur-x.xml", INVOICE.as_bytes())],
            Defects {
                indirect: true,
                utf16_names,
                ..Defects::default()
            },
        );
        let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
        assert_eq!(got.xml, INVOICE.as_bytes());
        assert_eq!(got.divergence, vec![], "utf16_names: {utf16_names}");
    }
}

/// Nothing in the file says whether the XML is the invoice.
#[test]
fn an_attachment_with_no_relationship_is_reported() {
    let pdf = pdf_with_defects(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Defects {
            no_relationship: true,
            ..Defects::default()
        },
    );
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.divergence, vec![Divergence::NoRelationship]);
    assert_eq!(got.relationship, None);
}

/// PDF/A-3 is normative, and parts 1 and 2 forbid the embedding outright.
#[test]
fn a_pdf_that_is_not_pdfa3_is_reported() {
    let packet =
        xmp("EN 16931", "factur-x.xml").replace(r#"pdfaid:part="3""#, r#"pdfaid:part="2""#);
    let pdf = build(&[("factur-x.xml", INVOICE.as_bytes())], Some(&packet));
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(
        got.divergence,
        vec![Divergence::NotPdfA3 {
            part: Some("2".into())
        }]
    );

    // …and no PDF/A conformance at all is a different finding from the wrong part.
    let packet =
        xmp("EN 16931", "factur-x.xml").replace(r#"pdfaid:part="3" pdfaid:conformance="B""#, "");
    let pdf = build(&[("factur-x.xml", INVOICE.as_bytes())], Some(&packet));
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.divergence, vec![Divergence::NotPdfA3 { part: None }]);
}

/// Every divergence prints a sentence, not a `Debug` dump.
#[test]
fn every_divergence_reads_as_prose() {
    for d in [
        Divergence::NotAssociated,
        Divergence::NotInEmbeddedFiles,
        Divergence::NoRelationship,
        Divergence::NotPdfA3 { part: None },
        Divergence::NotPdfA3 {
            part: Some("1".into()),
        },
        Divergence::NoXmp,
    ] {
        let text = d.to_string();
        assert!(text.len() > 40, "{d:?} prints {text:?}");
        assert!(!text.contains('{'), "{text:?} looks like a Debug dump");
    }
}

/// A mangled metadata packet must not stop a readable invoice being extracted.
#[test]
fn broken_xmp_does_not_prevent_extraction() {
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some("<x:xmpmeta not closed"),
    );
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default())
        .expect("the invoice is still readable");
    assert_eq!(got.xml, INVOICE.as_bytes());
    assert_eq!(
        got.divergence,
        vec![en16931_formats::zugferd::Divergence::NoXmp]
    );
}

// ── The payload as a model ─────────────────────────────────────────────────────

/// `read` returns the `Invoice` for a PDF, with the container beside it.
#[test]
fn the_payload_comes_back_as_the_model() {
    let inv = {
        let mut i = en16931::Invoice::default();
        i.number = Some("RE-2026-0001".into());
        i.issue_date = Some(en16931::Date::parse("2026-01-15").expect("date"));
        i.currency = Some(en16931::invoice::Code::new("EUR"));
        i.specification_id = Some("urn:cen.eu:en16931:2017".into());
        i
    };
    let payload = en16931_formats::write_unchecked(&inv, en16931_formats::Syntax::Cii)
        .expect("BT-1 and BT-2 are all D16B requires")
        .xml;
    let pdf = pdf_with(&[("factur-x.xml", payload.as_bytes())]);

    let read = en16931_formats::read(&pdf).expect("the payload parses as CII");
    assert_eq!(read.syntax, en16931_formats::Syntax::Cii);
    assert_eq!(read.invoice.number.as_deref(), Some("RE-2026-0001"));
    assert_eq!(read.invoice.issue_date, inv.issue_date);
    assert!(read.entries.is_empty(), "{:?}", read.entries);
    let got = read.pdf.expect("a PDF carries its container");
    assert_eq!(got.profile, en16931_formats::zugferd::Profile::En16931);
    assert_eq!(got.xml, payload.as_bytes(), "the bytes are still verbatim");
}

/// The whole chain: read the PDF, then validate with `en16931`.
///
/// The payload is `INVOICE`, which carries only BT-24.
#[test]
fn an_extracted_invoice_can_be_validated() {
    let pdf = pdf_with(&[("factur-x.xml", INVOICE.as_bytes())]);
    let read = en16931_formats::read(&pdf).expect("read");
    assert!(read.pdf.is_some());

    let report = en16931::validate(&read.invoice);
    assert!(!report.is_valid(), "an empty invoice is not valid");
    assert!(
        report.findings().iter().any(|f| f.rule == "BR-02"),
        "BR-02 (invoice number) should fire"
    );
}

/// A payload that is not an invoice at all is reported, not silently empty.
#[test]
fn a_non_cii_payload_is_reported() {
    let pdf = pdf_with(&[("factur-x.xml", b"<Invoice xmlns='urn:x'/>")]);
    // The container is fine: extraction alone succeeds.
    en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("the PDF is still readable");
    let read = en16931_formats::read(&pdf);
    assert!(read.is_err(), "the reason must be given: {read:?}");
}

// ── /AFRelationship ───────────────────────────────────────────────────────────

/// The value is read back verbatim, because a receiver may route on it.
#[test]
fn the_attachment_relationship_is_reported() {
    let pdf = pdf_with(&[("factur-x.xml", INVOICE.as_bytes())]);
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");
    assert_eq!(got.relationship.as_deref(), Some("Alternative"));
    assert!(
        !got.divergence
            .iter()
            .any(|d| matches!(d, Divergence::Relationship { .. })),
        "Alternative on a full-invoice profile is not the case sources disagree about"
    );
}

/// `Data` on a profile that carries lines is a divergence.
#[test]
fn data_on_a_full_invoice_profile_is_a_divergence() {
    let pdf = pdf_with_relationship(&[("factur-x.xml", INVOICE.as_bytes())], "Data");
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");

    assert_eq!(got.relationship.as_deref(), Some("Data"));
    let found = got
        .divergence
        .iter()
        .find_map(|d| match d {
            Divergence::Relationship { found, profile } => Some((found.clone(), *profile)),
            _ => None,
        })
        .expect("a full-invoice profile attached as /Data must be reported");
    assert_eq!(found.0, "Data");
    assert_eq!(found.1.is_en16931_invoice(), IsInvoice::Yes);
}

/// …but not for the two profiles that are genuinely supplementary data.
#[test]
fn data_is_correct_for_the_profiles_that_carry_no_lines() {
    // MINIMUM claims a BT-24 that is not an EN 16931 invoice.
    let minimum = INVOICE.replace("urn:cen.eu:en16931:2017", "urn:factur-x.eu:1p0:minimum");
    let pdf = pdf_with_relationship(&[("factur-x.xml", minimum.as_bytes())], "Data");
    let got = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("extract");

    assert_ne!(
        got.profile.is_en16931_invoice(),
        IsInvoice::Yes,
        "the premise"
    );
    assert!(
        !got.divergence
            .iter()
            .any(|d| matches!(d, Divergence::Relationship { .. })),
        "Data is the correct relationship for a booking aid: {:?}",
        got.divergence
    );
}

// ── Hostile PDFs: amplification is bounded by the reader ───────────────────────
//
// Each input is small, so no input-size cap would stop it; each must return a
// typed error naming the defect, promptly.

use en16931_formats::zugferd::{Error, Limit, PdfStream, TreeDefect};
use std::time::{Duration, Instant};

/// What a hostile case may take; generous for a debug build.
const PROMPTLY: Duration = Duration::from_secs(1);

const MIB: usize = 1024 * 1024;

/// The limits the hostile cases run under: 1 MiB per stream, 2 MiB per call,
/// so a broken bound costs a few MiB. The default limits are exercised once
/// ([`a_bomb_as_the_invoice_is_refused_at_the_limit`]).
const SMALL: Limits = Limits::new()
    .with_max_stream_bytes(MIB)
    .with_max_total_bytes(2 * MIB);

/// What [`bomb`] inflates to: four times [`SMALL`]'s per-stream limit.
const BOMB: usize = 1 + 258 * (4 * MIB / 258);

/// `extract` under [`SMALL`], timed against [`PROMPTLY`].
fn extract_promptly(pdf: &[u8]) -> Result<en16931_formats::zugferd::Extracted, Error> {
    let start = Instant::now();
    let got = en16931_formats::zugferd::extract(pdf, &SMALL);
    let took = start.elapsed();
    assert!(took < PROMPTLY, "took {took:?}: {:?}", got.as_ref().err());
    got
}

/// A PDF from raw object bodies, numbered from 1, with a computed
/// cross-reference table. Object 1 is the catalogue. Raw, because `lopdf`'s
/// writer drops object streams.
fn raw_pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// An indirect stream object: `dict` entries, `/Length` added.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut body = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// Objects 1–4 of every raw PDF here: catalogue (with `extra` entries), pages,
/// and a lawful `factur-x.xml` attachment. The caller's objects start at 5.
fn lawful_head(catalog_extra: &str) -> Vec<Vec<u8>> {
    vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>").into_bytes(),
        b"<< /Type /Pages /Count 0 /Kids [] >>".to_vec(),
        b"<< /Type /Filespec /F (factur-x.xml) /UF (factur-x.xml) /EF << /F 4 0 R >> >>".to_vec(),
        stream("", INVOICE.as_bytes()),
    ]
}

/// A zlib stream that inflates to `len` zero bytes, built from fixed-Huffman
/// deflate codes (one literal, then length-258 / distance-1 matches), so a test
/// never holds what it inflates to.
fn zlib_zeros(len: usize) -> Vec<u8> {
    struct Bits {
        out: Vec<u8>,
        acc: u32,
        n: u32,
    }
    impl Bits {
        fn put(&mut self, value: u32, bits: u32) {
            self.acc |= value << self.n;
            self.n += bits;
            while self.n >= 8 {
                self.out.push(self.acc as u8);
                self.acc >>= 8;
                self.n -= 8;
            }
        }
        /// Huffman codes go most-significant bit first.
        fn code(&mut self, code: u32, bits: u32) {
            self.put(code.reverse_bits() >> (32 - bits), bits);
        }
    }
    assert!(len >= 1 && (len - 1).is_multiple_of(258), "1 + 258·n bytes");
    let mut b = Bits {
        out: vec![0x78, 0x01],
        acc: 0,
        n: 0,
    };
    b.put(1, 1); // BFINAL
    b.put(1, 2); // BTYPE = fixed Huffman
    b.code(0x30, 8); // literal 0
    for _ in 0..(len - 1) / 258 {
        b.code(0xC5, 8); // length 258 (symbol 285)
        b.code(0, 5); // distance 1
    }
    b.code(0, 7); // end of block
    if b.n > 0 {
        b.out.push(b.acc as u8);
    }
    // Adler-32 of `len` zero bytes: a stays 1, b adds it once per byte.
    let adler = (((len % 65_521) as u32) << 16) | 1;
    b.out.extend_from_slice(&adler.to_be_bytes());
    b.out
}

/// Past [`SMALL`]'s per-stream limit, and only just: [`BOMB`] bytes.
fn bomb() -> Vec<u8> {
    zlib_zeros(BOMB)
}

/// Past the **default** per-stream limit — for the one case that proves it.
fn default_bomb() -> Vec<u8> {
    zlib_zeros(1 + 258 * (Limits::DEFAULT_MAX_STREAM_BYTES / 258 + 1))
}

/// A node whose `/Kids` lists itself *k* times (*k* in 1..=8), beside a lawful
/// attachment, is refused promptly.
#[test]
fn a_self_referencing_name_tree_is_refused_not_hung_on() {
    common::watchdog::isolated(
        "a_self_referencing_name_tree_is_refused_not_hung_on",
        || {
            for k in 1..=8 {
                let kids = vec!["5 0 R"; k].join(" ");
                let mut objects = lawful_head("/Names << /EmbeddedFiles 5 0 R >>");
                objects.push(
                    format!("<< /Names [(factur-x.xml) 3 0 R] /Kids [{kids}] >>").into_bytes(),
                );
                let pdf = raw_pdf(&objects);
                let err = extract_promptly(&pdf).expect_err("a cyclic tree is refused");
                assert!(
                    matches!(err, Error::EmbeddedFilesTree(TreeDefect::Cycle)),
                    "fan-out {k}: {err:?}"
                );
                assert!(err.to_string().contains("refers back to itself"), "{err}");
            }
        },
    );
}

#[test]
fn a_two_node_cycle_is_refused() {
    common::watchdog::isolated("a_two_node_cycle_is_refused", || {
        let mut objects = lawful_head("/Names << /EmbeddedFiles 5 0 R >>");
        objects.push(b"<< /Kids [6 0 R] >>".to_vec());
        objects.push(b"<< /Names [(factur-x.xml) 3 0 R] /Kids [5 0 R] >>".to_vec());
        let err = extract_promptly(&raw_pdf(&objects)).expect_err("cycle");
        assert!(
            matches!(err, Error::EmbeddedFilesTree(TreeDefect::Cycle)),
            "{err:?}"
        );
    });
}

/// A `/Kids` entry that points nowhere, or at a non-node, is a tree defect,
/// not a branch to skip.
#[test]
fn a_missing_or_non_dictionary_node_is_refused() {
    common::watchdog::isolated("a_missing_or_non_dictionary_node_is_refused", || {
        for kid in ["99 0 R", "6 0 R", "null"] {
            let mut objects = lawful_head("/Names << /EmbeddedFiles 5 0 R >>");
            objects.push(format!("<< /Names [(factur-x.xml) 3 0 R] /Kids [{kid}] >>").into_bytes());
            objects.push(b"(not a node)".to_vec());
            let err = extract_promptly(&raw_pdf(&objects)).expect_err("malformed tree");
            assert!(
                matches!(err, Error::EmbeddedFilesTree(TreeDefect::Malformed)),
                "{kid}: {err:?}"
            );
            assert!(err.to_string().contains("not a dictionary"), "{err}");
        }
    });
}

/// A node reached twice without a cycle is read once.
#[test]
fn a_node_shared_by_two_parents_is_walked_once() {
    common::watchdog::isolated("a_node_shared_by_two_parents_is_walked_once", || {
        let mut objects = lawful_head("/Names << /EmbeddedFiles 5 0 R >>");
        objects.push(b"<< /Kids [6 0 R 6 0 R] >>".to_vec());
        objects.push(b"<< /Names [(factur-x.xml) 3 0 R] >>".to_vec());
        let got = extract_promptly(&raw_pdf(&objects)).expect("a DAG is lawful enough");
        assert_eq!(got.xml, INVOICE.as_bytes());
        assert!(
            !got.divergence.contains(&Divergence::NotInEmbeddedFiles),
            "{:?}",
            got.divergence
        );
    });
}

/// The default object budget is 10 000: one node plus 10 000 entries exceeds it.
#[test]
fn a_name_tree_past_the_object_budget_is_refused() {
    common::watchdog::isolated("a_name_tree_past_the_object_budget_is_refused", || {
        let entries: String = (0..Limits::DEFAULT_MAX_OBJECTS)
            .map(|i| format!("(f{i}) null "))
            .collect();
        let mut objects = lawful_head("/Names << /EmbeddedFiles 5 0 R >>");
        objects.push(format!("<< /Names [{entries}] >>").into_bytes());
        let err = extract_promptly(&raw_pdf(&objects)).expect_err("over budget");
        assert!(
            matches!(
                err,
                Error::EmbeddedFilesTree(TreeDefect::Exceeded(Limit::Objects(10_000)))
            ),
            "{err:?}"
        );
    });
}

#[test]
fn a_name_tree_deeper_than_32_levels_is_refused() {
    common::watchdog::isolated("a_name_tree_deeper_than_32_levels_is_refused", || {
        let mut objects = lawful_head("/Names << /EmbeddedFiles 5 0 R >>");
        for i in 0..40 {
            objects.push(format!("<< /Kids [{} 0 R] >>", 6 + i).into_bytes());
        }
        objects.push(b"<< /Names [(factur-x.xml) 3 0 R] >>".to_vec());
        let err = extract_promptly(&raw_pdf(&objects)).expect_err("too deep");
        assert!(
            matches!(
                err,
                Error::EmbeddedFilesTree(TreeDefect::Exceeded(Limit::Depth(32)))
            ),
            "{err:?}"
        );
    });
}

/// A non-invoice attachment that inflates past the limit is never opened: the
/// invoice is chosen by name first.
#[test]
fn a_bomb_that_is_not_the_invoice_is_never_inflated() {
    common::watchdog::isolated("a_bomb_that_is_not_the_invoice_is_never_inflated", || {
        let mut objects =
            lawful_head("/Names << /EmbeddedFiles << /Names [(holiday.png) 5 0 R] >> >>");
        // No invoice at all: objects 3 and 4 become the bomb.
        objects[2] = b"<< /Type /Filespec /F (holiday.png) /EF << /F 4 0 R >> >>".to_vec();
        objects[3] = stream("/Filter /FlateDecode", &bomb());
        objects.push(b"null".to_vec());
        let err = extract_promptly(&raw_pdf(&objects)).expect_err("no invoice");
        let Error::NoInvoice { found, .. } = err else {
            panic!("expected NoInvoice, got {err:?}");
        };
        assert_eq!(found, ["holiday.png"]);
    });
}

#[test]
fn a_bomb_beside_a_lawful_invoice_does_not_stop_it() {
    common::watchdog::isolated("a_bomb_beside_a_lawful_invoice_does_not_stop_it", || {
        let mut objects = lawful_head(
            "/Names << /EmbeddedFiles << /Names [(factur-x.xml) 3 0 R (holiday.png) 5 0 R] >> >>",
        );
        objects.push(b"<< /Type /Filespec /F (holiday.png) /EF << /F 6 0 R >> >>".to_vec());
        objects.push(stream("/Filter /FlateDecode", &bomb()));
        let got = extract_promptly(&raw_pdf(&objects)).expect("the invoice is extracted");
        assert_eq!(got.xml, INVOICE.as_bytes());
    });
}

#[test]
fn a_bomb_as_the_invoice_is_refused_at_the_limit() {
    common::watchdog::isolated("a_bomb_as_the_invoice_is_refused_at_the_limit", || {
        let mut objects = lawful_head("");
        objects[3] = stream("/Filter /FlateDecode", &default_bomb());
        // Not `expect_err`: with a broken bound its message would format the
        // 64 MiB payload.
        let err = match en16931_formats::zugferd::extract(&raw_pdf(&objects), &Limits::default()) {
            Ok(got) => panic!("{} bytes came back past the 64 MiB limit", got.xml.len()),
            Err(e) => e,
        };
        assert!(
            matches!(
                &err,
                Error::LimitExceeded {
                    stream: PdfStream::Attachment { name },
                    limit: Limit::StreamBytes(n),
                } if name == "factur-x.xml" && *n == 64 * MIB
            ),
            "{err:?}"
        );
        assert!(err.to_string().contains("67108864"), "{err}");
    });
}

/// The XMP is inflated through the same bound; over it is an error.
#[test]
fn a_bomb_as_the_xmp_is_refused() {
    common::watchdog::isolated("a_bomb_as_the_xmp_is_refused", || {
        let mut objects = lawful_head("/Metadata 5 0 R");
        objects.push(stream(
            "/Type /Metadata /Subtype /XML /Filter /FlateDecode",
            &bomb(),
        ));
        let err = extract_promptly(&raw_pdf(&objects)).expect_err("over the limit");
        assert!(
            matches!(
                err,
                Error::LimitExceeded {
                    stream: PdfStream::Metadata,
                    limit: Limit::StreamBytes(_),
                }
            ),
            "{err:?}"
        );
    });
}

/// Object streams are measured before the parser loads them.
#[test]
fn a_bomb_as_an_object_stream_is_refused() {
    common::watchdog::isolated("a_bomb_as_an_object_stream_is_refused", || {
        let mut objects = lawful_head("");
        objects.push(stream(
            "/Type /ObjStm /N 1 /First 0 /Filter /FlateDecode",
            &bomb(),
        ));
        let err = extract_promptly(&raw_pdf(&objects)).expect_err("over the limit");
        assert!(
            matches!(
                err,
                Error::LimitExceeded {
                    stream: PdfStream::Structure,
                    limit: Limit::StreamBytes(_),
                }
            ),
            "{err:?}"
        );
        assert!(
            err.to_string()
                .contains("an object or cross-reference stream"),
            "{err}"
        );
    });
}

/// An object stream whose `/Type` is spelled `/Obj#53tm` is still measured.
#[test]
fn an_escaped_objstm_name_is_still_measured() {
    common::watchdog::isolated("an_escaped_objstm_name_is_still_measured", || {
        let mut objects = lawful_head("");
        objects.push(stream(
            "/Type /Obj#53tm /N 1 /First 0 /Filter /FlateDecode",
            &bomb(),
        ));
        let pdf = raw_pdf(&objects);
        assert!(
            !pdf.windows(6).any(|w| w == b"ObjStm"),
            "the name must not appear unescaped"
        );
        let err = extract_promptly(&pdf).expect_err("over the limit");
        assert!(
            matches!(
                err,
                Error::LimitExceeded {
                    stream: PdfStream::Structure,
                    limit: Limit::StreamBytes(_),
                }
            ),
            "{err:?}"
        );
        let err =
            en16931_formats::zugferd::embedded_files(&pdf, &SMALL).expect_err("over the limit");
        assert!(
            matches!(
                err,
                Error::LimitExceeded {
                    stream: PdfStream::Structure,
                    ..
                }
            ),
            "{err:?}"
        );
    });
}

/// Streams each under the per-stream limit, together over the call's total.
#[test]
fn the_sum_of_what_a_call_inflates_is_bounded() {
    let limits = Limits::new()
        .with_max_stream_bytes(1_000)
        .with_max_total_bytes(1_500);
    let pdf = pdf_with(&[("a.bin", &[b'a'; 800]), ("b.bin", &[b'b'; 800])]);
    let err = en16931_formats::zugferd::embedded_files(&pdf, &limits).expect_err("over the total");
    assert!(
        matches!(
            err,
            Error::LimitExceeded {
                stream: PdfStream::Attachment { .. },
                limit: Limit::TotalBytes(1_500),
            }
        ),
        "{err:?}"
    );

    // `extract` counts the invoice and the XMP against the same total.
    let invoice = format!("{INVOICE}{}", " ".repeat(800));
    let pdf = pdf_with(&[("factur-x.xml", invoice.as_bytes())]);
    let tight = Limits::new()
        .with_max_stream_bytes(2_000)
        .with_max_total_bytes(invoice.len() + 10);
    let err = en16931_formats::zugferd::extract(&pdf, &tight).expect_err("over the total");
    assert!(
        matches!(
            err,
            Error::LimitExceeded {
                stream: PdfStream::Metadata,
                limit: Limit::TotalBytes(_),
            }
        ),
        "{err:?}"
    );
}

/// Whatever a call returns fits its limits (each attachment per stream, all
/// within the total), and a call that would need more is refused. Swept over
/// small limits so every boundary is crossed.
#[test]
fn what_a_call_hands_back_stays_within_its_limits() {
    let pdf = pdf_with(&[
        ("a.bin", &[b'a'; 300]),
        ("b.bin", &[b'b'; 500]),
        ("c.bin", &[b'c'; 700]),
    ]);
    let (mut refused, mut returned) = (0, 0);
    for stream_bytes in (100..=800).step_by(100) {
        for total in (100..=1_600).step_by(100) {
            let limits = Limits::new()
                .with_max_stream_bytes(stream_bytes)
                .with_max_total_bytes(total);
            match en16931_formats::zugferd::embedded_files(&pdf, &limits) {
                Ok(files) => {
                    returned += 1;
                    assert!(files.values().all(|f| f.len() <= stream_bytes));
                    assert!(files.values().map(Vec::len).sum::<usize>() <= total);
                }
                Err(Error::LimitExceeded { .. }) => refused += 1,
                Err(e) => panic!("{stream_bytes}/{total}: {e:?}"),
            }
        }
    }
    assert!(
        refused > 0 && returned > 0,
        "{refused} refused, {returned} returned"
    );
}

/// `embedded_files` inflates every attachment, so it is not the side door.
#[test]
fn embedded_files_is_bounded_too() {
    common::watchdog::isolated("embedded_files_is_bounded_too", || {
        let mut objects = lawful_head("");
        objects[3] = stream("/Filter /FlateDecode", &bomb());
        let start = Instant::now();
        let err = en16931_formats::zugferd::embedded_files(&raw_pdf(&objects), &SMALL)
            .expect_err("over the limit");
        assert!(start.elapsed() < PROMPTLY);
        assert!(matches!(err, Error::LimitExceeded { .. }), "{err:?}");
    });
}

/// A corrupt Flate stream is refused, never handed back truncated or encoded.
#[test]
fn a_corrupt_flate_invoice_is_refused_not_returned_as_garbage() {
    common::watchdog::isolated(
        "a_corrupt_flate_invoice_is_refused_not_returned_as_garbage",
        || {
            let mut compressed = Stream::new(dictionary! {}, INVOICE.as_bytes().to_vec());
            compressed.compress().expect("compress");
            let truncated = &compressed.content[..compressed.content.len() / 2];
            for data in [truncated, &[0x78, 0x9C, 0xFF, 0xFF, 0xFF][..]] {
                let mut objects = lawful_head("");
                objects[3] = stream("/Filter /FlateDecode", data);
                let err = extract_promptly(&raw_pdf(&objects)).expect_err("corrupt");
                assert!(
                    matches!(
                        &err,
                        Error::Undecodable { stream: PdfStream::Attachment { name }, .. } if name == "factur-x.xml"
                    ),
                    "{err:?}"
                );
            }
        },
    );
}

#[test]
fn an_unsupported_filter_is_refused() {
    let mut objects = lawful_head("");
    objects[3] = stream("/Filter /JBIG2Decode", INVOICE.as_bytes());
    let err = extract_promptly(&raw_pdf(&objects)).expect_err("undecodable");
    assert!(matches!(err, Error::Undecodable { .. }), "{err:?}");
}

/// A Flate-compressed invoice — the common case — still comes back verbatim.
#[test]
fn a_compressed_invoice_is_inflated() {
    let mut compressed = Stream::new(dictionary! {}, INVOICE.as_bytes().to_vec());
    compressed.compress().expect("compress");
    let mut objects = lawful_head("");
    objects[3] = stream("/Filter /FlateDecode", &compressed.content);
    let got = extract_promptly(&raw_pdf(&objects)).expect("extract");
    assert_eq!(got.xml, INVOICE.as_bytes());
}

// ── Caller-tuned limits ────────────────────────────────────────────────────────

#[test]
fn a_limit_one_byte_short_of_the_payload_refuses_it_and_an_exact_one_does_not() {
    let pdf = build(&[("factur-x.xml", INVOICE.as_bytes())], None);
    let short = Limits::new().with_max_stream_bytes(INVOICE.len() - 1);
    let err = en16931_formats::zugferd::extract(&pdf, &short).expect_err("one byte short");
    assert!(
        matches!(err, Error::LimitExceeded { limit: Limit::StreamBytes(n), .. } if n == INVOICE.len() - 1),
        "{err:?}"
    );

    let exact = Limits::new().with_max_stream_bytes(INVOICE.len());
    let tuned = en16931_formats::zugferd::extract(&pdf, &exact).expect("exactly enough");
    let default = en16931_formats::zugferd::extract(&pdf, &Limits::default()).expect("default");
    assert_eq!(tuned.xml, default.xml);
    assert_eq!(tuned.filename, default.filename);
    assert_eq!(tuned.profile, default.profile);
    assert_eq!(tuned.xmp, default.xmp);
    assert_eq!(tuned.divergence, default.divergence);
}

#[test]
fn a_lowered_object_budget_refuses_a_lawful_tree() {
    let pdf = pdf_with(&[
        ("factur-x.xml", INVOICE.as_bytes()),
        ("a.csv", b"a"),
        ("b.csv", b"b"),
    ]);
    let err = en16931_formats::zugferd::extract(&pdf, &Limits::new().with_max_objects(3))
        .expect_err("four objects, budget three");
    assert!(
        matches!(
            err,
            Error::EmbeddedFilesTree(TreeDefect::Exceeded(Limit::Objects(3)))
        ),
        "{err:?}"
    );
}

/// The defaults match the documentation.
#[test]
fn the_default_limits_are_the_documented_ones() {
    assert_eq!(Limits::DEFAULT_MAX_STREAM_BYTES, 64 * MIB);
    assert_eq!(Limits::DEFAULT_MAX_TOTAL_BYTES, 128 * MIB);
    assert_eq!(Limits::DEFAULT_MAX_OBJECTS, 10_000);
    let d = Limits::default();
    assert_eq!(d, Limits::new());
    assert_eq!(d.max_stream_bytes(), 64 * MIB);
    assert_eq!(d.max_total_bytes(), 128 * MIB);
    assert_eq!(d.max_objects(), 10_000);

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut pages = vec![root.join("README.md")];
    let site = root.join("../../site/content/docs/zugferd.md");
    if site.exists() {
        pages.push(site);
    }
    for page in pages {
        let text = std::fs::read_to_string(&page).expect("read the page");
        for figure in ["64 MiB", "128 MiB", "10 000"] {
            assert!(
                text.contains(figure),
                "{} does not state the default {figure}",
                page.display()
            );
        }
    }
}

// ── The public ZUGFeRD corpus, at its pin ──────────────────────────────────────

/// Every PDF in the public ZUGFeRD corpus at the pinned commit extracts an
/// invoice, except three refused for their recorded reasons. Exact in both
/// directions.
#[test]
fn the_zugferd_corpus_extracts_as_measured() {
    use en16931_formats::zugferd::{Error, extract};

    /// (path under the corpus, what the refusal must be)
    const REFUSED: &[(&str, &str)] = &[
        // Mustang's sample for a deliberately broken encoding attribute.
        (
            "ZUGFeRDv2/fail/Mustangproject/factur-x-invalid-xml-encoding-attribute.pdf",
            "encoding UTF-8-W/O-BOM",
        ),
        // Mustang's sample for a deliberately wrong attachment name.
        (
            "ZUGFeRDv2/fail/Mustangproject/wrongFilename.pdf",
            "no invoice, found factur-y.xml",
        ),
        // A plain PDF invoice, filed under `unstructured/`: nothing attached.
        (
            "unstructured/RE-E-974-Hetzner_2016-01-19_R0005532486.pdf",
            "no invoice, found nothing",
        ),
    ];
    const PDFS: usize = 151;

    let Some(root) = common::require("ZUGFeRD corpus") else {
        return;
    };
    let root = root.join("zugferd-corpus");
    let mut pdfs = Vec::new();
    collect(&root, &mut pdfs);
    pdfs.sort();
    assert_eq!(pdfs.len(), PDFS, "the corpus at its pin holds {PDFS} PDFs");

    let mut extracted = 0usize;
    let mut refused = Vec::new();
    for path in &pdfs {
        let rel = path
            .strip_prefix(&root)
            .expect("under root")
            .to_string_lossy()
            .replace('\\', "/");
        match extract(&std::fs::read(path).expect("read"), &Limits::default()) {
            Ok(_) => extracted += 1,
            Err(e) => {
                let why = match &e {
                    Error::Encoding { found } => format!("encoding {found}"),
                    Error::NoInvoice { found, .. } if found.is_empty() => {
                        "no invoice, found nothing".to_owned()
                    }
                    Error::NoInvoice { found, .. } => {
                        format!("no invoice, found {}", found.join(", "))
                    }
                    other => format!("{other:?}"),
                };
                refused.push((rel, why));
            }
        }
    }
    refused.sort();
    let mut expected: Vec<(String, String)> = REFUSED
        .iter()
        .map(|(p, w)| ((*p).to_owned(), (*w).to_owned()))
        .collect();
    expected.sort();
    assert_eq!(
        refused, expected,
        "the corpus files refused, and why, differ from the measurement"
    );
    assert_eq!(extracted, PDFS - REFUSED.len());
    common::assert_page_states(
        "conformance.md",
        &[
            ("The <N> PDFs of the public ZUGFeRD corpus", PDFS),
            ("<N> come back with an invoice", extracted),
        ],
    );
}

fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
}

// ── Cross-reference streams the parser would trust ─────────────────────────────

/// A PDF whose only cross-reference section is a stream with `dict`.
fn with_xref_stream(dict: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n"
        .to_vec();
    let at = out.len();
    out.extend_from_slice(
        format!("3 0 obj\n<< /Root 1 0 R {dict} /Length 0 >>stream\n\nendstream\nendobj\n")
            .as_bytes(),
    );
    out.extend_from_slice(format!("startxref\n{at}\n%%EOF\n").as_bytes());
    out
}

/// A `/W` asking for a huge allocation is refused before parsing, through every
/// entry point that loads a PDF (allocation failure would abort, not panic).
#[test]
fn a_cross_reference_stream_with_an_absurd_field_width_is_refused() {
    common::watchdog::isolated(
        "a_cross_reference_stream_with_an_absurd_field_width_is_refused",
        || {
            use en16931_formats::zugferd::Error;
            for dict in [
                "/Type /XRef /Size 4 /W [1 4 25555555555555] /Index [1 3]",
                // No `/Type` — the parser does not check it — and the key escaped.
                "/Size 4 /#57 [1 99999999999 1]",
            ] {
                let pdf = with_xref_stream(dict);
                assert!(
                    matches!(extract_promptly(&pdf), Err(Error::Pdf(m)) if m.contains("/W")),
                    "{dict}"
                );
                assert!(en16931_formats::read(&pdf).is_err(), "{dict}");
            }
        },
    );
}

/// Zero-width entries, or a section longer than a PDF may have objects
/// (ISO 32000-1, Annex C: 8 388 607), are refused.
#[test]
fn a_cross_reference_stream_that_never_runs_out_is_refused() {
    common::watchdog::isolated(
        "a_cross_reference_stream_that_never_runs_out_is_refused",
        || {
            for dict in [
                "/Type /XRef /Size 4 /W [0 0 0] /Index [0 9223372036854775807]",
                "/Type /XRef /Size 4 /W [1 2 1] /Index [0 8388608]",
                "/Type /XRef /Size 99999999999 /W [1 2 1]",
            ] {
                let pdf = with_xref_stream(dict);
                assert!(
                    matches!(extract_promptly(&pdf), Err(Error::Pdf(_))),
                    "{dict}"
                );
            }
        },
    );
}

/// Only a real cross-reference stream is scanned: a font's `/W` array without
/// `/Size`, or one in a string or comment, is not one.
#[test]
fn glyph_widths_are_not_mistaken_for_a_cross_reference_stream() {
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some(&xmp("EN 16931", "factur-x.xml")),
    );
    let mut font = pdf.clone();
    let tail = font.len();
    font.splice(
        tail..tail,
        b"% /Size 4 /W [1 999 1]\n9 0 obj\n<< /Type /Font /Subtype /CIDFontType2 /W [1 100 600 [500 700]] (/Size 4 /W [1 999 1]) >>\nendobj\n"
            .iter()
            .copied(),
    );
    assert!(en16931_formats::zugferd::extract(&font, &Limits::default()).is_ok());
}
