//! Reading the project's own prose back, so a documented number can be
//! checked. Shared by `documented_numbers.rs` and `corpus.rs`.

#![allow(dead_code, reason = "each suite uses the part of this it needs")]

use std::path::{Path, PathBuf};

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/en16931 is two levels below the workspace root")
        .to_path_buf()
}

/// Every file whose prose is part of the published documentation: the
/// READMEs, the site (with its social card), and every source file. `CHANGELOG.md` is not, since it
/// records figures as they were at each release.
pub fn documentation() -> Vec<(String, String)> {
    let root = workspace_root();
    let mut out = Vec::new();
    let mut add = |rel: &str| {
        let p = root.join(rel);
        if let Ok(text) = std::fs::read_to_string(&p) {
            out.push((rel.to_owned(), text));
        }
    };
    add("README.md");
    for crate_dir in ["en16931", "en16931-formats", "en16931-cli"] {
        add(&format!("crates/{crate_dir}/README.md"));
    }
    // The site's default description, which every page without its own carries.
    add("site/config.toml");

    // Every source file: rustdoc publishes module prose too.
    let mut stack = vec![root.join("site/content"), root.join("site/static")];
    for crate_dir in ["en16931", "en16931-formats", "en16931-cli"] {
        stack.push(root.join(format!("crates/{crate_dir}/src")));
    }
    stack.push(root.join("xtask/src"));
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| matches!(e.to_str(), Some("md" | "rs" | "svg")))
            {
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((rel, text));
                }
            }
        }
    }
    assert!(
        out.len() > 10,
        "expected the READMEs and the site; found {} file(s)",
        out.len()
    );
    out
}

/// A claim the documentation makes, and the value it should be making it about.
pub struct Claim {
    /// What the number means, for the failure message.
    pub what: &'static str,
    /// The sentence to look for, with one `<N>` where the number goes.
    pub pattern: &'static str,
    /// The measured value.
    pub expected: usize,
}

/// The characters this project uses between the thousands of a figure,
/// including the two invisible spaces.
const SEPARATORS: [char; 4] = [' ', '_', '\u{a0}', '\u{202f}'];

/// Strip the separators and read the figure.
///
/// `None` for a run too large to be a count (an IBAN in a test fixture).
fn parse(found: &str) -> Option<usize> {
    found.replace(SEPARATORS, "").parse().ok()
}

/// Every maximal run of digits-and-separators, as `(number, start, end)`.
///
/// A run starts at a digit and may contain [`SEPARATORS`] (`1 339`, `1_339`);
/// `end` is just past the last digit. Runs too large for a `usize` are skipped.
pub fn digit_runs(text: &str) -> Vec<(usize, usize, usize)> {
    let mut runs = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if !c.is_ascii_digit() {
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(i, c)) = chars.peek() {
            if c.is_ascii_digit() {
                end = i + c.len_utf8();
            } else if !SEPARATORS.contains(&c) {
                break;
            }
            chars.next();
        }
        if let Some(n) = parse(&text[start..end]) {
            runs.push((n, start, end));
        }
    }
    runs
}

/// Every number matching `pattern`: literal text either side of one `<N>`,
/// case-sensitive.
///
/// A pattern ending in an alphanumeric needs a word boundary after the match
/// (`317 business rulesets` does not match `<N> business rules`), and the
/// `16931` of `EN 16931` is never a number.
pub fn find_all(pattern: &str, text: &str) -> Vec<usize> {
    let parts: Vec<&str> = pattern.split("<N>").collect();
    assert_eq!(parts.len(), 2, "a claim has exactly one <N>: {pattern}");
    let (before, after) = (parts[0], parts[1]);
    let needs_boundary = after
        .chars()
        .last()
        .is_none_or(|c| c.is_ascii_alphanumeric());

    digit_runs(text)
        .into_iter()
        .filter(|&(_, start, end)| {
            text[..start].ends_with(before)
                && text[end..].starts_with(after)
                && !text[..start].ends_with("EN ")
                && (!needs_boundary
                    || text[end + after.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_ascii_alphanumeric()))
        })
        .map(|(n, _, _)| n)
        .collect()
}

/// Check a set of claims against every documentation file, failing with all
/// mismatches at once, and with every claim that matched nothing.
pub fn check(claims: &[Claim]) {
    let docs = documentation();
    let mut wrong = Vec::new();
    let mut never_matched = Vec::new();
    for claim in claims {
        let mut matched = 0usize;
        for (file, text) in &docs {
            for got in find_all(claim.pattern, text) {
                matched += 1;
                if got != claim.expected {
                    wrong.push(format!(
                        "  {file}: {} — documented as {got}, measured {}",
                        claim.what, claim.expected
                    ));
                }
            }
        }
        if matched == 0 {
            never_matched.push(format!(
                "  {:?} — for {}; measured {}",
                claim.pattern, claim.what, claim.expected
            ));
        }
    }
    assert!(
        never_matched.is_empty(),
        "{} claim pattern(s) matched nothing anywhere, which means the prose was \
         reworded and they are now checking nothing. Update the pattern rather \
         than deleting it:\n{}",
        never_matched.len(),
        never_matched.join("\n")
    );
    assert!(
        wrong.is_empty(),
        "{} documented number(s) no longer match the code:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
