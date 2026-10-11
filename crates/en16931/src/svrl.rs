//! SVRL output for a [`Report`]. Behind `features = ["svrl"]`.
//!
//! SVRL is a report format, not an invoice syntax: writing it needs no syntax
//! binding and no dependency, only escaping.
//!
//! ```xml
//! <svrl:schematron-output title="EN 16931 — XRechnung 3.0" schemaVersion="EN 16931-1:2017+A1:2019">
//!   <svrl:active-pattern name="XRechnung 3.0"/>
//!   <svrl:failed-assert id="BR-02" flag="fatal" location="BT-1" test="en16931:BR-02">
//!     <svrl:text>An Invoice shall have an Invoice number (BT-1).</svrl:text>
//!   </svrl:failed-assert>
//! </svrl:schematron-output>
//! ```
//!
//! Two attributes differ from Schematron output, as a comment in the output
//! also says:
//!
//! * `location` is a business-term path (`BG-25[2]/BT-151`), not an XPath.
//! * `test` is `en16931:<rule-id>`, since the rules are code, not XPath.
//!
//! A finding's [`crate::Finding::hint`] becomes an `svrl:diagnostic-reference`
//! ahead of `svrl:text`:
//!
//! ```xml
//! <svrl:failed-assert id="BR-CL-25" flag="fatal" location="BG-7/BT-49" test="en16931:BR-CL-25">
//!   <svrl:diagnostic-reference diagnostic="en16931-hint">9958 was DE:LID … use 0204 instead</svrl:diagnostic-reference>
//!   <svrl:text>Endpoint identifier scheme identifier MUST belong to the CEF EAS code list.</svrl:text>
//! </svrl:failed-assert>
//! ```
//!
//! `svrl:text` stays the authority's exact wording. See [`HINT_DIAGNOSTIC`].

use core::fmt::Write as _;

use crate::report::{Layer, Report};

/// The SVRL namespace, as every Schematron implementation writes it.
pub const SVRL_NS: &str = "http://purl.oclc.org/dsdl/svrl";

/// The `@diagnostic` value on the element carrying a [`Finding::hint`].
///
/// In ISO Schematron `@diagnostic` is an `IDREF` into the schema; there is no
/// schema here, so the value is namespaced to avoid colliding with real ids.
///
/// [`Finding::hint`]: crate::Finding::hint
pub const HINT_DIAGNOSTIC: &str = "en16931-hint";

/// Escape the five XML metacharacters and drop characters XML 1.0 forbids.
fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 forbids most control characters.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
}

/// Write `<!-- body -->`, safely.
///
/// XML forbids `--` inside a comment and a comment ending in `-`; the body may
/// be caller input (suppressed rule ids). Entities are not escaped, since
/// comment content is not parsed.
fn comment(body: &str, out: &mut String) {
    out.push_str("<!-- ");
    let mut last_was_dash = false;
    for c in body.chars() {
        match c {
            // Collapse a run of hyphens to one.
            '-' if last_was_dash => {}
            '-' => {
                last_was_dash = true;
                out.push('-');
            }
            // XML 1.0 cannot represent these at all, comment or not.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => {
                last_was_dash = false;
                out.push(c);
            }
        }
    }
    // A comment may not end with `-`; the space before `-->` also guarantees it.
    out.push_str(" -->");
}

