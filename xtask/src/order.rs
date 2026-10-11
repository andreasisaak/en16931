//! Deriving element order: from the D16B schema for CII, from the authorities'
//! own instances for UBL.
//!
//! CII: each parent's observed children, sorted by their position in the parent
//! type's `xsd:sequence` in CEN's D16B subset schema. Fails if a parent's type
//! cannot be resolved, if one element name has two different sequences, or if an
//! observed child is not in its parent's sequence. (The instances alone get CII
//! wrong: KoSIT's mutation instances outvote valid ones, and mutually exclusive
//! elements never co-occur.)
//!
//! UBL: every pair of sibling children is counted in observed order and the
//! children are topologically sorted by the majority direction, so a single
//! deliberately invalid instance cannot reorder the table. Ties are broken by
//! mean relative position, then by name. Fails on a cycle or a pair seen equally
//! often in both directions.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{Generated, Table, escape, header, local};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Syntax {
    Ubl,
    Cii,
}

impl Syntax {
    /// The document elements that start a document in this syntax.
    fn roots(self) -> &'static [&'static str] {
        match self {
            // Invoice and CreditNote disagree on where `cbc:TaxPointDate` goes.
            Self::Ubl => &["Invoice", "CreditNote"],
            Self::Cii => &["CrossIndustryInvoice"],
        }
    }

    fn module(self) -> &'static str {
        match self {
            Self::Ubl => "ubl",
            Self::Cii => "cii",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Ubl => "UBL 2.1",
            Self::Cii => "CII D16B",
        }
    }
}

/// Pairwise precedence counts and observed positions, per parent element.
#[derive(Default)]
struct Observations {
    /// `parent -> (before, after) -> times seen in that order`.
    pairs: BTreeMap<String, BTreeMap<(String, String), u32>>,
    /// `parent -> child -> children in first-seen order`.
    names: Table,
    /// `parent -> child -> (sum of relative positions, count)`.
    positions: BTreeMap<String, BTreeMap<String, (f64, u32)>>,
}

impl Observations {
    fn walk(&mut self, node: roxmltree::Node<'_, '_>) {
        let kids: Vec<&str> = node
            .children()
            .filter(roxmltree::Node::is_element)
            .map(|c| c.tag_name().name())
            .collect();
        let parent = node.tag_name().name();
        if !kids.is_empty() {
            let names = self.names.entry(parent.to_owned()).or_default();
            let last = kids.len().saturating_sub(1);
            for (i, k) in kids.iter().enumerate() {
                if !names.iter().any(|n| n == k) {
                    names.push((*k).to_owned());
                }
                #[allow(clippy::cast_precision_loss)]
                let rel = if last == 0 {
                    0.0
                } else {
                    i as f64 / last as f64
                };
                let e = self
                    .positions
                    .entry(parent.to_owned())
                    .or_default()
                    .entry((*k).to_owned())
                    .or_insert((0.0, 0));
                e.0 += rel;
                e.1 += 1;
            }
            let pairs = self.pairs.entry(parent.to_owned()).or_default();
            for i in 0..kids.len() {
                for j in i + 1..kids.len() {
                    if kids[i] != kids[j] {
                        *pairs
                            .entry((kids[i].to_owned(), kids[j].to_owned()))
                            .or_default() += 1;
                    }
                }
            }
        }
        for c in node.children().filter(roxmltree::Node::is_element) {
            self.walk(c);
        }
    }
}

