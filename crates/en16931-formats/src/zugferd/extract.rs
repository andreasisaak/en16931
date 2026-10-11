//! Getting the invoice out of the PDF: parse, find the embedded file by name,
//! inflate it within [`Limits`], and compare the XMP metadata with the payload.
//!
//! ⚠ ZUGFeRD 2.1 and Factur-X embed `factur-x.xml`, ZUGFeRD 2.0
//! `zugferd-invoice.xml`, ZUGFeRD 1.0 `ZUGFeRD-invoice.xml`; the reader accepts
//! every name in [`FILENAMES`], case-insensitively.

use std::collections::{BTreeMap, HashSet};

use super::{Error, IsInvoice, Limit, PdfStream, Profile, TreeDefect};

/// Embedded filenames a ZUGFeRD or Factur-X invoice may use. ⚠
///
/// Ordered by preference: a file carrying two resolves to the first listed.
pub const FILENAMES: &[&str] = &[
    "factur-x.xml",
    "zugferd-invoice.xml",
    "xrechnung.xml",
    "order-x.xml",
];

/// What the PDF's XMP metadata declares about the invoice inside it.
///
/// The `fx:` fields are read from the ZUGFeRD / Factur-X namespaces only (the
/// newest winning per field), the `pdfaid:` fields from the PDF/A namespace
/// only; same-named properties elsewhere (`adt:DocumentFileName`) are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Xmp {
    /// `fx:DocumentType` — `INVOICE` or `ORDER`. ⚠
    pub document_type: Option<String>,
    /// `fx:DocumentFileName` — the embedded file's name, as the metadata
    /// claims it. May disagree with the file actually attached. ⚠
    pub document_filename: Option<String>,
    /// `fx:Version`: the version of the Factur-X XMP schema (`1.0` in every
    /// conforming file), not of ZUGFeRD. No XMP property carries the ZUGFeRD
    /// version; the profile is [`conformance_level`](Self::conformance_level).
    pub version: Option<String>,
    /// `fx:ConformanceLevel` — the profile, in the XMP's own vocabulary. ⚠
    pub conformance_level: Option<String>,
    /// `pdfaid:part`: which part of ISO 19005 the file claims. A hybrid invoice
    /// must say `3`; see [`Divergence::NotPdfA3`].
    pub pdfa_part: Option<String>,
    /// `pdfaid:conformance`: `A`, `B` or `U`, all permitted. Not checked.
    pub pdfa_conformance: Option<String>,
}

/// A disagreement between what the PDF declares and what it contains.
///
/// Reported, not refused: the payload is still readable.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Divergence {
    /// The XMP names a different profile than the payload's BT-24.
    Profile {
        /// What `fx:ConformanceLevel` says.
        xmp: String,
        /// What BT-24 says.
        payload: String,
    },
    /// The XMP names a different embedded filename than the one attached.
    Filename {
        /// What `fx:DocumentFileName` says.
        xmp: String,
        /// The file actually found.
        actual: String,
    },
    /// A full-invoice profile is attached as `/AFRelationship /Data`.
    ///
    /// `Data` is right only for MINIMUM and BASIC WL; for profiles with lines
    /// every source agrees it is wrong (they differ on `Alternative` vs
    /// `Source`, which is not checked).
    Relationship {
        /// The value found.
        found: String,
        /// The profile the payload claims, which is what makes it wrong.
        profile: Profile,
    },
    /// The PDF carries no XMP invoice metadata at all — no packet, or one
    /// that is not well-formed XML.
    ///
    /// The embedded file was still found by name.
    NoXmp,
    /// The invoice is not listed in the document catalogue's `/AF` array.
    ///
    /// Without `/AF` the XML is an ordinary attachment, not an associated file.
    NotAssociated,
    /// The invoice is not reachable from `/Names/EmbeddedFiles`.
    ///
    /// ZUGFeRD requires it there; it was reached through a page annotation or a
    /// bare file specification instead.
    NotInEmbeddedFiles,
    /// The invoice's file specification carries no `/AFRelationship`.
    ///
    /// Lawful PDF, but a receiver routing on the relationship cannot tell whether
    /// the XML is the invoice.
    NoRelationship,
    /// The PDF does not claim PDF/A-3.
    ///
    /// ZUGFeRD 2.x and Factur-X require PDF/A-3; parts 1 and 2 forbid embedding
    /// a file of arbitrary type.
    NotPdfA3 {
        /// `pdfaid:part`, when the file states one at all.
        part: Option<String>,
    },
}

impl std::fmt::Display for Divergence {
    /// One sentence naming both halves of the disagreement.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Profile { xmp, payload } => write!(
                f,
                "the PDF's XMP declares profile {xmp:?} and the payload's BT-24 says {payload:?} — \
                 a receiver routing on the metadata and one routing on the invoice will disagree"
            ),
            Self::Filename { xmp, actual } => write!(
                f,
                "the PDF's XMP names the embedded file {xmp:?} and the attached file is {actual:?}"
            ),
            Self::Relationship { found, profile } => write!(
                f,
                "the invoice is attached as /AFRelationship /{found}, but {profile:?} carries the \
                 invoice itself rather than accompanying one — /Data says the pages are the invoice"
            ),
            Self::NoXmp => f.write_str(
                "the PDF carries no XMP invoice metadata; the payload was found by filename, and \
                 a counterparty scanning metadata first will not see an e-invoice here",
            ),
            Self::NotAssociated => f.write_str(
                "the invoice is not in the document catalogue's /AF array, so it is an ordinary \
                 attachment rather than an associated file — a PDF/A-3 receiver asking what is \
                 associated with this document is told nothing",
            ),
            Self::NotInEmbeddedFiles => f.write_str(
                "the invoice is not reachable from /Names/EmbeddedFiles, where ZUGFeRD requires \
                 it so that readers without PDF/A-3 support still find it",
            ),
            Self::NoRelationship => f.write_str(
                "the invoice's file specification carries no /AFRelationship, so nothing in the \
                 PDF says whether the XML is the invoice or merely accompanies one",
            ),
            Self::NotPdfA3 { part } => match part {
                Some(p) => write!(
                    f,
                    "the PDF claims PDF/A-{p}; ZUGFeRD and Factur-X require PDF/A-3, and parts 1 \
                     and 2 forbid embedding a file of arbitrary type at all"
                ),
                None => f.write_str(
                    "the PDF claims no PDF/A conformance; ZUGFeRD and Factur-X require PDF/A-3, \
                     and a recipient running veraPDF on what arrives will reject it",
                ),
            },
        }
    }
}

