//! Numbers the documentation quotes (READMEs, site, source prose), checked
//! against the code that produces them.
//!
//! Each claim is a pattern with one number slot, applied to every
//! documentation file, and must match at least once. Figures that need the
//! reference corpus are asserted by the suites that read it; the CEN
//! assertion count is read from the conformance suite's `CEN_ASSERTIONS`.

use std::collections::BTreeSet;

mod common;
use common::docs::{Claim, check, documentation, find_all};

#[test]
fn every_number_the_documentation_quotes_is_the_one_the_code_produces() {
    // "Business rules" is the registry; a profile's restrictions count only
    // among its checks.
    let registered: BTreeSet<&str> = en16931::validation::rules::CORE
        .iter()
        .map(|r| r.id.as_str())
        .chain(
            en16931::profiles::ALL
                .iter()
                .flat_map(|p| p.extra_rules.iter().map(|r| r.id.as_str())),
        )
        .collect();
    let crate_own = registered.iter().filter(|id| id.starts_with("EN-")).count();
    let tables = en16931::codes::generated::TABLES;
    let code_values: usize = tables.iter().map(|(_, codes)| codes.len()).sum();
    let checks = |p: &en16931::Profile| p.check_ids().count();

    let claims = [
        // The five profile check counts, as the comparison tables print them.
        Claim {
            what: "checks the core profile runs",
            pattern: "| EN 16931 core | <N> |",
            expected: checks(&en16931::profiles::EN16931),
        },
        Claim {
            what: "checks XRechnung 3.0 runs",
            pattern: "| XRechnung 3.0 | <N> |",
            expected: checks(&en16931::profiles::XRECHNUNG),
        },
        Claim {
            what: "checks XRechnung 3.0 CVD runs",
            pattern: "| XRechnung 3.0 CVD | <N> |",
            expected: checks(&en16931::profiles::XRECHNUNG_CVD),
        },
        Claim {
            what: "checks XRechnung 3.0 Extension runs",
            pattern: "| XRechnung 3.0 Extension | <N> |",
            expected: checks(&en16931::profiles::XRECHNUNG_EXTENSION),
        },
        Claim {
            what: "checks Peppol BIS Billing 3.0 runs",
            pattern: "| Peppol BIS Billing 3.0 | <N> |",
            expected: checks(&en16931::profiles::PEPPOL_BIS_3),
        },
        Claim {
            what: "rules registered across every shipped profile",
            pattern: "<N> business rules",
            expected: registered.len(),
        },
        Claim {
            what: "rules registered across every shipped profile",
            pattern: "**<N>** rules registered",
            expected: registered.len(),
        },
        Claim {
            what: "rules registered across every shipped profile",
            pattern: "<N> rules registered",
            expected: registered.len(),
        },
        Claim {
            what: "rules registered, as the landing page's figure",
            pattern: "{ n = \"<N>\", k = \"rules registered\" }",
            expected: registered.len(),
        },
        // The constant `en16931-formats`' conformance suite asserts.
        Claim {
            what: "CEN unit-test assertions run",
            pattern: "<N> CEN unit-test assertions",
            expected: cen_assertions(),
        },
        Claim {
            what: "CEN unit-test assertions run, as the landing page's figure",
            pattern: "{ n = \"<N>\", k = \"CEN unit-test assertions",
            expected: cen_assertions(),
        },
        Claim {
            what: "this crate's own `EN-*` rules",
            pattern: "**<N>** of those are this crate's own",
            expected: crate_own,
        },
        Claim {
            what: "this crate's own `EN-*` rules",
            pattern: "| `EN-*` | <N> | this crate's own",
            expected: crate_own,
        },
        Claim {
            what: "generated code values",
            pattern: "<N> values across",
            expected: code_values,
        },
        Claim {
            what: "generated code lists",
            pattern: "values across <N> tables",
            expected: tables.len(),
        },
        Claim {
            what: "generated code lists",
            pattern: "Code lists — <N> ",
            expected: code_values,
        },
        Claim {
            what: "generated code values",
            pattern: "Eighteen lists, **<N> values**",
            expected: code_values,
        },
        // Two figures quoted in module prose.
        Claim {
            what: "`BR-DEC-*` assertions the two-decimal type retires",
            pattern: "as <N> separate assertions",
            expected: en16931::validation::rules::CORE
                .iter()
                .filter(|r| r.id.as_str().starts_with("BR-DEC-"))
                .count(),
        },
        // A pattern fits on one line of wrapped comment prose.
        Claim {
            what: "UN/ECE Rec 20/21 unit codes",
            pattern: "list of <N> values",
            expected: en16931::codes::generated::UNIT_CODES.len(),
        },
        Claim {
            what: "files `cargo xtask codegen` writes",
            pattern: "has <N> generated files",
            expected: generated_files(),
        },
        Claim {
            what: "files `cargo xtask check` re-derives",
            pattern: "re-derives all <N> and",
            expected: generated_files(),
        },
        Claim {
            what: "`cargo-fuzz` targets",
            pattern: "<N> `cargo-fuzz` targets",
            expected: fuzz_targets(),
        },
    ];

    check(&claims);
}

