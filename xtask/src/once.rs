//! Deriving the single-occurrence set: which elements a document may carry at
//! most once. Readers read the first occurrence and report every further one.
//!
//! Two sources:
//!
//! * The schemas: for each `(parent, child)` pair the authorities' instances use,
//!   the child's `maxOccurs` in the parent's type (CII: CEN's uncoupled D16B
//!   subset schema; UBL: OASIS UBL 2.1). The row's rule cell is empty.
//! * CEN's `count(path) <= 1` syntax rules from the preprocessed Schematron
//!   (e.g. `UBL-SR-09`), plus KoSIT's replacements listed in `REPLACED`. A path is
//!   a chain of child steps, optionally led by `//`, each with at most one
//!   predicate comparing a child's value (optionally upper-cased or
//!   whitespace-normalised) to a literal. Anything else goes to `UNCHECKED` with
//!   the reason.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::order::Syntax;
use crate::{Generated, escape, header};

const XS: &str = "http://www.w3.org/2001/XMLSchema";

/// The schema files whose types the readers walk, relative to the reference corpus.
fn schemas(syntax: Syntax, spec: &Path) -> Result<Vec<PathBuf>, String> {
    match syntax {
        Syntax::Ubl => Ok([
            "oasis-ubl-2.1/xsd/maindoc/UBL-Invoice-2.1.xsd",
            "oasis-ubl-2.1/xsd/maindoc/UBL-CreditNote-2.1.xsd",
            "oasis-ubl-2.1/xsd/common/UBL-CommonAggregateComponents-2.1.xsd",
        ]
        .iter()
        .map(|p| spec.join(p))
        .collect()),
        Syntax::Cii => {
            let dir = spec.join(
                "eInvoicing-EN16931/cii/schema/D16B SCRDM (Subset)/uncoupled clm/CII/uncefact/data/standard",
            );
            let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xsd"))
                .collect();
            out.sort();
            Ok(out)
        }
    }
}

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

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Element name → the types it is declared with; type → child → allowed once.
#[derive(Default)]
struct Model {
    element_types: BTreeMap<String, BTreeSet<String>>,
    types: BTreeMap<String, BTreeMap<String, bool>>,
}

impl Model {
    fn read(files: &[PathBuf]) -> Result<Self, String> {
        let mut m = Self::default();
        for path in files {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let doc = roxmltree::Document::parse(&text)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            for n in doc
                .root_element()
                .children()
                .filter(roxmltree::Node::is_element)
            {
                let is = |want: &str| {
                    n.tag_name().namespace() == Some(XS) && n.tag_name().name() == want
                };
                if is("element")
                    && let (Some(name), Some(ty)) = (n.attribute("name"), n.attribute("type"))
                {
                    m.element_types
                        .entry(name.to_owned())
                        .or_default()
                        .insert(local(ty).to_owned());
                } else if is("complexType")
                    && let Some(ty) = n.attribute("name")
                {
                    m.complex_type(ty, n);
                }
            }
        }
        Ok(m)
    }

    fn complex_type(&mut self, ty: &str, node: roxmltree::Node<'_, '_>) {
        let mut children: BTreeMap<String, bool> = BTreeMap::new();
        for d in node
            .descendants()
            .filter(|d| d.tag_name().namespace() == Some(XS) && d.tag_name().name() == "element")
        {
            let Some(child) = d
                .attribute("name")
                .or_else(|| d.attribute("ref").map(local))
            else {
                continue;
            };
            if let (Some(name), Some(t)) = (d.attribute("name"), d.attribute("type")) {
                self.element_types
                    .entry(name.to_owned())
                    .or_default()
                    .insert(local(t).to_owned());
            }
            // Once only if the element and every particle between it and the
            // type say once.
            let repeated =
                |x: roxmltree::Node<'_, '_>| x.attribute("maxOccurs").unwrap_or("1") != "1";
            let once = !repeated(d)
                && !d
                    .ancestors()
                    .skip(1)
                    .take_while(|a| a.id() != node.id())
                    .any(repeated);
            // The same name twice in one content model is a repetition.
            children
                .entry(child.to_owned())
                .and_modify(|o| *o = false)
                .or_insert(once);
        }
        self.types.insert(ty.to_owned(), children);
    }