/// What extraction produced.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Extracted {
    /// The payload verbatim, in whatever encoding it carries; [`crate::read`]
    /// reads it.
    pub xml: Vec<u8>,
    /// The embedded file's name, as the PDF records it.
    pub filename: String,
    /// The profile the document claims in its BT-24 (a claim, not a finding).
    pub profile: Profile,
    /// BT-24 verbatim, when the payload carries one.
    pub specification_id: Option<String>,
    /// `/AFRelationship` for the embedded invoice, verbatim.
    ///
    /// `None` when the attachment carries none. See [`Divergence::Relationship`].
    pub relationship: Option<String>,
    /// What the PDF's own metadata declares, independently of the payload.
    pub xmp: Xmp,
    /// Disagreements between the metadata and the payload. Usually empty.
    pub divergence: Vec<Divergence>,
}

/// How much one extraction call may inflate, and how far it may walk.
///
/// Compressed streams and object references both amplify a small input, so
/// the reader bounds them itself:
///
/// ```no_run
/// use en16931_formats::zugferd::{self, Limits};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let pdf = std::fs::read("invoice.pdf")?;
/// // The defaults: 64 MiB per stream, 128 MiB per call, 10 000 objects per walk.
/// let got = zugferd::extract(&pdf, &Limits::default())?;
///
/// // A public upload endpoint can be stricter, an archive of invoices that
/// // carry large attachments inline more generous.
/// let tight = Limits::new().with_max_stream_bytes(20 << 20);
/// let got = zugferd::extract(&pdf, &tight)?;
/// # Ok(()) }
/// ```
///
/// * Every stream a call inflates (invoice, XMP, object and cross-reference
///   streams) is cut off at [`max_stream_bytes`](Self::max_stream_bytes) + 1.
/// * Their sum is held to [`max_total_bytes`](Self::max_total_bytes);
///   cross-reference streams are bounded one at a time, not summed.
/// * Every object-graph walk visits each object once, at most
///   [`max_objects`](Self::max_objects), at most 32 levels deep, no cycles.
///
/// Input size and parse time are the caller's to cap. Exceeding a limit is
/// [`Error::LimitExceeded`] or [`Error::EmbeddedFilesTree`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    stream_bytes: usize,
    total_bytes: usize,
    objects: usize,
}

impl Limits {
    /// The default for [`max_stream_bytes`](Self::max_stream_bytes): 64 MiB,
    /// room for BG-24 attachments inline.
    pub const DEFAULT_MAX_STREAM_BYTES: usize = 64 * 1024 * 1024;
    /// The default for [`max_total_bytes`](Self::max_total_bytes): 128 MiB —
    /// one full-size payload plus one full-size metadata or structure stream.
    pub const DEFAULT_MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;
    /// The default for [`max_objects`](Self::max_objects): 10 000.
    pub const DEFAULT_MAX_OBJECTS: usize = 10_000;

    /// The default limits.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stream_bytes: Self::DEFAULT_MAX_STREAM_BYTES,
            total_bytes: Self::DEFAULT_MAX_TOTAL_BYTES,
            objects: Self::DEFAULT_MAX_OBJECTS,
        }
    }

    /// The most bytes any one stream may inflate to.
    #[must_use]
    pub const fn max_stream_bytes(&self) -> usize {
        self.stream_bytes
    }

    /// The most bytes one call may inflate, summed over every stream it
    /// inflates.
    #[must_use]
    pub const fn max_total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// The most PDF objects one walk over the document's structure may visit.
    #[must_use]
    pub const fn max_objects(&self) -> usize {
        self.objects
    }

    /// These limits with [`max_stream_bytes`](Self::max_stream_bytes) set.
    #[must_use]
    pub const fn with_max_stream_bytes(mut self, bytes: usize) -> Self {
        self.stream_bytes = bytes;
        self
    }

    /// These limits with [`max_total_bytes`](Self::max_total_bytes) set.
    #[must_use]
    pub const fn with_max_total_bytes(mut self, bytes: usize) -> Self {
        self.total_bytes = bytes;
        self
    }

    /// These limits with [`max_objects`](Self::max_objects) set.
    #[must_use]
    pub const fn with_max_objects(mut self, objects: usize) -> Self {
        self.objects = objects;
        self
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::new()
    }
}

/// How deep a name tree may nest; bounds recursion.
const MAX_DEPTH: usize = 32;

/// What one call has inflated so far, against its [`Limits`]. Every decoded
/// stream goes through [`Budget::inflate`].
pub(super) struct Budget {
    limits: Limits,
    used: usize,
}

impl Budget {
    pub(super) fn new(limits: &Limits) -> Self {
        Self {
            limits: *limits,
            used: 0,
        }
    }

    /// `stream`'s decoded content, or the error naming the limit it exceeded.
    fn inflate(
        &mut self,
        stream: &lopdf::Stream,
        which: impl Fn() -> PdfStream,
    ) -> Result<Vec<u8>, Error> {
        self.decode(stream, which, true)
    }

    /// Count `stream` against the limits without keeping what it inflates to:
    /// a Flate stream is decoded through one reused 64 KiB buffer.
    fn measure(
        &mut self,
        stream: &lopdf::Stream,
        which: impl Fn() -> PdfStream,
    ) -> Result<(), Error> {
        self.decode(stream, which, false).map(drop)
    }

    /// The cap is whichever is smaller: the per-stream limit, or what is left of
    /// the call's total — and the error names the one that bound.
    fn decode(
        &mut self,
        stream: &lopdf::Stream,
        which: impl Fn() -> PdfStream,
        keep: bool,
    ) -> Result<Vec<u8>, Error> {
        let remaining = self.limits.total_bytes.saturating_sub(self.used);
        let (cap, limit) = if self.limits.stream_bytes <= remaining {
            let n = self.limits.stream_bytes;
            (n, Limit::StreamBytes(n))
        } else {
            (remaining, Limit::TotalBytes(self.limits.total_bytes))
        };
        let (bytes, len) = decode(stream, cap, keep).map_err(|e| match e {
            Decode::TooLarge => Error::LimitExceeded {
                stream: which(),
                limit,
            },
            Decode::Broken(reason) => Error::Undecodable {
                stream: which(),
                reason,
            },
        })?;
        self.used += len;
        Ok(bytes)
    }
}

