//! Extracting the syntax rules' `not(x)` prohibitions with their contexts.
//!
//! `CII-DT-076` (`not(ram:ID)`) forbids `ram:ID` only under the element its rule
//! context selects, so each entry is `(rule, context, path relative to that
//! context)`. The source is the preprocessed Schematron, where every context is a
//! resolved XPath rather than a `$Variable`.
//!
//! A conditional test or context (a predicate, a function such as
//! `ends-with(name(), 'Amount')`, a wildcard) needs an XPath engine; such rules
//! are not extracted but counted in `UNEXTRACTED`.

use std::path::{Path, PathBuf};

use crate::order::Syntax;
use crate::{Generated, Set, escape, header};

/// Where each syntax's preprocessed artefact lives, relative to the reference corpus.
fn artefact(syntax: Syntax) -> &'static str {
    match syntax {
        Syntax::Ubl => {
            "eInvoicing-EN16931/ubl/schematron/preprocessed/EN16931-UBL-validation-preprocessed.sch"
        }
        Syntax::Cii => {
            "eInvoicing-EN16931/cii/schematron/preprocessed/EN16931-CII-validation-preprocessed.sch"
        }
    }
}

/// Is this a syntax-rule id (`UBL-CR-123`, `CII-DT-07`, `UBL-SR-4`)? Business
/// rules (`BR-*`) in the same file are skipped.
fn is_syntax_rule(id: &str) -> bool {
    let mut parts = id.split('-');
    let (Some(prefix), Some(kind), Some(num), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !prefix.is_empty()
        && prefix.chars().all(|c| c.is_ascii_uppercase())
        && matches!(kind, "CR" | "SR" | "DT")
        && !num.is_empty()
        && num.chars().all(|c| c.is_ascii_digit())
}

/// A chain of qualified element names, e.g. `cac:Party/cbc:Name` (no
/// predicates, functions or wildcards).
fn is_element_chain(s: &str) -> bool {
    !s.is_empty()
        && s.split('/').all(|seg| {
            let mut halves = seg.split(':');
            let (Some(prefix), Some(local), None) = (halves.next(), halves.next(), halves.next())
            else {
                return false;
            };
            !prefix.is_empty()
                && prefix.chars().next().is_some_and(char::is_alphabetic)
                && prefix
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
                && !local.is_empty()
                && local
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
        })
}

/// `not( … )`, with the contents trimmed — or `None` if it is not that shape.
fn strip_not(test: &str) -> Option<&str> {
    test.strip_prefix("not(")?.strip_suffix(')').map(str::trim)
}

/// Expand a forbidden path into one plain chain per branch, or `None`.
///
/// Handles a plain chain, `(a|b)/tail` (as in UBL's
/// `(cac:InvoiceLine|cac:CreditNoteLine)/cac:SubInvoiceLine`) and a bare `a|b`;
/// anything else is refused.
fn expand_alternation(inner: &str) -> Option<Vec<String>> {
    if is_element_chain(inner) {
        return Some(vec![inner.to_owned()]);
    }
    // `(a|b)/tail`
    if let Some(rest) = inner.strip_prefix('(')
        && let Some((head, tail)) = rest.split_once(')')
        && !head.contains('(')
    {
        let tail = tail.trim();
        // Either nothing after the group, or `/` and a plain chain.
        let suffix = match tail.strip_prefix('/') {
            Some(t) if is_element_chain(t) => Some(t),
            None if tail.is_empty() => None,
            _ => return None,
        };
        let mut out = Vec::new();
        for branch in head.split('|').map(str::trim) {
            if !is_element_chain(branch) {
                return None;
            }
            out.push(match suffix {
                Some(t) => format!("{branch}/{t}"),
                None => branch.to_owned(),
            });
        }
        return (!out.is_empty()).then_some(out);
    }
    // `a|b`
    if inner.contains('|') {
        let branches: Vec<&str> = inner.split('|').map(str::trim).collect();
        if branches.iter().all(|b| is_element_chain(b)) {
            return Some(branches.into_iter().map(str::to_owned).collect());
        }
    }
    None
}

/// Extract the prohibition tables for `syntax`.
///
/// # Errors
///
/// Fails if the artefact is missing or unparsable, or yields no prohibitions
/// (a sign the artefact layout changed).
pub fn extract(syntax: Syntax, spec: &Path, root: &Path) -> Result<Generated, String> {
    let path = spec.join(artefact(syntax));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc = roxmltree::Document::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    // Sets: deduplicated and byte-stable output.
    let mut paths: Set = Set::new();
    let mut attrs: Set = Set::new();
    let mut unextracted = 0usize;
    // Counted per assertion, not per emitted row (an alternation expands to
    // several rows).
    let mut assertions = 0usize;

    for rule in doc.descendants().filter(|n| n.has_tag_name("rule")) {
        let context = rule.attribute("context").unwrap_or("").trim();
        for assertion in rule.children().filter(|n| n.has_tag_name("assert")) {
            let Some(id) = assertion.attribute("id") else {
                continue;
            };
            if !is_syntax_rule(id) {
                continue;
            }
            let Some(test) = assertion.attribute("test").map(str::trim) else {
                continue;
            };
            let Some(inner) = strip_not(test) else {
                continue; // a count(…) or string-length(…) rule, not a prohibition
            };

            // `//@languageID` — forbidden anywhere, no context needed.
            assertions += 1;
            if let Some(attr) = inner.strip_prefix("//@")
                && !attr.is_empty()
                && attr.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                attrs.insert(format!("(\"{}\", \"{}\"),", escape(id), escape(attr)));
                continue;
            }
            let Some(forbidden) = expand_alternation(inner) else {
                unextracted += 1;
                continue;
            };

            // A context may alternate (`/ubl:Invoice | /cn:CreditNote`); every
            // branch must be usable, or the rule is not extracted at all.
            let branches: Vec<&str> = context.split('|').map(str::trim).collect();
            let usable = branches
                .iter()
                .all(|b| is_element_chain(b.trim_start_matches('/')) && !b.is_empty());
            if !usable || branches.is_empty() {
                unextracted += 1;
                continue;
            }
            for b in branches {
                for rel in &forbidden {
                    // Leading slashes are kept: `/x` anchors at the document
                    // element, `//x` matches at any depth.
                    paths.insert(format!(
                        "(\"{}\", \"{}\", \"{}\"),",
                        escape(id),
                        escape(b),
                        escape(rel)
                    ));
                }
            }
        }
    }

    if paths.is_empty() && attrs.is_empty() {
        return Err(format!(
            "{} yielded no prohibitions — has the artefact layout changed?",
            path.display()
        ));
    }

    Ok(render(
        syntax,
        &paths,
        &attrs,
        unextracted,
        assertions,
        root,
    ))
}

