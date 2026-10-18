// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Reports of payment service providers (PayPal activity download, Stripe balance reports, SumUp
//! and similar exports): every row with gross, fee and net amount becomes the payment itself plus
//! a separate fee movement; payouts to and top-ups from the bank go to Geldtransit.
//!
//! The columns are found by name (German and English exports). The fee is taken as net − gross,
//! which works whatever sign the provider uses for the fee column.

use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::services::bankimport::{ImportPreview, ImportedTx, parse_amount, parse_date, split_rows};

/// Movement kinds of imported rows.
pub const FEE: &str = "fee";
pub const TRANSIT: &str = "transit";

/// Types of payouts to the bank and of top-ups from the bank.
const TRANSIT_TYPES: &[&str] = &[
    "abbuchung",
    "auszahlung",
    "bankgutschrift",
    "einzahlung",
    "überweisung auf bankkonto",
    "withdrawal",
    "transfer to bank",
    "bank deposit",
    "payout",
    "topup",
];
const FEE_TYPES: &[&str] = &["fee", "stripe_fee", "network_cost", "gebühr", "gebuehr"];
/// Rows that did not (yet) change the balance.
const SKIP_STATUS: &[&str] = &["ausstehend", "pending", "fehlgeschlagen", "failed", "abgelehnt", "declined", "abgebrochen", "canceled", "cancelled", "void", "expired"];

struct Cols {
    date: usize,
    id: usize,
    gross: usize,
    fee: Option<usize>,
    net: Option<usize>,
    kind: Option<usize>,
    status: Option<usize>,
    currency: Option<usize>,
    name: Option<usize>,
    email: Option<usize>,
    description: Option<usize>,
    invoice: Option<usize>,
    impact: Option<usize>,
}

fn col(headers: &[String], names: &[&str]) -> Option<usize> {
    headers.iter().position(|h| {
        let h = h.trim().trim_matches('"').to_lowercase();
        names.contains(&h.as_str())
    })
}

fn columns(headers: &[String]) -> Option<Cols> {
    let cols = Cols {
        date: col(headers, &["datum", "date", "created", "created_utc", "created (utc)", "transaktionsdatum", "erstellt", "erstellt (utc)"])?,
        id: col(headers, &["transaktionscode", "transaction id", "balance_transaction_id", "id", "transaktions-id", "transaktions id", "transaktionsnummer"])?,
        gross: col(headers, &["brutto", "gross", "amount", "betrag", "bruttobetrag", "gesamtbetrag", "total amount"])?,
        fee: col(headers, &["gebühr", "gebühren", "gebuehr", "fee", "fees", "transaktionsgebühr"]),
        net: col(headers, &["netto", "net", "nettobetrag", "net amount"]),
        kind: col(headers, &["typ", "type", "reporting_category", "transaktionstyp", "transaction type"]),
        status: col(headers, &["status"]),
        currency: col(headers, &["währung", "waehrung", "currency"]),
        name: col(headers, &["name", "customer_name", "customer name", "kunde"]),
        email: col(headers, &["absender e-mail-adresse", "from email address", "customer_email", "e-mail", "email", "e-mail-adresse"]),
        description: col(headers, &["artikelbezeichnung", "item title", "description", "beschreibung", "betreff", "subject"]),
        invoice: col(headers, &["rechnungsnummer", "invoice number", "invoice number text"]),
        impact: col(headers, &["auswirkung auf guthaben", "balance impact"]),
    };
    // Without a fee column this is an ordinary bank export.
    if cols.fee.is_none() || (cols.net.is_none() && cols.kind.is_none()) {
        return None;
    }
    Some(cols)
}