    /// Whether `child` is allowed once under `parent`: every type `parent` is
    /// declared with that has `child` says so, and at least one has it.
    fn once(&self, parent: &str, child: &str) -> bool {
        let Some(types) = self.element_types.get(parent) else {
            return false;
        };
        let verdicts: Vec<bool> = types
            .iter()
            .filter_map(|t| self.types.get(t).and_then(|c| c.get(child)).copied())
            .collect();
        !verdicts.is_empty() && verdicts.iter().all(|o| *o)
    }
}

/// Every `(parent, child)` pair the authorities' instances use.
fn observed(syntax: Syntax, files: &[PathBuf]) -> BTreeSet<(String, String)> {
    let roots: &[&str] = match syntax {
        Syntax::Ubl => &["Invoice", "CreditNote"],
        Syntax::Cii => &["CrossIndustryInvoice"],
    };
    let mut out = BTreeSet::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(doc) = roxmltree::Document::parse(&text) else {
            continue;
        };
        if !roots.contains(&doc.root_element().tag_name().name()) {
            continue;
        }
        for n in doc.descendants().filter(roxmltree::Node::is_element) {
            for c in n.children().filter(roxmltree::Node::is_element) {
                out.insert((
                    n.tag_name().name().to_owned(),
                    c.tag_name().name().to_owned(),
                ));
            }
        }
    }
    out
}

fn is_chain(s: &str) -> bool {
    !s.is_empty()
        && s.split('/').all(|seg| {
            let mut halves = seg.split(':');
            matches!((halves.next(), halves.next(), halves.next()),
                (Some(p), Some(l), None) if !p.is_empty() && !l.is_empty()
                    && p.chars().all(|c| c.is_alphanumeric())
                    && l.chars().all(|c| c.is_alphanumeric() || c == '_'))
        })
}

/// Split on `/` outside brackets.
fn steps(path: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in path.char_indices() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            '/' if depth == 0 => {
                out.push(&path[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&path[start..]);
    out
}

/// A predicate body in the canonical spelling the reader compiles:
/// `upper:a/b=LIT`, `norm:a=LIT`, `a!=LIT`.
fn predicate(body: &str) -> Result<String, &'static str> {
    if body.contains("preceding::") || body.starts_with("not(") {
        return Err("a distinct-value count, which is not a count of occurrences");
    }
    if body.contains('@') {
        return Err("an attribute predicate");
    }
    let (lhs, op, rhs) = match body.split_once("!=") {
        Some((l, r)) => (l, "!=", r),
        None => {
            let (l, r) = body
                .split_once('=')
                .ok_or("a predicate that is not a comparison")?;
            (l, "=", r)
        }
    };
    let lit = rhs
        .trim()
        .strip_prefix('\'')
        .and_then(|r| r.strip_suffix('\''))
        .ok_or("a comparison with something other than a literal")?;
    let lhs = lhs.trim();
    let (prefix, chain) = if let Some(inner) = lhs
        .strip_prefix("normalize-space(")
        .and_then(|r| r.strip_suffix(')'))
    {
        ("norm:", inner.trim().to_owned())
    } else if let Some((head, tail)) = lhs.rsplit_once("upper-case(") {
        let inner = tail.strip_suffix(')').ok_or("an unbalanced upper-case()")?;
        ("upper:", format!("{head}{inner}"))
    } else {
        ("", lhs.to_owned())
    };
    if !is_chain(&chain) {
        return Err("a predicate on something other than an element path");
    }
    Ok(format!("{prefix}{chain}{op}{lit}"))
}

/// The canonical relative path, or why it cannot be expressed.
fn relative(path: &str) -> Result<String, &'static str> {
    let (lead, body) = match path.strip_prefix("//") {
        Some(rest) => ("//", rest),
        None => ("", path),
    };
    let mut out = Vec::new();
    for step in steps(body) {
        let (name, pred) = match step.split_once('[') {
            Some((n, p)) => (
                n,
                Some(p.strip_suffix(']').ok_or("an unbalanced predicate")?),
            ),
            None => (step, None),
        };
        if name.starts_with('@') || name.contains('.') {
            return Err("an attribute or value count, not an element count");
        }
        if !is_chain(name) || name.contains('/') {
            return Err("a step that is not an element name");
        }
        out.push(match pred {
            Some(p) => format!("{name}[{}]", predicate(p.trim())?),
            None => name.to_owned(),
        });
    }
    Ok(format!("{lead}{}", out.join("/")))
}

