//! [`Identifier`] and [`DocumentReference`] — EN 16931 §6.5.6 and §6.5.7.
//!
//! Two distinct semantic types: an identifier may carry a scheme, a document
//! reference never does.

use core::fmt;

/// An identifier — EN 16931-1 §6.5.6 `Identifier. Type`.
///
/// Three components, per Table 20:
///
/// | Component | Use | Example |
/// |---|---|---|
/// | Content | mandatory | `abc:123-DEF` |
/// | Scheme identifier | conditional | `GLN` |
/// | Scheme version identifier | conditional | `1.0` |
///
/// A missing scheme is representable so BR-62 … BR-65 (BT-34, BT-49, BT-157,
/// BT-158) can report it.
///
/// ```
/// use en16931::Identifier;
///
/// // A Peppol electronic address: scheme is the CEF EAS code, not free text.
/// let endpoint = Identifier::schemed("9958:DE123456789", "0204");
/// assert_eq!(endpoint.scheme(), Some("0204"));
/// assert_eq!(endpoint.to_string(), "0204:9958:DE123456789");
///
/// // A bare identifier is representable; whether it is *valid* is a rule.
/// assert_eq!(Identifier::new("INV-2026-001").scheme(), None);
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identifier {
    content: String,
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    scheme: Option<String>,
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    scheme_version: Option<String>,
}

impl Identifier {
    /// An identifier with no scheme.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            scheme: None,
            scheme_version: None,
        }
    }

    /// An identifier with a scheme identifier.
    pub fn schemed(content: impl Into<String>, scheme: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            scheme: Some(scheme.into()),
            scheme_version: None,
        }
    }

    /// BT-34 / BT-49 — an electronic address with its EAS scheme checked
    /// (`BR-CL-25`), and its value too where the scheme's format is checkable.
    ///
    /// ```
    /// use en16931::Identifier;
    /// use en16931::codes::guard;
    ///
    /// let ok = Identifier::eas("4012345000009", "0088")?;      // a real GLN
    /// assert_eq!(ok.scheme(), Some("0088"));
    ///
    /// // The Leitweg-ID scheme, withdrawn on 2023-07-31.
    /// let err = Identifier::eas("991-01234-56", "9958").unwrap_err();
    /// assert!(err.to_string().contains("use 0204 instead"));
    ///
    /// // A BDEW Marktlokations-ID is eleven digits, not a GLN.
    /// let err = Identifier::eas("51238696781", "0088").unwrap_err();
    /// assert!(err.to_string().contains("13 digits"), "{err}");
    ///
    /// // A scheme with no shape check accepts any value, and says so.
    /// assert!(Identifier::eas("991-01234-56", "0204").is_ok());
    /// assert!(!guard::eas_value_is_checkable("0204"));
    /// # Ok::<(), en16931::codes::guard::CodeError>(())
    /// ```
    ///
    /// The value is checked only for [`guard::CHECKED_EAS_SCHEMES`]; for other
    /// schemes `Ok` means only that the scheme is a live EAS code
    /// ([`guard::eas_value_is_checkable`] tells which).
    ///
    /// [`guard::CHECKED_EAS_SCHEMES`]: crate::codes::guard::CHECKED_EAS_SCHEMES
    /// [`guard::eas_value_is_checkable`]: crate::codes::guard::eas_value_is_checkable
    ///
    /// # Errors
    /// [`CodeError`](crate::codes::guard::CodeError) when `scheme` is not in the
    /// CEF EAS code list (carrying the successor of a withdrawn code where the
    /// authority names one), or when it is a scheme this crate can check and
    /// the content does not have its shape.
    pub fn eas(
        content: impl Into<String>,
        scheme: &str,
    ) -> Result<Self, crate::codes::guard::CodeError> {
        let content = content.into();
        let scheme = crate::codes::guard::eas(scheme)?;
        crate::codes::guard::eas_value(scheme.as_str(), &content)?;
        Ok(Self::schemed(content, scheme.as_str()))
    }

    /// Attach a scheme version identifier.
    #[must_use]
    pub fn with_scheme_version(mut self, version: impl Into<String>) -> Self {
        self.scheme_version = Some(version.into());
        self
    }

    /// The identifier value.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// The scheme identifier, if any — BR-62 … BR-65 require one on four terms.
    #[must_use]
    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }

    /// The scheme version identifier, if any.
    #[must_use]
    pub fn scheme_version(&self) -> Option<&str> {
        self.scheme_version.as_deref()
    }

    /// Whether the content is empty or whitespace only.
    ///
    /// §6.5: a business term that is used shall have content.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.content.trim().is_empty()
    }
}

impl fmt::Display for Identifier {
    /// `scheme:content` when a scheme is present, otherwise the bare content.
    ///
    /// A diagnostic rendering, not a wire format.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.scheme {
            Some(s) => write!(f, "{s}:{}", self.content),
            None => f.write_str(&self.content),
        }
    }
}

impl From<&str> for Identifier {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Identifier {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

/// A reference to another document — EN 16931-1 §6.5.7 `Document Reference. Type`.
///
/// Content only, no scheme (Table 21). Used by BT-12, BT-13, BT-14, BT-16,
/// BT-25, BT-122 and others.
///
/// ```
/// use en16931::DocumentReference;
///
/// let po = DocumentReference::new("PO-4711");
/// assert_eq!(po.as_str(), "PO-4711");
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocumentReference(String);

impl DocumentReference {
    /// Wrap a reference value.
    pub fn new(content: impl Into<String>) -> Self {
        Self(content.into())
    }

    /// The reference value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the content is empty or whitespace only.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Display for DocumentReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        crate::fmt::padded(f, &self.0)
    }
}

impl From<&str> for DocumentReference {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for DocumentReference {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_carries_all_three_components() {
        let id = Identifier::schemed("123456789", "0088").with_scheme_version("1.0");
        assert_eq!(id.content(), "123456789");
        assert_eq!(id.scheme(), Some("0088"));
        assert_eq!(id.scheme_version(), Some("1.0"), "the forgotten component");
    }

    #[test]
    fn a_missing_scheme_is_representable_so_it_can_be_reported() {
        // BR-62 … BR-65 report it, so it must be representable.
        let bare = Identifier::new("DE123456789");
        assert_eq!(bare.scheme(), None);
        assert_eq!(bare.to_string(), "DE123456789");
    }

    #[test]
    fn blank_content_is_detectable() {
        assert!(Identifier::new("   ").is_blank());
        assert!(!Identifier::new("x").is_blank());
        assert!(DocumentReference::new("").is_blank());
    }

    #[test]
    fn document_reference_has_no_scheme_by_construction() {
        let r = DocumentReference::new("PO-4711");
        assert_eq!(r.as_str(), "PO-4711");
        assert_eq!(r.to_string(), "PO-4711");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_omits_absent_components() {
        let bare = serde_json::to_string(&Identifier::new("X")).unwrap();
        assert_eq!(bare, r#"{"content":"X"}"#);
        let full = serde_json::to_string(&Identifier::schemed("X", "0088")).unwrap();
        assert_eq!(full, r#"{"content":"X","scheme":"0088"}"#);
        assert_eq!(
            serde_json::to_string(&DocumentReference::new("PO-1")).unwrap(),
            r#""PO-1""#
        );
    }
}
