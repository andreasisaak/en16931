+++
title = "Contributing"
weight = 10
description = "Set up the en16931 workspace: fetch the CEN, KoSIT and Peppol artefacts, run the full CI gate locally, and regenerate the tables derived from the standard."
+++

Three crates, one workspace, one version. [`just`](https://just.systems) is the
task runner; `just` alone lists every recipe.

```sh
git clone https://github.com/hupe1980/en16931
cd en16931

just references      # fetch the CEN / KoSIT / Peppol artefacts into the reference corpus — once
just ci              # everything CI runs, locally
```

## The artefacts

The reference corpus is **not committed**. The CEN artefacts are EUPL-1.2, a reciprocal
licence, and keeping them out of the tree is what keeps every crate here
`MIT OR Apache-2.0`.

The suites that need it skip without it, so `just references` is the first thing to
run. CI sets `EN16931_REQUIRE_SPEC=1`, which turns a skip into a **failure**;
`just test-artefacts` does the same locally.

Every source is pinned in `xtask/src/fetch.rs`. `cargo xtask watch` reports, as
JSON, every pin its source has moved past, tagged or not; the weekly *Upstream
watch* workflow keeps one issue open per moved source.

## Test-time tools

Two suites drive an external validator. Neither is a dependency of any crate;
each skips with a printed line when its tool is missing, and fails under
`EN16931_REQUIRE_SPEC=1`, as in CI.

| Tool | Suite | What it checks |
|---|---|---|
| `xmllint` (libxml2) | `schema_gate` | every CII and UBL document the suites write against its schema |
| Docker, with a running daemon | `zugferd_embed` | every PDF `zugferd::embed` writes, with veraPDF (PDF/A-3b) — the pinned `verapdf/rest` image, started once per run through `testcontainers` and pulled on first use (linux/amd64; emulated on Apple silicon) |

## The recipes worth knowing

| | |
|---|---|
| `just test` | unit and doc tests, default features — the fast loop |
| `just test-all` | every crate, every feature |
| `just test-artefacts` | as above, but a missing reference corpus fails |
| `just lint` | Clippy over every target and feature, warnings as errors |
| `just doc` | rustdoc with `-D warnings`, which is what catches a broken intra-doc link |
| `just wasm` | `en16931` still builds for `wasm32` |
| `just deps` | every documented dependency-graph size, measured |
| `just codegen-check` | fail if a generated table drifted from the artefacts |
| `just bench` | the criterion benchmarks |
| `just site` | build this site with [Zola](https://www.getzola.org) |
| `just features` | Clippy over every feature combination a consumer can select |
| `just tracked` | fail if a source file is gitignored, or missing from git |
| `just audit` | security advisories over `Cargo.lock`; nothing is ignored |
| `just ci` | all of the above that CI runs |
| `just fuzz <target> [seconds]` | one cargo-fuzz target (`read`, `extract`, `roundtrip`, `embed`); needs nightly and `cargo-fuzz` |
| `just mutants-diff` | mutate the lines your change touches; fails on a survivor the baseline does not record |
| `just mutants` | the full mutation sweep of the rules — optional and slow |

A few notes:

- **`just deps`** measures the dependency-graph sizes the documentation quotes.
  Raising one is a decision: the small graph is why `en16931` builds for
  `wasm32`.
- **`just features`** lints every feature combination, because `--all-features`
  cannot see code that is dead or does not compile without a feature.
- **`just tracked`** runs locally only: a checkout has only the tracked files,
  so a needed file that `.gitignore` hides is invisible to CI. Anchor ignore
  patterns (`/NOTES.md`, not `NOTES.md`).
- **`just wasm`** builds `-p en16931` only; the formats crate's PDF parser does
  not build for that target.
- **Mutation testing** is local and optional: `just mutants-diff` mutates the
  lines your change touches; `just mutants` is the full, slow sweep of the rules.
  It is not run in CI.
- **Fuzzing** runs in CI for 60 seconds per target; `just fuzz <target> 3600`
  is a long local run.

## The documentation is tested

Measured figures — rule counts, the coverage split, profile check counts,
code-list totals, conformance counts — are read back out of every README, every
source file and every page of this site, and compared against the code that
produces them (`crates/en16931/tests/documented_numbers.rs`). Reword a sentence
holding one and the test says its pattern stopped matching. Documentation
describes the current state only; history belongs in the
[changelog](https://github.com/hupe1980/en16931/blob/main/CHANGELOG.md).

## No advisory is set aside

`just audit` runs `cargo audit --deny warnings` over `Cargo.lock`, and nothing
is ignored: there is no `.cargo/audit.toml`. A security advisory or a yanked
crate anywhere in the lockfile fails it, and CI runs the same command. The fix
is a dependency update, not an ignore entry.

## Generated code

The workspace has 7 generated files, derived from the artefacts and none written
by hand: the code lists for `en16931`, and the element-order, prohibition and single-occurrence
tables for each syntax in `en16931-formats`.

```sh
cargo xtask codegen     # regenerate every generated file
cargo xtask check       # fail if a committed one differs
```

Each generator exits non-zero rather than emitting a table it could not derive
cleanly. If you change one, change the generator — a hand-edit will be reverted
by the next `codegen` run and caught by CI in between.

## Adding a rule

Every registered rule must have a fixture that makes it **fire**, or be declared
in `tests/corpus.rs` as `Why::Retired` (the type system makes the failure
unrepresentable) or `Why::Undecidable` (CEN's own Schematron binds it to
`true()` — exactly `BR-CO-05` … `BR-CO-08`). The coverage gate fails if a rule
is uncovered and undeclared, if a declared rule has since become covered, and if
anything else is declared undecidable.

A rule nobody has seen fire may be inverted, unreachable, or checking the wrong
field, and a suite of valid documents would be green either way.

If the rule is this crate's own rather than an authority's, namespace it `EN-*`.
A `BR-` id that CEN did not publish is indistinguishable from one that it did,
and someone will eventually go looking for it in CEN's index.

## Releasing

All three crates share one version, so one tag releases all of them, and
[`CHANGELOG.md`](https://github.com/hupe1980/en16931/blob/main/CHANGELOG.md)
carries one entry for the three:

```sh
# 1. move the Unreleased section of CHANGELOG.md under the new version
# 2. bump `[workspace.package] version` and the two dependency requirements
git tag "v$(cargo pkgid -p en16931 | sed 's/.*#//')" && git push --tags
```

They publish in dependency order: `en16931`, then `en16931-formats`, then
`en16931-cli`. The changelog is the one document the [documented-number
suite](#the-documentation-is-tested) does not scan, because it records
superseded figures on purpose.

## This site

[Zola](https://www.getzola.org), in `site/`.

```sh
just site-serve      # localhost with live reload
just site            # build into site/public
```

It deploys to GitHub Pages from `main` on every push that touches `site/`.