/// Render an interchange [`Report`] as SVRL.
///
/// Every layer is rendered, then the findings, in order:
///
/// * a layer entry at `fatal` or `warning` is an `svrl:failed-assert` with an
///   `@id` like `en16931:schema:duplicate` and the entry's level as `@flag`;
/// * an entry at `information` is an `svrl:successful-report`;
/// * a skipped layer is a comment saying why.
///
/// An entry's term, value and cited rule are `svrl:property-reference`
/// elements, its hint a `svrl:diagnostic-reference`.
/// [`Report::required`] is not rendered; call this again for it.
///
/// ```
/// use en16931::{Invoice, profiles, report::Report};
///
/// let report = Report::of(&profiles::XRECHNUNG.validate(&Invoice::default()));
/// let xml = en16931::svrl::to_svrl(&report);
/// assert!(xml.contains(r#"<svrl:failed-assert id="BR-02" flag="fatal""#));
/// ```
#[must_use]
pub fn to_svrl(report: &Report) -> String {
    let mut out = String::with_capacity(256 + report.findings.len() * 160);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<svrl:schematron-output xmlns:svrl=\"");
    out.push_str(SVRL_NS);
    out.push_str("\" title=\"EN 16931");
    if let Some(p) = &report.profile {
        out.push_str(" — ");
        escape(p, &mut out);
    }
    out.push_str("\" schemaVersion=\"");
    escape(&report.edition, &mut out);
    out.push_str("\">\n");

    line_comment(
        "Produced by en16931. Findings are judged on the semantic model: their \
         `location` is a business-term path, not an XPath, and `test` names the \
         rule; these rules are code, not XPath. Layer entries carry the \
         document's own syntax path.",
        &mut out,
    );
    line_comment(&report.attribution, &mut out);

    // SVRL has no element for artefact provenance, so it is a comment.
    if !report.artefacts.is_empty() {
        let refs: Vec<String> = report
            .artefacts
            .iter()
            .map(|a| format!("{} {} {}", a.authority, a.repo, a.git_ref))
            .collect();
        line_comment(&format!("verified against: {}", refs.join("; ")), &mut out);
    }
    if let Some(source) = &report.source {
        line_comment(&format!("source: {source}"), &mut out);
    }
    if let Some(id) = &report.identity {
        let or = |v: &Option<String>| v.clone().unwrap_or_else(|| "(absent)".to_owned());
        line_comment(
            &format!(
                "document: {} / invoice {} of {}",
                or(&id.seller),
                or(&id.number),
                or(&id.issue_date)
            ),
            &mut out,
        );
    }
    if let Some(d) = &report.declaration {
        line_comment(&format!("declared: {d}"), &mut out);
    }
    if let Some(r) = &report.recommendation {
        line_comment(&format!("recommendation: {r}"), &mut out);
        for why in &report.reasons {
            line_comment(&format!("because: {why}"), &mut out);
        }
    }

    out.push_str("  <svrl:active-pattern name=\"");
    escape(report.profile.as_deref().unwrap_or("EN 16931"), &mut out);
    out.push_str("\"/>\n");

    for id in &report.suppressed {
        line_comment(&format!("suppressed, NOT checked: {id}"), &mut out);
    }
    for r in &report.relevelled {
        line_comment(
            &format!("relevelled to {}: {}", r.severity, r.rule),
            &mut out,
        );
    }
    if !report.left_out.is_empty() {
        line_comment(
            &format!(
                "totals only; {} core rule(s) NOT checked: {}",
                report.left_out.len(),
                report.left_out.join(", ")
            ),
            &mut out,
        );
    }

    for layer in &report.layers {
        layer_svrl(layer, &mut out);
    }

    for f in &report.findings {
        out.push_str("  <svrl:failed-assert id=\"");
        escape(&f.rule, &mut out);
        out.push_str("\" flag=\"");
        escape(&f.severity, &mut out);
        out.push_str("\" location=\"");
        escape(&f.location, &mut out);
        out.push_str("\" test=\"en16931:");
        escape(&f.rule, &mut out);
        out.push_str("\">\n");
        // Must precede `svrl:text` in SVRL's content model.
        if let Some(h) = &f.hint {
            reference("diagnostic", HINT_DIAGNOSTIC, h, &mut out);
        }
        out.push_str("    <svrl:text>");
        escape(&f.text, &mut out);
        if let (Some(expected), Some(actual)) = (&f.expected, &f.actual) {
            let _ = write!(out, " (expected ");
            escape(expected, &mut out);
            let _ = write!(out, ", found ");
            escape(actual, &mut out);
            out.push(')');
        }
        out.push_str("</svrl:text>\n  </svrl:failed-assert>\n");
    }
    out.push_str("</svrl:schematron-output>\n");
    out
}