/// `count(x) <= 1`, with or without parentheses and spaces — `x`.
fn counted(test: &str) -> Option<&str> {
    let t = test.trim();
    let t = t
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .map_or(t, str::trim);
    let (lhs, rhs) = t.rsplit_once("<=")?;
    if rhs.trim() != "1" {
        return None;
    }
    lhs.trim().strip_prefix("count(")?.strip_suffix(')')
}

type Row = (String, String, String, String);

/// CEN syntax rules KoSIT replaces with a corrected rule, and the replacement
/// (paired in a comment beside each replacement in KoSIT's Schematron).
const REPLACED: &[(&str, &str)] = &[("CII-SR-475", "BR-TMP-4"), ("CII-SR-476", "BR-TMP-5")];

/// KoSIT's CII Schematron, where the replacements live.
const KOSIT_CII: &str =
    "xrechnung-schematron/src/validation/schematron/cii/XRechnung-CII-validation.sch";

/// A rule context's branches in the canonical spelling: element chains, the
/// last step optionally carrying one predicate.
fn contexts(context: &str) -> Result<Vec<String>, &'static str> {
    context
        .split('|')
        .map(str::trim)
        .map(|b| {
            let lead = if b.starts_with("//") {
                "//"
            } else if b.starts_with('/') {
                "/"
            } else {
                ""
            };
            let body = b.trim_start_matches('/');
            let parts = steps(body);
            let (last, init) = parts.split_last().ok_or("an empty context")?;
            if !init.iter().all(|s| is_chain(s)) {
                return Err("a conditional rule context");
            }
            let last = match last.split_once('[') {
                Some((name, pred)) if is_chain(name) => format!(
                    "{name}[{}]",
                    predicate(
                        pred.strip_suffix(']')
                            .ok_or("an unbalanced predicate")?
                            .trim()
                    )?
                ),
                None if is_chain(last) => (*last).to_owned(),
                _ => return Err("a conditional rule context"),
            };
            let mut chain: Vec<String> = init.iter().map(|s| (*s).to_owned()).collect();
            chain.push(last);
            Ok(format!("{lead}{}", chain.join("/")))
        })
        .collect()
}

/// The `count(…) <= 1` rows of KoSIT's replacements for [`REPLACED`].
fn kosit_rows(spec: &Path) -> Result<Vec<Row>, String> {
    let path = spec.join(KOSIT_CII);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc = roxmltree::Document::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut rows = Vec::new();
    for rule in doc.descendants().filter(|n| n.has_tag_name("rule")) {
        let context = rule.attribute("context").unwrap_or("").trim();
        for a in rule.children().filter(|n| n.has_tag_name("assert")) {
            let (Some(id), Some(test)) = (a.attribute("id"), a.attribute("test")) else {
                continue;
            };
            if !REPLACED.iter().any(|(_, by)| *by == id) {
                continue;
            }
            let rel = counted(test)
                .ok_or_else(|| format!("{id} is no longer a `count(…) <= 1` rule"))
                .and_then(|p| relative(p).map_err(|e| format!("{id}: {e}")))?;
            for b in contexts(context).map_err(|e| format!("{id}: {e}"))? {
                let level = a.attribute("flag").unwrap_or("fatal").to_owned();
                rows.push((id.to_owned(), level, b, rel.clone()));
            }
        }
    }
    if rows.len() != REPLACED.len() {
        return Err(format!(
            "KoSIT's replacements for {REPLACED:?} were not all found in {}",
            path.display()
        ));
    }
    Ok(rows)
}

