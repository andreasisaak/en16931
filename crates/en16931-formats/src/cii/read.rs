//! Reading UN/CEFACT CII D16B into the semantic model.
//!
//! Every element walked is mapped or recorded as
//! [`Unmapped`](crate::report::Kind::Unmapped) (`Parent/Child`). A value the
//! model cannot hold is recorded as [`Malformed`](crate::report::Kind::Malformed)
//! and the field left absent.
//!
//! `ram:ID` and `ram:GlobalID` are both read into `Party::identifiers`. BT-7
//! and BT-8, repeated on each tax breakdown entry, collapse to the document
//! level; BT-8 is translated from UNTDID 2475 to 2005 (see [`super`]).

use std::collections::BTreeSet;

use en16931::invoice::*;
use en16931::{Date, DocumentKind, DocumentReference, Identifier, InvoiceAmount, Quantity};

use crate::report::{Entry, Kind};
use crate::xml::{Node, kid, kids, name};

/// Reads one `rsm:CrossIndustryInvoice` into an [`Invoice`].
#[derive(Default)]
pub(crate) struct Reader {
    /// Element paths encountered and not mapped, as `Parent/Child`, each once.
    unmapped: BTreeSet<String>,
    /// Everything else the reader has to say, in document order.
    entries: Vec<Entry>,
    /// The checks of the declared profile ([`crate::xml::declared_checks`]).
    checks: BTreeSet<&'static str>,
}

/// Trimmed text of this element, empty when there is none.
fn own_text(n: Node<'_, '_>) -> String {
    crate::xml::text(n).unwrap_or_default()
}

/// Trimmed text of the first child named `want`, or `None` when the element is
/// absent or empty ([`crate::xml::child_text`], shared with the UBL reader).
fn text(n: Node<'_, '_>, want: &str) -> Option<String> {
    crate::xml::child_text(n, want)
}

/// `None` for an empty string.
fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

fn code(n: Node<'_, '_>, want: &str) -> Option<Code> {
    text(n, want).map(Code::new)
}

/// An `Identifier` from an element with an optional `@schemeID`.
fn identifier(n: Node<'_, '_>) -> Identifier {
    let content = own_text(n);
    match n.attribute("schemeID").or_else(|| n.attribute("listID")) {
        Some(s) => Identifier::schemed(content, s),
        None => Identifier::new(content),
    }
}

impl Reader {
    /// Note an element this reader does not map.
    fn push_path(&mut self, parent: &str, child: &str) {
        self.unmapped.insert(format!("{parent}/{child}"));
    }

    /// Note a combination the model cannot hold, at `path`.
    fn not_held(&mut self, path: &str, message: &str) {
        self.entries
            .push(Entry::new(Kind::Unmapped, path, message.to_owned()));
    }

    /// Report a required amount or quantity absent at `path` below `n`.
    fn require(&mut self, n: Node<'_, '_>, path: &[&str], term: &str, rule: &str) {
        self.entries
            .extend(crate::xml::absent(n, path, (term, rule), &self.checks));
    }

    /// The text of `n`'s child `want` as a decimal, or `None`: absent, or
    /// present and not an `xs:decimal` the model can hold, which is reported.
    fn decimal(&mut self, n: Node<'_, '_>, want: &str) -> Option<rust_decimal::Decimal> {
        let raw = text(n, want)?;
        match crate::xml::xs_decimal(&raw) {
            Ok(d) => Some(d),
            Err(e) => {
                self.malformed(format!("{}/{want}", name(n)), e, raw);
                None
            }
        }
    }

    /// Note a value the model cannot hold.
    fn malformed(&mut self, path: String, message: impl Into<String>, value: String) {
        self.entries
            .push(Entry::new(Kind::Malformed, path, message).value(value));
    }

    /// Everything the reader has to say: its own entries in document order, then
    /// each unmapped path once, sorted.
    pub(crate) fn finish(self) -> Vec<Entry> {
        let mut out = self.entries;
        out.extend(self.unmapped.into_iter().map(|path| {
            Entry::new(
                Kind::Unmapped,
                path,
                "not mapped: outside the EN 16931 subset, or an extension the declared \
                 profile does not define",
            )
        }));
        out
    }

    fn amount(&mut self, n: Node<'_, '_>, want: &str) -> Option<InvoiceAmount> {
        let raw = text(n, want)?;
        match InvoiceAmount::parse(&raw) {
            Ok(a) => Some(a),
            Err(e) => {
                self.malformed(format!("{}/{want}", name(n)), e.to_string(), raw);
                None
            }
        }
    }