/// Why a stream did not decode.
enum Decode {
    /// It would inflate past the cap.
    TooLarge,
    /// It declares a filter its data does not decode through.
    Broken(String),
}

/// A stream's content, decoded, never more than `cap` bytes of it.
///
/// * No `/Filter`: the stored bytes, held to the cap.
/// * A lone `/FlateDecode`: inflated here strictly (a corrupt block is an
///   error, not a truncated result); a broken zlib header is retried as raw
///   deflate.
/// * Anything else (LZW, ASCII85, a predictor, a chain): `lopdf`'s bounded
///   decoder.
///
/// A filter that does not decode is an error; stored bytes of an encoded
/// stream are never returned as its content.
fn decode(stream: &lopdf::Stream, cap: usize, keep: bool) -> Result<(Vec<u8>, usize), Decode> {
    if stream.dict.get(b"Filter").is_err() {
        let len = stream.content.len();
        return if len > cap {
            Err(Decode::TooLarge)
        } else if keep {
            Ok((stream.content.clone(), len))
        } else {
            Ok((Vec::new(), len))
        };
    }
    let filters = stream
        .filters()
        .map_err(|e| Decode::Broken(format!("unreadable /Filter: {e}")))?;
    if filters == [b"FlateDecode"] && stream.dict.get(b"DecodeParms").is_err() {
        return inflate(&stream.content, cap, keep);
    }
    match stream.decompressed_content_with_limit(cap) {
        Ok(bytes) if bytes.len() <= cap => {
            let len = bytes.len();
            Ok((bytes, len))
        }
        Ok(_)
        | Err(lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. })) => {
            Err(Decode::TooLarge)
        }
        Err(e) => Err(Decode::Broken(e.to_string())),
    }
}

/// zlib data — or, when its zlib wrapper is broken, the raw deflate data
/// behind it — inflated to at most `cap` bytes.
///
/// Output never grows past `cap + 1` bytes. The data must reach its
/// end-of-stream marker: a truncated stream is corrupt.
fn inflate(raw: &[u8], cap: usize, keep: bool) -> Result<(Vec<u8>, usize), Decode> {
    if raw.is_empty() {
        return Ok((Vec::new(), 0));
    }
    match inflate_with(raw, cap, true, keep) {
        Err(Decode::Broken(zlib)) => match raw.get(2..) {
            Some(deflate) if !deflate.is_empty() => inflate_with(deflate, cap, false, keep)
                .map_err(|e| match e {
                    Decode::TooLarge => Decode::TooLarge,
                    Decode::Broken(_) => Decode::Broken(zlib),
                }),
            _ => Err(Decode::Broken(zlib)),
        },
        other => other,
    }
}

fn inflate_with(
    input: &[u8],
    cap: usize,
    zlib: bool,
    keep: bool,
) -> Result<(Vec<u8>, usize), Decode> {
    const STEP: usize = 64 * 1024;
    let broken = |why: &str| Decode::Broken(format!("corrupt /FlateDecode data: {why}"));
    let mut inflater = flate2::Decompress::new(zlib);
    // Kept, the output grows toward `cap + 1`; only measured, one buffer of
    // `STEP` bytes is reused, and `counted` carries the length.
    let mut out: Vec<u8> = Vec::new();
    let mut counted = 0_usize;
    loop {
        if counted + out.len() > cap {
            return Err(Decode::TooLarge);
        }
        if !keep {
            counted += out.len();
            out.clear();
        }
        if out.len() == out.capacity() {
            let room = cap.saturating_add(1) - counted - out.len();
            out.reserve_exact(room.min(out.len().max(STEP)));
        }
        let consumed = usize::try_from(inflater.total_in()).unwrap_or(usize::MAX);
        let (in_before, out_before) = (inflater.total_in(), inflater.total_out());
        let status = inflater
            .decompress_vec(
                input.get(consumed..).unwrap_or_default(),
                &mut out,
                flate2::FlushDecompress::None,
            )
            .map_err(|e| broken(&e.to_string()))?;
        match status {
            flate2::Status::StreamEnd => {
                let len = counted + out.len();
                return if len > cap {
                    Err(Decode::TooLarge)
                } else {
                    Ok((out, len))
                };
            }
            flate2::Status::Ok | flate2::Status::BufError
                if inflater.total_in() == in_before && inflater.total_out() == out_before =>
            {
                return Err(broken("the stream ends before its end-of-stream marker"));
            }
            flate2::Status::Ok | flate2::Status::BufError => {}
        }
    }
}

/// Parse `pdf` with every load-time inflation bounded.
///
/// The parser silently drops an object stream over its own bound, so a first
/// pass inflates each object stream through `budget` and refuses one over a
/// limit before the real load.
pub(super) fn load(pdf: &[u8], budget: &mut Budget) -> Result<lopdf::Document, Error> {
    /// Keep object streams only, renamed so the parser leaves them compressed.
    ///
    /// The filter runs on each top-level object before the parser would inflate
    /// it, and what it does to the object sticks; its return value is only a
    /// keep-or-drop flag on this path.
    fn object_streams_only(
        id: lopdf::ObjectId,
        object: &mut lopdf::Object,
    ) -> Option<(lopdf::ObjectId, lopdf::Object)> {
        let stream = object.as_stream_mut().ok()?;
        if !stream.dict.has_type(b"ObjStm") {
            return None;
        }
        stream.dict.set("Type", "DeferredObjStm");
        Some((id, lopdf::Object::Null))
    }

    let per_stream = budget.limits.stream_bytes;
    let load = |filter| {
        let options = lopdf::LoadOptions {
            filter,
            max_decompressed_size: Some(per_stream),
            ..lopdf::LoadOptions::default()
        };
        lopdf::Document::load_mem_with_options(pdf, options).map_err(|e| match e {
            lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. }) => {
                Error::LimitExceeded {
                    stream: PdfStream::Structure,
                    limit: Limit::StreamBytes(per_stream),
                }
            }
            e => Error::Pdf(e.to_string()),
        })
    };

    xref_streams_are_bounded(pdf)?;
    // Always, not only when the bytes spell `ObjStm`: a name may be written
    // with `#hh` escapes (`/Obj#53tm`).
    let streams = load(Some(object_streams_only))?;
    for object in streams.objects.values() {
        if let Ok(stream) = object.as_stream() {
            budget.measure(stream, || PdfStream::Structure)?;
        }
    }
    load(None)
}

