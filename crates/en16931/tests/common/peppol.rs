//! Reading Peppol's Schematron at the pinned commit: every assert's flag, and
//! which ids are national rules.

use std::collections::BTreeMap;

/// Peppol's UBL Schematron, relative to the reference corpus.
pub const UBL_SCH: &str = "peppol-bis-invoice-3/rules/sch/PEPPOL-EN16931-UBL.sch";

/// Every `<assert>` / `<report>` id in a Schematron, with its `flag`
/// (`fatal` when absent). Parsed, not scanned.
#[must_use]
pub fn flags(xml: &str) -> BTreeMap<String, String> {
    let doc = roxmltree::Document::parse(xml).expect("Schematron is XML");
    let mut out = BTreeMap::new();
    for n in doc.descendants() {
        if !matches!(n.tag_name().name(), "assert" | "report") {
            continue;
        }
        if let Some(id) = n.attribute("id") {
            let flag = n.attribute("flag").unwrap_or("fatal").to_owned();
            if let Some(prev) = out.insert(id.to_owned(), flag.clone()) {
                assert_eq!(prev, flag, "{id} carries two flags");
            }
        }
    }
    out
}

/// A national rule: `XX-R-…` or `XX-S-…`, `XX` a country code (matched by
/// shape, so new countries are included).
#[must_use]
pub fn is_national(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() > 5
        && b[..2].iter().all(u8::is_ascii_uppercase)
        && b[2] == b'-'
        && matches!(b[3], b'R' | b'S')
        && b[4] == b'-'
}

/// The KoSIT rule a Peppol `DE-R-*` rule is the Peppol-side copy of:
/// `DE-R-001` → `BR-DE-1`, `DE-R-023-1` → `BR-DE-23-a`, `DE-R-T02` → `BR-TMP-2`.
#[must_use]
pub fn kosit_equivalent(id: &str) -> Option<String> {
    let rest = id.strip_prefix("DE-R-")?;
    if let Some(t) = rest.strip_prefix('T') {
        return Some(format!("BR-TMP-{}", t.parse::<u32>().ok()?));
    }
    let (num, part) = match rest.split_once('-') {
        Some((n, "1")) => (n, Some("a")),
        Some((n, "2")) => (n, Some("b")),
        Some(_) => return None,
        None => (rest, None),
    };
    let n: u32 = num.parse().ok()?;
    Some(match part {
        Some(p) => format!("BR-DE-{n}-{p}"),
        None => format!("BR-DE-{n}"),
    })
}