/// Source files carrying the generator's do-not-edit header.
fn generated_files() -> usize {
    let root = common::docs::workspace_root();
    let mut stack = vec![root.join("crates")];
    let mut n = 0;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !path.ends_with("tests") {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let head: String = text.lines().take(6).collect::<Vec<_>>().join(" ");
                n += usize::from(
                    head.contains("Do not edit") && head.contains("cargo xtask codegen"),
                );
            }
        }
    }
    n
}

/// The `cargo-fuzz` targets under `fuzz/fuzz_targets/`.
fn fuzz_targets() -> usize {
    std::fs::read_dir(common::docs::workspace_root().join("fuzz/fuzz_targets"))
        .map(|dir| {
            dir.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
                .count()
        })
        .unwrap_or(0)
}

/// `CEN_ASSERTIONS` in `en16931-formats`' conformance suite: the number of CEN
/// unit-test assertions that suite runs and asserts exactly.
fn cen_assertions() -> usize {
    let path = common::docs::workspace_root().join("crates/en16931-formats/tests/conformance.rs");
    let source =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let line = source
        .lines()
        .find(|l| l.starts_with("const CEN_ASSERTIONS: usize = "))
        .expect("the conformance suite declares CEN_ASSERTIONS");
    line.trim_start_matches("const CEN_ASSERTIONS: usize = ")
        .trim_end_matches(';')
        .replace('_', "")
        .parse()
        .expect("CEN_ASSERTIONS is an integer literal")
}

/// The family table is a partition of the registry of distinct ids: every
/// registered rule lands in exactly one bucket, the buckets sum to the
/// registry, and each is compared with the prose.
#[test]
fn the_family_table_partitions_the_registry() {
    /// Longest prefix first, so `BR-DE-` does not swallow `BR-DEC-` / `BR-DEX-`.
    const FAMILIES: &[(&str, &str)] = &[
        ("PEPPOL-EN16931-", "| `PEPPOL-EN16931-*` | <N> |"),
        ("PEPPOL-COMMON-", "| `PEPPOL-COMMON-*` | <N> |"),
        ("BR-DEC-", "| `BR-DEC-*` | <N> |"),
        ("BR-DEX-", "| `BR-DEX-*` | <N> |"),
        ("BR-DE-", "| `BR-DE-*` | <N> |"),
        ("BR-CO-", "| `BR-CO-*` | <N> |"),
        ("BR-CL-", "| `BR-CL-*` | <N> |"),
        ("BR-TMP-", "| `BR-TMP-*` | <N> |"),
        ("EN-", "| `EN-*` | <N> |"),
    ];
    /// Everything left over: `BR-01`…`BR-65` and the nine VAT category families.
    const REST: &str = "| `BR-*` and the nine VAT families | <N> |";

    let registered: BTreeSet<&str> = en16931::validation::rules::CORE
        .iter()
        .map(|r| r.id.as_str())
        .chain(
            en16931::profiles::ALL
                .iter()
                .flat_map(|p| p.extra_rules.iter().map(|r| r.id.as_str())),
        )
        .collect();

    let mut counts = vec![0usize; FAMILIES.len()];
    let mut rest = 0usize;
    for id in &registered {
        match FAMILIES.iter().position(|(p, _)| id.starts_with(p)) {
            Some(i) => counts[i] += 1,
            None => {
                assert!(
                    id.starts_with("BR-"),
                    "{id} belongs to no documented family — add a row for it"
                );
                rest += 1;
            }
        }
    }
    assert_eq!(
        counts.iter().sum::<usize>() + rest,
        registered.len(),
        "the buckets are not a partition"
    );

    let mut claims: Vec<Claim> = FAMILIES
        .iter()
        .zip(&counts)
        .map(|((prefix, pattern), n)| Claim {
            what: prefix,
            pattern,
            expected: *n,
        })
        .collect();
    claims.push(Claim {
        what: "BR-* and the nine VAT families",
        pattern: REST,
        expected: rest,
    });
    check(&claims);
}

