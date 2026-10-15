//! E-Rechnung: XRechnung (CII) und ZUGFeRD/Factur-X (EN 16931) erzeugen,
//! eingehende XRechnungen (CII und UBL) sowie ZUGFeRD-PDFs einlesen.

use std::fmt::Write as _;
use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::Serialize;
use ts_rs::TS;

use crate::db;
use crate::models::*;
use crate::services::calc;
use crate::services::documents::lines_of;
use crate::services::receipts::ReceiptInput;
use crate::services::settings as cfg;
use crate::util::{self, r2};

pub const GUIDELINE_XRECHNUNG: &str = "urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0";
pub const GUIDELINE_EN16931: &str = "urn:cen.eu:en16931:2017";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn amt(d: Decimal) -> String {
    format!("{:.2}", r2(d))
}

fn qty(d: Decimal) -> String {
    let n = d.round_dp(4).normalize();
    if n.scale() == 0 { format!("{n}") } else { n.to_string() }
}

fn date102(d: NaiveDate) -> String {
    d.format("%Y%m%d").to_string()
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct EinvoiceCheck {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

struct Category {
    code: &'static str,
    reason: &'static str,
    vatex: &'static str,
}

fn category(doc: &Document, rate: Decimal) -> Category {
    match doc.tax_mode.as_str() {
        "small_business" => Category { code: "E", reason: "Kein Ausweis von Umsatzsteuer, da Kleinunternehmer gemäß § 19 UStG", vatex: "" },
        "reverse_charge" => Category { code: "AE", reason: "Steuerschuldnerschaft des Leistungsempfängers (§ 13b UStG)", vatex: "VATEX-EU-AE" },
        "eu_service" => Category { code: "AE", reason: "Reverse Charge – Steuerschuldnerschaft des Leistungsempfängers", vatex: "VATEX-EU-AE" },
        "eu_delivery" => Category { code: "K", reason: "Steuerfreie innergemeinschaftliche Lieferung (§ 4 Nr. 1b UStG)", vatex: "VATEX-EU-IC" },
        "export" => Category { code: "G", reason: "Steuerfreie Ausfuhrlieferung (§ 4 Nr. 1a UStG)", vatex: "VATEX-EU-G" },
        "third_country" => Category { code: "O", reason: "Nicht im Inland steuerbare Leistung", vatex: "VATEX-EU-O" },
        _ if rate.is_zero() => Category { code: "E", reason: "Steuerfreier Umsatz", vatex: "" },
        _ => Category { code: "S", reason: "", vatex: "" },
    }
}

pub fn check(conn: &Connection, doc: &Document, xrechnung: bool) -> Result<EinvoiceCheck> {
    let s = cfg::load(conn)?;
    let c = &s.company;
    let contact = doc.contact_id.and_then(|id| db::find::<Contact>(conn, id).ok().flatten());
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    if doc.status == "draft" {
        errors.push("Die Rechnung muss zuerst abgeschlossen werden.".into());
    }
    if !doc.is_invoice_like() {
        errors.push("E-Rechnungen können nur für Rechnungen und Gutschriften erstellt werden.".into());
    }
    if c.name.is_empty() || c.street.is_empty() || c.zip.is_empty() || c.city.is_empty() {
        errors.push("Unternehmensname und vollständige Anschrift fehlen (Einstellungen → Unternehmen).".into());
    }
    if c.vat_id.is_empty() && c.tax_number.is_empty() {
        errors.push("USt-IdNr. oder Steuernummer des Unternehmens fehlt.".into());
    }
    if c.iban.is_empty() && doc.doc_type == "invoice" {
        warnings.push("Keine IBAN hinterlegt – es wird keine Zahlungsanweisung übermittelt.".into());
    }
    match &contact {
        None => errors.push("Der Rechnung ist kein Kontakt zugeordnet.".into()),
        Some(k) => {
            if k.city.is_empty() || k.zip.is_empty() {
                errors.push("Postleitzahl und Ort des Kunden fehlen.".into());
            }
            if xrechnung && k.email.is_empty() {
                errors.push("XRechnung: E-Mail-Adresse des Kunden (elektronische Adresse, BT-49) fehlt.".into());
            }
            if (doc.tax_mode == "eu_service" || doc.tax_mode == "eu_delivery") && k.vat_id.is_empty() {
                errors.push("Für innergemeinschaftliche Umsätze ist die USt-IdNr. des Kunden erforderlich.".into());
            }
        }
    }
    if xrechnung {
        if c.email.is_empty() {
            errors.push("XRechnung: E-Mail-Adresse des Unternehmens (BT-34/BT-43) fehlt.".into());
        }
        if c.phone.is_empty() {
            errors.push("XRechnung: Telefonnummer des Unternehmens (BT-42) fehlt.".into());
        }
        if doc.buyer_reference.is_empty() {
            warnings.push("XRechnung: Keine Leitweg-ID / Käuferreferenz (BT-10) angegeben – es wird die Kundennummer verwendet.".into());
        }
    }
    if (doc.tax_mode == "reverse_charge" || doc.tax_mode == "eu_service" || doc.tax_mode == "eu_delivery") && c.vat_id.is_empty() {
        errors.push("Für Reverse-Charge und innergemeinschaftliche Umsätze ist die eigene USt-IdNr. erforderlich.".into());
    }
    Ok(EinvoiceCheck { errors, warnings })
}

fn party_address(out: &mut String, street: &str, zip: &str, city: &str, country: &str) {
    out.push_str("<ram:PostalTradeAddress>");
    if !zip.is_empty() {
        let _ = write!(out, "<ram:PostcodeCode>{}</ram:PostcodeCode>", esc(zip));
    }
    if !street.is_empty() {
        let _ = write!(out, "<ram:LineOne>{}</ram:LineOne>", esc(street));
    }
    if !city.is_empty() {
        let _ = write!(out, "<ram:CityName>{}</ram:CityName>", esc(city));
    }
    let _ = write!(out, "<ram:CountryID>{}</ram:CountryID>", esc(if country.is_empty() { "DE" } else { country }));
    out.push_str("</ram:PostalTradeAddress>");
}

/// Generates a CII invoice (CrossIndustryInvoice D16B).
pub fn generate_cii(conn: &Connection, doc: &Document, guideline: &str) -> Result<String> {
    let s = cfg::load(conn)?;
    let c = &s.company;
    let contact = doc.contact_id.and_then(|id| db::find::<Contact>(conn, id).ok().flatten()).unwrap_or_default();
    let mut lines = lines_of(conn, doc.id)?;
    let totals = calc::calculate(doc, &mut lines);
    let xrechnung = guideline == GUIDELINE_XRECHNUNG;
    // Cancellations carry negative amounts; they are transmitted as credit notes with positive amounts.
    let flip = if doc.kind == "cancellation" { Decimal::NEGATIVE_ONE } else { Decimal::ONE };
    let type_code = match (doc.doc_type.as_str(), doc.kind.as_str()) {
        ("credit_note", _) | (_, "cancellation") => "381",
        (_, "partial") => "326",
        _ => "380",
    };
    let mut x = String::with_capacity(8192);
    x.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    x.push_str(r#"<rsm:CrossIndustryInvoice xmlns:rsm="urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100" xmlns:ram="urn:un:unece:uncefact:data:standard:ReusableAggregateBusinessInformationEntity:100" xmlns:qdt="urn:un:unece:uncefact:data:standard:QualifiedDataType:100" xmlns:udt="urn:un:unece:uncefact:data:standard:UnqualifiedDataType:100">"#);
    x.push_str("<rsm:ExchangedDocumentContext>");
    if xrechnung {
        x.push_str("<ram:BusinessProcessSpecifiedDocumentContextParameter><ram:ID>urn:fdc:peppol.eu:2017:poacc:billing:01:1.0</ram:ID></ram:BusinessProcessSpecifiedDocumentContextParameter>");
    }
    let _ = write!(x, "<ram:GuidelineSpecifiedDocumentContextParameter><ram:ID>{guideline}</ram:ID></ram:GuidelineSpecifiedDocumentContextParameter>");
    x.push_str("</rsm:ExchangedDocumentContext>");

    // Header
    x.push_str("<rsm:ExchangedDocument>");
    let _ = write!(x, "<ram:ID>{}</ram:ID><ram:TypeCode>{type_code}</ram:TypeCode>", esc(&doc.number));
    let _ = write!(x, "<ram:IssueDateTime><udt:DateTimeString format=\"102\">{}</udt:DateTimeString></ram:IssueDateTime>", date102(doc.date));
    let notes = crate::pdf::data::tax_note(doc, Some(&contact), &s);
    for n in notes.iter().filter(|n| !n.is_empty()) {
        let _ = write!(x, "<ram:IncludedNote><ram:Content>{}</ram:Content></ram:IncludedNote>", esc(n));
    }
    let mut reg = vec![c.name.clone()];
    if !c.owner.is_empty() {
        reg.push(format!("Inhaber/Geschäftsführung: {}", c.owner));
    }
    if !c.register_number.is_empty() {
        reg.push(format!("{} {}", c.register_court, c.register_number));
    }
    let _ = write!(x, "<ram:IncludedNote><ram:Content>{}</ram:Content><ram:SubjectCode>REG</ram:SubjectCode></ram:IncludedNote>", esc(&reg.join(", ")));
    x.push_str("</rsm:ExchangedDocument>");

    x.push_str("<rsm:SupplyChainTradeTransaction>");
    // Lines
    let mut line_no = 0;
    let mut line_total_sum = Decimal::ZERO;
    for l in lines.iter().filter(|l| l.line_type == "item" || l.line_type == "deduction") {
        line_no += 1;
        let rate = calc::effective_rate(doc, l);
        let cat = category(doc, rate);
        let net = r2(l.net_amount * flip);
        line_total_sum += net;
        // The item net price must not be negative (BR-27): negative amounts
        // (deductions, discount lines) are expressed through a negative quantity.
        let mut q = if l.line_type == "deduction" { Decimal::ONE } else { l.quantity * flip };
        let mut price = if q.is_zero() { Decimal::ZERO } else { (net / q).round_dp(4) };
        if price < Decimal::ZERO {
            q = -q;
            price = -price;
        }
        x.push_str("<ram:IncludedSupplyChainTradeLineItem>");
        let _ = write!(x, "<ram:AssociatedDocumentLineDocument><ram:LineID>{line_no}</ram:LineID></ram:AssociatedDocumentLineDocument>");
        x.push_str("<ram:SpecifiedTradeProduct>");
        if !l.article_number.is_empty() {
            let _ = write!(x, "<ram:SellerAssignedID>{}</ram:SellerAssignedID>", esc(&l.article_number));
        }
        let name = if l.name.is_empty() { "Position".to_string() } else { l.name.clone() };
        let _ = write!(x, "<ram:Name>{}</ram:Name>", esc(&name));
        if !l.description.is_empty() {
            let _ = write!(x, "<ram:Description>{}</ram:Description>", esc(&l.description));
        }
        x.push_str("</ram:SpecifiedTradeProduct>");
        let _ = write!(x, "<ram:SpecifiedLineTradeAgreement><ram:NetPriceProductTradePrice><ram:ChargeAmount>{}</ram:ChargeAmount></ram:NetPriceProductTradePrice></ram:SpecifiedLineTradeAgreement>", qty(price));
        let unit = if l.line_type == "deduction" { "C62" } else { util::unit_code(&l.unit) };
        let _ = write!(x, "<ram:SpecifiedLineTradeDelivery><ram:BilledQuantity unitCode=\"{unit}\">{}</ram:BilledQuantity></ram:SpecifiedLineTradeDelivery>", qty(q));
        x.push_str("<ram:SpecifiedLineTradeSettlement><ram:ApplicableTradeTax><ram:TypeCode>VAT</ram:TypeCode>");
        let _ = write!(x, "<ram:CategoryCode>{}</ram:CategoryCode>", cat.code);
        if cat.code != "O" {
            let _ = write!(x, "<ram:RateApplicablePercent>{}</ram:RateApplicablePercent>", qty(rate));
        }
        x.push_str("</ram:ApplicableTradeTax>");
        let _ = write!(x, "<ram:SpecifiedTradeSettlementLineMonetarySummation><ram:LineTotalAmount>{}</ram:LineTotalAmount></ram:SpecifiedTradeSettlementLineMonetarySummation>", amt(net));
        x.push_str("</ram:SpecifiedLineTradeSettlement></ram:IncludedSupplyChainTradeLineItem>");
    }

    // Agreement
    x.push_str("<ram:ApplicableHeaderTradeAgreement>");
    let buyer_ref = if !doc.buyer_reference.is_empty() {
        doc.buyer_reference.clone()
    } else if !contact.number.is_empty() {
        contact.number.clone()
    } else {
        "n/a".into()
    };
    let _ = write!(x, "<ram:BuyerReference>{}</ram:BuyerReference>", esc(&buyer_ref));
    x.push_str("<ram:SellerTradeParty>");
    let _ = write!(x, "<ram:Name>{}</ram:Name>", esc(&c.name));
    if !c.register_number.is_empty() {
        let _ = write!(x, "<ram:SpecifiedLegalOrganization><ram:ID>{}</ram:ID></ram:SpecifiedLegalOrganization>", esc(&c.register_number));
    }
    if !c.phone.is_empty() || !c.email.is_empty() {
        x.push_str("<ram:DefinedTradeContact>");
        let person = if !c.contact_name.is_empty() { &c.contact_name } else if !c.owner.is_empty() { &c.owner } else { &c.name };
        let _ = write!(x, "<ram:PersonName>{}</ram:PersonName>", esc(person));
        if !c.phone.is_empty() {
            let _ = write!(x, "<ram:TelephoneUniversalCommunication><ram:CompleteNumber>{}</ram:CompleteNumber></ram:TelephoneUniversalCommunication>", esc(&c.phone));
        }
        if !c.email.is_empty() {
            let _ = write!(x, "<ram:EmailURIUniversalCommunication><ram:URIID>{}</ram:URIID></ram:EmailURIUniversalCommunication>", esc(&c.email));
        }
        x.push_str("</ram:DefinedTradeContact>");
    }
    party_address(&mut x, &c.street, &c.zip, &c.city, &c.country);
    if !c.email.is_empty() {
        let _ = write!(x, "<ram:URIUniversalCommunication><ram:URIID schemeID=\"EM\">{}</ram:URIID></ram:URIUniversalCommunication>", esc(&c.email));
    }
    let seller_vat_allowed = doc.tax_mode != "third_country";
    if !c.vat_id.is_empty() && seller_vat_allowed {
        let _ = write!(x, "<ram:SpecifiedTaxRegistration><ram:ID schemeID=\"VA\">{}</ram:ID></ram:SpecifiedTaxRegistration>", esc(&c.vat_id.replace(' ', "")));
    }
    if !c.tax_number.is_empty() {
        let _ = write!(x, "<ram:SpecifiedTaxRegistration><ram:ID schemeID=\"FC\">{}</ram:ID></ram:SpecifiedTaxRegistration>", esc(&c.tax_number));
    }
    x.push_str("</ram:SellerTradeParty>");
    x.push_str("<ram:BuyerTradeParty>");
    if !contact.number.is_empty() {
        let _ = write!(x, "<ram:ID>{}</ram:ID>", esc(&contact.number));
    }
    let buyer_name = if contact.id > 0 { contact.display_name() } else { doc.address.lines().next().unwrap_or_default().to_string() };
    let _ = write!(x, "<ram:Name>{}</ram:Name>", esc(&buyer_name));
    if !doc.contact_person.is_empty() {
        let _ = write!(x, "<ram:DefinedTradeContact><ram:PersonName>{}</ram:PersonName></ram:DefinedTradeContact>", esc(&doc.contact_person));
    }
    party_address(&mut x, &contact.street, &contact.zip, &contact.city, &contact.country);
    if !contact.email.is_empty() {
        let _ = write!(x, "<ram:URIUniversalCommunication><ram:URIID schemeID=\"EM\">{}</ram:URIID></ram:URIUniversalCommunication>", esc(&contact.email));
    }
    if !contact.vat_id.is_empty() && seller_vat_allowed {
        let _ = write!(x, "<ram:SpecifiedTaxRegistration><ram:ID schemeID=\"VA\">{}</ram:ID></ram:SpecifiedTaxRegistration>", esc(&contact.vat_id.replace(' ', "")));
    }
    x.push_str("</ram:BuyerTradeParty>");
    if !doc.reference.is_empty() && doc.kind != "cancellation" {
        let _ = write!(x, "<ram:BuyerOrderReferencedDocument><ram:IssuerAssignedID>{}</ram:IssuerAssignedID></ram:BuyerOrderReferencedDocument>", esc(&doc.reference));
    }
    x.push_str("</ram:ApplicableHeaderTradeAgreement>");

    // Delivery
    x.push_str("<ram:ApplicableHeaderTradeDelivery>");
    if doc.tax_mode == "eu_delivery" || doc.tax_mode == "export" {
        x.push_str("<ram:ShipToTradeParty>");
        let _ = write!(x, "<ram:Name>{}</ram:Name>", esc(&buyer_name));
        party_address(&mut x, &contact.street, &contact.zip, &contact.city, &contact.country);
        x.push_str("</ram:ShipToTradeParty>");
    }
    if let Some(d) = doc.service_date
        && doc.service_date_end.is_none()
    {
        let _ = write!(x, "<ram:ActualDeliverySupplyChainEvent><ram:OccurrenceDateTime><udt:DateTimeString format=\"102\">{}</udt:DateTimeString></ram:OccurrenceDateTime></ram:ActualDeliverySupplyChainEvent>", date102(d));
    }
    x.push_str("</ram:ApplicableHeaderTradeDelivery>");

    // Settlement
    x.push_str("<ram:ApplicableHeaderTradeSettlement>");
    if doc.doc_type == "invoice" && doc.kind != "cancellation" {
        let _ = write!(x, "<ram:PaymentReference>{}</ram:PaymentReference>", esc(&doc.number));
    }
    let _ = write!(x, "<ram:InvoiceCurrencyCode>{}</ram:InvoiceCurrencyCode>", esc(&doc.currency));
    if !c.iban.is_empty() && doc.doc_type == "invoice" && doc.kind != "cancellation" {
        x.push_str("<ram:SpecifiedTradeSettlementPaymentMeans><ram:TypeCode>58</ram:TypeCode>");
        let _ = write!(x, "<ram:PayeePartyCreditorFinancialAccount><ram:IBANID>{}</ram:IBANID><ram:AccountName>{}</ram:AccountName></ram:PayeePartyCreditorFinancialAccount>", util::normalize_iban(&c.iban), esc(&c.name));
        if !c.bic.is_empty() {
            let _ = write!(x, "<ram:PayeeSpecifiedCreditorFinancialInstitution><ram:BICID>{}</ram:BICID></ram:PayeeSpecifiedCreditorFinancialInstitution>", esc(&c.bic));
        }
        x.push_str("</ram:SpecifiedTradeSettlementPaymentMeans>");
    }
    for g in &totals.groups {
        let cat = category(doc, g.rate);
        x.push_str("<ram:ApplicableTradeTax>");
        let _ = write!(x, "<ram:CalculatedAmount>{}</ram:CalculatedAmount><ram:TypeCode>VAT</ram:TypeCode>", amt(g.tax * flip));
        if !cat.reason.is_empty() {
            let _ = write!(x, "<ram:ExemptionReason>{}</ram:ExemptionReason>", esc(cat.reason));
        }
        let _ = write!(x, "<ram:BasisAmount>{}</ram:BasisAmount><ram:CategoryCode>{}</ram:CategoryCode>", amt(g.net * flip), cat.code);
        if !cat.vatex.is_empty() {
            let _ = write!(x, "<ram:ExemptionReasonCode>{}</ram:ExemptionReasonCode>", cat.vatex);
        }
        if cat.code != "O" {
            let _ = write!(x, "<ram:RateApplicablePercent>{}</ram:RateApplicablePercent>", qty(g.rate));
        }
        x.push_str("</ram:ApplicableTradeTax>");
    }
    if let (Some(a), Some(b)) = (doc.service_date, doc.service_date_end) {
        let _ = write!(x, "<ram:BillingSpecifiedPeriod><ram:StartDateTime><udt:DateTimeString format=\"102\">{}</udt:DateTimeString></ram:StartDateTime><ram:EndDateTime><udt:DateTimeString format=\"102\">{}</udt:DateTimeString></ram:EndDateTime></ram:BillingSpecifiedPeriod>", date102(a), date102(b));
    }
    let gross = totals.gross * flip;
    if doc.doc_type == "invoice" && doc.kind != "cancellation" {
        x.push_str("<ram:SpecifiedTradePaymentTerms>");
        let mut desc = match doc.due_date {
            Some(d) => format!("Zahlbar bis {} ohne Abzug.", util::fmt_date(d)),
            None => "Zahlbar sofort ohne Abzug.".into(),
        };
        if doc.cash_discount_percent > Decimal::ZERO && doc.cash_discount_days > 0 {
            if xrechnung {
                desc = format!("#SKONTO#TAGE={}#PROZENT={:.2}#\n{desc}", doc.cash_discount_days, doc.cash_discount_percent);
            } else {
                desc = format!("{desc} Bei Zahlung innerhalb von {} Tagen {} % Skonto.", doc.cash_discount_days, doc.cash_discount_percent.normalize());
            }
        }
        let _ = write!(x, "<ram:Description>{}</ram:Description>", esc(&desc));
        if let Some(d) = doc.due_date {
            let _ = write!(x, "<ram:DueDateDateTime><udt:DateTimeString format=\"102\">{}</udt:DateTimeString></ram:DueDateDateTime>", date102(d));
        }
        x.push_str("</ram:SpecifiedTradePaymentTerms>");
    }
    let prepaid = if doc.kind == "cancellation" { Decimal::ZERO } else { doc.paid_total.max(Decimal::ZERO).min(gross) };
    x.push_str("<ram:SpecifiedTradeSettlementHeaderMonetarySummation>");
    let _ = write!(x, "<ram:LineTotalAmount>{}</ram:LineTotalAmount>", amt(line_total_sum));
    x.push_str("<ram:ChargeTotalAmount>0.00</ram:ChargeTotalAmount><ram:AllowanceTotalAmount>0.00</ram:AllowanceTotalAmount>");
    let _ = write!(x, "<ram:TaxBasisTotalAmount>{}</ram:TaxBasisTotalAmount>", amt(totals.net * flip));
    let _ = write!(x, "<ram:TaxTotalAmount currencyID=\"{}\">{}</ram:TaxTotalAmount>", esc(&doc.currency), amt(totals.tax * flip));
    let _ = write!(x, "<ram:GrandTotalAmount>{}</ram:GrandTotalAmount>", amt(gross));
    let _ = write!(x, "<ram:TotalPrepaidAmount>{}</ram:TotalPrepaidAmount>", amt(prepaid));
    let _ = write!(x, "<ram:DuePayableAmount>{}</ram:DuePayableAmount>", amt(gross - prepaid));
    x.push_str("</ram:SpecifiedTradeSettlementHeaderMonetarySummation>");
    // Preceding invoices (cancellation, credit note, final invoice)
    let mut refs: Vec<(String, NaiveDate)> = Vec::new();
    if let Some(cid) = doc.cancels_id
        && let Some(o) = db::find::<Document>(conn, cid)?
    {
        refs.push((o.number, o.date));
    }
    if doc.doc_type == "credit_note"
        && let Some(sid) = doc.source_id
        && let Some(o) = db::find::<Document>(conn, sid)?
        && o.doc_type == "invoice"
    {
        refs.push((o.number, o.date));
    }
    for l in lines.iter().filter(|l| l.line_type == "deduction") {
        if let Some(pid) = l.ref_document_id
            && let Some(p) = db::find::<Document>(conn, pid)?
            && !refs.iter().any(|(n, _)| *n == p.number)
        {
            refs.push((p.number, p.date));
        }
    }
    for (n, d) in refs {
        let _ = write!(x, "<ram:InvoiceReferencedDocument><ram:IssuerAssignedID>{}</ram:IssuerAssignedID><ram:FormattedIssueDateTime><qdt:DateTimeString format=\"102\">{}</qdt:DateTimeString></ram:FormattedIssueDateTime></ram:InvoiceReferencedDocument>", esc(&n), date102(d));
    }
    x.push_str("</ram:ApplicableHeaderTradeSettlement>");
    x.push_str("</rsm:SupplyChainTradeTransaction></rsm:CrossIndustryInvoice>");
    Ok(x)
}

// ---------------------------------------------------------------------------
// ZUGFeRD: XMP metadata for PDF/A-3 + attachment extraction
// ---------------------------------------------------------------------------

const FX_XMP: &str = r#"<rdf:Description rdf:about="" xmlns:pdfaExtension="http://www.aiim.org/pdfa/ns/extension/" xmlns:pdfaSchema="http://www.aiim.org/pdfa/ns/schema#" xmlns:pdfaProperty="http://www.aiim.org/pdfa/ns/property#"><pdfaExtension:schemas><rdf:Bag><rdf:li rdf:parseType="Resource"><pdfaSchema:schema>Factur-X PDFA Extension Schema</pdfaSchema:schema><pdfaSchema:namespaceURI>urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#</pdfaSchema:namespaceURI><pdfaSchema:prefix>fx</pdfaSchema:prefix><pdfaSchema:property><rdf:Seq><rdf:li rdf:parseType="Resource"><pdfaProperty:name>DocumentFileName</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>The name of the embedded XML document</pdfaProperty:description></rdf:li><rdf:li rdf:parseType="Resource"><pdfaProperty:name>DocumentType</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>The type of the hybrid document in capital letters, e.g. INVOICE or ORDER</pdfaProperty:description></rdf:li><rdf:li rdf:parseType="Resource"><pdfaProperty:name>Version</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>The actual version of the standard applying to the embedded XML document</pdfaProperty:description></rdf:li><rdf:li rdf:parseType="Resource"><pdfaProperty:name>ConformanceLevel</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>The conformance level of the embedded XML document</pdfaProperty:description></rdf:li></rdf:Seq></pdfaSchema:property></rdf:li></rdf:Bag></pdfaExtension:schemas></rdf:Description><rdf:Description rdf:about="" xmlns:fx="urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#"><fx:DocumentType>INVOICE</fx:DocumentType><fx:DocumentFileName>factur-x.xml</fx:DocumentFileName><fx:Version>1.0</fx:Version><fx:ConformanceLevel>EN 16931</fx:ConformanceLevel></rdf:Description>"#;

/// Adds the Factur-X XMP extension to a PDF/A-3 file produced by Typst.
pub fn add_facturx_xmp(pdf: &[u8]) -> Result<Vec<u8>> {
    let mut doc = lopdf::Document::load_mem(pdf).context("PDF konnte nicht gelesen werden")?;
    let meta_id = doc.catalog()?.get(b"Metadata")?.as_reference()?;
    let stream = doc.get_object_mut(meta_id)?.as_stream_mut()?;
    let content = stream.decompressed_content().unwrap_or_else(|_| stream.content.clone());
    let mut xmp = String::from_utf8_lossy(&content).to_string();
    if !xmp.contains("urn:factur-x:pdfa") {
        if let Some(pos) = xmp.rfind("</rdf:RDF>") {
            xmp.insert_str(pos, FX_XMP);
        } else {
            anyhow::bail!("XMP-Metadaten ohne rdf:RDF");
        }
    }
    stream.dict.remove(b"Filter");
    stream.dict.remove(b"DecodeParms");
    stream.set_content(xmp.into_bytes());
    let mut out = Vec::new();
    doc.save_to(&mut out)?;
    Ok(out)
}

/// Extracts an embedded XML invoice (factur-x.xml, zugferd-invoice.xml, xrechnung.xml) from a PDF.
pub fn extract_xml_from_pdf(pdf: &[u8]) -> Option<String> {
    let doc = lopdf::Document::load_mem(pdf).ok()?;
    let mut candidates: Vec<(String, Vec<u8>)> = Vec::new();
    for (_, obj) in doc.objects.iter() {
        let Ok(dict) = obj.as_dict() else { continue };
        let is_filespec = dict.get(b"Type").ok().and_then(|t| t.as_name().ok()).map(|n| n == b"Filespec").unwrap_or(false)
            || dict.get(b"EF").is_ok();
        if !is_filespec {
            continue;
        }
        let name = dict
            .get(b"UF")
            .or_else(|_| dict.get(b"F"))
            .ok()
            .and_then(|o| o.as_str().ok())
            .map(|s| String::from_utf8_lossy(s).to_string())
            .unwrap_or_default();
        let Ok(ef) = dict.get(b"EF").and_then(|e| e.as_dict()) else { continue };
        let Ok(fref) = ef.get(b"F").or_else(|_| ef.get(b"UF")).and_then(|f| f.as_reference()) else { continue };
        let Ok(stream) = doc.get_object(fref).and_then(|o| o.as_stream()) else { continue };
        let data = stream.decompressed_content().unwrap_or_else(|_| stream.content.clone());
        candidates.push((name.to_lowercase(), data));
    }
    candidates.sort_by_key(|(n, _)| {
        if n.contains("factur-x") || n.contains("zugferd") || n.contains("xrechnung") { 0 } else if n.ends_with(".xml") { 1 } else { 2 }
    });
    candidates
        .into_iter()
        .map(|(_, d)| String::from_utf8_lossy(&d).trim_start_matches('\u{feff}').to_string())
        .find(|s| s.contains("CrossIndustryInvoice") || s.contains("<Invoice") || s.contains(":Invoice"))
}

// ---------------------------------------------------------------------------
// Parsing incoming e-invoices
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct ParsedParty {
    pub name: String,
    pub street: String,
    pub zip: String,
    pub city: String,
    pub country: String,
    pub vat_id: String,
    pub tax_number: String,
    pub email: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct ParsedLine {
    pub name: String,
    pub description: String,
    #[ts(type = "number")]
    pub quantity: Decimal,
    pub unit_code: String,
    #[ts(type = "number")]
    pub unit_price: Decimal,
    #[ts(type = "number")]
    pub net: Decimal,
    #[ts(type = "number")]
    pub tax_rate: Decimal,
    pub tax_category: String,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct ParsedTax {
    pub category: String,
    #[ts(type = "number")]
    pub rate: Decimal,
    #[ts(type = "number")]
    pub basis: Decimal,
    #[ts(type = "number")]
    pub tax: Decimal,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct ParsedInvoice {
    pub syntax: String,
    pub profile: String,
    pub number: String,
    pub type_code: String,
    pub issue_date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
    pub delivery_date: Option<NaiveDate>,
    pub currency: String,
    pub seller: ParsedParty,
    pub buyer: ParsedParty,
    pub lines: Vec<ParsedLine>,
    pub taxes: Vec<ParsedTax>,
    #[ts(type = "number")]
    pub net_total: Decimal,
    #[ts(type = "number")]
    pub tax_total: Decimal,
    #[ts(type = "number")]
    pub gross_total: Decimal,
    #[ts(type = "number")]
    pub prepaid: Decimal,
    #[ts(type = "number")]
    pub due_amount: Decimal,
    pub iban: String,
    pub bic: String,
    pub payment_terms: String,
    pub buyer_reference: String,
    pub order_reference: String,
    pub notes: Vec<String>,
}

fn kids<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Vec<roxmltree::Node<'a, 'i>> {
    n.children().filter(|c| c.is_element() && c.tag_name().name() == name).collect()
}

fn path<'a, 'i>(n: roxmltree::Node<'a, 'i>, names: &[&str]) -> Option<roxmltree::Node<'a, 'i>> {
    let mut cur = n;
    for name in names {
        cur = cur.children().find(|c| c.is_element() && c.tag_name().name() == *name)?;
    }
    Some(cur)
}

fn text(n: roxmltree::Node, names: &[&str]) -> String {
    path(n, names).and_then(|x| x.text()).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn num(n: roxmltree::Node, names: &[&str]) -> Decimal {
    Decimal::from_str(&text(n, names)).unwrap_or_default()
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    NaiveDate::parse_from_str(s, "%Y%m%d").ok().or_else(|| NaiveDate::parse_from_str(s.get(..10).unwrap_or(s), "%Y-%m-%d").ok())
}

fn cii_party(p: Option<roxmltree::Node>) -> ParsedParty {
    let Some(p) = p else { return ParsedParty::default() };
    let mut out = ParsedParty {
        name: text(p, &["Name"]),
        street: text(p, &["PostalTradeAddress", "LineOne"]),
        zip: text(p, &["PostalTradeAddress", "PostcodeCode"]),
        city: text(p, &["PostalTradeAddress", "CityName"]),
        country: text(p, &["PostalTradeAddress", "CountryID"]),
        email: text(p, &["DefinedTradeContact", "EmailURIUniversalCommunication", "URIID"]),
        phone: text(p, &["DefinedTradeContact", "TelephoneUniversalCommunication", "CompleteNumber"]),
        ..Default::default()
    };
    if out.email.is_empty() {
        out.email = text(p, &["URIUniversalCommunication", "URIID"]);
    }
    for reg in kids(p, "SpecifiedTaxRegistration") {
        if let Some(id) = path(reg, &["ID"]) {
            let v = id.text().unwrap_or_default().trim().to_string();
            match id.attribute("schemeID") {
                Some("VA") => out.vat_id = v,
                Some("FC") => out.tax_number = v,
                _ => {}
            }
        }
    }
    out
}

fn parse_cii(root: roxmltree::Node) -> ParsedInvoice {
    let mut inv = ParsedInvoice { syntax: "CII".into(), ..Default::default() };
    inv.profile = text(root, &["ExchangedDocumentContext", "GuidelineSpecifiedDocumentContextParameter", "ID"]);
    if let Some(d) = path(root, &["ExchangedDocument"]) {
        inv.number = text(d, &["ID"]);
        inv.type_code = text(d, &["TypeCode"]);
        inv.issue_date = parse_date(&text(d, &["IssueDateTime", "DateTimeString"]));
        // Registration notes (SubjectCode REG) only carry the seller's legal details.
        inv.notes = kids(d, "IncludedNote")
            .into_iter()
            .filter(|n| text(*n, &["SubjectCode"]) != "REG")
            .map(|n| text(n, &["Content"]))
            .filter(|s| !s.is_empty())
            .collect();
    }
    let Some(tx) = path(root, &["SupplyChainTradeTransaction"]) else { return inv };
    for li in kids(tx, "IncludedSupplyChainTradeLineItem") {
        let qty_node = path(li, &["SpecifiedLineTradeDelivery", "BilledQuantity"]);
        let quantity = qty_node.and_then(|n| n.text()).and_then(|t| Decimal::from_str(t.trim()).ok()).unwrap_or(Decimal::ONE);
        let mut price = num(li, &["SpecifiedLineTradeAgreement", "NetPriceProductTradePrice", "ChargeAmount"]);
        let basis = num(li, &["SpecifiedLineTradeAgreement", "NetPriceProductTradePrice", "BasisQuantity"]);
        if basis > Decimal::ZERO {
            price /= basis;
        }
        inv.lines.push(ParsedLine {
            name: text(li, &["SpecifiedTradeProduct", "Name"]),
            description: text(li, &["SpecifiedTradeProduct", "Description"]),
            quantity,
            unit_code: qty_node.and_then(|n| n.attribute("unitCode")).unwrap_or_default().to_string(),
            unit_price: price,
            net: num(li, &["SpecifiedLineTradeSettlement", "SpecifiedTradeSettlementLineMonetarySummation", "LineTotalAmount"]),
            tax_rate: num(li, &["SpecifiedLineTradeSettlement", "ApplicableTradeTax", "RateApplicablePercent"]),
            tax_category: text(li, &["SpecifiedLineTradeSettlement", "ApplicableTradeTax", "CategoryCode"]),
        });
    }
    if let Some(a) = path(tx, &["ApplicableHeaderTradeAgreement"]) {
        inv.buyer_reference = text(a, &["BuyerReference"]);
        inv.seller = cii_party(path(a, &["SellerTradeParty"]));
        inv.buyer = cii_party(path(a, &["BuyerTradeParty"]));
        inv.order_reference = text(a, &["BuyerOrderReferencedDocument", "IssuerAssignedID"]);
    }
    if let Some(dl) = path(tx, &["ApplicableHeaderTradeDelivery"]) {
        inv.delivery_date = parse_date(&text(dl, &["ActualDeliverySupplyChainEvent", "OccurrenceDateTime", "DateTimeString"]));
    }
    if let Some(st) = path(tx, &["ApplicableHeaderTradeSettlement"]) {
        inv.currency = text(st, &["InvoiceCurrencyCode"]);
        if let Some(pm) = kids(st, "SpecifiedTradeSettlementPaymentMeans").into_iter().next() {
            inv.iban = text(pm, &["PayeePartyCreditorFinancialAccount", "IBANID"]);
            inv.bic = text(pm, &["PayeeSpecifiedCreditorFinancialInstitution", "BICID"]);
        }
        for t in kids(st, "ApplicableTradeTax") {
            inv.taxes.push(ParsedTax {
                category: text(t, &["CategoryCode"]),
                rate: num(t, &["RateApplicablePercent"]),
                basis: num(t, &["BasisAmount"]),
                tax: num(t, &["CalculatedAmount"]),
            });
        }
        if let Some(pt) = kids(st, "SpecifiedTradePaymentTerms").into_iter().next() {
            inv.payment_terms = text(pt, &["Description"]);
            inv.due_date = parse_date(&text(pt, &["DueDateDateTime", "DateTimeString"]));
        }
        if let Some(m) = path(st, &["SpecifiedTradeSettlementHeaderMonetarySummation"]) {
            inv.net_total = num(m, &["TaxBasisTotalAmount"]);
            inv.tax_total = kids(m, "TaxTotalAmount").first().and_then(|n| n.text()).and_then(|t| Decimal::from_str(t.trim()).ok()).unwrap_or_default();
            inv.gross_total = num(m, &["GrandTotalAmount"]);
            inv.prepaid = num(m, &["TotalPrepaidAmount"]);
            inv.due_amount = num(m, &["DuePayableAmount"]);
        }
    }
    inv
}

fn ubl_party(p: Option<roxmltree::Node>) -> ParsedParty {
    let Some(p) = p else { return ParsedParty::default() };
    let p = path(p, &["Party"]).unwrap_or(p);
    let mut name = text(p, &["PartyName", "Name"]);
    if name.is_empty() {
        name = text(p, &["PartyLegalEntity", "RegistrationName"]);
    }
    let mut out = ParsedParty {
        name,
        street: text(p, &["PostalAddress", "StreetName"]),
        zip: text(p, &["PostalAddress", "PostalZone"]),
        city: text(p, &["PostalAddress", "CityName"]),
        country: text(p, &["PostalAddress", "Country", "IdentificationCode"]),
        email: text(p, &["Contact", "ElectronicMail"]),
        phone: text(p, &["Contact", "Telephone"]),
        ..Default::default()
    };
    if out.email.is_empty() {
        out.email = text(p, &["EndpointID"]);
    }
    for ts in kids(p, "PartyTaxScheme") {
        let id = text(ts, &["CompanyID"]);
        let scheme = text(ts, &["TaxScheme", "ID"]);
        if scheme == "VAT" {
            out.vat_id = id;
        } else if !id.is_empty() {
            out.tax_number = id;
        }
    }
    out
}

fn parse_ubl(root: roxmltree::Node) -> ParsedInvoice {
    let mut inv = ParsedInvoice { syntax: "UBL".into(), ..Default::default() };
    inv.profile = text(root, &["CustomizationID"]);
    inv.number = text(root, &["ID"]);
    inv.type_code = {
        let t = text(root, &["InvoiceTypeCode"]);
        if t.is_empty() { text(root, &["CreditNoteTypeCode"]) } else { t }
    };
    inv.issue_date = parse_date(&text(root, &["IssueDate"]));
    inv.due_date = parse_date(&text(root, &["DueDate"]));
    inv.currency = text(root, &["DocumentCurrencyCode"]);
    inv.buyer_reference = text(root, &["BuyerReference"]);
    inv.order_reference = text(root, &["OrderReference", "ID"]);
    inv.notes = kids(root, "Note").into_iter().filter_map(|n| n.text().map(|t| t.trim().to_string())).collect();
    inv.seller = ubl_party(path(root, &["AccountingSupplierParty"]));
    inv.buyer = ubl_party(path(root, &["AccountingCustomerParty"]));
    inv.delivery_date = parse_date(&text(root, &["Delivery", "ActualDeliveryDate"]));
    if let Some(pm) = kids(root, "PaymentMeans").into_iter().next() {
        inv.iban = text(pm, &["PayeeFinancialAccount", "ID"]);
        inv.bic = text(pm, &["PayeeFinancialAccount", "FinancialInstitutionBranch", "ID"]);
        if inv.due_date.is_none() {
            inv.due_date = parse_date(&text(pm, &["PaymentDueDate"]));
        }
    }
    inv.payment_terms = text(root, &["PaymentTerms", "Note"]);
    for tt in kids(root, "TaxTotal") {
        for sub in kids(tt, "TaxSubtotal") {
            inv.taxes.push(ParsedTax {
                category: text(sub, &["TaxCategory", "ID"]),
                rate: num(sub, &["TaxCategory", "Percent"]),
                basis: num(sub, &["TaxableAmount"]),
                tax: num(sub, &["TaxAmount"]),
            });
        }
        if inv.tax_total.is_zero() {
            inv.tax_total = num(tt, &["TaxAmount"]);
        }
    }
    if let Some(m) = path(root, &["LegalMonetaryTotal"]) {
        inv.net_total = num(m, &["TaxExclusiveAmount"]);
        inv.gross_total = num(m, &["TaxInclusiveAmount"]);
        inv.prepaid = num(m, &["PrepaidAmount"]);
        inv.due_amount = num(m, &["PayableAmount"]);
    }
    let line_tag = if root.tag_name().name() == "CreditNote" { "CreditNoteLine" } else { "InvoiceLine" };
    for li in kids(root, line_tag) {
        let qn = path(li, &["InvoicedQuantity"]).or_else(|| path(li, &["CreditedQuantity"]));
        inv.lines.push(ParsedLine {
            name: text(li, &["Item", "Name"]),
            description: text(li, &["Item", "Description"]),
            quantity: qn.and_then(|n| n.text()).and_then(|t| Decimal::from_str(t.trim()).ok()).unwrap_or(Decimal::ONE),
            unit_code: qn.and_then(|n| n.attribute("unitCode")).unwrap_or_default().to_string(),
            unit_price: num(li, &["Price", "PriceAmount"]),
            net: num(li, &["LineExtensionAmount"]),
            tax_rate: num(li, &["Item", "ClassifiedTaxCategory", "Percent"]),
            tax_category: text(li, &["Item", "ClassifiedTaxCategory", "ID"]),
        });
    }
    inv
}

pub fn parse_xml(xml: &str) -> Result<ParsedInvoice> {
    let xml = xml.trim_start_matches('\u{feff}');
    let doc = roxmltree::Document::parse(xml).context("Die Datei ist kein gültiges XML")?;
    let root = doc.root_element();
    match root.tag_name().name() {
        "CrossIndustryInvoice" => Ok(parse_cii(root)),
        "Invoice" | "CreditNote" => Ok(parse_ubl(root)),
        other => Err(crate::error::UserError(format!("Unbekanntes E-Rechnungsformat ({other}). Unterstützt werden XRechnung/ZUGFeRD (CII) und UBL.")).into()),
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ImportResult {
    pub parsed: ParsedInvoice,
    pub draft: ReceiptInput,
    pub contact_match: Option<Contact>,
    pub warnings: Vec<String>,
    pub file_id: Option<i64>,
    pub file: Option<FileMeta>,
}

pub fn to_receipt(conn: &Connection, inv: &ParsedInvoice, source: &str) -> Result<(ReceiptInput, Option<Contact>, Vec<String>)> {
    let s = cfg::load(conn)?;
    let mut warnings = Vec::new();
    let contacts = db::query::<Contact>(conn, "WHERE archived = 0", &[])?;
    let norm = |x: &str| x.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let matched = contacts
        .iter()
        .find(|c| !inv.seller.vat_id.is_empty() && norm(&c.vat_id) == norm(&inv.seller.vat_id))
        .or_else(|| contacts.iter().find(|c| !inv.seller.name.is_empty() && norm(&c.display_name()) == norm(&inv.seller.name)))
        .cloned();
    let default_account = crate::services::receipts::suggest_account(conn, matched.as_ref().map(|c| c.id), &inv.seller.name)?
        .map(|(a, _)| a)
        .unwrap_or_else(|| crate::chart::role(&s.tax.chart, "sonst_aufw"));
    let has = |cat: &str| inv.taxes.iter().any(|t| t.category == cat) || inv.lines.iter().any(|l| l.tax_category == cat);
    let seller_country = if inv.seller.country.is_empty() { "DE".to_string() } else { inv.seller.country.to_uppercase() };
    let tax_mode = if has("AE") {
        if seller_country != "DE" && util::is_eu(&seller_country) { "rc_eu" } else { "rc_other" }
    } else if has("K") {
        "eu_acquisition"
    } else {
        "standard"
    };
    let credit = inv.type_code == "381";
    let sign = if credit { Decimal::NEGATIVE_ONE } else { Decimal::ONE };
    // One receipt line per VAT rate keeps receipts readable; names are kept as description.
    let mut groups: Vec<(Decimal, Decimal, Vec<String>)> = Vec::new();
    for l in &inv.lines {
        // Reverse charge / intra-EU: the supplier invoices 0 %, we owe German VAT (19 %).
        let rate = if tax_mode == "standard" { l.tax_rate } else { Decimal::from(19) };
        match groups.iter_mut().find(|g| g.0 == rate) {
            Some(g) => {
                g.1 += l.net;
                g.2.push(l.name.clone());
            }
            None => groups.push((rate, l.net, vec![l.name.clone()])),
        }
    }
    if groups.is_empty() {
        for t in &inv.taxes {
            groups.push((t.rate, t.basis, vec![]));
        }
    }
    let lines: Vec<ReceiptLine> = groups
        .into_iter()
        .map(|(rate, net, names)| {
            let mut desc = names.into_iter().filter(|n| !n.is_empty()).collect::<Vec<_>>().join(", ");
            if desc.chars().count() > 120 {
                desc = desc.chars().take(117).collect::<String>() + "…";
            }
            ReceiptLine { account: default_account.clone(), description: desc, net: net * sign, tax_rate: rate, ..Default::default() }
        })
        .collect();
    let receipt = Receipt {
        receipt_type: "expense".into(),
        contact_id: matched.as_ref().map(|c| c.id),
        supplier_name: inv.seller.name.clone(),
        invoice_number: inv.number.clone(),
        date: inv.issue_date.unwrap_or_else(util::today),
        due_date: inv.due_date,
        service_date: inv.delivery_date,
        description: inv.lines.first().map(|l| l.name.clone()).unwrap_or_default(),
        tax_mode: tax_mode.into(),
        prices_gross: false,
        source: source.into(),
        iban: inv.iban.clone(),
        notes: inv.notes.join("\n"),
        ..Default::default()
    };
    let mut r = receipt.clone();
    let mut l2 = lines.clone();
    crate::services::receipts::calculate(&mut r, &mut l2);
    if tax_mode == "standard" && (r.gross_total - inv.gross_total * sign).abs() > Decimal::new(2, 2) && !inv.gross_total.is_zero() {
        warnings.push(format!(
            "Der berechnete Bruttobetrag ({}) weicht vom Rechnungsbetrag ({}) ab. Bitte prüfen.",
            util::fmt_money(r.gross_total),
            util::fmt_money(inv.gross_total)
        ));
    }
    if !inv.buyer.vat_id.is_empty() && !s.company.vat_id.is_empty() && norm(&inv.buyer.vat_id) != norm(&s.company.vat_id) {
        warnings.push("Die USt-IdNr. des Rechnungsempfängers stimmt nicht mit Ihrer USt-IdNr. überein.".into());
    }
    if matched.is_none() {
        warnings.push(format!("Lieferant „{}“ ist noch nicht als Kontakt angelegt.", inv.seller.name));
    }
    if inv.currency != "EUR" && !inv.currency.is_empty() {
        warnings.push(format!("Rechnungswährung {} – Beträge bitte in EUR umrechnen.", inv.currency));
    }
    Ok((ReceiptInput { receipt, lines, payment: None }, matched, warnings))
}
