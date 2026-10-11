//! A stable interchange shape for a [`ValidationReport`].
//!
//! [`Report`] is versioned by [`SCHEMA`], independently of the internal
//! `Serialize` layout of `ValidationReport`. Its [`Entry`] mirrors SVRL's
//! `failed-assert` (`id`, `flag`, `location`, `text`), except that `location` is
//! a business-term path such as `BG-25[2]/BT-151` rather than an XPath.
//! [`Report::attribution`] carries the CEN notice, which must stay visible to users.

use std::borrow::Cow;

use crate::validation::{Finding, Severity, Source, ValidationReport};

/// The interchange schema this module emits.
///
/// Any change to [`Report`]'s fields, including an addition, bumps it.
/// Consumers should reject a version they do not know.
pub const SCHEMA: &str = "en16931-report/4";

/// A validation report in a shape safe to store, diff and send elsewhere.
///
/// See the [module documentation](self).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Always [`SCHEMA`]. Reject what you do not recognise.
    pub schema: Cow<'static, str>,
    /// Whether anything fatal fired.
    pub valid: bool,
    /// The profile it was checked against, or `null` for the bare core rules.
    pub profile: Option<String>,
    /// Which edition of EN 16931-1 those rules belong to.
    pub edition: Cow<'static, str>,
    /// How many rules ran — SVRL's `fired-rule` count, in effect.
    pub rules_checked: usize,
    /// The CEN attribution notice, required by the licence.
    pub attribution: Cow<'static, str>,
    /// The authority releases the rules that ran were verified against.
    ///
    /// One entry per authority release (for XRechnung: CEN, KoSIT's Schematron,
    /// KoSIT's validator configuration and OpenPeppol).
    pub artefacts: Vec<Artefact>,
    /// Rules the caller asked to skip, which were not checked. Always present;
    /// non-empty means `valid` is a weaker claim.
    pub suppressed: Vec<String>,
    /// Rules reported at another severity than the authority publishes.
    pub relevelled: Vec<Relevelled>,
    /// Core rules the profile does not run — non-empty only for a totals-only
    /// profile (ZUGFeRD MINIMUM, BASIC WL).
    pub left_out: Vec<String>,
    /// Where the document came from — a file name — when the caller knows.
    pub source: Option<String>,
    /// Which document: seller, number, issue date. `None` for a report on a
    /// model that no document carried.
    pub identity: Option<Identity>,
    /// What the document's BT-24 declares, as classified — `XRechnung 3.0`, or
    /// `unknown specification identifier "…"`. `None` when no document was read.
    pub declaration: Option<String>,
    /// For a received document: `accept`, `reject` or `undefined`.
    pub recommendation: Option<Cow<'static, str>>,
    /// Why, one line per cause. Empty for `accept`.
    pub reasons: Vec<String>,
    /// The layers a document passed through — container, syntax, schema,
    /// semantic — in that order. Empty for a report on a bare model, where
    /// there was no document.
    pub layers: Vec<Layer>,
    /// The same document under the profile the receiver requires, when that is
    /// not the declared one.
    pub required: Option<Box<Report>>,
    /// One entry per finding, most severe first.
    pub findings: Vec<Entry>,
}

/// A rule reported at another severity than its authority publishes.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relevelled {
    /// The rule id.
    pub rule: String,
    /// The severity it was reported at: `fatal`, `warning` or `information`.
    pub severity: Cow<'static, str>,
}

/// Which document a report is about.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Identity {
    /// BT-27, the seller's name.
    pub seller: Option<String>,
    /// BT-1, the invoice number.
    pub number: Option<String>,
    /// BT-2, the issue date, `YYYY-MM-DD`.
    pub issue_date: Option<String>,
}

/// One layer a document passed through.
///
/// Syntax-neutral: entries carry the binding's own syntax path and `kind`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    /// `container`, `syntax`, `schema` or `semantic`.
    pub name: Cow<'static, str>,
    /// `valid`, `invalid` or `skipped`.
    pub validity: Cow<'static, str>,
    /// Why the layer was skipped. `None` unless `validity` is `skipped`.
    pub reason: Option<String>,
    /// What the layer has to say about the document, in document order. The
    /// semantic layer's verdicts are [`Report::findings`], not entries.
    pub entries: Vec<DocumentEntry>,
}