fn render(
    syntax: Syntax,
    paths: &Set,
    attrs: &Set,
    unextracted: usize,
    assertions: usize,
    root: &Path,
) -> Generated {
    let (label, module) = match syntax {
        Syntax::Ubl => ("UBL", "ubl"),
        Syntax::Cii => ("CII", "cii"),
    };
    let total = assertions;
    let checked = assertions - unextracted;
    let rows = paths.len() + attrs.len();

    // `Set` sorts by the rendered line, i.e. by rule id.
    let path_body: String = paths.iter().map(|l| format!("    {l}\n")).collect();
    let attr_body: String = attrs.iter().map(|l| format!("    {l}\n")).collect();

    let doc = header(
        &format!("{label} prohibitions, extracted from CEN's preprocessed Schematron."),
        &format!(
            "//! A `{label}-…` rule of the form `not(x)` says the element its context\n\
             //! selects may not have an `x` child, so each entry is\n\
             //! `(rule, context, relative path)` and a match requires both halves.\n\
             //! Contexts come from the preprocessed artefact, fully resolved.\n\
             //!\n\
             //! {checked} of the artefact's {total} `not(…)` assertions are represented,\n\
             //! as {p} element and {a} attribute rows (an alternation is one rule and\n\
             //! several paths). [`UNEXTRACTED`] = {unextracted} are not: their test is\n\
             //! conditional (a predicate, `ends-with(…)`, a wildcard) and needs an XPath\n\
             //! engine.\n",
            p = paths.len(),
            a = attrs.len(),
        ),
    );

    let contents = format!(
        "{doc}\n\
         /// `(rule id, context, forbidden path relative to that context)`.\n\
         ///\n\
         /// A context beginning `/` anchors at the document element; `//` or a bare\n\
         /// name matches at any depth.\n\
         pub static FORBIDDEN_PATHS: &[(&str, &str, &str)] = &[\n\
         {path_body}];\n\
         \n\
         /// `(rule id, attribute name)` — prohibited anywhere in the document.\n\
         pub static FORBIDDEN_ATTRIBUTES: &[(&str, &str)] = &[\n\
         {attr_body}];\n\
         \n\
         /// Prohibitions whose test this crate cannot evaluate, and which\n\
         /// [`FORBIDDEN_PATHS`] therefore does **not** cover.\n\
         pub const UNEXTRACTED: usize = {unextracted};\n\
         \n\
         /// Every `{label}-*` `not(…)` assertion in the artefact, checkable or not.\n\
         ///\n\
         /// One per **assertion**. The tables above hold {rows} rows, which is more,\n\
         /// because one alternating rule expands to one row per branch.\n\
         pub const TOTAL_PARAMS: usize = {total};\n"
    );

    Generated {
        path: root
            .join("src")
            .join(module)
            .join("prohibitions_generated.rs"),
        contents,
    }
}