/// The most indirect objects a PDF may hold (ISO 32000-1, Annex C).
const MAX_PDF_OBJECTS: i64 = 8_388_607;

/// Refuse a cross-reference stream the parser would trust into an allocation
/// or a loop, before it is handed one.
///
/// The parser trusts `/W` widths for allocation and `/Index` / `/Size` for
/// iteration. So each dictionary with `/W` and `/Size` is checked first: every
/// width at most 8 bytes, not all zero, no section longer than
/// [`MAX_PDF_OBJECTS`].
///
/// A lexical scan through comments, strings and stream data; names are
/// compared decoded (`/#57` is `/W`).
fn xref_streams_are_bounded(pdf: &[u8]) -> Result<(), Error> {
    #[derive(Default)]
    struct Dict {
        /// `[` nesting inside this dictionary; a name is a key only at 0.
        arrays: usize,
        w: Option<Vec<i64>>,
        index: Option<Vec<i64>>,
        size: Option<i64>,
    }
    let refuse = |what: String| Err(Error::Pdf(format!("a cross-reference stream {what}")));
    let delimiter = |b: u8| b.is_ascii_whitespace() || b"()<>[]{}/%".contains(&b) || b == 0;
    let mut stack: Vec<Dict> = Vec::new();
    let mut i = 0;
    while i < pdf.len() {
        match pdf[i] {
            b'%' => {
                while i < pdf.len() && !matches!(pdf[i], b'\r' | b'\n') {
                    i += 1;
                }
            }
            b'(' => {
                let mut depth = 0usize;
                while i < pdf.len() {
                    match pdf[i] {
                        b'\\' => i += 1,
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                i += 1;
                continue;
            }
            b'<' if pdf.get(i + 1) == Some(&b'<') => {
                stack.push(Dict::default());
                i += 2;
                continue;
            }
            b'<' => {
                while i < pdf.len() && pdf[i] != b'>' {
                    i += 1;
                }
            }
            b'>' if pdf.get(i + 1) == Some(&b'>') => {
                if let Some(d) = stack.pop()
                    && let (Some(w), Some(size)) = (d.w, d.size)
                {
                    if w.iter().any(|&n| !(0..=8).contains(&n)) {
                        return refuse(format!(
                            "declares field widths /W {w:?}; each is at most 8 bytes"
                        ));
                    }
                    let index = d.index.unwrap_or_else(|| vec![0, size]);
                    let counts: Vec<i64> =
                        index.chunks(2).filter_map(|c| c.get(1).copied()).collect();
                    if counts.iter().any(|&n| !(0..=MAX_PDF_OBJECTS).contains(&n))
                        || !(0..=MAX_PDF_OBJECTS).contains(&size)
                    {
                        return refuse(format!(
                            "declares /Size {size} and sections {counts:?}; a PDF holds at most \
                             {MAX_PDF_OBJECTS} objects"
                        ));
                    }
                    if w.iter().all(|&n| n == 0) && counts.iter().any(|&n| n > 0) {
                        return refuse("declares entries of zero width".to_owned());
                    }
                }
                i += 2;
                // Stream data is opaque: skip to `endstream`.
                let mut j = i;
                while j < pdf.len() && pdf[j].is_ascii_whitespace() {
                    j += 1;
                }
                if pdf[j..].starts_with(b"stream") {
                    match pdf[j..].windows(9).position(|w| w == b"endstream") {
                        Some(end) => i = j + end + 9,
                        None => return Ok(()),
                    }
                }
                continue;
            }
            b'[' => {
                if let Some(d) = stack.last_mut() {
                    d.arrays += 1;
                }
            }
            b']' => {
                if let Some(d) = stack.last_mut() {
                    d.arrays = d.arrays.saturating_sub(1);
                }
            }
            b'/' => {
                let start = i + 1;
                i = start;
                while i < pdf.len() && !delimiter(pdf[i]) {
                    i += 1;
                }
                let name = decode_name(&pdf[start..i]);
                if let Some(d) = stack.last_mut()
                    && d.arrays == 0
                {
                    match name.as_slice() {
                        b"W" => d.w = integers(pdf, &mut i),
                        b"Index" => d.index = integers(pdf, &mut i),
                        b"Size" => d.size = integer(pdf, &mut i),
                        _ => {}
                    }
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

/// A PDF name's bytes with `#hh` escapes decoded.
fn decode_name(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut k = 0;
    while k < raw.len() {
        if raw[k] == b'#'
            && let Some(byte) = raw
                .get(k + 1..k + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            k += 3;
        } else {
            out.push(raw[k]);
            k += 1;
        }
    }
    out
}

/// An integer token at `*i` (after white space), advancing past it; `None`,
/// and `*i` unmoved, when there is none.
fn integer(pdf: &[u8], i: &mut usize) -> Option<i64> {
    let mut k = *i;
    while k < pdf.len() && pdf[k].is_ascii_whitespace() {
        k += 1;
    }
    let start = k;
    if k < pdf.len() && matches!(pdf[k], b'+' | b'-') {
        k += 1;
    }
    let digits = k;
    while k < pdf.len() && pdf[k].is_ascii_digit() {
        k += 1;
    }
    if k == digits {
        return None;
    }
    let text = std::str::from_utf8(&pdf[start..k]).ok()?;
    *i = k;
    // Saturating: a number too long for `i64` is refused as too large.
    Some(text.parse::<i64>().unwrap_or(if text.starts_with('-') {
        i64::MIN
    } else {
        i64::MAX
    }))
}

/// An array of integers at `*i`, advancing past it; `None` (and `*i` past the
/// `[` only) when the value is not one.
fn integers(pdf: &[u8], i: &mut usize) -> Option<Vec<i64>> {
    let mut k = *i;
    while k < pdf.len() && pdf[k].is_ascii_whitespace() {
        k += 1;
    }
    if pdf.get(k) != Some(&b'[') {
        return None;
    }
    k += 1;
    let mut out = Vec::new();
    loop {
        if let Some(n) = integer(pdf, &mut k) {
            out.push(n);
            continue;
        }
        while k < pdf.len() && pdf[k].is_ascii_whitespace() {
            k += 1;
        }
        if pdf.get(k) == Some(&b']') {
            *i = k + 1;
            return Some(out);
        }
        return None;
    }
}

/// Every embedded file in a PDF, by name, inflated.
///
/// Held to `limits` like [`extract`]: each attachment to the per-stream limit,
/// all of them to the per-call total.
///
/// # Errors
///
/// [`Error::Pdf`] if the bytes are not a readable PDF; [`Error::LimitExceeded`]
/// if an attachment, or all of them together, inflate past `limits`;
/// [`Error::Undecodable`] if one declares a filter its data does not decode
/// through.
pub fn embedded_files(pdf: &[u8], limits: &Limits) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let mut budget = Budget::new(limits);
    let doc = load(pdf, &mut budget)?;
    filespecs(&doc)
        .into_iter()
        .map(|(name, spec)| {
            let bytes =
                budget.inflate(spec.stream, || PdfStream::Attachment { name: name.clone() })?;
            Ok((name, bytes))
        })
        .collect()
}

/// A `/Filespec` that carries an embedded stream, found without inflating it.
pub(super) struct Filespec<'a> {
    stream: &'a lopdf::Stream,
}

/// Every file specification with an embedded stream, by name. Nothing is
/// inflated: the invoice is chosen by name first, and only it is decoded.
///
/// Embedded files live in the `/Names/EmbeddedFiles` tree or in page
/// `/FileAttachment` annotations, so this scans the object table for file
/// specifications. Of two with the same name, the later object wins.
pub(super) fn filespecs(doc: &lopdf::Document) -> BTreeMap<String, Filespec<'_>> {
    let mut out = BTreeMap::new();
    for object in doc.objects.values() {
        let Ok(dict) = object.as_dict() else { continue };
        let is_filespec = dict
            .get(b"Type")
            .ok()
            .and_then(|t| t.as_name().ok())
            .is_some_and(|n| n == b"Filespec");
        if !is_filespec {
            continue;
        }
        let Some(name) = filespec_name(doc, dict) else {
            continue;
        };
        let Some(stream) = filespec_stream(doc, dict) else {
            continue;
        };
        out.insert(name, Filespec { stream });
    }
    out
}

/// Where the PDF puts the invoice, structurally.
///
/// Three independent facts; any combination occurs.
#[derive(Debug, Default)]
struct Placement {
    /// Referenced from the document catalogue's `/AF` array.
    associated: bool,
    /// Reachable from the catalogue's `/Names/EmbeddedFiles` name tree.
    in_name_tree: bool,
    /// `/AFRelationship`, verbatim.
    relationship: Option<String>,
}

/// How the file specification for `filename` is wired into the document.
///
/// All three facts come from the first file specification of that name (not
/// necessarily the one extracted, so a duplicate shows as a defect). Nothing
/// is inflated; the name tree is the one already walked.
fn placement(doc: &lopdf::Document, tree: &[String], filename: &str) -> Placement {
    let Some((id, dict)) = doc.objects.iter().find_map(|(id, object)| {
        let dict = object.as_dict().ok()?;
        (dict.get(b"Type").ok()?.as_name().ok()? == b"Filespec"
            && filespec_name(doc, dict).is_some_and(|n| n.eq_ignore_ascii_case(filename)))
        .then_some((*id, dict))
    }) else {
        return Placement::default();
    };
    Placement {
        associated: catalog_associated_files(doc).contains(&id),
        in_name_tree: tree.iter().any(|n| n.eq_ignore_ascii_case(filename)),
        relationship: dict
            .get(b"AFRelationship")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|raw| String::from_utf8_lossy(raw).into_owned()),
    }
}

/// The object ids in the document catalogue's `/AF` array.
///
/// Empty when there is no `/AF`.
fn catalog_associated_files(doc: &lopdf::Document) -> Vec<lopdf::ObjectId> {
    let Ok(catalog) = doc.catalog() else {
        return Vec::new();
    };
    // `/AF` may be an inline array or a reference to one.
    let Ok(af) = catalog
        .get(b"AF")
        .and_then(|o| doc.dereference(o))
        .and_then(|(_, o)| o.as_array())
    else {
        return Vec::new();
    };
    af.iter()
        .filter_map(|o| match o {
            lopdf::Object::Reference(id) => Some(*id),
            _ => None,
        })
        .collect()
}

/// Every name in the catalogue's `/Names/EmbeddedFiles` name tree.
///
/// Follows `/Kids` as well as `/Names`. Each object is visited once; a cycle,
/// more than [`Limits::max_objects`] objects or more than [`MAX_DEPTH`] levels
/// is an error, never a truncated list.
fn embedded_file_names(doc: &lopdf::Document, limits: &Limits) -> Result<Vec<String>, Error> {
    Ok(embedded_file_entries(doc, limits)?
        .iter()
        .filter_map(|(key, _)| text_string(key))
        .collect())
}

/// Every entry of the catalogue's `/Names/EmbeddedFiles` name tree, as the
/// key (resolved) and the value (as written, usually a reference), in tree
/// order — the walk [`embedded_file_names`] reads, kept whole so that
/// [`super::embed`] can rewrite the tree without losing an entry.
pub(super) fn embedded_file_entries(
    doc: &lopdf::Document,
    limits: &Limits,
) -> Result<Vec<(lopdf::Object, lopdf::Object)>, Error> {
    struct Walk<'a> {
        doc: &'a lopdf::Document,
        max_objects: usize,
        visited: usize,
        on_path: HashSet<lopdf::ObjectId>,
        done: HashSet<lopdf::ObjectId>,
        out: Vec<(lopdf::Object, lopdf::Object)>,
    }

    impl Walk<'_> {
        fn count(&mut self) -> Result<(), TreeDefect> {
            self.visited += 1;
            if self.visited > self.max_objects {
                return Err(TreeDefect::Exceeded(Limit::Objects(self.max_objects)));
            }
            Ok(())
        }

        fn node(&mut self, node: &lopdf::Object, depth: usize) -> Result<(), TreeDefect> {
            if depth > MAX_DEPTH {
                return Err(TreeDefect::Exceeded(Limit::Depth(MAX_DEPTH)));
            }
            let doc = self.doc;
            let Ok((id, object)) = doc.dereference(node) else {
                return Err(TreeDefect::Malformed);
            };
            if let Some(id) = id {
                if self.on_path.contains(&id) {
                    return Err(TreeDefect::Cycle);
                }
                if !self.done.insert(id) {
                    return Ok(());
                }
            }
            self.count()?;
            let Ok(dict) = object.as_dict() else {
                return Err(TreeDefect::Malformed);
            };
            if let Some(id) = id {
                self.on_path.insert(id);
            }
            if let Ok(names) = dict
                .get(b"Names")
                .and_then(|o| doc.dereference(o))
                .and_then(|(_, o)| o.as_array())
            {
                // `[name value name value …]` — the names are the even positions.
                for pair in names.chunks(2) {
                    self.count()?;
                    if let [key, value] = pair
                        && let Ok((_, key)) = doc.dereference(key)
                    {
                        self.out.push((key.clone(), value.clone()));
                    }
                }
            }
            if let Ok(kids) = dict
                .get(b"Kids")
                .and_then(|o| doc.dereference(o))
                .and_then(|(_, o)| o.as_array())
            {
                for kid in kids {
                    self.node(kid, depth + 1)?;
                }
            }
            if let Some(id) = id {
                self.on_path.remove(&id);
            }
            Ok(())
        }
    }

    let mut walk = Walk {
        doc,
        max_objects: limits.objects,
        visited: 0,
        on_path: HashSet::new(),
        done: HashSet::new(),
        out: Vec::new(),
    };
    if let Ok(catalog) = doc.catalog()
        && let Ok(names) = catalog
            .get(b"Names")
            .and_then(|o| doc.dereference(o))
            .and_then(|(_, o)| o.as_dict())
        && let Ok(tree) = names.get(b"EmbeddedFiles")
    {
        walk.node(tree, 0).map_err(Error::EmbeddedFilesTree)?;
    }
    Ok(walk.out)
}

/// A `/Filespec`'s name, preferring `/UF` (Unicode) over `/F` (byte string).
///
/// `/Desc` is never consulted: it is a description, not a name. A `/UF` that
/// does not decode, or decodes to nothing, falls through to `/F`.
fn filespec_name(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> Option<String> {
    [&b"UF"[..], &b"F"[..]]
        .into_iter()
        .filter_map(|key| dict.get(key).and_then(|o| doc.dereference(o)).ok())
        .filter_map(|(_, o)| text_string(o))
        .find(|name| !name.is_empty())
}

/// A PDF text string (PDF 32000-1 § 7.9.2.2), or `None` if it does not decode.
///
/// UTF-16BE (`FE FF`), UTF-8 (`EF BB BF`, PDF 2.0) or `PDFDocEncoding`,
/// decoded strictly: an odd trailing UTF-16 byte is refused, the UTF-8 mark is
/// stripped, and a `PDFDocEncoding` result is accepted only when every byte
/// maps to a character (`lopdf::decode_text_string` pads, keeps the mark and
/// drops undefined bytes respectively).
pub(super) fn text_string(obj: &lopdf::Object) -> Option<String> {
    let raw = obj.as_str().ok()?;
    if let Some(be) = raw.strip_prefix(b"\xFE\xFF") {
        if be.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = be
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        return String::from_utf16(&units).ok();
    }
    if let Some(utf8) = raw.strip_prefix(b"\xEF\xBB\xBF") {
        return String::from_utf8(utf8.to_vec()).ok();
    }
    let s = lopdf::decode_text_string(obj).ok()?;
    (s.chars().count() == raw.len()).then_some(s)
}

/// A `/Filespec`'s embedded stream, not yet decoded.
fn filespec_stream<'a>(
    doc: &'a lopdf::Document,
    dict: &'a lopdf::Dictionary,
) -> Option<&'a lopdf::Stream> {
    let ef = doc
        .dereference(dict.get(b"EF").ok()?)
        .ok()?
        .1
        .as_dict()
        .ok()?;
    // `/F` is the usual key; `/UF` appears alongside it in files produced for
    // Unicode-aware readers and is the same stream.
    let stream_ref = ef.get(b"F").or_else(|_| ef.get(b"UF")).ok()?;
    match stream_ref {
        lopdf::Object::Reference(id) => doc.get_object(*id).ok()?.as_stream().ok(),
        other => other.as_stream().ok(),
    }
}

