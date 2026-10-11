# en16931-cli

**The EN 16931 e-invoice validator as a command.** UBL, CII and ZUGFeRD /
Factur-X in; a decision out — *accept*, *reject* or *undefined*, the three
answers KoSIT's validator gives — as text, JSON or SVRL. One static binary, no
JVM, no network.

```sh
cargo install en16931-cli      # the binary is `en16931`
```

```console
$ en16931 validate rechnung.xml
rechnung.xml
recommendation: REJECT
  because under the declared XRechnung 3.0: 1 fatal finding(s): BR-DE-15
declared: XRechnung 3.0
…
semantic   INVALID
  XRechnung 3.0 validation (EN 16931-1:2017+A1:2019) — 282 rule(s) checked, 1 finding(s), INVALID
    fatal        [BR-DE-15] BT-10 — Buyer reference (BT-10) shall be present
$ echo $?
1
```

## Commands

| Command | Does |
|---|---|
| `validate` | receive documents: validate each as its BT-24 declares, optionally `--require` a profile, and decide |
| `convert` | convert between UBL and CII through the model; refuses a lossy conversion unless `--allow-loss` |
| `extract` | the XML payload of a ZUGFeRD / Factur-X PDF, verbatim |
| `embed` | embed an invoice into a PDF/A-3 file: a ZUGFeRD / Factur-X hybrid |
| `diff` | compare two documents as invoices, not as XML |
| `inspect` | what a document is, without validating it |
| `explain` | look a rule up by id, with the profiles that run it |
| `profiles` | the profiles this build validates against, and their artefact releases |
| `categories` | whether VAT categories may share one invoice |
| `rules` | the whole rule catalogue, as text or JSON |
| `generate` | shell completions and a man page |

## Exit codes

| | `validate` |
|---|---|
| `0` | every document is accepted |
| `1` | a document is rejected (including bytes that are not an invoice), or undefined (ZUGFeRD MINIMUM / BASIC WL) |
| `2` | a file could not be opened, or the command was misused |

## Documentation

Every command, flag and output format:
[the command line](https://hupe1980.github.io/en16931/docs/cli/).

## Licence

MIT OR Apache-2.0, at your option. The rules and code lists are derived from
CEN's validation artefacts; every report carries the CEN attribution notice.