/// A pasted sample report's header line is regenerated here, and some
/// documentation file must contain it verbatim.
#[test]
fn the_pasted_sample_reports_are_the_ones_the_code_prints() {
    use en16931::validation::Check;
    use en16931::{AdvancePayment, Invoice, examples, profiles, validate};

    // The billing page's final invoice: a valid core invoice carrying an
    // itemised advance, so `EN-EXT-01` warns.
    let mut final_invoice = examples::for_profile(&profiles::EN16931).expect("shipped");
    final_invoice
        .extensions
        .advance_payments
        .push(AdvancePayment {
            gross: final_invoice.totals.gross_total,
            received_on: None,
            tax: final_invoice.vat_breakdown.clone(),
            reference: None,
            reference_date: None,
        });

    let samples = [
        Check::new(&profiles::XRECHNUNG)
            .without("BR-DE-15")
            .run(&Invoice::default()),
        validate(&final_invoice),
        // The CLI's sample: an XRechnung invoice without BT-10.
        profiles::XRECHNUNG.validate(&{
            let mut inv = examples::for_profile(&profiles::XRECHNUNG).expect("shipped");
            inv.buyer_reference = None;
            inv
        }),
    ];

    for report in samples {
        // The header line carries every figure.
        let header = report
            .to_string()
            .lines()
            .next()
            .expect("a report always has a header")
            .to_owned();
        assert!(
            documentation()
                .iter()
                .any(|(_, text)| text.contains(&header)),
            "no documentation file contains the line this run prints:\n  {header}\n\
             A sample report was pasted and has since gone stale.\n{report}"
        );
    }
}

/// The matcher itself.
#[test]
fn the_matcher_finds_what_it_should_and_nothing_else() {
    assert_eq!(find_all("<N> business rules", "317 business rules"), [317]);
    assert_eq!(
        find_all("<N> business rules", "the 1 339 business rules here"),
        [1339],
        "a thousands separator is part of the number"
    );
    assert_eq!(
        find_all("<N> business rules", "the 1\u{a0}339 business rules here"),
        [1339],
        "…including a non-breaking space"
    );
    assert_eq!(
        find_all("<N> business rules", "the 1\u{202f}339 business rules here"),
        [1339],
        "…and the narrow one editors like to insert"
    );
    assert_eq!(
        find_all("<N> business rules", "317 business rulesets"),
        Vec::<usize>::new(),
        "a word boundary after the match, or `rulesets` counts as `rules`"
    );
    assert_eq!(
        find_all("<N> business rules", "the EN 16931 business rules"),
        Vec::<usize>::new(),
        "the standard's name ends in four digits and is not a count"
    );
    assert_eq!(
        find_all("<N> business rules", "EN 16931 has 317 business rules"),
        [317],
        "…but a real count in the same sentence still matches"
    );
    assert_eq!(
        find_all("<N> tables", "no number here"),
        Vec::<usize>::new()
    );
    assert_eq!(
        find_all(
            "Code lists — <N> ",
            "## Code lists — 4 887 values, generated"
        ),
        [4887],
        "a pattern ending in a space has made its own word boundary"
    );
    assert_eq!(
        find_all("about <N>%", "about 91% of them"),
        [91],
        "…and so has one ending in punctuation"
    );
    assert_eq!(
        find_all("a <N> b", "a 1 b and a 2 b"),
        [1, 2],
        "every occurrence, not just the first"
    );
}