/// Derive the order table for `syntax` from `files` (and, for CII, the D16B
/// schema under `spec`).
///
/// # Errors
///
/// Fails if no instance of this syntax was found; for UBL, if the corpus does
/// not agree on a total order for some parent — a cycle, or a pair seen equally
/// often in both directions; for CII, if the schema cannot order an observed
/// parent's children.
pub fn derive(
    syntax: Syntax,
    files: &[PathBuf],
    spec: &Path,
    root: &Path,
) -> Result<Generated, String> {
    let mut obs = Observations::default();
    let mut instances = 0usize;

    for path in files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(doc) = roxmltree::Document::parse(&text) else {
            continue; // the corpus deliberately contains malformed documents
        };
        let name = doc.root_element().tag_name().name();
        if !syntax.roots().contains(&name) {
            continue;
        }
        instances += 1;
        obs.walk(doc.root_element());
    }

    if instances == 0 {
        return Err(format!(
            "no {} instances found under the reference corpus",
            syntax.label()
        ));
    }

    let mut table: Vec<(String, Vec<String>)> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();

    for (parent, names) in &obs.names {
        let empty = BTreeMap::new();
        let pairs = obs.pairs.get(parent).unwrap_or(&empty);
        // `before[a]` = everything that must follow `a`.
        let mut before: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for a in names {
            for b in names {
                if a == b {
                    continue;
                }
                let ab = pairs.get(&(a.clone(), b.clone())).copied().unwrap_or(0);
                let ba = pairs.get(&(b.clone(), a.clone())).copied().unwrap_or(0);
                if ab > ba {
                    before.entry(a).or_default().push(b);
                } else if ab > 0 && ab == ba {
                    conflicts.push(format!("{parent}: {a} vs {b} seen {ab}× each way"));
                }
            }
        }
        let mean = |c: &str| -> f64 {
            obs.positions
                .get(parent)
                .and_then(|m| m.get(c))
                .map_or(0.0, |(sum, n)| sum / f64::from(*n))
        };

        let mut remaining: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut ordered: Vec<String> = Vec::new();
        while !remaining.is_empty() {
            let mut ready: Vec<&str> = remaining
                .iter()
                .copied()
                .filter(|c| {
                    !remaining
                        .iter()
                        .any(|o| o != c && before.get(o).is_some_and(|v| v.contains(c)))
                })
                .collect();
            if ready.is_empty() {
                conflicts.push(format!("{parent}: cycle among {remaining:?}"));
                ready.clone_from(&remaining);
            }
            // Stable and deterministic: least mean position, then by name.
            let pick = ready
                .iter()
                .copied()
                .min_by(|a, b| {
                    mean(a)
                        .partial_cmp(&mean(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.cmp(b))
                })
                .expect("ready is non-empty");
            ordered.push(pick.to_owned());
            remaining.retain(|c| *c != pick);
        }
        if ordered.len() > 1 {
            table.push((parent.clone(), ordered));
        }
    }

    if syntax == Syntax::Cii {
        // The schema orders CII; the instances only say which parents and
        // children occur. Their disagreements are moot.
        conflicts.clear();
        let sequences = d16b_sequences(&spec.join(D16B))?;
        for (parent, kids) in &mut table {
            let seq = sequences.get(parent.as_str()).ok_or_else(|| {
                format!("CII: <{parent}> has no element sequence in the D16B schema")
            })?;
            if let Some(stray) = kids.iter().find(|k| !seq.contains(k)) {
                return Err(format!(
                    "CII: <{stray}> observed under <{parent}>, whose D16B sequence does \
                     not contain it"
                ));
            }
            kids.sort_by_key(|k| seq.iter().position(|s| s == k));
        }
    }

    if !conflicts.is_empty() {
        return Err(format!(
            "{} could not be derived — the corpus disagrees with itself:\n  {}",
            syntax.label(),
            conflicts.join("\n  ")
        ));
    }

    table.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(render(syntax, instances, &table, root))
}

/// The uncoupled D16B subset schema directory, relative to the reference corpus.
const D16B: &str =
    "eInvoicing-EN16931/cii/schema/D16B SCRDM (Subset)/uncoupled clm/CII/uncefact/data/standard";

