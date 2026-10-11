//! ZUGFeRD and Factur-X: a PDF/A-3 file carrying a CII invoice.
//!
//! The CII payload belongs to [`crate::cii`]; this module owns the PDF half in
//! both directions — [`extract`] reads it, [`embed`] writes it. It adds no
//! business rules. Most callers need neither: [`crate::read`] and
//! [`crate::receive`] recognise a PDF by its signature and read the payload.
//!
//! # Reading
//!
//! ```no_run
//! use en16931_formats::zugferd::{self, IsInvoice};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let pdf = std::fs::read("invoice.pdf")?;
//! let got = zugferd::extract(&pdf, &zugferd::Limits::default())?;
//!
//! // MINIMUM and BASIC WL carry no lines and are not EN 16931 invoices.
//! println!("{:?} in {}", got.profile, got.filename);
//! if got.profile.is_en16931_invoice() == IsInvoice::Yes {
//!     let read = en16931_formats::read(&pdf)?;
//!     println!("{}", en16931::validate(&read.invoice));
//! }
//! // `got.xml` is the payload verbatim.
//! # Ok(()) }
//! ```
//!
//! Every call runs under [`Limits`] (default 64 MiB per stream, 128 MiB per
//! call, 10 000 objects per walk), and the invoice is chosen by name before
//! anything is inflated. Exceeding a limit is [`Error::LimitExceeded`] or
//! [`Error::EmbeddedFilesTree`]; a stream that does not decode is
//! [`Error::Undecodable`].
//!
//! Extraction also reports how the payload is wired in, as [`Divergence`]
//! values on [`Extracted::divergence`] — not associated through `/AF`, missing
//! from `/Names/EmbeddedFiles`, no `/AFRelationship`, not declared PDF/A-3, or
//! XMP that disagrees with the payload. None stops extraction.
//!
//! # Writing
//!
//! ```no_run
//! use en16931::profiles::XRechnung;
//! use en16931::validation::profile::Validated;
//! use en16931_formats::zugferd::{self, EmbedOptions};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let invoice = en16931::Invoice::default();
//! let proof = Validated::<XRechnung>::new(invoice)?;
//! let rendered = std::fs::read("rendered-pdfa3.pdf")?;
//! let hybrid = zugferd::embed(&rendered, &proof, &EmbedOptions::default())?;
//! std::fs::write("invoice.pdf", hybrid)?;
//! # Ok(()) }
//! ```
//!
//! The rendering is yours and must already be PDF/A-3: [`embed`] checks the
//! XMP declaration and refuses anything else ([`EmbedError`]); it never
//! converts to PDF/A. [`embed`] adds the CII payload of a validated invoice,
//! the filename and `fx:ConformanceLevel` for the profile ([`Embeddable`]:
//! EN 16931 and XRechnung), `/AF`, `/AFRelationship`, the embedded-files name
//! tree and the Factur-X XMP — merged into an existing
//! `pdfaExtension:schemas` bag, since XMP allows that property once per
//! packet. The result is an incremental update, so your bytes stay an
//! unchanged prefix; [`embed`] reads it back with [`extract`] and refuses
//! output with any [`Divergence`]. The test suite checks its output with
//! veraPDF (PDF/A-3b).
//!
//! `/AFRelationship` is `Alternative` by default; `Source` is offered through
//! [`Relationship`]. `Data` is reported as a [`Divergence::Relationship`] on a
//! profile that carries lines.
//!
//! Profile names, attachment filenames and the XMP structure are checked
//! against the Factur-X reference implementation
//! ([`facturx.py`](https://raw.githubusercontent.com/akretion/factur-x/master/src/facturx/facturx.py)),
//! not against the ZUGFeRD / Factur-X specifications themselves.

mod embed;
mod extract;
mod profile;

pub use embed::{EmbedError, EmbedOptions, Embeddable, Existing, Metadata, Relationship, embed};
pub use extract::{Divergence, Extracted, FILENAMES, Limits, Xmp, embedded_files, extract};
pub use profile::{IsInvoice, Profile};