/// The national-rules sentence on the site's profiles page, measured.
///
/// National = every assert id in Peppol's pinned Schematron of the form
/// `XX-R-*` / `XX-S-*`. A German one is covered when its KoSIT equivalent is
/// registered under XRechnung.
#[test]
fn national_rules_figures_are_measured() {
    let Some(root) = common::require("Peppol national-rule figures") else {
        return;
    };
    let path = root.join(common::peppol::UBL_SCH);
    let flags = common::peppol::flags(
        &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    );
    let national: Vec<(&String, &String)> = flags
        .iter()
        .filter(|(id, _)| common::peppol::is_national(id))
        .collect();
    // The release named in the pinned Schematron's header.
    let sch = std::fs::read_to_string(&path).expect("read the Peppol Schematron");
    let release = sch
        .split_once("release ")
        .and_then(|(_, rest)| {
            rest.split(|c: char| !(c.is_ascii_digit() || c == '.'))
                .next()
        })
        .map(|v| v.trim_end_matches('.'))
        .filter(|v| !v.is_empty())
        .expect("the Peppol Schematron header names its release");
    let page = std::fs::read_to_string(
        common::docs::workspace_root().join("site/content/docs/profiles.md"),
    )
    .expect("read the profiles page");
    assert!(
        page.contains(&format!("national rules in Peppol BIS Billing {release},")),
        "the profiles page's national-rules sentence does not name the pinned release {release}"
    );

    // `is_national` and "not `PEPPOL-*`" pick the same set.
    let neither: Vec<&String> = flags
        .keys()
        .filter(|id| !id.starts_with("PEPPOL-") && !common::peppol::is_national(id))
        .collect();
    assert!(
        neither.is_empty(),
        "ids that are neither PEPPOL-* nor national by shape: {neither:?}"
    );

    let xr = &en16931::profiles::XRECHNUNG;
    let mut covered = 0usize;
    let mut absent: Vec<(&str, &str)> = Vec::new();
    for (id, flag) in &national {
        if id.starts_with("DE-") {
            let eq = common::peppol::kosit_equivalent(id)
                .unwrap_or_else(|| panic!("{id}: no KoSIT equivalent by its numbering"));
            assert!(
                xr.check_ids().any(|c| c == eq),
                "{id} maps to {eq}, which XRechnung does not run"
            );
            covered += 1;
        } else {
            absent.push((&id[..2], flag.as_str()));
        }
    }
    let country = |c: &str| absent.iter().filter(|(k, _)| *k == c).count();
    let level = |l: &str| absent.iter().filter(|(_, f)| *f == l).count();
    assert_eq!(
        absent.len(),
        ["DK", "GR", "IS", "IT", "NL", "NO", "SE"]
            .iter()
            .map(|c| country(c))
            .sum::<usize>(),
        "a country appeared that the sentence does not list"
    );

    check(&[
        Claim {
            what: "Peppol national rules",
            pattern: "Of the **<N>** national rules",
            expected: national.len(),
        },
        Claim {
            what: "German national rules, covered",
            pattern: "the <N> German",
            expected: covered,
        },
        Claim {
            what: "national rules not implemented",
            pattern: "**<N> are not implemented**",
            expected: absent.len(),
        },
        Claim {
            what: "absent, Denmark",
            pattern: "(DK <N>,",
            expected: country("DK"),
        },
        Claim {
            what: "absent, Greece",
            pattern: "GR <N>, IS",
            expected: country("GR"),
        },
        Claim {
            what: "absent, Iceland",
            pattern: "IS <N>, IT",
            expected: country("IS"),
        },
        Claim {
            what: "absent, Italy",
            pattern: "IT <N>, NL",
            expected: country("IT"),
        },
        Claim {
            what: "absent, Netherlands",
            pattern: "NL <N>, NO",
            expected: country("NL"),
        },
        Claim {
            what: "absent, Norway",
            pattern: "NO <N>,",
            expected: country("NO"),
        },
        Claim {
            what: "absent, Sweden",
            pattern: "SE <N>;",
            expected: country("SE"),
        },
        Claim {
            what: "absent, fatal",
            pattern: "; <N> fatal and",
            expected: level("fatal"),
        },
        Claim {
            what: "absent, warning",
            pattern: "fatal and <N> warning",
            expected: level("warning"),
        },
    ]);
}
