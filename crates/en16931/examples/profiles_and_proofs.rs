//! Validate against a profile, and turn a clean run into a **typed proof**.
//!
//! ```sh
//! cargo run --example profiles_and_proofs
//! ```
//!
//! A profile runs the core rules plus its own (XRechnung adds `BR-DE-*`).
//! `Validated<P>` can only be built by a clean run against `P`, so code that
//! takes one cannot receive an unchecked invoice.

use en16931::profiles::{self, XRechnung};
use en16931::validation::profile::Validated;
use en16931::{Invoice, validate};

fn main() {
    let invoice = Invoice::default();

    // Every profile the crate knows, and how many rules each checks.
    let width = profiles::ALL
        .iter()
        .map(|p| p.id.len())
        .chain(std::iter::once("profile".len()))
        .max()
        .unwrap_or(24);

    println!("{:<width$} {:>6} {:>9}", "profile", "rules", "findings");
    for profile in profiles::ALL {
        let report = profile.validate(&invoice);
        println!(
            "{:<width$} {:>6} {:>9}",
            profile.id,
            report.rules_checked(),
            report.findings().len()
        );
    }

    println!(
        "\nthe bare core model:  {} rules",
        validate(&invoice).rules_checked()
    );

    // An empty invoice cannot produce a proof; the rejection returns the
    // invoice and the report.
    println!("\n── asking for a proof of XRechnung conformance ──");
    match Validated::<XRechnung>::new(invoice) {
        Ok(proof) => println!("proved: {:?}", proof.invoice().number),
        Err(rejected) => {
            let report = rejected.report();
            println!("refused, with {} finding(s):", report.findings().len());

            // Findings are ordered by business-term path, not by rule id.
            const SHOWN: usize = 8;
            let shown = &report.findings()[..SHOWN.min(report.findings().len())];
            let w = shown.iter().map(|f| f.rule.len()).max().unwrap_or(0);
            for f in shown {
                println!("  {:<w$} {}", f.rule, f.path);
            }
            if let Some(rest) = report.findings().len().checked_sub(SHOWN)
                && rest > 0
            {
                println!("  … and {rest} more, in business-term order");
            }
            println!("\nThe invoice comes back with the report, so a caller can fix and retry.");
        }
    }
}