/// `  <!-- body -->\n`.
fn line_comment(body: &str, out: &mut String) {
    out.push_str("  ");
    comment(body, out);
    out.push('\n');
}

/// `<svrl:{kind}-reference {kind}="{name}">{text}</…>`.
fn reference(kind: &str, name: &str, text: &str, out: &mut String) {
    let _ = write!(out, "    <svrl:{kind}-reference {kind}=\"{name}\">");
    escape(text, out);
    let _ = writeln!(out, "</svrl:{kind}-reference>");
}

/// One layer's entries.
fn layer_svrl(layer: &Layer, out: &mut String) {
    if layer.validity == "skipped" {
        line_comment(
            &format!(
                "{} layer skipped: {}",
                layer.name,
                layer.reason.as_deref().unwrap_or("not run")
            ),
            out,
        );
        return;
    }
    for e in &layer.entries {
        let element = if e.level == "information" {
            "successful-report"
        } else {
            "failed-assert"
        };
        let _ = write!(out, "  <svrl:{element} id=\"en16931:");
        escape(&layer.name, out);
        out.push(':');
        escape(&e.kind, out);
        out.push_str("\" flag=\"");
        escape(&e.level, out);
        out.push_str("\" location=\"");
        escape(&e.path, out);
        out.push_str("\" test=\"en16931:");
        escape(&e.kind, out);
        out.push_str("\">\n");
        if let Some(h) = &e.hint {
            reference("diagnostic", HINT_DIAGNOSTIC, h, out);
        }
        for (name, value) in [
            (TERM_PROPERTY, &e.term),
            (VALUE_PROPERTY, &e.value),
            (RULE_PROPERTY, &e.rule),
        ] {
            if let Some(v) = value {
                reference("property", name, v, out);
            }
        }
        out.push_str("    <svrl:text>");
        escape(&e.message, out);
        let _ = writeln!(out, "</svrl:text>\n  </svrl:{element}>");
    }
}

