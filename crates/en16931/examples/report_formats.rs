//! Turn a report into something another system can read.
//!
//! ```sh
//! cargo run --example report_formats --features serde,svrl
//! ```
//!
//! * `Report`: the versioned JSON interchange shape, for storing reports
//!   (`ValidationReport`'s own `Serialize` follows its internal layout).
//! * SVRL, as Schematron tools emit it. Its `location` holds a business-term
//!   path, since there is no source document.

use en16931::{Invoice, profiles};

fn main() {
    let report = profiles::XRECHNUNG.validate(&Invoice::default());

    let interchange = en16931::Report::of(&report);
    {
        let json = serde_json::to_string_pretty(&interchange).expect("serialise");
        // Only the head.
        println!("── JSON ({}) ──", en16931::report::SCHEMA);
        for line in json.lines().take(24) {
            println!("{line}");
        }
        println!("  …\n");
    }

    {
        println!("── SVRL ──");
        for line in en16931::svrl::to_svrl(&interchange).lines().take(14) {
            println!("{line}");
        }
        println!("  …");
    }
}
