//! Which edition of EN 16931-1 a profile is a usage specification of.
//!
//! The edition is a property of the profile, not of the [`crate::Invoice`]
//! model. [`Edition`] carries no term-to-edition map; [`Edition::En2026`] is a
//! classification only, and [`Edition::is_implemented`] is `false` for it.

use core::fmt;

/// An edition of EN 16931-1.
///
/// `#[non_exhaustive]`, so a downstream `match` needs a wildcard arm.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[non_exhaustive]
pub enum Edition {
    /// EN 16931-1:2017 + A1:2019 (+ AC:2020) — the default, and the edition of
    /// every shipped profile.
    #[default]
    En2017A1,
    /// EN 16931-1:2026. No shipped profile targets it.
    En2026,
}

impl Edition {
    /// The edition's designation, as CEN writes it.
    #[must_use]
    pub const fn designation(self) -> &'static str {
        match self {
            Self::En2017A1 => "EN 16931-1:2017+A1:2019",
            Self::En2026 => "EN 16931-1:2026",
        }
    }

    /// Whether this crate has a rule set for the edition; `false` for
    /// [`En2026`](Self::En2026).
    #[must_use]
    pub const fn is_implemented(self) -> bool {
        matches!(self, Self::En2017A1)
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        crate::fmt::padded(f, self.designation())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_deployed_edition() {
        assert_eq!(Edition::default(), Edition::En2017A1);
        assert!(Edition::En2017A1.is_implemented());
    }

    /// No shipped profile declares an edition without a rule set.
    #[test]
    fn en2026_is_classified_but_not_implemented() {
        assert!(!Edition::En2026.is_implemented());
        assert_eq!(Edition::En2026.designation(), "EN 16931-1:2026");
        for p in crate::profiles::ALL {
            assert!(
                p.edition.is_implemented(),
                "{} declares {} — but this crate has no rule set for it",
                p.id,
                p.edition
            );
        }
    }

    #[test]
    fn editions_order_oldest_first() {
        assert!(Edition::En2017A1 < Edition::En2026);
    }
}