/// Extract the invoice from a ZUGFeRD / Factur-X PDF, within `limits`.
///
/// The invoice is chosen by name before anything is inflated; only it and the
/// XMP packet are then inflated, within [`Limits`].
///
/// # Errors
///
/// * [`Error::Pdf`] if the bytes are not a readable PDF;
/// * [`Error::EmbeddedFilesTree`] if the `/Names/EmbeddedFiles` tree is cyclic,
///   deeper than 32 levels, or holds more than [`Limits::max_objects`] objects;
/// * [`Error::NoInvoice`] if no embedded file has a recognised name (listing
///   the names it does carry);
/// * [`Error::LimitExceeded`] if the invoice, the XMP packet or the document
///   structure inflates past `limits`, naming the stream and the limit;
/// * [`Error::Undecodable`] if one of them declares a filter its data does not
///   decode through;
/// * [`Error::Encoding`] if the payload is not UTF-8 or UTF-16.
pub fn extract(pdf: &[u8], limits: &Limits) -> Result<Extracted, Error> {
    let mut budget = Budget::new(limits);
    let doc = load(pdf, &mut budget)?;
    // Walked first, so a refused tree is always an error, never `NoInvoice`.
    let tree = embedded_file_names(&doc, limits)?;
    let mut specs = filespecs(&doc);

    // `FILENAMES` preference order, not the PDF's order.
    let wanted = FILENAMES
        .iter()
        .find_map(|want| specs.keys().find(|k| k.eq_ignore_ascii_case(want)).cloned());

    // `remove_entry`, so no lookup remains that could fail.
    let Some((filename, spec)) = wanted.and_then(|f| specs.remove_entry(&f)) else {
        return Err(Error::NoInvoice {
            looked_for: FILENAMES,
            found: specs.into_keys().collect(),
        });
    };
    let bytes = budget.inflate(spec.stream, || PdfStream::Attachment {
        name: filename.clone(),
    })?;
    let specification_id = match crate::xml::decode(&bytes) {
        Ok(text) => specification_id(&text),
        Err(crate::Error::Encoding { found }) => return Err(Error::Encoding { found }),
        Err(_) => None,
    };
    let xml = bytes;
    let xmp = read_xmp(&doc, &mut budget)?;
    let profile = specification_id
        .as_deref()
        .map_or(Profile::Unknown, Profile::parse);

    let placement = placement(&doc, &tree, &filename);
    let relationship = placement.relationship.clone();

    Ok(Extracted {
        divergence: diverge(
            &xmp,
            &filename,
            profile,
            specification_id.as_deref(),
            &placement,
        ),
        xml,
        filename,
        profile,
        specification_id,
        relationship,
        xmp,
    })
}

