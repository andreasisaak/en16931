//! Embedding the invoice: a PDF/A-3 file in, a hybrid invoice out.
//!
//! One incremental update is appended; the caller's bytes stay an unchanged
//! prefix. The update holds:
//!
//! * the CII payload, from [`crate::write`], as an embedded-file stream
//!   (`/Subtype /text#2Fxml`, `/Params` with `/Size` and `/ModDate`);
//! * its file specification (`/F`, `/UF`, `/Desc`, `/AFRelationship`);
//! * the catalogue: the file joins `/AF` and the `/Names/EmbeddedFiles` tree
//!   (rewritten as one sorted leaf);
//! * the XMP packet, edited by range: the four Factur-X properties, their
//!   extension-schema description in the one `pdfaExtension:schemas` bag, and
//!   title, subject and author;
//! * the Info dictionary, agreeing with the XMP;
//! * a cross-reference section of the file's existing kind, chained with `/Prev`.
//!
//! The result is read back with [`super::extract`]; any [`Divergence`] refuses it.
//!
//! # ⚠ Provenance
//!
//! The extension-schema description is PDFlib's from the Factur-X 1.0 info
//! package (`Factur-X_extension_schema.xmp` in akretion/factur-x). Filenames
//! and conformance levels are corroborated against factur-x, Mustang and
//! ZUGFeRD-csharp, not the normative text.

use std::fmt::Write as _;
use std::ops::Range;
use std::time::SystemTime;

use en16931::profiles::{En16931, XRechnung};
use en16931::validation::profile::{ProfileMarker, Validated};
use lopdf::{Dictionary, Object, ObjectId};

use super::extract::{self, Budget};
use super::{Divergence, Error, FILENAMES, Limits, Profile};

/// The Factur-X XMP namespace. Mixed case and the trailing `#` are part of it.
const FX_NS: &str = "urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#";
/// `fx:Version`: the version of the Factur-X XMP schema.
const FX_VERSION: &str = "1.0";

const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const PDFAID_NS: &str = "http://www.aiim.org/pdfa/ns/id/";
const EXTENSION_NS: &str = "http://www.aiim.org/pdfa/ns/extension/";
const SCHEMA_NS: &str = "http://www.aiim.org/pdfa/ns/schema#";
const PROPERTY_NS: &str = "http://www.aiim.org/pdfa/ns/property#";

mod sealed {
    pub trait Sealed {}
    impl Sealed for en16931::profiles::En16931 {}
    impl Sealed for en16931::profiles::XRechnung {}
}

/// A validation profile that has a ZUGFeRD / Factur-X carrier.
///
/// Sealed, implemented for [`En16931`] and [`XRechnung`] only: the profile
/// decides the filename and conformance level.
///
/// ```compile_fail
/// # fn f(proof: en16931::validation::profile::Validated<en16931::profiles::PeppolBis3>) {
/// use en16931_formats::zugferd::{embed, EmbedOptions};
/// // Peppol BIS Billing 3.0 is not a ZUGFeRD profile: this does not compile.
/// let _ = embed(b"%PDF-1.7", &proof, &EmbedOptions::default());
/// # }
/// ```
pub trait Embeddable: ProfileMarker + sealed::Sealed {
    /// The ZUGFeRD profile written to `fx:ConformanceLevel`.
    const ZUGFERD: Profile;
    /// The name the payload is embedded under.
    const FILENAME: &'static str;
}

impl Embeddable for En16931 {
    const ZUGFERD: Profile = Profile::En16931;
    const FILENAME: &'static str = "factur-x.xml";
}

impl Embeddable for XRechnung {
    const ZUGFERD: Profile = Profile::XRechnung;
    const FILENAME: &'static str = "xrechnung.xml";
}

/// How the payload relates to the pages: `/AFRelationship`.
///
/// `Alternative` is what German guidance and the reference implementations
/// write; PDFlib documents `Source` for Factur-X sent abroad. `Data` is not
/// offered: [`super::extract`] reports it as [`Divergence::Relationship`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Relationship {
    /// `/Alternative`.
    #[default]
    Alternative,
    /// `/Source`.
    Source,
}

impl Relationship {
    fn name(self) -> &'static str {
        match self {
            Self::Alternative => "Alternative",
            Self::Source => "Source",
        }
    }
}