/// `(rule id, why it is not expressible)` for assertions outside the subset.
type Unchecked = Vec<(String, String)>;

fn rules(syntax: Syntax, spec: &Path) -> Result<(Vec<Row>, Unchecked), String> {
    let path = spec.join(artefact(syntax));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc = roxmltree::Document::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut rows = Vec::new();
    let mut unchecked = Vec::new();
    for rule in doc.descendants().filter(|n| n.has_tag_name("rule")) {
        let context = rule.attribute("context").unwrap_or("").trim();
        for a in rule.children().filter(|n| n.has_tag_name("assert")) {
            let (Some(id), Some(test)) = (a.attribute("id"), a.attribute("test")) else {
                continue;
            };
            let syntax_rule = id.split('-').nth(1) == Some("SR");
            let Some(path) = counted(test).filter(|_| syntax_rule) else {
                continue;
            };
            let level = a.attribute("flag").unwrap_or("fatal");
            if let Some((_, by)) = REPLACED.iter().find(|(cen, _)| *cen == id) {
                unchecked.push((
                    id.to_owned(),
                    format!(
                        "its context counts across every BG-24 (CEN issue 508); KoSIT's {by} \
                         checks it per BG-24 and is used instead"
                    ),
                ));
                continue;
            }
            let Ok(branches) = contexts(context) else {
                unchecked.push((id.to_owned(), "a conditional rule context".to_owned()));
                continue;
            };
            match relative(path) {
                Ok(rel) => {
                    for b in branches {
                        rows.push((id.to_owned(), level.to_owned(), b, rel.clone()));
                    }
                }
                Err(why) => unchecked.push((id.to_owned(), why.to_owned())),
            }
        }
    }
    if rows.is_empty() {
        return Err(format!(
            "{} yielded no `count(…) <= 1` syntax rules — has the artefact layout changed?",
            path.display()
        ));
    }
    Ok((rows, unchecked))
}

/// Derive `src/{ubl,cii}/once.rs`.
///
/// # Errors
///
/// A schema or the Schematron is missing or unreadable, or either source
/// yields nothing.
pub fn derive(
    syntax: Syntax,
    files: &[PathBuf],
    spec: &Path,
    root: &Path,
) -> Result<Generated, String> {
    let model = Model::read(&schemas(syntax, spec)?)?;
    let mut rows: BTreeSet<Row> = BTreeSet::new();
    let pairs = observed(syntax, files);
    for (parent, child) in &pairs {
        if model.once(parent, child) {
            rows.insert((
                String::new(),
                "fatal".to_owned(),
                parent.clone(),
                child.clone(),
            ));
        }
    }
    let schema_rows = rows.len();
    if schema_rows == 0 {
        return Err(format!(
            "no single-occurrence pair derived for {} — are the schemas fetched?",
            match syntax {
                Syntax::Ubl => "UBL",
                Syntax::Cii => "CII",
            }
        ));
    }
    let (mut rule_rows, mut unchecked) = rules(syntax, spec)?;
    if syntax == Syntax::Cii {
        rule_rows.extend(kosit_rows(spec)?);
    }
    let rule_count = rule_rows.len();
    rows.extend(rule_rows);
    unchecked.sort();
    unchecked.dedup();
    Ok(render(
        syntax,
        &rows,
        schema_rows,
        rule_count,
        &unchecked,
        pairs.len(),
        root,
    ))
}