/// One thing a reader, the container or a writer had to say about a document.
///
/// Not a finding: it carries a syntax path, may cite an artefact rule, and is
/// never counted as a failed rule.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentEntry {
    /// What happened — `unmapped`, `malformed`, `duplicate`, `repeated`,
    /// `schema`, `unreadable`, `container`, `ambiguous`, `dropped`,
    /// `prohibited`, `derived`.
    pub kind: String,
    /// `fatal`, `warning` or `information`. A fatal entry makes its layer
    /// invalid.
    pub level: Cow<'static, str>,
    /// Where, as a syntax path.
    pub path: String,
    /// The business term, when the binding knows it.
    pub term: Option<String>,
    /// The value as the document carries it.
    pub value: Option<String>,
    /// The artefact rule the entry cites — never a registry rule id.
    pub rule: Option<String>,
    /// The binding's statement of what happened.
    pub message: String,
    /// What to do about it, where known.
    pub hint: Option<String>,
}

/// One authority release a report's rules were verified against.
///
/// Owned, so a stored report can be deserialised.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artefact {
    /// The authority, as it calls itself — `CEN`, `KoSIT`, `OpenPeppol`.
    pub authority: String,
    /// The repository, without the host, so the ref can be fetched.
    pub repo: String,
    /// The release tag, verbatim.
    pub git_ref: String,
}

/// One finding, shaped like SVRL's `failed-assert`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The rule id, as the authority publishes it — SVRL's `@id`.
    pub rule: String,
    /// SVRL's `@flag`: `fatal`, `warning` or `information`.
    pub severity: Cow<'static, str>,
    /// Where the rule came from: CEN, a profile, or this crate. Not in SVRL.
    pub source: Cow<'static, str>,
    /// A **business-term** path, `BG-25[2]/BT-151`, where SVRL puts an XPath.
    pub location: String,
    /// The rule's own wording — SVRL's `svrl:text`.
    pub text: String,
    /// For arithmetic rules: what the rule computed.
    pub expected: Option<String>,
    /// For arithmetic rules: what the document states.
    pub actual: Option<String>,
    /// What to do about it. Not in SVRL; kept out of `text`, which stays the
    /// authority's wording.
    pub hint: Option<String>,
}

/// The interchange word for a severity: `fatal`, `warning`, `information`.
#[must_use]
pub const fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Fatal => "fatal",
        Severity::Warning => "warning",
        Severity::Info => "information",
    }
}

const fn source_name(s: Source) -> &'static str {
    match s {
        Source::Both => "standard+artefact",
        Source::StandardOnly => "standard",
        Source::ArtefactOnly => "artefact",
        Source::Crate => "en16931",
    }
}

impl Report {
    /// Convert a [`ValidationReport`] into the interchange shape.
    #[must_use]
    pub fn of(report: &ValidationReport) -> Self {
        Self {
            schema: Cow::Borrowed(SCHEMA),
            valid: report.is_valid(),
            profile: report.profile().map(str::to_owned),
            edition: Cow::Borrowed(report.edition().designation()),
            rules_checked: report.rules_checked(),
            attribution: Cow::Borrowed(crate::ATTRIBUTION),
            artefacts: report
                .artefacts()
                .iter()
                .map(|a| Artefact {
                    authority: a.authority.to_owned(),
                    repo: a.repo.to_owned(),
                    git_ref: a.git_ref.to_owned(),
                })
                .collect(),
            suppressed: report.suppressed().to_vec(),
            relevelled: report
                .relevelled()
                .iter()
                .map(|(rule, level)| Relevelled {
                    rule: rule.clone(),
                    severity: Cow::Borrowed(severity_name(*level)),
                })
                .collect(),
            left_out: Vec::new(),
            source: None,
            identity: None,
            declaration: None,
            recommendation: None,
            reasons: Vec::new(),
            layers: Vec::new(),
            required: None,
            findings: report.findings().iter().map(Entry::of).collect(),
        }
    }
}

impl Report {
    /// A report on which no rule set ran — a document that could not be read.
    ///
    /// Not valid, no profile, nothing checked; a caller fills
    /// [`layers`](Self::layers) with what refused the document.
    #[must_use]
    pub fn unchecked() -> Self {
        Self {
            schema: Cow::Borrowed(SCHEMA),
            valid: false,
            profile: None,
            edition: Cow::Borrowed(crate::Edition::En2017A1.designation()),
            rules_checked: 0,
            attribution: Cow::Borrowed(crate::ATTRIBUTION),
            artefacts: Vec::new(),
            suppressed: Vec::new(),
            relevelled: Vec::new(),
            left_out: Vec::new(),
            source: None,
            identity: None,
            declaration: None,
            recommendation: None,
            reasons: Vec::new(),
            layers: Vec::new(),
            required: None,
            findings: Vec::new(),
        }
    }
}