/// The document metadata written to the XMP (`dc:title`, `dc:description`,
/// `dc:creator`) and to the Info dictionary (`/Title`, `/Subject`, `/Author`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// The document title.
    pub title: String,
    /// The subject — the XMP's `dc:description`.
    pub subject: String,
    /// The author — the XMP's `dc:creator`.
    pub author: String,
}

impl Metadata {
    /// What [`embed`] writes when it is not told: `"{seller}: Invoice {number}"`,
    /// `"Invoice {number} dated {date} issued by {seller}"`, and the seller —
    /// the factur-x reference implementation's convention, from BT-27, BT-1,
    /// BT-2 (and "Credit note" for a credit note).
    #[must_use]
    pub fn derived(invoice: &en16931::Invoice) -> Self {
        let seller = invoice.seller.name.clone().unwrap_or_default();
        let number = invoice.number.as_deref().unwrap_or_default();
        let date = invoice
            .issue_date
            .map(|d| d.to_string())
            .unwrap_or_default();
        let kind = match invoice.kind {
            en16931::DocumentKind::Invoice => "Invoice",
            en16931::DocumentKind::CreditNote => "Credit note",
        };
        Self {
            title: format!("{seller}: {kind} {number}"),
            subject: format!("{kind} {number} dated {date} issued by {seller}"),
            author: seller,
        }
    }
}

/// What [`embed`] may be told. Everything has a default.
#[derive(Debug, Clone, Default)]
pub struct EmbedOptions {
    /// `/AFRelationship`; [`Relationship::Alternative`] by default.
    pub relationship: Relationship,
    /// The document metadata; [`Metadata::derived`] from the invoice when `None`.
    pub metadata: Option<Metadata>,
    /// The embedded file's `/ModDate`; the time of the call when `None`. Fix it
    /// for byte-identical output from identical input.
    pub modified: Option<SystemTime>,
}

/// What made [`embed`] refuse a PDF it already carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Existing {
    /// XMP of the ZUGFeRD / Factur-X / Order-X family, by namespace.
    Xmp {
        /// The namespace found.
        namespace: String,
    },
    /// An attachment under a name a reader takes for the invoice.
    Attachment {
        /// The attachment's name.
        name: String,
    },
}

impl std::fmt::Display for Existing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Xmp { namespace } => write!(f, "invoice metadata in the namespace {namespace:?}"),
            Self::Attachment { name } => write!(f, "an attachment named {name:?}"),
        }
    }
}

/// Why [`embed`] returned no PDF.
///
/// `embed` never converts or repairs its input.
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// The bytes are not a PDF the reader parses, within its default limits.
    #[error(transparent)]
    Read(#[from] Error),
    /// The XMP declares no PDF/A conformance, or a part other than 3.
    ///
    /// PDF/A-1 and PDF/A-2 forbid embedding an XML file.
    #[error(
        "{} — a hybrid invoice must be PDF/A-3, and embed does not convert to PDF/A",
        match part { Some(p) => format!("the PDF declares PDF/A-{p}"), None => "the PDF declares no PDF/A conformance".to_owned() }
    )]
    NotPdfA3 {
        /// `pdfaid:part`, when the XMP states one.
        part: Option<String>,
    },
    /// The PDF is encrypted, which PDF/A forbids.
    #[error("the PDF is encrypted, which PDF/A forbids")]
    Encrypted,
    /// The catalogue has no `/Metadata` stream to declare PDF/A in.
    #[error("the PDF has no /Metadata stream, so it declares no PDF/A conformance")]
    NoMetadata,
    /// The `/Metadata` stream is filtered; PDF/A requires it plain.
    #[error("the /Metadata stream is filtered; PDF/A requires the XMP packet unfiltered")]
    FilteredMetadata,
    /// The XMP packet is not UTF-8 XML with an `rdf:RDF` element.
    #[error("the XMP packet cannot be edited: {0}")]
    Metadata(String),
    /// The PDF already carries an invoice or its metadata.
    #[error("the PDF already carries {0}; embedding a second invoice is refused")]
    AlreadyHybrid(Existing),
    /// The file written does not read back without a divergence. A defect in
    /// `embed`, reported instead of shipped.
    #[error("the written file reads back with divergences: {0:?}")]
    Divergent(Vec<Divergence>),
    /// The file written does not read back with the payload written. A
    /// defect in `embed`, reported instead of shipped.
    #[error("the written file reads back with a different payload")]
    PayloadChanged,
}