/// `element name -> child element names in sequence order`, for every element
/// with element content reachable from `rsm:CrossIndustryInvoice`.
fn d16b_sequences(dir: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    const XS: &str = "http://www.w3.org/2001/XMLSchema";
    let mut types: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut root_type = None;
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "xsd") {
            continue;
        }
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc =
            roxmltree::Document::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let unprefixed = |t: &str| t.rsplit(':').next().unwrap_or(t).to_owned();
        for n in doc
            .root_element()
            .children()
            .filter(roxmltree::Node::is_element)
        {
            match (n.tag_name().namespace(), n.tag_name().name()) {
                (Some(XS), "complexType") => {
                    let Some(name) = n.attribute("name") else {
                        continue;
                    };
                    let kids = n
                        .descendants()
                        .filter(|d| {
                            d.tag_name().namespace() == Some(XS) && d.tag_name().name() == "element"
                        })
                        .filter_map(|d| {
                            Some((
                                d.attribute("name")?.to_owned(),
                                unprefixed(d.attribute("type")?),
                            ))
                        })
                        .collect();
                    types.insert(name.to_owned(), kids);
                }
                (Some(XS), "element") if n.attribute("name") == Some("CrossIndustryInvoice") => {
                    root_type = n.attribute("type").map(unprefixed);
                }
                _ => {}
            }
        }
    }
    let root_type = root_type.ok_or("D16B: no CrossIndustryInvoice element declaration")?;

    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stack = vec![("CrossIndustryInvoice".to_owned(), root_type)];
    let mut seen = std::collections::BTreeSet::new();
    while let Some((name, ty)) = stack.pop() {
        let Some(kids) = types.get(&ty) else { continue };
        if kids.is_empty() || !seen.insert((name.clone(), ty.clone())) {
            continue;
        }
        let seq: Vec<String> = kids.iter().map(|(k, _)| k.clone()).collect();
        if let Some(prev) = out.get(&name)
            && *prev != seq
        {
            return Err(format!("D16B: <{name}> has two different content models"));
        }
        out.insert(name, seq);
        stack.extend(kids.iter().cloned());
    }
    Ok(out)
}

fn render(
    syntax: Syntax,
    instances: usize,
    table: &[(String, Vec<String>)],
    root: &Path,
) -> Generated {
    let label = syntax.label();
    let module = syntax.module();
    let mut body = String::new();
    for (parent, kids) in table {
        let list = kids
            .iter()
            .map(|k| format!("\"{}\"", escape(k)))
            .collect::<Vec<_>>()
            .join(", ");
        body.push_str(&format!("    (\"{}\", &[{list}]),\n", escape(parent)));
    }

    let method = match syntax {
        Syntax::Cii => {
            "//! For each parent, the children are those observed, in the order of the\n\
             //! parent type's `xsd:sequence` in CEN's uncoupled D16B subset schema.\n\
             //! `cargo xtask codegen` fails if the schema cannot order an observed\n\
             //! parent's children.\n"
        }
        Syntax::Ubl => {
            "//! For each parent, the child order is the topological sort of the pairwise\n\
             //! precedences observed, taking the majority direction where instances\n\
             //! disagree. `cargo xtask codegen` fails on a cycle or a tied pair.\n"
        }
    };
    let title = match syntax {
        Syntax::Cii => format!("The {label} element order, from the D16B schema."),
        Syntax::Ubl => {
            format!("The {label} element order, derived from the authorities' own instances.")
        }
    };
    let doc = header(
        &title,
        &format!(
            "//! Parents and children observed in **{instances}** published `{roots}`\n\
             //! documents of the reference corpus, covering {parents} parent elements.\n\
             //! {label} content models are XSD `sequence`s, and no Schematron rule\n\
             //! checks order.\n\
             //!\n\
             {method}\
             //!\n\
             //! The shared serialiser sorts every written element by this table.\n",
            roots = syntax.roots().join("` / `"),
            parents = table.len(),
        ),
    );

    let contents = format!(
        "{doc}\n\
         /// `(parent, children in sequence order)` for every element observed with more\n\
         /// than one distinct child. Sorted by parent, for binary search.\n\
         pub static ORDER: &[(&str, &[&str])] = &[\n\
         {body}];\n\
         \n\
         /// The canonical child order for a parent element, if one was observed.\n\
         #[must_use]\n\
         pub fn children_of(parent: &str) -> Option<&'static [&'static str]> {{\n\
         \x20   ORDER\n\
         \x20       .binary_search_by_key(&parent, |(p, _)| *p)\n\
         \x20       .ok()\n\
         \x20       .map(|i| ORDER[i].1)\n\
         }}\n\
         \n\
         /// The position of `child` within `parent`'s sequence.\n\
         #[must_use]\n\
         pub fn index_of(parent: &str, child: &str) -> Option<usize> {{\n\
         \x20   children_of(parent)?.iter().position(|c| *c == child)\n\
         }}\n"
    );

    // `crate::local` has no other caller; this keeps it referenced.
    let _ = local;

    Generated {
        path: root.join("src").join(module).join("order.rs"),
        contents,
    }
}