/// Anything that stopped a hybrid PDF being read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The bytes are not a PDF this crate can parse; the parser's message.
    #[error("not a readable PDF: {0}")]
    Pdf(String),
    /// A readable PDF with no embedded file that looks like an invoice.
    #[error("no embedded invoice found (looked for {looked_for:?}, found {found:?})")]
    NoInvoice {
        /// The filenames that would have been accepted.
        looked_for: &'static [&'static str],
        /// The embedded filenames actually present, for diagnosis.
        found: Vec<String>,
    },
    /// The embedded file is in an encoding an XML reader must refuse — named.
    #[error("the embedded invoice is encoded as {found}; it must be UTF-8 or UTF-16")]
    Encoding {
        /// What was found: a declared encoding, or what the bytes are not.
        found: String,
    },
    /// A stream would inflate past one of the [`Limits`] the call runs under.
    ///
    /// Raised as soon as the limit is reached, before the stream is inflated
    /// in full.
    #[error("{stream} inflates past {limit}")]
    LimitExceeded {
        /// Which stream.
        stream: PdfStream,
        /// Which limit, with its value.
        limit: Limit,
    },
    /// A stream declares a filter its data does not decode through.
    ///
    /// Corrupt compressed data, or a filter the reader does not implement.
    #[error("{stream} does not decode: {reason}")]
    Undecodable {
        /// Which stream.
        stream: PdfStream,
        /// What the decoder reported.
        reason: String,
    },
    /// The catalogue's `/Names/EmbeddedFiles` name tree was refused.
    ///
    /// A cycle, more nodes or levels than a walk may visit, or a node that is
    /// missing or not a dictionary.
    #[error("the /Names/EmbeddedFiles name tree {0}")]
    EmbeddedFilesTree(TreeDefect),
}

/// Which stream of a PDF an [`Error::LimitExceeded`] or [`Error::Undecodable`]
/// is about.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PdfStream {
    /// An embedded file, by the name the PDF records.
    Attachment {
        /// The embedded file's name.
        name: String,
    },
    /// The document catalogue's XMP metadata packet.
    Metadata,
    /// An object or cross-reference stream the PDF parser decodes while
    /// loading — the document's structure rather than its content.
    Structure,
}

impl std::fmt::Display for PdfStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Attachment { name } => write!(f, "the embedded file {name:?}"),
            Self::Metadata => f.write_str("the XMP metadata stream"),
            Self::Structure => {
                f.write_str("the document structure (an object or cross-reference stream)")
            }
        }
    }
}

/// One of the bounds a read runs under, with its value. See [`Limits`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Limit {
    /// [`Limits::max_stream_bytes`]: the most bytes one stream may inflate to.
    StreamBytes(usize),
    /// [`Limits::max_total_bytes`]: the most bytes one call may inflate in sum.
    TotalBytes(usize),
    /// [`Limits::max_objects`]: the most objects one walk may visit.
    Objects(usize),
    /// The most levels a name tree may nest. Fixed at 32.
    Depth(usize),
}

impl std::fmt::Display for Limit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StreamBytes(n) => write!(f, "the per-stream limit of {n} bytes"),
            Self::TotalBytes(n) => write!(f, "the per-call limit of {n} inflated bytes"),
            Self::Objects(n) => write!(f, "the limit of {n} objects per walk"),
            Self::Depth(n) => write!(f, "the depth limit of {n} levels"),
        }
    }
}

/// Why a name tree was refused. See [`Error::EmbeddedFilesTree`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TreeDefect {
    /// A `/Kids` entry leads back to a node on its own path.
    Cycle,
    /// The tree holds more objects, or nests deeper, than a walk may go.
    Exceeded(Limit),
    /// A node does not resolve to an object, or is not a dictionary.
    Malformed,
}

impl std::fmt::Display for TreeDefect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cycle => f.write_str("refers back to itself through /Kids"),
            Self::Exceeded(limit) => write!(f, "exceeds {limit}"),
            Self::Malformed => f.write_str("has a node that is missing or not a dictionary"),
        }
    }
}