/// The `@property` of the element carrying a layer entry's business term.
pub const TERM_PROPERTY: &str = "en16931-term";
/// The `@property` of the element carrying a layer entry's value.
pub const VALUE_PROPERTY: &str = "en16931-value";
/// The `@property` of the element carrying the artefact rule an entry cites.
pub const RULE_PROPERTY: &str = "en16931-rule";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Invoice, profiles, validate};

    #[test]
    fn it_is_well_formed_and_carries_the_findings() {
        let report = profiles::XRECHNUNG.validate(&Invoice::default());
        let xml = to_svrl(&Report::of(&report));

        assert!(xml.starts_with("<?xml version=\"1.0\""));
        assert!(xml.contains("xmlns:svrl=\"http://purl.oclc.org/dsdl/svrl\""));
        assert!(xml.contains("schemaVersion=\"EN 16931-1:2017+A1:2019\""));
        assert!(xml.contains("<svrl:active-pattern name=\"XRechnung 3.0\"/>"));
        assert!(xml.contains("id=\"BR-02\""));
        assert!(xml.contains("flag=\"fatal\""));
        assert!(xml.contains("location=\"BT-1\""));
        assert!(xml.ends_with("</svrl:schematron-output>\n"));
        assert_eq!(
            xml.matches("<svrl:failed-assert").count(),
            report.findings().len()
        );
    }

    /// The licence notice is in the output.
    #[test]
    fn the_attribution_is_present() {
        let xml = to_svrl(&Report::of(&validate(&Invoice::default())));
        // Inside a comment, so hyphen runs are collapsed; check the wording.
        assert!(xml.contains("implementation of the EN 16931-1 semantic data model"));
        assert!(xml.contains("© CEN"));
    }

    /// Suppressions are recorded.
    #[test]
    fn suppressions_are_recorded() {
        let report = crate::validation::Check::new(&profiles::EN16931)
            .without("BR-CO-26")
            .run(&Invoice::default());
        let xml = to_svrl(&Report::of(&report));
        assert!(xml.contains("suppressed, NOT checked: BR-CO-26"));
    }

    /// The output parses, and each finding's attributes and text match.
    #[test]
    fn the_output_is_well_formed_xml() {
        let report = profiles::XRECHNUNG.validate(&Invoice::default());
        let xml = to_svrl(&Report::of(&report));
        let doc = roxmltree::Document::parse(&xml).expect("well-formed SVRL");

        let root = doc.root_element();
        assert_eq!(root.tag_name().name(), "schematron-output");
        assert_eq!(root.tag_name().namespace(), Some(SVRL_NS));
        assert!(
            root.attribute("title")
                .is_some_and(|t| t.contains("XRechnung"))
        );
        assert_eq!(
            root.attribute("schemaVersion"),
            Some("EN 16931-1:2017+A1:2019")
        );

        let asserts: Vec<_> = root
            .children()
            .filter(|n| n.has_tag_name((SVRL_NS, "failed-assert")))
            .collect();
        assert_eq!(asserts.len(), report.findings().len());

        for (node, finding) in asserts.iter().zip(report.findings()) {
            assert_eq!(node.attribute("id"), Some(finding.rule.as_str()));
            assert_eq!(
                node.attribute("location"),
                Some(finding.path.to_string().as_str())
            );
            assert_eq!(
                node.attribute("test"),
                Some(format!("en16931:{}", finding.rule).as_str())
            );
            assert!(matches!(
                node.attribute("flag"),
                Some("fatal" | "warning" | "information")
            ));
            let text = node
                .children()
                .find(|n| n.has_tag_name((SVRL_NS, "text")))
                .and_then(|n| n.text())
                .unwrap_or_default();
            assert!(text.starts_with(&finding.message), "{text:?}");
        }
    }

    /// Caller-chosen suppressed ids, written into comments, stay well-formed.
    #[test]
    fn hostile_suppressions_stay_well_formed() {
        for id in [
            "BR--CO-26",  // `--` is illegal anywhere inside a comment
            "BR-CO-26--", // …including immediately before the close
            "BR-CO-26-",  // a comment may not end with `-`
            "a<b&c>d",    // metacharacters, which a comment does not escape
            "BR\u{7}CO",  // a control character XML cannot represent
            "----------",
        ] {
            let report = crate::validation::Check::new(&profiles::EN16931)
                .without(id)
                .run(&Invoice::default());
            let xml = to_svrl(&Report::of(&report));
            roxmltree::Document::parse(&xml)
                .unwrap_or_else(|e| panic!("suppressing {id:?} produced invalid XML: {e}\n{xml}"));
        }
    }

    fn layered(text: &str) -> Report {
        use crate::report::{DocumentEntry, Layer};
        let entry = |kind: &str, level: &'static str| DocumentEntry {
            kind: kind.to_owned(),
            level: level.into(),
            path: format!("Invoice/{text}"),
            term: Some(format!("BT-1 {text}")),
            value: Some(text.to_owned()),
            rule: Some(format!("UBL-{text}")),
            message: format!("said {text}"),
            hint: Some(format!("do {text}")),
        };
        let mut r = Report::of(&profiles::XRECHNUNG.validate(&Invoice::default()));
        r.source = Some(format!("{text}.xml"));
        r.declaration = Some(text.to_owned());
        r.recommendation = Some("reject".into());
        r.reasons = vec![text.to_owned()];
        r.layers = vec![
            Layer {
                name: "container".into(),
                validity: "skipped".into(),
                reason: Some(text.to_owned()),
                entries: Vec::new(),
            },
            Layer {
                name: "syntax".into(),
                validity: "valid".into(),
                reason: None,
                entries: vec![entry("unmapped", "information")],
            },
            Layer {
                name: "schema".into(),
                validity: "invalid".into(),
                reason: None,
                entries: vec![entry("duplicate", "fatal"), entry("repeated", "warning")],
            },
        ];
        r
    }

    /// Every layer is rendered: fatal and warning entries as failed asserts in
    /// this crate's namespace, information as successful reports; the findings
    /// follow unchanged and in order.
    #[test]
    fn every_layer_is_rendered() {
        let report = layered("x");
        let xml = to_svrl(&report);
        let doc = roxmltree::Document::parse(&xml).expect("well-formed SVRL");
        let root = doc.root_element();
        let kids: Vec<_> = root
            .children()
            .filter(roxmltree::Node::is_element)
            .collect();
        let reports: Vec<_> = kids
            .iter()
            .filter(|n| n.has_tag_name((SVRL_NS, "successful-report")))
            .collect();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].attribute("id"), Some("en16931:syntax:unmapped"));
        assert_eq!(reports[0].attribute("flag"), Some("information"));
        let asserts: Vec<_> = kids
            .iter()
            .filter(|n| n.has_tag_name((SVRL_NS, "failed-assert")))
            .collect();
        assert_eq!(asserts.len(), 2 + report.findings.len());
        assert_eq!(asserts[0].attribute("id"), Some("en16931:schema:duplicate"));
        assert_eq!(asserts[0].attribute("flag"), Some("fatal"));
        assert_eq!(asserts[0].attribute("location"), Some("Invoice/x"));
        assert_eq!(asserts[1].attribute("flag"), Some("warning"));
        for (node, f) in asserts[2..].iter().zip(&report.findings) {
            assert_eq!(node.attribute("id"), Some(f.rule.as_str()));
        }
        let props: Vec<_> = asserts[0]
            .children()
            .filter(|n| n.has_tag_name((SVRL_NS, "property-reference")))
            .map(|n| {
                (
                    n.attribute("property").unwrap_or(""),
                    n.text().unwrap_or(""),
                )
            })
            .collect();
        assert_eq!(
            props,
            [
                (TERM_PROPERTY, "BT-1 x"),
                (VALUE_PROPERTY, "x"),
                (RULE_PROPERTY, "UBL-x")
            ]
        );
        assert!(xml.contains("container layer skipped: x"));
        assert!(xml.contains("recommendation: reject"));
    }

    /// Adversarial strings in every layer field stay well-formed.
    #[test]
    fn hostile_layer_content_stays_well_formed() {
        for text in [
            "a<b&c>d\"e'f",
            "--",
            "x-",
            "]]>",
            "\u{7}\u{0}",
            "<!-- -->",
            "\u{202e}rtl",
        ] {
            let xml = to_svrl(&layered(text));
            roxmltree::Document::parse(&xml)
                .unwrap_or_else(|e| panic!("{text:?} produced invalid XML: {e}\n{xml}"));
        }
    }

    /// Comment content is not entity-escaped.
    #[test]
    fn comments_are_not_entity_escaped() {
        let mut s = String::new();
        comment("a & b < c", &mut s);
        assert_eq!(s, "<!-- a & b < c -->");
    }

    #[test]
    fn text_is_escaped() {
        let mut s = String::new();
        escape("a & b < c > d \" e ' f", &mut s);
        assert_eq!(s, "a &amp; b &lt; c &gt; d &quot; e &apos; f");
        let mut s = String::new();
        escape("a\u{7}b", &mut s);
        assert_eq!(s, "ab");
    }

    /// Escaping must hold for every rule text the crate ships, not a sample.
    #[test]
    fn every_rule_text_survives_escaping() {
        for r in crate::validation::rules::all() {
            let mut s = String::new();
            escape(r.text, &mut s);
            assert!(!s.contains('<'), "{}", r.id);
            assert!(!s.contains('>'), "{}", r.id);
            // `&` may only appear as the start of an entity.
            for (i, _) in s.match_indices('&') {
                assert!(
                    s[i..].starts_with("&amp;")
                        || s[i..].starts_with("&lt;")
                        || s[i..].starts_with("&gt;")
                        || s[i..].starts_with("&quot;")
                        || s[i..].starts_with("&apos;"),
                    "{} has a bare ampersand",
                    r.id
                );
            }
        }
    }
}