/// Compare what the PDF declares against what it contains.
fn diverge(
    xmp: &Xmp,
    filename: &str,
    profile: Profile,
    specification_id: Option<&str>,
    placement: &Placement,
) -> Vec<Divergence> {
    let mut out = Vec::new();

    // ── how the file is wired in (independent of the XMP) ───────────────────
    if !placement.associated {
        out.push(Divergence::NotAssociated);
    }
    if !placement.in_name_tree {
        out.push(Divergence::NotInEmbeddedFiles);
    }
    match placement.relationship.as_deref() {
        None => out.push(Divergence::NoRelationship),
        // Only `Data` on a profile with lines; see `Divergence::Relationship`.
        Some(found)
            if found.eq_ignore_ascii_case("Data")
                && profile.is_en16931_invoice() == IsInvoice::Yes =>
        {
            out.push(Divergence::Relationship {
                found: found.to_owned(),
                profile,
            });
        }
        Some(_) => {}
    }

    // ── what the metadata says ───────────────────────────────────────────────
    if *xmp == Xmp::default() {
        // No packet implies no PDF/A part; reported once.
        out.push(Divergence::NoXmp);
        return out;
    }
    if xmp.pdfa_part.as_deref() != Some("3") {
        out.push(Divergence::NotPdfA3 {
            part: xmp.pdfa_part.clone(),
        });
    }
    if let Some(level) = &xmp.conformance_level {
        let claimed = Profile::parse(level);
        // Compare profiles, not strings: `EN 16931` (XMP) vs a URN (BT-24).
        if claimed != Profile::Unknown && claimed != profile {
            out.push(Divergence::Profile {
                xmp: level.clone(),
                payload: specification_id.unwrap_or("<absent>").to_owned(),
            });
        }
    }
    if let Some(declared) = &xmp.document_filename
        && !declared.eq_ignore_ascii_case(filename)
    {
        out.push(Divergence::Filename {
            xmp: declared.clone(),
            actual: filename.to_owned(),
        });
    }
    out
}