/// Keeps the otherwise unused `PathBuf` import referenced.
#[allow(dead_code)]
type _Unused = PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternations_expand_into_one_chain_per_branch() {
        let expand = |s: &str| expand_alternation(s);

        // A plain chain passes through.
        assert_eq!(expand("cbc:UUID"), Some(vec!["cbc:UUID".to_owned()]));
        assert_eq!(
            expand("cac:Party/cbc:Name"),
            Some(vec!["cac:Party/cbc:Name".to_owned()])
        );

        // A leading alternation with a common tail.
        assert_eq!(
            expand("(cac:InvoiceLine|cac:CreditNoteLine)/cac:SubInvoiceLine"),
            Some(vec![
                "cac:InvoiceLine/cac:SubInvoiceLine".to_owned(),
                "cac:CreditNoteLine/cac:SubInvoiceLine".to_owned(),
            ])
        );
        // Whitespace around branches is trimmed.
        assert_eq!(
            expand("( cac:A | cac:B )/cbc:C"),
            Some(vec!["cac:A/cbc:C".to_owned(), "cac:B/cbc:C".to_owned()])
        );
        // A group with no tail, and a bare alternation.
        assert_eq!(
            expand("(cac:A|cac:B)"),
            Some(vec!["cac:A".to_owned(), "cac:B".to_owned()])
        );
        assert_eq!(
            expand("cac:A|cac:B"),
            Some(vec!["cac:A".to_owned(), "cac:B".to_owned()])
        );
        // Three branches, and a multi-segment tail.
        assert_eq!(
            expand("(cac:A|cac:B|cac:C)/cac:D/cbc:E").map(|v| v.len()),
            Some(3)
        );
    }

    /// Anything needing an XPath engine stays unexpanded.
    #[test]
    fn conditional_tests_are_refused_rather_than_guessed_at() {
        for hard in [
            "cac:Party[cbc:Name]",              // a predicate
            "//*[ends-with(name(), 'Amount')]", // a function
            "cac:*",                            // a wildcard
            "@languageID",                      // an attribute
            "(cac:A|cac:B[x])/cbc:C",           // a predicate inside a branch
            "(cac:A|cac:B)[1]",                 // a predicate after the group
            "(cac:A|(cac:B|cac:C))/cbc:D",      // nested groups
            "",
        ] {
            assert_eq!(expand_alternation(hard), None, "{hard} must not expand");
        }
    }

    #[test]
    fn only_syntax_rule_ids_are_treated_as_prohibitions() {
        for yes in ["UBL-CR-1", "CII-SR-452", "UBL-DT-13"] {
            assert!(is_syntax_rule(yes), "{yes}");
        }
        // Business rules and malformed ids.
        for no in [
            "BR-01",
            "BR-CO-14",
            "PEPPOL-EN16931-R120",
            "UBL-XX-1",
            "UBL-CR-x",
        ] {
            assert!(!is_syntax_rule(no), "{no}");
        }
    }
}