/// Embed a validated invoice into a PDF/A-3 file.
///
/// The payload is [`crate::write`]`(invoice, Syntax::Cii)`; its filename and
/// `fx:ConformanceLevel` follow from `P` ([`Embeddable`]). The input must
/// already be PDF/A-3. The result is the input plus one incremental update,
/// read back through [`super::extract`] before it is returned.
///
/// ```no_run
/// use en16931::profiles::En16931;
/// use en16931::validation::profile::Validated;
/// use en16931_formats::zugferd::{self, EmbedOptions};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let invoice = en16931::Invoice::default();
/// let proof = Validated::<En16931>::new(invoice)?;
/// let pdf_a3 = std::fs::read("rendered.pdf")?;
/// let hybrid = zugferd::embed(&pdf_a3, &proof, &EmbedOptions::default())?;
/// std::fs::write("invoice.pdf", hybrid)?;
/// # Ok(()) }
/// ```
///
/// # Errors
///
/// [`EmbedError`]: a PDF that does not parse, does not declare PDF/A-3, is
/// encrypted, has no or a filtered `/Metadata` stream or an XMP packet that
/// is not XML, or already carries an invoice.
pub fn embed<P: Embeddable>(
    pdf_a: &[u8],
    invoice: &Validated<P>,
    options: &EmbedOptions,
) -> Result<Vec<u8>, EmbedError> {
    let limits = Limits::default();
    if pdf_a.starts_with(b"%PDF") && encrypted(pdf_a) {
        return Err(EmbedError::Encrypted);
    }
    let doc = extract::load(pdf_a, &mut Budget::new(&limits))?;
    if doc.trailer.has(b"Encrypt") || doc.encryption_state.is_some() {
        return Err(EmbedError::Encrypted);
    }
    let malformed = |what: &str| EmbedError::Read(Error::Pdf(what.to_owned()));
    let catalog_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| malformed("the trailer has no /Root reference"))?;
    let catalog = doc
        .get_dictionary(catalog_id)
        .map_err(|_| malformed("/Root is not a dictionary"))?;
    let meta_id = catalog
        .get(b"Metadata")
        .and_then(Object::as_reference)
        .map_err(|_| EmbedError::NoMetadata)?;
    let meta = doc
        .get_object(meta_id)
        .and_then(Object::as_stream)
        .map_err(|_| EmbedError::NoMetadata)?;
    if meta.dict.has(b"Filter") {
        return Err(EmbedError::FilteredMetadata);
    }
    let packet = std::str::from_utf8(&meta.content)
        .map_err(|e| EmbedError::Metadata(format!("not UTF-8: {e}")))?;
    let xmp = Xmp::parse(packet)?;
    if xmp.part.as_deref() != Some("3") {
        return Err(EmbedError::NotPdfA3 { part: xmp.part });
    }
    if let Some(namespace) = xmp.invoice_namespace {
        return Err(EmbedError::AlreadyHybrid(Existing::Xmp { namespace }));
    }
    if let Some(name) = extract::filespecs(&doc)
        .into_keys()
        .find(|n| FILENAMES.iter().any(|f| f.eq_ignore_ascii_case(n)))
    {
        return Err(EmbedError::AlreadyHybrid(Existing::Attachment { name }));
    }

    let payload = crate::write(invoice, crate::Syntax::Cii).xml.into_bytes();
    let metadata = options
        .metadata
        .clone()
        .unwrap_or_else(|| Metadata::derived(invoice.invoice()));
    let new_packet = xmp.edit(packet, P::ZUGFERD, P::FILENAME, &metadata);

    let mut next = doc
        .trailer
        .get(b"Size")
        .and_then(Object::as_i64)
        .ok()
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
        .max(doc.max_id + 1);
    let mut allocate = || {
        let id = (next, 0);
        next += 1;
        id
    };
    let file_id = allocate();
    let spec_id = allocate();
    let tree_id = allocate();

    let mut update = Update::new(pdf_a);

    // ── the embedded file and its specification ──────────────────────────
    let compressed = deflate(&payload);
    let mut params = Dictionary::new();
    params.set("Size", Object::Integer(len(payload.len())));
    params.set(
        "ModDate",
        text(&pdf_date(options.modified.unwrap_or_else(SystemTime::now))),
    );
    let mut file = Dictionary::new();
    file.set("Type", Object::Name(b"EmbeddedFile".to_vec()));
    file.set("Subtype", Object::Name(b"text/xml".to_vec()));
    file.set("Params", Object::Dictionary(params));
    file.set("Filter", Object::Name(b"FlateDecode".to_vec()));
    update.stream(file_id, file, &compressed);

    let mut ef = Dictionary::new();
    ef.set("F", Object::Reference(file_id));
    ef.set("UF", Object::Reference(file_id));
    let mut spec = Dictionary::new();
    spec.set("Type", Object::Name(b"Filespec".to_vec()));
    spec.set("F", text(P::FILENAME));
    spec.set("UF", text(P::FILENAME));
    spec.set(
        "Desc",
        text(&format!(
            "ZUGFeRD / Factur-X invoice, profile {}",
            P::ZUGFERD
        )),
    );
    spec.set(
        "AFRelationship",
        Object::Name(options.relationship.name().as_bytes().to_vec()),
    );
    spec.set("EF", Object::Dictionary(ef));
    update.object(spec_id, &Object::Dictionary(spec));

    // ── the name tree, flattened into one sorted leaf ────────────────────
    let mut entries = extract::embedded_file_entries(&doc, &limits)?;
    entries.push((text(P::FILENAME), Object::Reference(spec_id)));
    entries.sort_by(|(a, _), (b, _)| key_bytes(a).cmp(key_bytes(b)));
    let names: Vec<Object> = entries.into_iter().flat_map(|(k, v)| [k, v]).collect();
    let mut tree = Dictionary::new();
    tree.set("Names", Object::Array(names));
    update.object(tree_id, &Object::Dictionary(tree));

    // ── the catalogue ────────────────────────────────────────────────────
    let mut root = catalog.clone();
    let mut af = catalog
        .get(b"AF")
        .and_then(|o| doc.dereference(o))
        .and_then(|(_, o)| o.as_array())
        .cloned()
        .unwrap_or_default();
    af.push(Object::Reference(spec_id));
    root.set("AF", Object::Array(af));
    let mut name_dict = catalog
        .get(b"Names")
        .and_then(|o| doc.dereference(o))
        .and_then(|(_, o)| o.as_dict())
        .cloned()
        .unwrap_or_default();
    name_dict.set("EmbeddedFiles", Object::Reference(tree_id));
    root.set("Names", Object::Dictionary(name_dict));
    update.object(catalog_id, &Object::Dictionary(root));

    // ── the XMP packet ───────────────────────────────────────────────────
    let mut meta_dict = meta.dict.clone();
    meta_dict.remove(b"Length");
    update.stream(meta_id, meta_dict, new_packet.as_bytes());

    // ── the Info dictionary, agreeing with the XMP ───────────────────────
    let info_ref = doc.trailer.get(b"Info").and_then(Object::as_reference).ok();
    let mut info = doc
        .trailer
        .get(b"Info")
        .and_then(|o| doc.dereference(o))
        .and_then(|(_, o)| o.as_dict())
        .cloned()
        .unwrap_or_default();
    info.set("Title", text(&metadata.title));
    info.set("Subject", text(&metadata.subject));
    info.set("Author", text(&metadata.author));
    let info_id = info_ref.unwrap_or_else(&mut allocate);
    update.object(info_id, &Object::Dictionary(info));

    let mut trailer = Dictionary::new();
    trailer.set("Root", Object::Reference(catalog_id));
    trailer.set("Info", Object::Reference(info_id));
    if let Ok(id) = doc.trailer.get(b"ID") {
        trailer.set("ID", id.clone());
    }
    let prev = last_startxref(pdf_a).ok_or_else(|| malformed("no startxref"))?;
    let out = update.finish(prev, xref_is_stream(pdf_a, prev), trailer, allocate);

    // ── read it back, as a receiver will ─────────────────────────────────
    let got = super::extract(&out, &limits)?;
    if !got.divergence.is_empty() {
        return Err(EmbedError::Divergent(got.divergence));
    }
    if got.xml != payload {
        return Err(EmbedError::PayloadChanged);
    }
    Ok(out)
}