fn provider(headers: &[String]) -> (&'static str, &'static str) {
    let has = |names: &[&str]| col(headers, names).is_some();
    if has(&["auswirkung auf guthaben", "balance impact", "zeitzone", "timezone"]) {
        ("paypal", "PayPal")
    } else if has(&["balance_transaction_id", "reporting_category", "available on (utc)", "available_on_utc"]) {
        ("stripe", "Stripe")
    } else if has(&["kartentyp", "card type", "letzte ziffern", "last 4 digits", "trinkgeld", "tip amount"]) {
        ("sumup", "SumUp")
    } else {
        ("psp", "Zahlungsdienstleister")
    }
}

/// Provider named in a bank movement (payouts and direct debits of PayPal, Stripe, SumUp).
pub fn provider_of(text: &str) -> Option<&'static str> {
    let t = text.to_lowercase();
    [("paypal", "paypal"), ("stripe", "stripe"), ("sumup", "sumup")].into_iter().find(|(k, _)| t.contains(k)).map(|(_, p)| p)
}

/// Dates like 03/04/2026 are read as month/day when the file is from a US account (some day in
/// the file is greater than 12 in the second position).
fn date_parser(rows: &[Vec<String>], c: usize) -> impl Fn(&str) -> Option<NaiveDate> {
    let us = rows.iter().filter_map(|r| r.get(c)).any(|d| {
        let parts: Vec<&str> = d.trim().split('/').collect();
        parts.len() == 3 && parts[1].parse::<u32>().is_ok_and(|x| x > 12)
    });
    move |s: &str| {
        let s = s.trim();
        if us && let Ok(d) = NaiveDate::parse_from_str(s.get(..10).unwrap_or(s), "%m/%d/%Y") {
            return Some(d);
        }
        parse_date(s)
    }
}

fn tx(date: NaiveDate, amount: Decimal, counterparty: String, purpose: String, account: String, kind: &str, provider: &str) -> ImportedTx {
    ImportedTx {
        date,
        amount,
        counterparty,
        iban: String::new(),
        purpose,
        hash: String::new(),
        duplicate: false,
        locked: false,
        account,
        kind: kind.into(),
        provider: provider.into(),
    }
}