    /// `<ram:Foo><udt:DateTimeString format="102">20260115</udt:DateTimeString></ram:Foo>`
    ///
    /// `ram:TaxPointDate` (BT-7) carries a `udt:DateString` instead. Either
    /// child is read on any wrapper; a format other than `102` is reported, not
    /// guessed at.
    fn date(&mut self, n: Node<'_, '_>, want: &str) -> Option<Date> {
        let wrapper = kid(n, want)?;
        let s = kid(wrapper, "DateTimeString").or_else(|| kid(wrapper, "DateString"))?;
        let path = format!("{}/{want}/{}", name(n), name(s));
        let raw = own_text(s);
        // D16B types BT-7 `udt:DateType` (`udt:DateString`) and every other
        // date `udt:DateTimeString`. The other spelling is read and reported.
        let expected = if want == "TaxPointDate" {
            "DateString"
        } else {
            "DateTimeString"
        };
        if name(s) != expected {
            self.entries.push(
                Entry::new(
                    Kind::Schema,
                    path.clone(),
                    format!(
                        "the D16B schema has a {expected} here; this spelling is read and \
                         reported"
                    ),
                )
                .value(raw.clone()),
            );
        }
        match s.attribute("format") {
            Some(super::write::DATE_FORMAT) | None => {}
            Some(other) => {
                self.malformed(
                    path,
                    format!("format {other:?}; only 102 (CCYYMMDD) is permitted"),
                    raw,
                );
                return None;
            }
        }
        if raw.len() != 8 || !raw.bytes().all(|b| b.is_ascii_digit()) {
            self.malformed(path, "not CCYYMMDD", raw);
            return None;
        }
        let f = format!("{}-{}-{}", &raw[0..4], &raw[4..6], &raw[6..8]);
        match Date::parse(&f) {
            Ok(d) => Some(d),
            Err(e) => {
                self.malformed(path, e.to_string(), raw);
                None
            }
        }
    }

    fn note(&mut self, parent: &str, n: Node<'_, '_>) {
        for c in kids(n) {
            if !matches!(name(c), "Content" | "SubjectCode") {
                self.push_path(parent, name(c));
            }
        }
    }

    /// Read the document element.
    pub(crate) fn read(&mut self, root: Node<'_, '_>) -> Invoice {
        let mut inv = Invoice::default();
        self.checks = crate::xml::declared_checks(
            kid(root, "ExchangedDocumentContext")
                .and_then(|c| kid(c, "GuidelineSpecifiedDocumentContextParameter"))
                .and_then(|g| text(g, "ID"))
                .as_deref(),
        );
        for c in kids(root) {
            match name(c) {
                "ExchangedDocumentContext" => self.context(&mut inv, c),
                "ExchangedDocument" => self.document(&mut inv, c),
                "SupplyChainTradeTransaction" => self.transaction(&mut inv, c),
                other => self.push_path("CrossIndustryInvoice", other),
            }
        }
        // The kind comes from BT-3 against the whole UNTDID 1001 credit-note
        // list (`381`, `396`, `532`, `83`, …). `81` is on both lists and is
        // read as a credit note, as `PEPPOL-EN16931-P0101` expects.
        if inv
            .type_code
            .as_ref()
            .is_some_and(|c| c.is_in(en16931::codes::generated::CREDIT_NOTE_TYPE_CODES))
        {
            inv.kind = DocumentKind::CreditNote;
        }
        inv
    }

    fn context(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        for c in kids(n) {
            match name(c) {
                "BusinessProcessSpecifiedDocumentContextParameter" => {
                    inv.business_process = text(c, "ID");
                }
                "GuidelineSpecifiedDocumentContextParameter" => {
                    inv.specification_id = text(c, "ID");
                }
                other => self.push_path("ExchangedDocumentContext", other),
            }
        }
    }

    fn document(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        for c in kids(n) {
            match name(c) {
                "ID" => inv.number = non_empty(own_text(c)),
                "TypeCode" => inv.type_code = Some(Code::new(own_text(c))),
                "IssueDateTime" => inv.issue_date = self.date(n, "IssueDateTime"),
                "IncludedNote" => {
                    self.note("IncludedNote", c);
                    inv.notes.push(InvoiceNote {
                        subject_code: code(c, "SubjectCode"),
                        note: text(c, "Content"),
                    });
                }
                other => self.push_path("ExchangedDocument", other),
            }
        }
    }

    fn transaction(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        for c in kids(n) {
            match name(c) {
                "IncludedSupplyChainTradeLineItem" => inv.lines.push(self.line(c)),
                "ApplicableHeaderTradeAgreement" => self.agreement(inv, c),
                "ApplicableHeaderTradeDelivery" => self.delivery(inv, c),
                "ApplicableHeaderTradeSettlement" => self.settlement(inv, c),
                other => self.push_path("SupplyChainTradeTransaction", other),
            }
        }
        for (want, term, rule) in [
            ("LineTotalAmount", "BT-106", "BR-12"),
            ("TaxBasisTotalAmount", "BT-109", "BR-13"),
            ("GrandTotalAmount", "BT-112", "BR-14"),
            ("DuePayableAmount", "BT-115", "BR-15"),
        ] {
            self.require(
                n,
                &[
                    "ApplicableHeaderTradeSettlement",
                    "SpecifiedTradeSettlementHeaderMonetarySummation",
                    want,
                ],
                term,
                rule,
            );
        }
    }