/// Whether the file's last trailer names an `/Encrypt` dictionary — checked on
/// the bytes, because the parser may fail on an encrypted file before saying so.
fn encrypted(pdf: &[u8]) -> bool {
    let tail = &pdf[pdf.len().saturating_sub(4096)..];
    tail.windows(8).any(|w| w == b"/Encrypt")
}

fn len(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn key_bytes(o: &Object) -> &[u8] {
    o.as_str().unwrap_or_default()
}

/// A PDF text string: `PDFDocEncoding` for printable ASCII, UTF-16BE otherwise.
fn text(s: &str) -> Object {
    let bytes = if s.bytes().all(|b| (0x20..0x7F).contains(&b)) {
        s.as_bytes().to_vec()
    } else {
        [0xFE, 0xFF]
            .into_iter()
            .chain(s.encode_utf16().flat_map(u16::to_be_bytes))
            .collect()
    };
    Object::String(bytes, lopdf::StringFormat::Hexadecimal)
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    // Writing into a `Vec` cannot fail.
    let _ = z.write_all(bytes);
    z.finish().unwrap_or_default()
}

/// `D:YYYYMMDDHHmmSS+00'00'`, in UTC.
fn pdf_date(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "D:{year:04}{month:02}{day:02}{:02}{:02}{:02}+00'00'",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The byte offset the file's last `startxref` points at.
fn last_startxref(pdf: &[u8]) -> Option<usize> {
    let needle = b"startxref";
    let at = pdf.windows(needle.len()).rposition(|w| w == needle)? + needle.len();
    let end = (at + 40).min(pdf.len());
    String::from_utf8_lossy(&pdf[at..end])
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Whether the section at `offset` is a cross-reference stream rather than a
/// table: an update keeps the kind the file already uses.
fn xref_is_stream(pdf: &[u8], offset: usize) -> bool {
    let rest = pdf.get(offset..).unwrap_or_default();
    let start = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(0);
    !rest[start..].starts_with(b"xref")
}

// ── the appended revision ─────────────────────────────────────────────────

/// The input, then the objects of one incremental update, then its
/// cross-reference section.
struct Update {
    out: Vec<u8>,
    offsets: Vec<(ObjectId, usize)>,
}

impl Update {
    fn new(original: &[u8]) -> Self {
        let mut out = Vec::with_capacity(original.len() + 16 * 1024);
        out.extend_from_slice(original);
        if !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        Self {
            out,
            offsets: Vec::new(),
        }
    }

    fn object(&mut self, id: ObjectId, object: &Object) {
        self.offsets.push((id, self.out.len()));
        let _ = writeln!(Bytes(&mut self.out), "{} {} obj", id.0, id.1);
        write_object(&mut self.out, object);
        self.out.extend_from_slice(b"\nendobj\n");
    }

    fn stream(&mut self, id: ObjectId, mut dict: Dictionary, content: &[u8]) {
        dict.set("Length", Object::Integer(len(content.len())));
        self.offsets.push((id, self.out.len()));
        let _ = writeln!(Bytes(&mut self.out), "{} {} obj", id.0, id.1);
        write_object(&mut self.out, &Object::Dictionary(dict));
        self.out.extend_from_slice(b"\nstream\n");
        self.out.extend_from_slice(content);
        self.out.extend_from_slice(b"\nendstream\nendobj\n");
    }

    /// Append the cross-reference section — a table, or a stream object when
    /// the file uses streams — and the trailer chained to `prev`.
    fn finish(
        mut self,
        prev: usize,
        as_stream: bool,
        mut trailer: Dictionary,
        mut allocate: impl FnMut() -> ObjectId,
    ) -> Vec<u8> {
        trailer.set("Prev", Object::Integer(len(prev)));
        if as_stream {
            let id = allocate();
            let xref_at = self.out.len();
            self.offsets.push((id, xref_at));
            self.offsets.sort_unstable();
            let size = id.0 + 1;
            let width: u8 = if u32::try_from(xref_at).is_ok() { 4 } else { 8 };
            let mut index = Vec::new();
            let mut rows = Vec::new();
            for run in runs(&self.offsets) {
                index.push(Object::Integer(i64::from(run[0].0.0)));
                index.push(Object::Integer(len(run.len())));
                for ((_, generation), offset) in run {
                    rows.push(1);
                    let offset = offset.to_be_bytes();
                    rows.extend_from_slice(&offset[offset.len() - usize::from(width)..]);
                    rows.extend_from_slice(&generation.to_be_bytes());
                }
            }
            trailer.set("Type", Object::Name(b"XRef".to_vec()));
            trailer.set("Size", Object::Integer(i64::from(size)));
            trailer.set("Index", Object::Array(index));
            trailer.set(
                "W",
                Object::Array(vec![
                    Object::Integer(1),
                    Object::Integer(i64::from(width)),
                    Object::Integer(2),
                ]),
            );
            self.offsets.pop();
            self.stream(id, trailer, &rows);
            let _ = write!(Bytes(&mut self.out), "startxref\n{xref_at}\n%%EOF\n");
        } else {
            let size = allocate().0;
            let xref_at = self.out.len();
            self.offsets.sort_unstable();
            self.out.extend_from_slice(b"xref\n");
            for run in runs(&self.offsets) {
                let _ = writeln!(Bytes(&mut self.out), "{} {}", run[0].0.0, run.len());
                for ((_, generation), offset) in run {
                    let _ = writeln!(Bytes(&mut self.out), "{offset:010} {generation:05} n ");
                }
            }
            trailer.set("Size", Object::Integer(i64::from(size)));
            self.out.extend_from_slice(b"trailer\n");
            write_object(&mut self.out, &Object::Dictionary(trailer));
            let _ = write!(Bytes(&mut self.out), "\nstartxref\n{xref_at}\n%%EOF\n");
        }
        self.out
    }
}

/// Consecutive object numbers, as cross-reference subsections.
fn runs(sorted: &[(ObjectId, usize)]) -> Vec<&[(ObjectId, usize)]> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..=sorted.len() {
        if i == sorted.len() || sorted[i].0.0 != sorted[i - 1].0.0 + 1 {
            out.push(&sorted[start..i]);
            start = i;
        }
    }
    out
}

/// `fmt::Write` over a byte buffer.
struct Bytes<'a>(&'a mut Vec<u8>);

impl std::fmt::Write for Bytes<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

/// One PDF object, in the plainest spelling: strings as hex, names escaped.
fn write_object(out: &mut Vec<u8>, object: &Object) {
    match object {
        Object::Null | Object::Stream(_) => out.extend_from_slice(b"null"),
        Object::Boolean(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Object::Integer(i) => {
            let _ = write!(Bytes(out), "{i}");
        }
        Object::Real(r) => {
            let _ = write!(Bytes(out), "{r}");
        }
        Object::Name(name) => write_name(out, name),
        Object::String(bytes, _) => {
            out.push(b'<');
            for b in bytes {
                let _ = write!(Bytes(out), "{b:02X}");
            }
            out.push(b'>');
        }
        Object::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write_object(out, item);
            }
            out.push(b']');
        }
        Object::Dictionary(dict) => {
            out.extend_from_slice(b"<<");
            for (key, value) in dict {
                write_name(out, key);
                out.push(b' ');
                write_object(out, value);
                out.push(b' ');
            }
            out.extend_from_slice(b">>");
        }
        Object::Reference((n, g)) => {
            let _ = write!(Bytes(out), "{n} {g} R");
        }
    }
}