impl Entry {
    /// Convert one finding.
    #[must_use]
    pub fn of(f: &Finding) -> Self {
        // An id that resolves to no rule is a profile restriction.
        let source =
            crate::validation::rules::explain(&f.rule).map_or("profile", |r| source_name(r.source));
        Self {
            rule: f.rule.clone(),
            severity: Cow::Borrowed(severity_name(f.severity)),
            source: Cow::Borrowed(source),
            location: f.path.to_string(),
            text: f.message.clone(),
            expected: f.detail.as_ref().map(|d| d.expected.clone()),
            actual: f.detail.as_ref().map(|d| d.actual.clone()),
            hint: f.hint.clone(),
        }
    }
}

impl From<&ValidationReport> for Report {
    fn from(r: &ValidationReport) -> Self {
        Self::of(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Invoice, profiles, validate};

    #[test]
    fn the_shape_carries_what_svrl_carries() {
        let report = profiles::XRECHNUNG.validate(&Invoice::default());
        let out = Report::of(&report);

        assert_eq!(out.schema, SCHEMA);
        assert!(!out.valid);
        assert_eq!(out.profile.as_deref(), Some("XRechnung 3.0"));
        assert_eq!(out.edition, "EN 16931-1:2017+A1:2019");
        assert!(out.rules_checked > 0);
        assert_eq!(out.attribution, crate::ATTRIBUTION);

        let e = out.findings.first().expect("findings");
        assert!(!e.rule.is_empty());
        assert!(matches!(&*e.severity, "fatal" | "warning" | "information"));
        assert!(!e.location.is_empty(), "SVRL's `location`, semantically");
        assert!(!e.text.is_empty(), "SVRL's `svrl:text`");
    }

    /// The core path reports no profile rather than guessing one.
    #[test]
    fn a_core_report_names_no_profile() {
        let out = Report::of(&validate(&Invoice::default()));
        assert_eq!(out.profile, None);
        assert_eq!(out.edition, "EN 16931-1:2017+A1:2019");
    }

    /// Suppressions appear in the interchange shape.
    #[test]
    fn suppressions_travel_with_the_report() {
        let report = crate::validation::Check::new(&profiles::EN16931)
            .without("BR-CO-26")
            .run(&Invoice::default());
        let out = Report::of(&report);
        assert_eq!(out.suppressed, ["BR-CO-26"]);

        let clean = Report::of(&validate(&Invoice::default()));
        assert!(clean.suppressed.is_empty());
    }

    /// The shape round-trips through JSON.
    #[cfg(feature = "serde")]
    #[test]
    fn a_report_survives_a_round_trip_through_json() {
        let report = profiles::XRECHNUNG.validate(&Invoice::default());
        let out = Report::of(&report);

        let json = serde_json::to_string(&out).expect("serialise");
        let back: Report = serde_json::from_str(&json).expect("deserialise");

        assert_eq!(back, out);
        assert_eq!(back.schema, SCHEMA);
        assert_eq!(back.findings.len(), report.findings().len());
    }

    /// A consumer must be able to reject a schema version it does not know.
    #[cfg(feature = "serde")]
    #[test]
    fn an_unknown_schema_version_is_visible_to_a_reader() {
        let json = serde_json::to_string(&Report::of(&validate(&Invoice::default())))
            .expect("serialise")
            .replace(SCHEMA, "en16931-report/99");
        let back: Report = serde_json::from_str(&json).expect("deserialise");
        assert_ne!(back.schema, SCHEMA, "a reader can compare and refuse");
    }

    /// Provenance survives, so a reader can tell CEN's rules from ours.
    #[test]
    fn provenance_is_carried_per_finding() {
        let out = Report::of(&validate(&Invoice::default()));
        let br = out
            .findings
            .iter()
            .find(|e| e.rule.starts_with("BR-"))
            .expect("a CEN rule fired");
        assert!(
            matches!(&*br.source, "standard+artefact" | "standard" | "artefact"),
            "{}",
            br.source
        );
    }
}