    // ---- agreement ------------------------------------------------------

    fn agreement(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        for c in kids(n) {
            match name(c) {
                "BuyerReference" => inv.buyer_reference = non_empty(own_text(c)),
                "SellerTradeParty" => inv.seller = self.party(c, true),
                "BuyerTradeParty" => inv.buyer = self.party(c, false),
                "SellerTaxRepresentativeTradeParty" => {
                    inv.tax_representative = Some(TaxRepresentative {
                        name: text(c, "Name"),
                        vat_identifier: Self::tax_registration(c, "VA"),
                        address: kid(c, "PostalTradeAddress")
                            .map(|a| self.address(a))
                            .unwrap_or_default(),
                    });
                }
                "SellerOrderReferencedDocument" => {
                    inv.sales_order_reference =
                        text(c, "IssuerAssignedID").map(DocumentReference::new);
                }
                "BuyerOrderReferencedDocument" => {
                    inv.purchase_order_reference =
                        text(c, "IssuerAssignedID").map(DocumentReference::new);
                }
                "ContractReferencedDocument" => {
                    inv.contract_reference =
                        text(c, "IssuerAssignedID").map(DocumentReference::new);
                }
                "AdditionalReferencedDocument" => self.additional_document(inv, c),
                "SpecifiedProcuringProject" => {
                    inv.project_reference = text(c, "ID").map(DocumentReference::new);
                }
                other => self.push_path("ApplicableHeaderTradeAgreement", other),
            }
        }
    }

    /// `ram:AdditionalReferencedDocument` carries three different terms, told
    /// apart by `ram:TypeCode`: 50 is BT-17, 130 is BT-18, 916 is BG-24.
    fn additional_document(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        let id = text(n, "IssuerAssignedID").unwrap_or_default();
        match text(n, "TypeCode").as_deref() {
            Some("50") => inv.tender_reference = Some(DocumentReference::new(id)),
            Some("130") => {
                let mut i = Identifier::new(id);
                if let Some(s) = text(n, "ReferenceTypeCode") {
                    i = Identifier::schemed(i.content().to_owned(), s);
                }
                inv.object_identifier = Some(i);
            }
            _ => {
                let attachment = kid(n, "AttachmentBinaryObject").and_then(|b| {
                    match en16931::Attachment::new(
                        crate::xml::decode_base64(&own_text(b)),
                        b.attribute("mimeCode").unwrap_or_default(),
                        b.attribute("filename").unwrap_or_default(),
                    ) {
                        Ok(a) => Some(a),
                        Err(e) => {
                            self.entries.push(
                                Entry::new(
                                    Kind::Malformed,
                                    "AdditionalReferencedDocument/AttachmentBinaryObject",
                                    e.to_string(),
                                )
                                .term("BT-125"),
                            );
                            None
                        }
                    }
                });
                inv.attachments.push(SupportingDocument {
                    reference: DocumentReference::new(id),
                    description: text(n, "Name"),
                    uri: text(n, "URIID"),
                    attachment,
                });
            }
        }
    }

    fn party(&mut self, n: Node<'_, '_>, seller: bool) -> Party {
        let mut p = Party::default();
        for c in kids(n) {
            match name(c) {
                // BT-29 from both `ram:GlobalID` and `ram:ID`.
                "ID" | "GlobalID" => p.identifiers.push(identifier(c)),
                "Name" => p.name = non_empty(own_text(c)),
                "Description" if seller => p.additional_legal_information = non_empty(own_text(c)),
                "SpecifiedLegalOrganization" => {
                    p.legal_registration = kid(c, "ID").map(identifier);
                    p.trading_name = text(c, "TradingBusinessName");
                }
                "DefinedTradeContact" => {
                    p.contact = Contact {
                        name: text(c, "PersonName").or_else(|| text(c, "DepartmentName")),
                        phone: kid(c, "TelephoneUniversalCommunication")
                            .and_then(|t| text(t, "CompleteNumber")),
                        email: kid(c, "EmailURIUniversalCommunication")
                            .and_then(|e| text(e, "URIID")),
                    };
                }
                "PostalTradeAddress" => p.address = self.address(c),
                "URIUniversalCommunication" => {
                    p.electronic_address = kid(c, "URIID").map(identifier);
                }
                "SpecifiedTaxRegistration" => {
                    let Some(id) = kid(c, "ID") else { continue };
                    // `VA` is BT-31/BT-48; anything else is BT-32.
                    if id.attribute("schemeID") == Some("VA") {
                        p.vat_identifier = non_empty(own_text(id));
                    } else {
                        p.tax_registration = non_empty(own_text(id));
                    }
                }
                other => self.push_path("TradeParty", other),
            }
        }
        p
    }

    fn tax_registration(n: Node<'_, '_>, scheme: &str) -> Option<String> {
        kids(n)
            .filter(|c| name(*c) == "SpecifiedTaxRegistration")
            .filter_map(|c| kid(c, "ID"))
            .find(|id| id.attribute("schemeID") == Some(scheme))
            .map(own_text)
    }