fn write_name(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b'/');
    for &b in name {
        if b" \t\n\r\x0C()<>[]{}/%#".contains(&b) || !(33..=126).contains(&b) {
            let _ = write!(Bytes(out), "#{b:02X}");
        } else {
            out.push(b);
        }
    }
}

// ── the XMP packet ────────────────────────────────────────────────────────

/// What the packet says, and where the edits go.
struct Xmp {
    /// `pdfaid:part`, as an attribute or an element.
    part: Option<String>,
    /// A ZUGFeRD / Factur-X / Order-X namespace already present.
    invoice_namespace: Option<String>,
    /// `rdf:about` of the first description, which every new one repeats.
    about: String,
    /// Existing `dc:title`, `dc:description`, `dc:creator` — replaced.
    replaced: Vec<Range<usize>>,
    /// Where the extension schema goes.
    bag: Bag,
    /// Just before `</rdf:RDF>`.
    end_of_rdf: usize,
}

enum Bag {
    /// After the last item of the existing `pdfaExtension:schemas` bag.
    Append(usize),
    /// The bag exists and is empty: replace it.
    Replace(Range<usize>),
    /// No `pdfaExtension:schemas` property: create one.
    None,
}

impl Xmp {
    fn parse(packet: &str) -> Result<Self, EmbedError> {
        let doc =
            roxmltree::Document::parse(packet).map_err(|e| EmbedError::Metadata(e.to_string()))?;
        let rdf = doc
            .descendants()
            .find(|n| n.has_tag_name((RDF_NS, "RDF")))
            .ok_or_else(|| EmbedError::Metadata("no rdf:RDF element".to_owned()))?;
        let descriptions: Vec<_> = rdf
            .children()
            .filter(|n| n.has_tag_name((RDF_NS, "Description")))
            .collect();
        let part = descriptions.iter().find_map(|d| {
            d.attribute((PDFAID_NS, "part"))
                .map(str::to_owned)
                .or_else(|| {
                    d.children()
                        .find(|c| c.has_tag_name((PDFAID_NS, "part")))
                        .and_then(|c| c.text())
                        .map(|t| t.trim().to_owned())
                })
        });
        let about = descriptions
            .first()
            .and_then(|d| d.attribute((RDF_NS, "about")))
            .unwrap_or_default()
            .to_owned();
        let invoice_namespace = doc.descendants().find_map(|n| {
            n.namespaces()
                .map(roxmltree::Namespace::uri)
                .chain(n.is_text().then(|| n.text().unwrap_or_default().trim()))
                .find(|uri| invoice_family(uri))
                .map(str::to_owned)
        });
        let replaced = descriptions
            .iter()
            .flat_map(roxmltree::Node::children)
            .filter(|c| {
                c.tag_name().namespace() == Some(DC_NS)
                    && matches!(c.tag_name().name(), "title" | "description" | "creator")
            })
            .map(|c| c.range())
            .collect();
        let bag = match rdf
            .descendants()
            .find(|n| n.has_tag_name((EXTENSION_NS, "schemas")))
        {
            None => Bag::None,
            Some(schemas) => {
                let bag = schemas
                    .children()
                    .find(|n| n.has_tag_name((RDF_NS, "Bag")))
                    .ok_or_else(|| {
                        EmbedError::Metadata("pdfaExtension:schemas carries no rdf:Bag".to_owned())
                    })?;
                match bag.children().rfind(roxmltree::Node::is_element) {
                    Some(item) => Bag::Append(item.range().end),
                    None => Bag::Replace(bag.range()),
                }
            }
        };
        let end_of_rdf = rdf
            .last_child()
            .map(|c| c.range().end)
            .ok_or_else(|| EmbedError::Metadata("rdf:RDF is empty".to_owned()))?;
        Ok(Self {
            part,
            invoice_namespace,
            about,
            replaced,
            bag,
            end_of_rdf,
        })
    }

