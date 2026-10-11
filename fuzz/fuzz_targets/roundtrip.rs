//! A generated invoice (from `crates/en16931/tests/generators`, seeded from the
//! fuzz input via [`seed`]) with a fixed header, written as UBL and as CII and
//! read back.
//!
//! Asserts, per syntax: `write_unchecked` refuses (`Unwritable`) or writes;
//! what it writes reads back as its own syntax; and when nothing is reported,
//! the invoice read back equals the one written (as canonical JSON).
#![no_main]

#[allow(dead_code, unused_imports)]
#[path = "../../crates/en16931/tests/generators/mod.rs"]
mod generators;

use en16931::Invoice;
use en16931_formats::Syntax;
use libfuzzer_sys::fuzz_target;
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

fuzz_target!(|data: &[u8]| {
    let rng = TestRng::from_seed(RngAlgorithm::ChaCha, &seed(data));
    let mut runner = TestRunner::new_with_rng(Config::default(), rng);
    let Ok(tree) = generators::any_invoice().new_tree(&mut runner) else {
        return;
    };
    let mut inv = tree.current();
    inv.number = Some("RE-1".to_owned());
    inv.issue_date = Some(en16931::Date::parse("2026-01-15").expect("date"));
    inv.seller.name = Some("Seller".to_owned());
    inv.buyer.name = Some("Buyer".to_owned());
    for syntax in [Syntax::Ubl, Syntax::Cii] {
        roundtrip(&inv, syntax);
    }
});

/// A ChaCha seed from the whole input (FNV-1a, widened by xorshift).
///
/// Seeding, rather than proptest's pass-through RNG, keeps every generation
/// finite: that RNG yields zeros once exhausted, on which `rand`'s rejection
/// sampling never terminates.
fn seed(data: &[u8]) -> [u8; 32] {
    let mut x = data.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    }) | 1;
    let mut out = [0u8; 32];
    for chunk in out.chunks_mut(8) {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        chunk.copy_from_slice(&x.to_le_bytes());
    }
    out
}

fn roundtrip(inv: &Invoice, syntax: Syntax) {
    let Ok(out) = en16931_formats::write_unchecked(inv, syntax) else {
        return;
    };
    let read = match en16931_formats::read(out.xml.as_bytes()) {
        Ok(read) => read,
        Err(e) => panic!(
            "{syntax:?}: the writer emitted what its reader refuses: {e}\n{}",
            out.xml
        ),
    };
    assert_eq!(read.syntax, syntax);
    if out.entries.is_empty() && read.entries.is_empty() {
        let (a, b) = (canonical(inv), canonical(&read.invoice));
        assert!(
            a == b,
            "{syntax:?}: nothing was reported and the invoice changed\nwrote {a}\nread  {b}\n{}",
            out.xml
        );
    }
}

/// The invoice as JSON, in the two normalisations the readers document: text
/// is trimmed (`str::trim`) and blank text compares as `null`; each party's
/// identifiers (BT-29, BT-46) are sorted, since CII's split fixes an order
/// EN 16931 gives no meaning.
fn canonical(inv: &Invoice) -> serde_json::Value {
    let mut v = serde_json::to_value(inv).expect("serialise");
    trim(&mut v);
    for party in ["seller", "buyer"] {
        // `Invoice` serialises as its versioned envelope, `{"en16931", "invoice"}`.
        if let Some(ids) = v
            .pointer_mut(&format!("/invoice/{party}/identifiers"))
            .and_then(serde_json::Value::as_array_mut)
        {
            ids.sort_by_key(ToString::to_string);
        }
    }
    v
}

fn trim(v: &mut serde_json::Value) {
    use serde_json::Value;
    match v {
        Value::String(s) => {
            // `str::trim`, as the readers do: Unicode white space, not only XML's.
            let t = s.trim();
            *v = if t.is_empty() {
                Value::Null
            } else {
                Value::String(t.to_owned())
            };
        }
        Value::Array(items) => items.iter_mut().for_each(trim),
        Value::Object(fields) => fields.values_mut().for_each(trim),
        _ => {}
    }
}