    fn address(&mut self, n: Node<'_, '_>) -> PostalAddress {
        let mut a = PostalAddress::default();
        for c in kids(n) {
            match name(c) {
                "PostcodeCode" => a.post_code = non_empty(own_text(c)),
                "LineOne" => a.line1 = non_empty(own_text(c)),
                "LineTwo" => a.line2 = non_empty(own_text(c)),
                "LineThree" => a.line3 = non_empty(own_text(c)),
                "CityName" => a.city = non_empty(own_text(c)),
                "CountryID" => a.country = Some(Code::new(own_text(c))),
                "CountrySubDivisionName" => a.subdivision = non_empty(own_text(c)),
                other => self.push_path("PostalTradeAddress", other),
            }
        }
        a
    }

    // ---- delivery -------------------------------------------------------

    fn delivery(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        let mut d = Delivery::default();
        let mut any = false;
        for c in kids(n) {
            match name(c) {
                "ShipToTradeParty" => {
                    for g in kids(c) {
                        match name(g) {
                            "ID" | "GlobalID" => {
                                d.location = Some(identifier(g));
                                any = true;
                            }
                            "Name" => {
                                d.party_name = non_empty(own_text(g));
                                any = true;
                            }
                            "PostalTradeAddress" => {
                                d.address = Some(self.address(g));
                                any = true;
                            }
                            other => self.push_path("ShipToTradeParty", other),
                        }
                    }
                }
                "ActualDeliverySupplyChainEvent" => {
                    d.date = self.date(c, "OccurrenceDateTime");
                    any |= d.date.is_some();
                }
                "DespatchAdviceReferencedDocument" => {
                    inv.despatch_advice_reference =
                        text(c, "IssuerAssignedID").map(DocumentReference::new);
                }
                "ReceivingAdviceReferencedDocument" => {
                    inv.receiving_advice_reference =
                        text(c, "IssuerAssignedID").map(DocumentReference::new);
                }
                other => self.push_path("ApplicableHeaderTradeDelivery", other),
            }
        }
        if any {
            inv.delivery = Some(d);
        }
    }