/// Parses a provider report; None if the file is no such report. `fee_account` and
/// `transit_account` are the ledger accounts for fees and transfers.
pub fn parse(text: &str, fee_account: &str, transit_account: &str) -> Option<ImportPreview> {
    let text = text.trim_start_matches('\u{feff}');
    let first = text.lines().take(10).collect::<Vec<_>>().join("\n");
    let delim = b",;\t".iter().copied().max_by_key(|d| first.matches(*d as char).count()).unwrap_or(b',');
    let rows = split_rows(text, delim);
    let (header_row, cols) = rows.iter().enumerate().take(10).find_map(|(i, r)| columns(r).map(|c| (i, c)))?;
    let headers = &rows[header_row];
    let (provider, label) = provider(headers);
    let parse_d = date_parser(&rows[header_row + 1..], cols.date);
    let cell = |r: &[String], c: Option<usize>| c.and_then(|i| r.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();
    let mut txs = Vec::new();
    let mut errors = Vec::new();
    let (mut skipped, mut foreign) = (0, 0);
    for (i, r) in rows.iter().enumerate().skip(header_row + 1) {
        if r.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let status = cell(r, cols.status).to_lowercase();
        let impact = cell(r, cols.impact).to_lowercase();
        if SKIP_STATUS.iter().any(|s| status.contains(s)) || impact == "memo" {
            skipped += 1;
            continue;
        }
        let currency = cell(r, cols.currency).to_uppercase();
        if !currency.is_empty() && currency != "EUR" {
            foreign += 1;
            continue;
        }
        let Some(date) = parse_d(&cell(r, Some(cols.date))) else {
            errors.push(format!("Zeile {}: Datum nicht lesbar", i + 1));
            continue;
        };
        let Some(gross) = parse_amount(&cell(r, Some(cols.gross))) else {
            errors.push(format!("Zeile {}: Betrag nicht lesbar", i + 1));
            continue;
        };
        let fee_col = parse_amount(&cell(r, cols.fee)).unwrap_or_default();
        let net = parse_amount(&cell(r, cols.net));
        // Effect of the fee on the balance (negative = paid).
        let fee = match net {
            Some(n) => n - gross,
            None => -fee_col.abs() * if gross < Decimal::ZERO { Decimal::NEGATIVE_ONE } else { Decimal::ONE },
        };
        let kind = cell(r, cols.kind);
        let kind_l = kind.to_lowercase();
        let id = cell(r, Some(cols.id));
        let party = [cell(r, cols.name), cell(r, cols.email)].into_iter().find(|s| !s.is_empty()).unwrap_or_else(|| label.to_string());

        if TRANSIT_TYPES.iter().any(|t| kind_l.contains(t)) {
            let amount = net.unwrap_or(gross);
            if !amount.is_zero() {
                txs.push(tx(date, amount, label.into(), format!("{kind} {id}").trim().to_string(), transit_account.into(), TRANSIT, provider));
            }
            continue;
        }
        if FEE_TYPES.iter().any(|t| kind_l == *t || kind_l.starts_with(&format!("{t} ")) || kind_l.contains("gebühr")) {
            let amount = net.unwrap_or(gross + fee);
            if !amount.is_zero() {
                txs.push(tx(date, amount, label.into(), format!("{kind} {id}").trim().to_string(), fee_account.into(), FEE, provider));
            }
            continue;
        }
        let invoice = cell(r, cols.invoice);
        let purpose = [kind.clone(), cell(r, cols.description), if invoice.is_empty() { String::new() } else { format!("Rechnung {invoice}") }, id.clone()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        if !gross.is_zero() {
            txs.push(tx(date, gross, party, purpose, String::new(), "", provider));
        }
        if !fee.is_zero() {
            txs.push(tx(date, fee, label.into(), format!("Gebühr {label} zu {id}"), fee_account.into(), FEE, provider));
        }
    }
    if foreign > 0 {
        let what = if foreign == 1 { "Umsatz" } else { "Umsätze" };
        errors.push(format!("{foreign} {what} in Fremdwährung nicht importiert – nur Euro-Beträge werden übernommen."));
    }
    if skipped > 0 {
        let what = if skipped == 1 { "Zeile" } else { "Zeilen" };
        errors.push(format!("{skipped} ausstehende, abgelehnte oder nicht saldowirksame {what} übersprungen."));
    }
    Some(ImportPreview {
        format: provider.into(),
        columns: headers.clone(),
        sample: vec![],
        mapping: None,
        transactions: txs,
        duplicates: 0,
        locked: 0,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn rows(p: &ImportPreview) -> Vec<(String, Decimal, &str, &str)> {
        p.transactions.iter().map(|t| (t.date.to_string(), t.amount, t.kind.as_str(), t.account.as_str())).collect()
    }

    const PAYPAL_DE: &str = "\u{feff}\"Datum\",\"Uhrzeit\",\"Zeitzone\",\"Name\",\"Typ\",\"Status\",\"Währung\",\"Brutto\",\"Gebühr\",\"Netto\",\"Absender E-Mail-Adresse\",\"Empfänger E-Mail-Adresse\",\"Transaktionscode\",\"Artikelbezeichnung\",\"Rechnungsnummer\",\"Auswirkung auf Guthaben\"\n\
\"02.03.2026\",\"10:15:00\",\"CET\",\"Kunde AG\",\"Website-Zahlung\",\"Abgeschlossen\",\"EUR\",\"1.190,00\",\"-41,90\",\"1.148,10\",\"einkauf@kunde.de\",\"shop@firma.de\",\"1AB23456CD789012E\",\"Beratung\",\"RE-2026-0001\",\"Haben\"\n\
\"03.03.2026\",\"11:00:00\",\"CET\",\"Kunde AG\",\"Rückzahlung\",\"Abgeschlossen\",\"EUR\",\"-119,00\",\"3,20\",\"-115,80\",\"\",\"einkauf@kunde.de\",\"9ZZ23456CD789012E\",\"\",\"\",\"Soll\"\n\
\"04.03.2026\",\"09:00:00\",\"CET\",\"\",\"Allgemeine Abbuchung – Bankkonto\",\"Abgeschlossen\",\"EUR\",\"-1.000,00\",\"0,00\",\"-1.000,00\",\"\",\"\",\"8XY23456CD789012E\",\"\",\"\",\"Soll\"\n\
\"05.03.2026\",\"09:00:00\",\"CET\",\"Stock AG\",\"Express-Checkout-Zahlung\",\"Abgeschlossen\",\"EUR\",\"-39,00\",\"0,00\",\"-39,00\",\"\",\"\",\"7XY23456CD789012E\",\"Foto\",\"\",\"Soll\"\n\
\"05.03.2026\",\"09:00:00\",\"CET\",\"\",\"Bankgutschrift auf PayPal-Konto\",\"Abgeschlossen\",\"EUR\",\"39,00\",\"0,00\",\"39,00\",\"\",\"\",\"6XY23456CD789012E\",\"\",\"\",\"Haben\"\n\
\"06.03.2026\",\"09:00:00\",\"CET\",\"Kunde US\",\"Website-Zahlung\",\"Abgeschlossen\",\"USD\",\"100,00\",\"-3,49\",\"96,51\",\"\",\"\",\"5XY23456CD789012E\",\"\",\"\",\"Haben\"\n\
\"07.03.2026\",\"09:00:00\",\"CET\",\"Kunde B\",\"Website-Zahlung\",\"Ausstehend\",\"EUR\",\"50,00\",\"-1,00\",\"49,00\",\"\",\"\",\"4XY23456CD789012E\",\"\",\"\",\"Haben\"\n\
\"07.03.2026\",\"09:00:00\",\"CET\",\"Kunde C\",\"Allgemeine Autorisierung\",\"Abgeschlossen\",\"EUR\",\"20,00\",\"0,00\",\"20,00\",\"\",\"\",\"3XY23456CD789012E\",\"\",\"\",\"Memo\"\n";

    #[test]
    fn paypal_german_activity_download() {
        let p = parse(PAYPAL_DE, "4970", "1360").unwrap();
        assert_eq!(p.format, "paypal");
        assert_eq!(
            rows(&p),
            vec![
                ("2026-03-02".into(), dec!(1190.00), "", ""),
                ("2026-03-02".into(), dec!(-41.90), "fee", "4970"),
                ("2026-03-03".into(), dec!(-119.00), "", ""),
                ("2026-03-03".into(), dec!(3.20), "fee", "4970"),
                ("2026-03-04".into(), dec!(-1000.00), "transit", "1360"),
                ("2026-03-05".into(), dec!(-39.00), "", ""),
                ("2026-03-05".into(), dec!(39.00), "transit", "1360"),
            ]
        );
        let sale = &p.transactions[0];
        assert_eq!(sale.counterparty, "Kunde AG");
        assert_eq!(sale.purpose, "Website-Zahlung · Beratung · Rechnung RE-2026-0001 · 1AB23456CD789012E");
        assert_eq!(p.transactions[1].purpose, "Gebühr PayPal zu 1AB23456CD789012E");
        assert!(p.errors.iter().any(|e| e.contains("1 Umsatz in Fremdwährung")), "{:?}", p.errors);
        assert!(p.errors.iter().any(|e| e.contains("2 ausstehende")), "{:?}", p.errors);
    }

    #[test]
    fn paypal_english_us_dates() {
        let csv = "Date,Time,TimeZone,Name,Type,Status,Currency,Gross,Fee,Net,From Email Address,Transaction ID,Balance Impact\n\
                   03/04/2026,10:00:00,PST,Buyer,Website Payment,Completed,EUR,\"1,000.00\",-35.00,965.00,b@x.com,1A,Credit\n\
                   03/18/2026,10:00:00,PST,,General Withdrawal,Completed,EUR,-965.00,0.00,-965.00,,2A,Debit\n";
        let p = parse(csv, "4970", "1360").unwrap();
        assert_eq!(
            rows(&p),
            vec![("2026-03-04".into(), dec!(1000.00), "", ""), ("2026-03-04".into(), dec!(-35.00), "fee", "4970"), ("2026-03-18".into(), dec!(-965.00), "transit", "1360")]
        );
    }

    #[test]
    fn stripe_balance_report() {
        let csv = "balance_transaction_id,created_utc,available_on_utc,currency,gross,fee,net,reporting_category,source_id,description,customer_name,customer_email\n\
                   txn_1,2026-03-02 10:00:00,2026-03-09 00:00:00,eur,119.00,1.92,117.08,charge,ch_1,Rechnung RE-2026-0002,Kunde AG,a@b.de\n\
                   txn_2,2026-03-03 10:00:00,2026-03-10 00:00:00,eur,-119.00,0.00,-119.00,refund,re_1,,Kunde AG,\n\
                   txn_3,2026-03-05 10:00:00,2026-03-05 00:00:00,eur,-5.00,0.00,-5.00,fee,,Billing usage,,\n\
                   txn_4,2026-03-09 10:00:00,2026-03-09 00:00:00,eur,-112.08,0.00,-112.08,payout,po_1,STRIPE PAYOUT,,\n";
        let p = parse(csv, "4970", "1360").unwrap();
        assert_eq!(p.format, "stripe");
        assert_eq!(
            rows(&p),
            vec![
                ("2026-03-02".into(), dec!(119.00), "", ""),
                ("2026-03-02".into(), dec!(-1.92), "fee", "4970"),
                ("2026-03-03".into(), dec!(-119.00), "", ""),
                ("2026-03-05".into(), dec!(-5.00), "fee", "4970"),
                ("2026-03-09".into(), dec!(-112.08), "transit", "1360"),
            ]
        );
        assert_eq!(p.transactions[0].counterparty, "Kunde AG");
        assert!(p.transactions[0].purpose.contains("RE-2026-0002"));
    }

    #[test]
    fn sumup_like_export_without_net() {
        let csv = "E-Mail-Adresse;Datum;Transaktions-ID;Zahlungsart;Status;Kartentyp;Beschreibung;Betrag;Gebühr\n\
                   kasse@firma.de;2026-03-02 12:01;TX1;Karte;Erfolgreich;VISA;Kaffee;4,50;0,07\n\
                   kasse@firma.de;2026-03-02 12:05;TX2;Karte;Fehlgeschlagen;VISA;Kaffee;4,50;0,07\n";
        // No type and no net column: not recognised – it is read like a bank export.
        assert!(parse(csv, "4970", "1360").is_none());
        let with_net = csv.replace("Betrag;Gebühr", "Betrag;Gebühr;Netto").replace("4,50;0,07\n", "4,50;0,07;4,43\n");
        let p = parse(&with_net, "4970", "1360").unwrap();
        assert_eq!(p.format, "sumup");
        assert_eq!(rows(&p), vec![("2026-03-02".into(), dec!(4.50), "", ""), ("2026-03-02".into(), dec!(-0.07), "fee", "4970")]);
    }

    #[test]
    fn bank_exports_are_not_taken_for_provider_reports() {
        let bank = "Buchungstag;Name Zahlungsbeteiligter;Verwendungszweck;Betrag\n03.02.2021;Café;Kartenzahlung;-4,90\n";
        assert!(parse(bank, "4970", "1360").is_none());
        assert_eq!(provider_of("PayPal Europe S.a.r.l. et Cie S.C.A"), Some("paypal"));
        assert_eq!(provider_of("STRIPE PAYMENTS EUROPE"), Some("stripe"));
        assert_eq!(provider_of("Telekom"), None);
    }
}