fn render(
    syntax: Syntax,
    rows: &BTreeSet<Row>,
    schema_rows: usize,
    rule_rows: usize,
    unchecked: &[(String, String)],
    pairs: usize,
    root: &Path,
) -> Generated {
    let (label, module, schema) = match syntax {
        Syntax::Ubl => ("UBL", "ubl", "the OASIS UBL 2.1 schemas"),
        Syntax::Cii => ("CII", "cii", "CEN's uncoupled D16B subset schema"),
    };
    let body: String = rows
        .iter()
        .map(|(rule, level, ctx, rel)| {
            format!(
                "    (\"{}\", \"{}\", \"{}\", \"{}\"),\n",
                escape(rule),
                escape(level),
                escape(ctx),
                escape(rel)
            )
        })
        .collect();
    let unchecked_body: String = unchecked
        .iter()
        .map(|(id, why)| format!("    (\"{}\", \"{}\"),\n", escape(id), escape(why)))
        .collect();
    let doc = header(
        &format!("The {label} single-occurrence set, from {schema} and CEN's syntax rules."),
        &format!(
            "//! A reader reads the **first** occurrence of each element below and reports\n\
             //! every further one; nothing of a second occurrence reaches the model.\n\
             //!\n\
             //! * {schema_rows} rows come from the schema: of the {pairs} `(parent, child)`\n\
             //!   pairs the authorities' instances use, those whose child {schema} declare\n\
             //!   `maxOccurs=\"1\"` in the parent's type. Their rule cell is empty.\n\
             //! * {rule_rows} rows come from CEN's `count(…) <= 1` syntax rules, with the\n\
             //!   level CEN publishes for each — and, where KoSIT replaces a CEN rule whose\n\
             //!   context is wrong, from KoSIT's replacement.\n\
             //! * [`UNCHECKED`] lists the {n} such rules this table cannot express, and why.\n\
             //!\n\
             //! A context beginning `/` anchors at the document element; `//` or a bare\n\
             //! name matches at any depth; a context's last step may carry a predicate.\n\
             //! A predicate is written\n\
             //! `[path=literal]`, `[path!=literal]`, with `upper:` or `norm:` when CEN\n\
             //! compares the value upper-cased or with its whitespace normalised.\n",
            n = unchecked.len(),
        ),
    );
    let contents = format!(
        "{doc}\n\
         /// `(rule, level, context, relative path)` — an empty rule is the schema's.\n\
         pub static ONCE: &[(&str, &str, &str, &str)] = &[\n\
         {body}];\n\
         \n\
         /// `count(…) <= 1` syntax rules this table cannot express, with the reason.\n\
         pub static UNCHECKED: &[(&str, &str)] = &[\n\
         {unchecked_body}];\n"
    );
    Generated {
        path: root.join("src").join(module).join("once.rs"),
        contents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_recognised_with_or_without_parentheses() {
        assert_eq!(counted("(count(cac:A/cbc:B) <= 1)"), Some("cac:A/cbc:B"));
        assert_eq!(
            counted("count(ram:X) &lt;= 1".replace("&lt;", "<").as_str()),
            Some("ram:X")
        );
        assert_eq!(counted("( count(ram:A/ram:B)  <= 1)"), Some("ram:A/ram:B"));
        assert_eq!(counted("count(x) <= 2"), None);
        assert_eq!(counted("not(cbc:X)"), None);
    }

    #[test]
    fn predicates_are_canonicalised_or_refused() {
        assert_eq!(
            relative("cac:AdditionalDocumentReference[cbc:DocumentTypeCode='130']/cbc:ID"),
            Ok("cac:AdditionalDocumentReference[cbc:DocumentTypeCode=130]/cbc:ID".to_owned())
        );
        assert_eq!(
            relative("cac:P/cac:T[cac:TaxScheme/upper-case(cbc:ID)!='VAT']/cbc:C"),
            Ok("cac:P/cac:T[upper:cac:TaxScheme/cbc:ID!=VAT]/cbc:C".to_owned())
        );
        assert_eq!(
            relative("ram:A[normalize-space(ram:TypeCode) = '916']/ram:Name"),
            Ok("ram:A[norm:ram:TypeCode=916]/ram:Name".to_owned())
        );
        assert_eq!(
            relative("//ram:S/ram:PaymentReference"),
            Ok("//ram:S/ram:PaymentReference".to_owned())
        );
        assert!(relative("cac:PaymentMeans/cbc:PaymentMeansCode/@name").is_err());
        assert!(relative("//cbc:PaymentID[not(preceding::cbc:PaymentID/. = .)]").is_err());
        assert!(relative("//cac:P/cbc:ID[upper-case(@schemeID) = 'SEPA']").is_err());
    }
}