    /// The packet with the edits applied, every other byte as it was.
    fn edit(self, packet: &str, profile: Profile, filename: &str, meta: &Metadata) -> String {
        let about = escape(&self.about);
        let mut edits: Vec<(Range<usize>, String)> = self
            .replaced
            .into_iter()
            .map(|r| (r, String::new()))
            .collect();
        match self.bag {
            Bag::Append(at) => edits.push((at..at, schema_item())),
            Bag::Replace(range) => edits.push((
                range,
                format!(
                    "<rdf:Bag xmlns:rdf=\"{RDF_NS}\">{}</rdf:Bag>",
                    schema_item()
                ),
            )),
            Bag::None => edits.push((
                self.end_of_rdf..self.end_of_rdf,
                format!(
                    "\n<rdf:Description xmlns:rdf=\"{RDF_NS}\" xmlns:pdfaExtension=\"{EXTENSION_NS}\" rdf:about=\"{about}\"><pdfaExtension:schemas><rdf:Bag>{}</rdf:Bag></pdfaExtension:schemas></rdf:Description>",
                    schema_item()
                ),
            )),
        }
        edits.push((
            self.end_of_rdf..self.end_of_rdf,
            format!(
                "\n<rdf:Description xmlns:rdf=\"{RDF_NS}\" xmlns:dc=\"{DC_NS}\" xmlns:fx=\"{FX_NS}\" rdf:about=\"{about}\">\
<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{title}</rdf:li></rdf:Alt></dc:title>\
<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{subject}</rdf:li></rdf:Alt></dc:description>\
<dc:creator><rdf:Seq><rdf:li>{author}</rdf:li></rdf:Seq></dc:creator>\
<fx:DocumentType>INVOICE</fx:DocumentType>\
<fx:DocumentFileName>{filename}</fx:DocumentFileName>\
<fx:Version>{FX_VERSION}</fx:Version>\
<fx:ConformanceLevel>{level}</fx:ConformanceLevel>\
</rdf:Description>\n",
                title = escape(&meta.title),
                subject = escape(&meta.subject),
                author = escape(&meta.author),
                level = profile.as_str(),
            ),
        ));
        // Applied from the end, so earlier ranges stay valid; ties keep their
        // order of insertion.
        edits.sort_by_key(|(r, _)| r.start);
        let mut out = packet.to_owned();
        for (range, with) in edits.into_iter().rev() {
            out.replace_range(range, &with);
        }
        out
    }
}