/// The document-level XMP packet, if there is one, read by namespace.
///
/// The packet is parsed as XML and a field is taken only from the namespaces
/// that define it:
///
/// * The invoice fields come from the ZUGFeRD / Factur-X family only —
///   Factur-X (`urn:factur-x:pdfa:…`), ZUGFeRD 2 (`urn:zugferd:pdfa:…`),
///   ZUGFeRD 1 (`urn:ferd:pdfa:CrossIndustryDocument:…`) and the 1.0 release
///   candidate (`urn:ferd:pdfa:invoice:rc#`). Per field, the newest wins.
/// * `pdfaid:part` and `pdfaid:conformance` come from the PDF/A
///   identification namespace only.
///
/// A packet that is not well-formed XML yields no fields
/// ([`Divergence::NoXmp`]). The packet is inflated through the call's budget;
/// over the limit is an error.
fn read_xmp(doc: &lopdf::Document, budget: &mut Budget) -> Result<Xmp, Error> {
    let Some(packet) = xmp_packet(doc, budget)? else {
        return Ok(Xmp::default());
    };
    Ok(parse_xmp(&packet))
}

/// The PDF/A identification namespace (ISO 19005).
const PDFAID_NS: &str = "http://www.aiim.org/pdfa/ns/id/";

/// How recent a ZUGFeRD / Factur-X XMP namespace is; `None` outside the family.
fn invoice_family(uri: &str) -> Option<u8> {
    let uri = uri.trim().to_ascii_lowercase();
    if uri.starts_with("urn:factur-x:pdfa:") {
        Some(3)
    } else if uri.starts_with("urn:zugferd:pdfa:") {
        Some(2)
    } else if uri.starts_with("urn:ferd:pdfa:crossindustrydocument:") {
        Some(1)
    } else if uri.starts_with("urn:ferd:pdfa:invoice:rc") {
        Some(0)
    } else {
        None
    }
}

/// [`Xmp`] from a packet's text.
fn parse_xmp(packet: &str) -> Xmp {
    let Ok(doc) = roxmltree::Document::parse(packet) else {
        return Xmp::default();
    };
    // (rank, value) per field: the newest family wins, the first occurrence
    // within one family.
    let mut invoice: [Option<(u8, String)>; 4] = Default::default();
    let mut pdfa: [Option<String>; 2] = Default::default();
    let mut take = |ns: Option<&str>, name: &str, value: &str| {
        let value = value.trim();
        let Some(ns) = ns.filter(|_| !value.is_empty()) else {
            return;
        };
        if ns == PDFAID_NS {
            let slot = match name {
                "part" => &mut pdfa[0],
                "conformance" => &mut pdfa[1],
                _ => return,
            };
            slot.get_or_insert_with(|| value.to_owned());
        } else if let Some(rank) = invoice_family(ns) {
            let slot = match name {
                "DocumentType" => &mut invoice[0],
                "DocumentFileName" => &mut invoice[1],
                "Version" => &mut invoice[2],
                "ConformanceLevel" => &mut invoice[3],
                _ => return,
            };
            if slot.as_ref().is_none_or(|(held, _)| rank > *held) {
                *slot = Some((rank, value.to_owned()));
            }
        }
    };
    // A simple property may be an element or an attribute; both occur.
    for node in doc.descendants().filter(roxmltree::Node::is_element) {
        if let Some(text) = node.text()
            && node.children().all(|c| !c.is_element())
        {
            take(node.tag_name().namespace(), node.tag_name().name(), text);
        }
        for attr in node.attributes() {
            take(attr.namespace(), attr.name(), attr.value());
        }
    }
    let [document_type, document_filename, version, conformance_level] =
        invoice.map(|slot| slot.map(|(_, value)| value));
    let [pdfa_part, pdfa_conformance] = pdfa;
    Xmp {
        document_type,
        document_filename,
        version,
        conformance_level,
        pdfa_part,
        pdfa_conformance,
    }
}