    // ---- settlement -----------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn settlement(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        let mut payment = PaymentInstructions::default();
        let mut have_payment = false;
        // BT-8 is read from its first occurrence: the rest are required to
        // agree with it (CII-SR-462), and one that does not is reported.
        let mut bt8_first: Option<String> = None;
        let mut creditor_id = None;
        let mut mandate = None;
        for c in kids(n) {
            match name(c) {
                "CreditorReferenceID" => creditor_id = non_empty(own_text(c)),
                "PaymentReference" => {
                    payment.remittance_information = non_empty(own_text(c));
                    have_payment = true;
                }
                "TaxCurrencyCode" => inv.vat_accounting_currency = Some(Code::new(own_text(c))),
                "InvoiceCurrencyCode" => inv.currency = Some(Code::new(own_text(c))),
                "PayeeTradeParty" => {
                    inv.payee = Some(Payee {
                        name: text(c, "Name"),
                        identifier: kid(c, "GlobalID").or_else(|| kid(c, "ID")).map(identifier),
                        legal_registration: kid(c, "SpecifiedLegalOrganization")
                            .and_then(|l| kid(l, "ID"))
                            .map(identifier),
                    });
                }
                "SpecifiedTradeSettlementPaymentMeans" => {
                    self.payment_means(&mut payment, c);
                    have_payment = true;
                }
                "ApplicableTradeTax" => {
                    // BT-7/BT-8 repeat per entry; the first wins.
                    if inv.vat_point_date.is_none() {
                        inv.vat_point_date = self.date(c, "TaxPointDate");
                    }
                    // A repetition that differs (CII-SR-462) is reported.
                    if let Some(raw) = text(c, "DueDateTypeCode") {
                        match &bt8_first {
                            None => {
                                inv.vat_point_date_code = self.vat_point_date_code(&raw);
                                bt8_first = Some(raw);
                            }
                            Some(first) if *first != raw => self.entries.push(
                                Entry::new(
                                    Kind::Duplicate,
                                    "ApplicableHeaderTradeSettlement/ApplicableTradeTax/DueDateTypeCode",
                                    format!(
                                        "a second, different VAT point date code; the first ({first}) is read"
                                    ),
                                )
                                .term("BT-8")
                                .value(raw)
                                .rule("CII-SR-462"),
                            ),
                            Some(_) => {}
                        }
                    }
                    self.require(c, &["BasisAmount"], "BT-116", "BR-45");
                    self.require(c, &["CalculatedAmount"], "BT-117", "BR-46");
                    inv.vat_breakdown.push(VatBreakdown {
                        taxable_amount: self.amount(c, "BasisAmount").unwrap_or_default(),
                        tax_amount: self.amount(c, "CalculatedAmount").unwrap_or_default(),
                        category: code(c, "CategoryCode").unwrap_or_else(|| Code::new("")),
                        rate: self
                            .decimal(c, "RateApplicablePercent")
                            .map(|d| en16931::Percentage::new(d.normalize())),
                        exemption_reason: text(c, "ExemptionReason"),
                        exemption_reason_code: code(c, "ExemptionReasonCode"),
                    });
                }
                "BillingSpecifiedPeriod" => inv.invoicing_period = Some(self.period(c)),
                "SpecifiedTradeAllowanceCharge" => {
                    let is_charge = self.is_charge(c, "SpecifiedTradeAllowanceCharge");
                    let (term, rule) = if is_charge {
                        ("BT-99", "BR-36")
                    } else {
                        ("BT-92", "BR-31")
                    };
                    self.require(c, &["ActualAmount"], term, rule);
                    let a = DocumentAllowanceCharge {
                        amount: self.amount(c, "ActualAmount").unwrap_or_default(),
                        base_amount: self.amount(c, "BasisAmount"),
                        percentage: self
                            .decimal(c, "CalculationPercent")
                            .map(|d| en16931::Percentage::new(d.normalize())),
                        vat: kid(c, "CategoryTradeTax")
                            .map(|t| self.line_vat(t))
                            .unwrap_or_default(),
                        reason: text(c, "Reason"),
                        reason_code: code(c, "ReasonCode"),
                    };
                    if is_charge {
                        inv.charges.push(a);
                    } else {
                        inv.allowances.push(a);
                    }
                }
                // ZUGFeRD / Factur-X EXTENDED's freight and packing charge, a
                // document-level charge (FeRD's BR-FXEXT-CO-12 counts it in
                // BT-108). Mapped only under an extension (`#conformant#` in
                // BT-24, EN 16931 §4.3).
                "SpecifiedLogisticsServiceCharge"
                    if inv.specification_id.as_deref().is_some_and(|s| {
                        s.contains("#conformant#") || s.contains(":conformant:")
                    }) =>
                {
                    self.require(c, &["AppliedAmount"], "BT-99", "BR-36");
                    inv.charges.push(DocumentAllowanceCharge {
                        amount: self.amount(c, "AppliedAmount").unwrap_or_default(),
                        base_amount: None,
                        percentage: None,
                        vat: kid(c, "AppliedTradeTax")
                            .map(|t| self.line_vat(t))
                            .unwrap_or_default(),
                        reason: text(c, "Description"),
                        reason_code: None,
                    });
                }
                // Under the core: CII-SR-396 says it should not be present and
                // BR-CO-12 does not count it. Not mapped.
                "SpecifiedLogisticsServiceCharge" => self.entries.push(
                    Entry::new(
                        Kind::Unmapped,
                        "ApplicableHeaderTradeSettlement/SpecifiedLogisticsServiceCharge",
                        "a logistics service charge, which the core does not count",
                    )
                    .rule("CII-SR-396")
                    .hint(
                        "declare the extension in BT-24 (`#conformant#`), or send the charge \
                         as a document-level charge (BG-21)",
                    ),
                ),
                "SpecifiedTradePaymentTerms" => {
                    // Not trimmed: `BR-DE-18` requires the Skonto block to
                    // end with a newline.
                    inv.payment_terms = kid(c, "Description").and_then(crate::xml::raw_text);
                    inv.due_date = self.date(c, "DueDateDateTime");
                    mandate = text(c, "DirectDebitMandateID");
                }
                "SpecifiedTradeSettlementHeaderMonetarySummation" => self.totals(inv, c),
                "InvoiceReferencedDocument" => {
                    inv.preceding_invoices.push(PrecedingInvoice {
                        reference: text(c, "IssuerAssignedID")
                            .map_or_else(|| DocumentReference::new(""), DocumentReference::new),
                        issue_date: self.date(c, "FormattedIssueDateTime"),
                    });
                }
                "ReceivableSpecifiedTradeAccountingAccount" => {
                    inv.accounting_reference = text(c, "ID");
                }
                other => self.push_path("ApplicableHeaderTradeSettlement", other),
            }
        }
        // The mandate and creditor identifier arrive from two different
        // aggregates and belong to one `DirectDebit`, so they are joined last.
        // Beside a card or an account read first, they are reported.
        if creditor_id.is_some() || mandate.is_some() {
            match &mut payment.means {
                Some(PaymentMeans::DirectDebit(d)) => {
                    d.mandate_reference = mandate.or(d.mandate_reference.take());
                    d.creditor_identifier = creditor_id.or(d.creditor_identifier.take());
                }
                slot @ None => {
                    *slot = Some(PaymentMeans::DirectDebit(DirectDebit {
                        mandate_reference: mandate,
                        creditor_identifier: creditor_id,
                        debited_account: None,
                    }));
                }
                Some(_) => {
                    if mandate.is_some() {
                        self.not_held(
                            "SpecifiedTradePaymentTerms/DirectDebitMandateID",
                            "a mandate beside a card or an account; BG-16 holds one kind",
                        );
                    }
                    if creditor_id.is_some() {
                        self.not_held(
                            "ApplicableHeaderTradeSettlement/CreditorReferenceID",
                            "a creditor identifier beside a card or an account; BG-16 holds \
                             one kind",
                        );
                    }
                }
            }
            have_payment = true;
        }
        if have_payment {
            inv.payment = Some(payment);
        }
    }

    /// `means` into `p` unless `p` already holds a kind of means; then reported.
    fn first_means(
        &mut self,
        p: &mut PaymentInstructions,
        means: PaymentMeans,
        path: &str,
        message: &str,
    ) {
        if p.means.is_none() {
            p.means = Some(means);
        } else {
            self.not_held(path, message);
        }
    }

    fn payment_means(&mut self, p: &mut PaymentInstructions, n: Node<'_, '_>) {
        // Called once per `ram:SpecifiedTradeSettlementPaymentMeans`; several
        // are one BG-16 with several BG-17 accounts (0..1 per element), so
        // everything accumulates into `p`.
        let mut transfers: Vec<CreditTransfer> = Vec::new();
        for c in kids(n) {
            match name(c) {
                "TypeCode" => {
                    if p.means_code.is_none() {
                        p.means_code = Some(Code::new(own_text(c)));
                    }
                }
                "Information" => {
                    if p.means_text.is_none() {
                        p.means_text = non_empty(own_text(c));
                    }
                }
                // The model holds one of BG-17/18/19: the first kind read
                // wins and a later one is reported, as in the UBL reader.
                "ApplicableTradeSettlementFinancialCard" => self.first_means(
                    p,
                    PaymentMeans::Card(PaymentCard {
                        primary_account_number: text(c, "ID"),
                        holder_name: text(c, "CardholderName"),
                    }),
                    "SpecifiedTradeSettlementPaymentMeans/ApplicableTradeSettlementFinancialCard",
                    "a card beside another payment means; BG-16 holds one kind",
                ),
                "PayerPartyDebtorFinancialAccount" => self.first_means(
                    p,
                    PaymentMeans::DirectDebit(DirectDebit {
                        debited_account: text(c, "IBANID").or_else(|| text(c, "ProprietaryID")),
                        ..DirectDebit::default()
                    }),
                    "SpecifiedTradeSettlementPaymentMeans/PayerPartyDebtorFinancialAccount",
                    "a debited account beside another payment means; BG-16 holds one kind",
                ),
                "PayeePartyCreditorFinancialAccount" => transfers.push(CreditTransfer {
                    account_identifier: text(c, "IBANID").or_else(|| text(c, "ProprietaryID")),
                    account_name: text(c, "AccountName"),
                    provider_identifier: None,
                }),
                "PayeeSpecifiedCreditorFinancialInstitution" => {
                    // BT-86 arrives *after* the account it belongs to, as a
                    // sibling rather than a child, so it is attached here.
                    if let Some(t) = transfers.last_mut() {
                        t.provider_identifier = text(c, "BICID");
                    }
                }
                other => self.push_path("SpecifiedTradeSettlementPaymentMeans", other),
            }
        }
        if !transfers.is_empty() {
            match &mut p.means {
                Some(PaymentMeans::CreditTransfer(existing)) => existing.extend(transfers),
                slot @ None => *slot = Some(PaymentMeans::CreditTransfer(transfers)),
                // An account on top of a card or mandate cannot be
                // represented; reported.
                Some(_) => self.not_held(
                    "SpecifiedTradeSettlementPaymentMeans/PayeePartyCreditorFinancialAccount",
                    "an account beside a card or mandate; BG-16 holds one kind",
                ),
            }
        }
    }

    fn totals(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        let mut t = DocumentTotals::default();
        // Two `ram:TaxTotalAmount` elements may appear, told apart by
        // `@currencyID`: BT-110 and BT-111 (accounting currency).
        let doc_ccy = inv.currency.as_ref().map(Code::as_str);
        let tax_ccy = inv.vat_accounting_currency.as_ref().map(Code::as_str);
        for c in kids(n) {
            match name(c) {
                "LineTotalAmount" => {
                    t.line_total = self.amount(n, "LineTotalAmount").unwrap_or_default();
                }
                "ChargeTotalAmount" => t.charge_total = self.amount(n, "ChargeTotalAmount"),
                "AllowanceTotalAmount" => {
                    t.allowance_total = self.amount(n, "AllowanceTotalAmount");
                }
                "TaxBasisTotalAmount" => {
                    t.taxable_total = self.amount(n, "TaxBasisTotalAmount").unwrap_or_default();
                }
                "TaxTotalAmount" => {
                    let raw = own_text(c);
                    let parsed = InvoiceAmount::parse(&raw).ok();
                    if parsed.is_none() {
                        self.malformed(
                            "SpecifiedTradeSettlementHeaderMonetarySummation/TaxTotalAmount"
                                .to_owned(),
                            "not an amount the model can hold (at most two decimals)",
                            raw,
                        );
                    }
                    // Trimmed; a blank `@currencyID` means the document currency.
                    match c
                        .attribute("currencyID")
                        .map(str::trim)
                        .filter(|c| !c.is_empty())
                    {
                        // BT-6 == BT-5: one element is both totals (`BR-53`).
                        Some(cur) if Some(cur) == tax_ccy => {
                            if Some(cur) == doc_ccy && t.vat_total.is_none() {
                                t.vat_total = parsed;
                            }
                            t.vat_total_accounting = parsed;
                        }
                        Some(cur) if Some(cur) != doc_ccy => t.vat_total_accounting = parsed,
                        _ => t.vat_total = parsed,
                    }
                }
                "RoundingAmount" => t.rounding = self.amount(n, "RoundingAmount"),
                "GrandTotalAmount" => {
                    t.gross_total = self.amount(n, "GrandTotalAmount").unwrap_or_default();
                }
                "TotalPrepaidAmount" => t.paid = self.amount(n, "TotalPrepaidAmount"),
                "DuePayableAmount" => {
                    t.due = self.amount(n, "DuePayableAmount").unwrap_or_default();
                }
                other => self.push_path("SpecifiedTradeSettlementHeaderMonetarySummation", other),
            }
        }
        inv.totals = t;
    }

    fn period(&mut self, n: Node<'_, '_>) -> Period {
        Period {
            start: self.date(n, "StartDateTime"),
            end: self.date(n, "EndDateTime"),
        }
    }

    /// `ram:ChargeIndicator/udt:Indicator` — `true` is a charge.
    ///
    /// All four `xs:boolean` lexical forms are accepted; anything else is
    /// reported, never defaulted to an allowance.
    fn is_charge(&mut self, n: Node<'_, '_>, at: &str) -> bool {
        let raw = kid(n, "ChargeIndicator")
            .and_then(|i| kid(i, "Indicator"))
            .map(own_text);
        match raw.as_deref() {
            Some("true" | "1") => true,
            Some("false" | "0") => false,
            _ => {
                let e = Entry::new(
                    Kind::Malformed,
                    format!("{at}/ChargeIndicator/Indicator"),
                    match &raw {
                        Some(_) => "not an xs:boolean",
                        None => "absent, and CII requires it",
                    },
                )
                .hint("the value must be `true` or `false`");
                self.entries.push(match raw {
                    Some(v) => e.value(v),
                    None => e,
                });
                false
            }
        }
    }

    /// BT-8 from its CII spelling (UNTDID 2475) to the model's (UNTDID 2005).
    ///
    /// A value outside CII's list (including a model code like `3`) is not
    /// passed on.
    fn vat_point_date_code(&mut self, raw: &str) -> Option<Code> {
        if let Some(model) = super::vat_point_date_code_from_cii(raw) {
            return Some(Code::new(model));
        }
        self.entries.push(
            Entry::new(
                Kind::Malformed,
                "ApplicableTradeTax/DueDateTypeCode",
                "not a CII VAT point date code (5, 29 or 72, UNTDID 2475)",
            )
            .term("BT-8")
            .value(raw)
            .hint(
                "CII spells the model's 3, 35 and 432 as 5, 29 and 72 — the same three \
                 meanings",
            ),
        );
        None
    }

    fn line_vat(&mut self, n: Node<'_, '_>) -> LineVat {
        LineVat {
            category: code(n, "CategoryCode").unwrap_or_else(|| Code::new("")),
            rate: self
                .decimal(n, "RateApplicablePercent")
                .map(|d| en16931::Percentage::new(d.normalize())),
        }
    }

    // ---- lines ----------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn line(&mut self, n: Node<'_, '_>) -> InvoiceLine {
        let mut l = InvoiceLine {
            id: String::new(),
            note: None,
            order_line_reference: None,
            accounting_reference: None,
            object_identifier: None,
            quantity: Quantity::ZERO,
            unit_code: Code::new(""),
            net_amount: InvoiceAmount::default(),
            period: None,
            allowances: Vec::new(),
            charges: Vec::new(),
            price: PriceDetails::default(),
            vat: LineVat::default(),
            item: Item::default(),
        };
        for c in kids(n) {
            match name(c) {
                "AssociatedDocumentLineDocument" => {
                    l.id = text(c, "LineID").unwrap_or_default();
                    l.note = kid(c, "IncludedNote").and_then(|x| text(x, "Content"));
                }
                "SpecifiedTradeProduct" => l.item = self.item(c),
                "SpecifiedLineTradeAgreement" => {
                    for g in kids(c) {
                        match name(g) {
                            "BuyerOrderReferencedDocument" => {
                                l.order_line_reference =
                                    text(g, "LineID").map(DocumentReference::new);
                            }
                            "GrossPriceProductTradePrice" => {
                                l.price.gross_price = self
                                    .decimal(g, "ChargeAmount")
                                    .map(en16931::UnitPriceAmount::new);
                                l.price.price_discount = kid(g, "AppliedTradeAllowanceCharge")
                                    .and_then(|a| self.decimal(a, "ActualAmount"))
                                    .map(en16931::UnitPriceAmount::new);
                            }
                            "NetPriceProductTradePrice" => {
                                l.price.net_price = self
                                    .decimal(g, "ChargeAmount")
                                    .map(en16931::UnitPriceAmount::new)
                                    .unwrap_or_default();
                                l.price.base_quantity =
                                    self.decimal(g, "BasisQuantity").map(Quantity::new);
                                // An empty `unitCode` is an absent BT-150
                                // (else `PEPPOL-EN16931-R130` fires).
                                l.price.base_quantity_code = kid(g, "BasisQuantity")
                                    .and_then(|q| q.attribute("unitCode"))
                                    .filter(|u| !u.trim().is_empty())
                                    .map(Code::new);
                            }
                            other => self.push_path("SpecifiedLineTradeAgreement", other),
                        }
                    }
                }
                "SpecifiedLineTradeDelivery" => {
                    if let Some(q) = kid(c, "BilledQuantity") {
                        l.quantity = self
                            .decimal(c, "BilledQuantity")
                            .map_or(Quantity::ZERO, Quantity::new);
                        l.unit_code = q
                            .attribute("unitCode")
                            .map_or_else(|| Code::new(""), Code::new);
                    }
                }
                "SpecifiedLineTradeSettlement" => self.line_settlement(&mut l, c),
                other => self.push_path("IncludedSupplyChainTradeLineItem", other),
            }
        }
        self.require(
            n,
            &["SpecifiedLineTradeDelivery", "BilledQuantity"],
            "BT-129",
            "BR-22",
        );
        self.require(
            n,
            &[
                "SpecifiedLineTradeAgreement",
                "NetPriceProductTradePrice",
                "ChargeAmount",
            ],
            "BT-146",
            "BR-26",
        );
        self.require(
            n,
            &[
                "SpecifiedLineTradeSettlement",
                "SpecifiedTradeSettlementLineMonetarySummation",
                "LineTotalAmount",
            ],
            "BT-131",
            "BR-24",
        );
        l
    }

    fn line_settlement(&mut self, l: &mut InvoiceLine, n: Node<'_, '_>) {
        for c in kids(n) {
            match name(c) {
                "ApplicableTradeTax" => l.vat = self.line_vat(c),
                "BillingSpecifiedPeriod" => l.period = Some(self.period(c)),
                "SpecifiedTradeAllowanceCharge" => {
                    let is_charge = self.is_charge(
                        c,
                        "IncludedSupplyChainTradeLineItem/SpecifiedTradeAllowanceCharge",
                    );
                    let (term, rule) = if is_charge {
                        ("BT-141", "BR-43")
                    } else {
                        ("BT-136", "BR-41")
                    };
                    self.require(c, &["ActualAmount"], term, rule);
                    let a = LineAllowanceCharge {
                        amount: self.amount(c, "ActualAmount").unwrap_or_default(),
                        base_amount: self.amount(c, "BasisAmount"),
                        percentage: self
                            .decimal(c, "CalculationPercent")
                            .map(|d| en16931::Percentage::new(d.normalize())),
                        reason: text(c, "Reason"),
                        reason_code: code(c, "ReasonCode"),
                    };
                    if is_charge {
                        l.charges.push(a);
                    } else {
                        l.allowances.push(a);
                    }
                }
                "SpecifiedTradeSettlementLineMonetarySummation" => {
                    l.net_amount = self.amount(c, "LineTotalAmount").unwrap_or_default();
                }
                "AdditionalReferencedDocument" => {
                    let id = text(c, "IssuerAssignedID").unwrap_or_default();
                    l.object_identifier = Some(match text(c, "ReferenceTypeCode") {
                        Some(s) => Identifier::schemed(id, s),
                        None => Identifier::new(id),
                    });
                }
                "ReceivableSpecifiedTradeAccountingAccount" => {
                    l.accounting_reference = text(c, "ID");
                }
                other => self.push_path("SpecifiedLineTradeSettlement", other),
            }
        }
    }

    fn item(&mut self, n: Node<'_, '_>) -> Item {
        let mut i = Item::default();
        for c in kids(n) {
            match name(c) {
                "GlobalID" => i.standard_identifier = Some(identifier(c)),
                "SellerAssignedID" => i.seller_identifier = non_empty(own_text(c)),
                "BuyerAssignedID" => i.buyer_identifier = non_empty(own_text(c)),
                "Name" => i.name = non_empty(own_text(c)),
                "Description" => i.description = non_empty(own_text(c)),
                "ApplicableProductCharacteristic" => i.attributes.push(ItemAttribute {
                    name: text(c, "Description"),
                    value: text(c, "Value"),
                }),
                "DesignatedProductClassification" => {
                    if let Some(cc) = kid(c, "ClassCode") {
                        i.classification_identifiers.push(identifier(cc));
                    }
                }
                "OriginTradeCountry" => i.origin_country = code(c, "ID"),
                other => self.push_path("SpecifiedTradeProduct", other),
            }
        }
        i
    }
}