/// Whether `uri` is a namespace of the ZUGFeRD / Factur-X / Order-X family:
/// ZUGFeRD 1.0's `urn:ferd:` (its release candidate included), ZUGFeRD 2.0's
/// `urn:zugferd:`, Factur-X's `urn:factur-x:`, or any spelling of
/// `CrossIndustryDocument`.
fn invoice_family(uri: &str) -> bool {
    let uri = uri.trim().to_ascii_lowercase();
    uri.contains("crossindustrydocument")
        || ["urn:ferd:", "urn:zugferd:", "urn:factur-x:"]
            .iter()
            .any(|p| uri.starts_with(p))
}

/// The Factur-X extension-schema description, as one `rdf:li` of a
/// `pdfaExtension:schemas` bag, declaring the prefixes it uses.
fn schema_item() -> String {
    let property = |name: &str, description: &str| {
        format!(
            "<rdf:li rdf:parseType=\"Resource\"><pdfaProperty:name>{name}</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>{description}</pdfaProperty:description></rdf:li>"
        )
    };
    format!(
        "<rdf:li xmlns:rdf=\"{RDF_NS}\" xmlns:pdfaSchema=\"{SCHEMA_NS}\" xmlns:pdfaProperty=\"{PROPERTY_NS}\" rdf:parseType=\"Resource\"><pdfaSchema:schema>Factur-X PDFA Extension Schema</pdfaSchema:schema><pdfaSchema:namespaceURI>{FX_NS}</pdfaSchema:namespaceURI><pdfaSchema:prefix>fx</pdfaSchema:prefix><pdfaSchema:property><rdf:Seq>{}{}{}{}</rdf:Seq></pdfaSchema:property></rdf:li>",
        property("DocumentFileName", "name of the embedded XML invoice file"),
        property("DocumentType", "INVOICE"),
        property("Version", "The actual version of the Factur-X XML schema"),
        property(
            "ConformanceLevel",
            "The conformance level of the embedded Factur-X data"
        ),
    )
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_civil_utc() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_791_640_800);
        assert_eq!(pdf_date(t), "D:20261010140000+00'00'");
        assert_eq!(pdf_date(SystemTime::UNIX_EPOCH), "D:19700101000000+00'00'");
    }

    #[test]
    fn names_are_escaped() {
        let mut out = Vec::new();
        write_name(&mut out, b"text/xml");
        assert_eq!(out, b"/text#2Fxml");
    }

    #[test]
    fn subsections_follow_consecutive_numbers() {
        let ids = [((3, 0), 1), ((4, 0), 2), ((9, 0), 3)];
        let r = runs(&ids);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].len(), 2);
    }

    #[test]
    fn the_namespace_is_the_specified_spelling() {
        assert_eq!(
            FX_NS,
            "urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#"
        );
    }
}