fn xmp_packet(doc: &lopdf::Document, budget: &mut Budget) -> Result<Option<String>, Error> {
    let Ok(catalog) = doc.catalog() else {
        return Ok(None);
    };
    let Ok(meta) = catalog.get(b"Metadata") else {
        return Ok(None);
    };
    let stream = match meta {
        lopdf::Object::Reference(id) => doc.get_object(*id).ok().and_then(|o| o.as_stream().ok()),
        other => other.as_stream().ok(),
    };
    let Some(stream) = stream else {
        return Ok(None);
    };
    let bytes = budget.inflate(stream, || PdfStream::Metadata)?;
    // Lossy: a mangled packet must not stop extraction.
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// BT-24 (`ram:GuidelineSpecifiedDocumentContextParameter/ram:ID`) from a CII
/// payload, by substring search; the payload itself is parsed by [`crate::read`].
fn specification_id(xml: &str) -> Option<String> {
    let anchor = xml.find("GuidelineSpecifiedDocumentContextParameter")?;
    let rest = &xml[anchor..];
    let open = rest.find(":ID>").map(|i| i + 4)?;
    let close = rest[open..].find("</")?;
    let value = rest[open..open + close].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn bt24_is_found_in_a_cii_fragment() {
        let xml = r"<rsm:ExchangedDocumentContext>
          <ram:GuidelineSpecifiedDocumentContextParameter>
            <ram:ID>urn:cen.eu:en16931:2017</ram:ID>
          </ram:GuidelineSpecifiedDocumentContextParameter>
        </rsm:ExchangedDocumentContext>";
        assert_eq!(
            specification_id(xml).as_deref(),
            Some("urn:cen.eu:en16931:2017")
        );
    }

    /// The *guideline* parameter, not the business-process one that precedes it.
    #[test]
    fn bt23_is_not_mistaken_for_bt24() {
        let xml = r"<ram:BusinessProcessSpecifiedDocumentContextParameter>
            <ram:ID>urn:process</ram:ID>
          </ram:BusinessProcessSpecifiedDocumentContextParameter>
          <ram:GuidelineSpecifiedDocumentContextParameter>
            <ram:ID>urn:factur-x.eu:1p0:basic</ram:ID>
          </ram:GuidelineSpecifiedDocumentContextParameter>";
        assert_eq!(
            specification_id(xml).as_deref(),
            Some("urn:factur-x.eu:1p0:basic")
        );
    }

    #[test]
    fn a_payload_without_bt24_reports_none() {
        assert_eq!(specification_id("<rsm:CrossIndustryInvoice/>"), None);
        assert_eq!(
            specification_id("<ram:GuidelineSpecifiedDocumentContextParameter/>"),
            None
        );
    }

    #[test]
    fn bytes_that_are_not_a_pdf_say_so() {
        assert!(matches!(
            extract(b"not a pdf", &Limits::default()),
            Err(Error::Pdf(_))
        ));
    }

    fn literal(raw: &[u8]) -> lopdf::Object {
        lopdf::Object::String(raw.to_vec(), lopdf::StringFormat::Literal)
    }

    fn utf16be(name: &str) -> Vec<u8> {
        [0xFE, 0xFF]
            .into_iter()
            .chain(name.encode_utf16().flat_map(u16::to_be_bytes))
            .collect()
    }

    #[test]
    fn a_utf16be_text_string_is_decoded() {
        let got = text_string(&literal(&utf16be("factur-x.xml")));
        assert_eq!(got.as_deref(), Some("factur-x.xml"));
    }

    /// `lopdf::decode_text_string` pads the stray byte and returns a name.
    #[test]
    fn an_odd_utf16be_length_does_not_decode() {
        let mut raw = utf16be("factur-x.xml");
        raw.push(b'A');
        assert_eq!(text_string(&literal(&raw)), None);
    }

    #[test]
    fn an_unpaired_surrogate_does_not_decode() {
        let mut raw = utf16be("factur-x.xml");
        raw.extend([0xD8, 0x00]);
        assert_eq!(text_string(&literal(&raw)), None);
    }

    /// `lopdf::decode_text_string` keeps the mark: `"\u{FEFF}factur-x.xml"`.
    #[test]
    fn a_utf8_text_string_loses_its_mark() {
        let got = text_string(&literal(b"\xEF\xBB\xBFfactur-x.xml"));
        assert_eq!(got.as_deref(), Some("factur-x.xml"));
    }

    #[test]
    fn pdfdocencoding_is_decoded() {
        assert_eq!(
            text_string(&literal(b"factur-x.xml")).as_deref(),
            Some("factur-x.xml")
        );
        // 0xE4 is `ä` in PDFDocEncoding; `from_utf8_lossy` made it U+FFFD.
        assert_eq!(
            text_string(&literal(b"rechnung-\xE4.xml")).as_deref(),
            Some("rechnung-\u{e4}.xml")
        );
    }

    /// 0x7F is undefined in `PDFDocEncoding`, and `lopdf` drops it: the name
    /// would come back as exactly `factur-x.xml`.
    #[test]
    fn an_undefined_pdfdocencoding_byte_does_not_decode() {
        assert_eq!(text_string(&literal(b"factur-x.xml\x7F")), None);
        assert_eq!(text_string(&literal(b"factur\x7F-x.xml")), None);
    }

    #[test]
    fn a_unicode_name_that_does_not_decode_falls_back_to_the_byte_name() {
        let mut uf = utf16be("factur-x.xml");
        uf.push(b'A');
        let dict = dictionary! { "UF" => literal(&uf), "F" => literal(b"factur-x.xml") };
        assert_eq!(
            filespec_name(&lopdf::Document::new(), &dict).as_deref(),
            Some("factur-x.xml")
        );

        let dict = dictionary! { "UF" => literal(b""), "F" => literal(b"factur-x.xml") };
        assert_eq!(
            filespec_name(&lopdf::Document::new(), &dict).as_deref(),
            Some("factur-x.xml")
        );
    }

    #[test]
    fn the_unicode_name_wins_when_it_decodes() {
        let dict = dictionary! {
            "UF" => literal(&utf16be("factur-x.xml")),
            "F" => literal(b"FACTUR~1.XML"),
        };
        assert_eq!(
            filespec_name(&lopdf::Document::new(), &dict).as_deref(),
            Some("factur-x.xml")
        );
    }
}
