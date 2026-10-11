//! `Display` helpers that never truncate or round a value.
//!
//! Unlike [`core::fmt::Formatter::pad`], which truncates to `precision`
//! characters (`{:.2}` on `1190.00` would print `11`):
//!
//! * [`padded`] honours width, fill and alignment and ignores precision (dates,
//!   codes, paths);
//! * [`number`] reads precision as a minimum number of fraction digits:
//!   `{:.4}` on `1190.00` is `1190.0000`, `{:.0}` is still `1190.00`. Round
//!   with [`rust_decimal::Decimal::round_dp`] if you need to.

use core::fmt;

/// Write `s` honouring width, fill and alignment, ignoring precision.
pub fn padded(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    let Some(width) = f.width() else {
        return f.write_str(s);
    };
    let len = s.chars().count();
    if len >= width {
        return f.write_str(s);
    }
    let pad = width - len;
    // Left by default, as `Formatter::pad` does.
    let (before, after) = match f.align() {
        Some(fmt::Alignment::Right) => (pad, 0),
        Some(fmt::Alignment::Center) => (pad / 2, pad - pad / 2),
        Some(fmt::Alignment::Left) | None => (0, pad),
    };
    let fill = f.fill();
    for _ in 0..before {
        f.write_str(fill.encode_utf8(&mut [0u8; 4]))?;
    }
    f.write_str(s)?;
    for _ in 0..after {
        f.write_str(fill.encode_utf8(&mut [0u8; 4]))?;
    }
    Ok(())
}

/// As [`padded`], reading precision as a minimum number of fraction digits.
///
/// `value` is the canonical rendering (`"1190.00"`, `"19"`, `"0.28901"`);
/// zeros are appended to reach the requested scale, nothing is removed.
pub fn number(f: &mut fmt::Formatter<'_>, value: &str) -> fmt::Result {
    let Some(want) = f.precision() else {
        return padded(f, value);
    };
    let have = value.split_once('.').map_or(0, |(_, frac)| frac.len());
    if have >= want {
        return padded(f, value);
    }
    let mut out = String::with_capacity(value.len() + (want - have) + 1);
    out.push_str(value);
    if have == 0 {
        out.push('.');
    }
    for _ in have..want {
        out.push('0');
    }
    padded(f, &out)
}

#[cfg(test)]
mod tests {
    use crate::{Date, InvoiceAmount, Percentage, Quantity, UnitPriceAmount};
    use rust_decimal::dec;

    /// `{:.n}` is a minimum scale, never a truncation.
    #[test]
    fn precision_never_truncates_a_value() {
        let a = InvoiceAmount::parse("1190.00").expect("amount");
        assert_eq!(format!("{a:.2}"), "1190.00");
        assert_eq!(format!("{a:.3}"), "1190.000");
        assert_eq!(format!("{a:.0}"), "1190.00", "precision is a minimum");
        assert_eq!(format!("{a:>12.4}"), "   1190.0000");

        let d = Date::parse("2026-07-31").expect("date");
        assert_eq!(format!("{d:.4}"), "2026-07-31");
        assert_eq!(format!("{d:.2}"), "2026-07-31");
    }

    /// Padding reaches the requested scale on every numeric type.
    #[test]
    fn precision_pads_to_the_requested_scale() {
        assert_eq!(format!("{:.4}", Percentage::new(dec!(19))), "19.0000");
        assert_eq!(format!("{:.2}", Quantity::new(dec!(10000))), "10000.00");
        assert_eq!(
            format!("{:.7}", UnitPriceAmount::new(dec!(0.28901))),
            "0.2890100"
        );
        // …and never reduces one that is already longer.
        assert_eq!(
            format!("{:.2}", UnitPriceAmount::new(dec!(0.28901))),
            "0.28901"
        );
    }

    #[test]
    fn width_fill_and_alignment_still_behave() {
        let a = InvoiceAmount::parse("1.00").expect("amount");
        assert_eq!(format!("{a:>10}"), "      1.00");
        assert_eq!(format!("{a:<10}"), "1.00      ");
        assert_eq!(format!("{a:^10}"), "   1.00   ");
        assert_eq!(format!("{a:*>10}"), "******1.00");
        assert_eq!(format!("{a}"), "1.00", "no width, no padding");
        // A width narrower than the value never clips it.
        assert_eq!(format!("{a:>2}"), "1.00");
        // A multi-byte fill is one character wide, not one byte.
        assert_eq!(format!("{a:—>8}"), "————1.00");
    }
}
