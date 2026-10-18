// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Import von Kontoumsätzen: CSV (automatische Spaltenerkennung für gängige
//! deutsche Banken), CAMT.053 (ISO 20022) und Berichte von Zahlungsdienstleistern (`psp`).

use std::str::FromStr;

use anyhow::Result;
use chrono::NaiveDate;
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::{PaymentAccount, Transaction};
use crate::services::finance::{self, TransactionInput};
use crate::services::psp;

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct CsvMapping {
    pub delimiter: String,
    pub header_row: i64,
    pub date_col: i64,
    pub amount_col: i64,
    pub debit_col: i64,
    pub credit_col: i64,
    pub counterparty_col: i64,
    pub iban_col: i64,
    pub purpose_col: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportedTx {
    pub date: NaiveDate,
    #[ts(type = "number")]
    pub amount: Decimal,
    pub counterparty: String,
    pub iban: String,
    pub purpose: String,
    pub hash: String,
    pub duplicate: bool,
    /// Dated within the festgeschriebenen period – cannot be imported.
    #[serde(default)]
    pub locked: bool,
    /// Booked to this account right away (fees and transfers in provider reports).
    #[serde(default)]
    pub account: String,
    /// "" | fee | transit
    #[serde(default)]
    pub kind: String,
    /// Payment service provider of the report (paypal, stripe, sumup, psp); empty for bank data.
    #[serde(default)]
    pub provider: String,
    /// End-to-end reference and return reason (CAMT); appended to the purpose when imported, but
    /// not part of the hash, so that files imported before are still recognised.
    #[serde(default)]
    pub reference: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ImportPreview {
    pub format: String,
    pub columns: Vec<String>,
    pub sample: Vec<Vec<String>>,
    pub mapping: Option<CsvMapping>,
    pub transactions: Vec<ImportedTx>,
    pub duplicates: i64,
    pub locked: i64,
    pub errors: Vec<String>,
}

fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.trim_start_matches('\u{feff}').to_string(),
        Err(_) => encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned(),
    }
}

pub fn parse_amount(s: &str) -> Option<Decimal> {
    let mut t: String = s.trim().replace(['€', ' ', '\u{a0}', '+'], "").replace("EUR", "");
    if t.is_empty() {
        return None;
    }
    let neg = t.ends_with('-') || t.starts_with('-') || t.starts_with('(');
    t = t.trim_matches(|c| c == '-' || c == '(' || c == ')').to_string();
    let last_comma = t.rfind(',');
    let last_dot = t.rfind('.');
    let normalized = match (last_comma, last_dot) {
        (Some(c), Some(d)) if c > d => t.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => t.replace(',', ""),
        (Some(_), None) => t.replace(',', "."),
        _ => t,
    };
    let d = Decimal::from_str(&normalized).ok()?;
    Some(if neg { -d } else { d })
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    for f in ["%d.%m.%Y", "%Y-%m-%d", "%d/%m/%Y", "%d.%m.%y", "%Y%m%d"] {
        if let Ok(d) = NaiveDate::parse_from_str(s, f) {
            return Some(d);
        }
    }
    s.get(..10).and_then(|x| NaiveDate::parse_from_str(x, "%Y-%m-%d").ok())
}

fn hash(account_id: i64, t: &ImportedTx, occurrence: usize) -> String {
    let mut key = format!("{account_id}|{}|{}|{}|{}|{}", t.date, t.amount.normalize(), t.counterparty.trim(), t.iban.trim(), t.purpose.trim());
    if occurrence > 0 {
        key.push_str(&format!("|{occurrence}"));
    }
    let mut h = Sha256::new();
    h.update(key);
    let out = h.finalize();
    out.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// Identical movements on the same day (e.g. two equal card payments) are told apart by their
/// order in the file. The first one keeps the plain hash, so earlier imports are still recognised.
fn assign_hashes(account_id: i64, txs: &mut [ImportedTx]) {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for t in txs.iter_mut() {
        let plain = hash(account_id, t, 0);
        let n = seen.entry(plain.clone()).or_default();
        t.hash = if *n == 0 { plain } else { hash(account_id, t, *n) };
        *n += 1;
    }
}

fn find_col(headers: &[String], patterns: &[&str]) -> i64 {
    for p in patterns {
        if let Some(i) = headers.iter().position(|h| h.to_lowercase().trim() == *p) {
            return i as i64;
        }
    }
    for p in patterns {
        if let Some(i) = headers.iter().position(|h| h.to_lowercase().contains(p)) {
            return i as i64;
        }
    }
    -1
}

pub(crate) fn split_rows(text: &str, delimiter: u8) -> Vec<Vec<String>> {
    let mut rdr = csv::ReaderBuilder::new().delimiter(delimiter).has_headers(false).flexible(true).from_reader(text.as_bytes());
    rdr.records().filter_map(|r| r.ok()).map(|r| r.iter().map(|c| c.trim().to_string()).collect()).collect()
}

pub fn detect_mapping(rows: &[Vec<String>], delimiter: &str) -> Option<CsvMapping> {
    for (i, row) in rows.iter().enumerate().take(40) {
        let date_col = find_col(row, &["buchungstag", "buchungsdatum", "datum", "date", "booking date", "wertstellung", "valuta"]);
        let amount_col = find_col(row, &["betrag (eur)", "betrag", "umsatz", "amount", "betrag in eur"]);
        let debit_col = find_col(row, &["soll", "ausgang", "belastung", "debit"]);
        let credit_col = find_col(row, &["haben", "eingang", "gutschrift", "credit"]);
        if date_col >= 0 && (amount_col >= 0 || (debit_col >= 0 && credit_col >= 0)) {
            return Some(CsvMapping {
                delimiter: delimiter.into(),
                header_row: i as i64,
                date_col,
                amount_col,
                debit_col: if amount_col >= 0 { -1 } else { debit_col },
                credit_col: if amount_col >= 0 { -1 } else { credit_col },
                counterparty_col: find_col(
                    row,
                    &[
                        "name zahlungsbeteiligter",
                        "beguenstigter/zahlungspflichtiger",
                        "begünstigter/zahlungspflichtiger",
                        "empfänger/zahlungspflichtiger",
                        "auftraggeber / begünstigter",
                        "auftraggeber/empfänger",
                        "zahlungsempfänger",
                        "empfänger",
                        "auftraggeber",
                        "name",
                        "payee",
                        "counterparty",
                    ],
                ),
                iban_col: find_col(row, &["iban zahlungsbeteiligter", "kontonummer/iban", "iban", "kontonummer"]),
                purpose_col: find_col(row, &["verwendungszweck", "vorgang/verwendungszweck", "purpose", "description", "buchungstext", "text"]),
            });
        }
    }
    None
}

fn cell(row: &[String], i: i64) -> String {
    if i < 0 { String::new() } else { row.get(i as usize).cloned().unwrap_or_default() }
}

fn parse_csv(text: &str, mapping: Option<CsvMapping>) -> ImportPreview {
    let first = text.lines().take(20).collect::<Vec<_>>().join("\n");
    let delim = if let Some(m) = &mapping {
        m.delimiter.clone()
    } else {
        let sc = first.matches(';').count();
        let cc = first.matches(',').count();
        let tc = first.matches('\t').count();
        if sc >= cc && sc >= tc { ";".into() } else if tc > cc { "\t".into() } else { ",".into() }
    };
    let rows = split_rows(text, delim.as_bytes().first().copied().unwrap_or(b';'));
    let mapping = mapping.filter(|m| m.date_col >= 0).or_else(|| detect_mapping(&rows, &delim));
    let mut errors = Vec::new();
    let mut transactions = Vec::new();
    let (columns, sample) = match &mapping {
        Some(m) => {
            let hr = m.header_row.max(0) as usize;
            let cols = rows.get(hr).cloned().unwrap_or_default();
            let sample = rows.iter().skip(hr + 1).take(5).cloned().collect();
            for (i, row) in rows.iter().enumerate().skip(hr + 1) {
                if row.iter().all(|c| c.is_empty()) {
                    continue;
                }
                let Some(date) = parse_date(&cell(row, m.date_col)) else {
                    if transactions.is_empty() {
                        continue;
                    }
                    errors.push(format!("Zeile {}: Datum nicht lesbar", i + 1));
                    continue;
                };
                let amount = if m.amount_col >= 0 {
                    parse_amount(&cell(row, m.amount_col))
                } else {
                    let d = parse_amount(&cell(row, m.debit_col)).unwrap_or_default().abs();
                    let c = parse_amount(&cell(row, m.credit_col)).unwrap_or_default().abs();
                    Some(c - d)
                };
                let Some(amount) = amount.filter(|a| !a.is_zero()) else { continue };
                transactions.push(ImportedTx {
                    date,
                    amount,
                    counterparty: cell(row, m.counterparty_col),
                    iban: crate::util::normalize_iban(&cell(row, m.iban_col)),
                    purpose: cell(row, m.purpose_col),
                    hash: String::new(),
                    duplicate: false,
                    locked: false,
                    account: String::new(),
                    kind: String::new(),
                    provider: String::new(),
                    reference: String::new(),
                });
            }
            (cols, sample)
        }
        None => {
            errors.push("Spalten konnten nicht automatisch erkannt werden. Bitte die Zuordnung manuell festlegen.".into());
            (rows.first().cloned().unwrap_or_default(), rows.iter().skip(1).take(5).cloned().collect())
        }
    };
    ImportPreview { format: "csv".into(), columns, sample, mapping, transactions, duplicates: 0, locked: 0, errors }
}

fn by_name<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.descendants().find(|d| d.is_element() && d.tag_name().name() == name)
}

fn parse_camt(xml: &str) -> Result<ImportPreview> {
    let doc = roxmltree::Document::parse(xml)?;
    let mut transactions = Vec::new();
    let mut errors = Vec::new();
    let mut pending = 0;
    for ntry in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Ntry") {
        let child = |name: &str| ntry.children().find(|c| c.is_element() && c.tag_name().name() == name);
        // Only booked entries: pending ones (camt.052) come again once they are booked.
        let status = child("Sts").and_then(|s| by_name(s, "Cd").and_then(|c| c.text()).or(s.text())).unwrap_or_default().trim().to_uppercase();
        if !status.is_empty() && status != "BOOK" {
            pending += 1;
            continue;
        }
        let amt = child("Amt");
        let amount = amt.and_then(|a| a.text()).and_then(|t| Decimal::from_str(t.trim()).ok()).unwrap_or_default();
        let credit = child("CdtDbtInd").and_then(|a| a.text()).map(|t| t.trim() == "CRDT").unwrap_or(true);
        let date_node = child("BookgDt").or_else(|| child("ValDt"));
        let date = date_node.and_then(|d| by_name(d, "Dt").or_else(|| by_name(d, "DtTm"))).and_then(|d| d.text()).and_then(parse_date);
        let Some(date) = date else { continue };
        let currency = amt.and_then(|a| a.attribute("Ccy")).unwrap_or("EUR");
        if currency != "EUR" {
            errors.push(format!("Umsatz vom {} über {} {currency} nicht importiert – nur Euro-Beträge werden übernommen.", crate::util::fmt_date(date), amount));
            continue;
        }
        // A reversal keeps its own direction (CdtDbtInd); it is only marked in the purpose.
        let reversal = child("RvslInd").and_then(|n| n.text()).is_some_and(|t| t.trim().eq_ignore_ascii_case("true"));
        let party_tag = if credit { "Dbtr" } else { "Cdtr" };
        let acct_tag = if credit { "DbtrAcct" } else { "CdtrAcct" };
        let rltd = by_name(ntry, "RltdPties");
        let counterparty = rltd
            .and_then(|r| r.children().find(|c| c.tag_name().name() == party_tag))
            .and_then(|p| by_name(p, "Nm"))
            .and_then(|n| n.text())
            .unwrap_or_default()
            .trim()
            .to_string();
        let iban = rltd
            .and_then(|r| r.children().find(|c| c.tag_name().name() == acct_tag))
            .and_then(|a| by_name(a, "IBAN"))
            .and_then(|n| n.text())
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut purpose: Vec<String> = ntry
            .descendants()
            .filter(|d| d.is_element() && d.tag_name().name() == "Ustrd")
            .filter_map(|d| d.text().map(|t| t.trim().to_string()))
            .collect();
        if purpose.is_empty()
            && let Some(t) = by_name(ntry, "AddtlNtryInf").and_then(|n| n.text())
        {
            purpose.push(t.trim().to_string());
        }
        let purpose = purpose.join(" ");
        // End-to-end reference and return reason identify collections and Rücklastschriften.
        let mut reference = Vec::new();
        if let Some(e2e) = by_name(ntry, "EndToEndId").and_then(|n| n.text()).map(str::trim)
            && !e2e.is_empty()
            && e2e != "NOTPROVIDED"
            && !purpose.contains(e2e)
        {
            reference.push(format!("EREF: {e2e}"));
        }
        if let Some(code) = by_name(ntry, "RtrInf").and_then(|r| by_name(r, "Cd")).and_then(|n| n.text()) {
            reference.push(format!("Rückgabegrund: {}", code.trim()));
        }
        transactions.push(ImportedTx {
            date,
            amount: if credit { amount } else { -amount },
            counterparty,
            iban,
            purpose: if reversal { format!("Rückbuchung: {purpose}") } else { purpose },
            hash: String::new(),
            duplicate: false,
            locked: false,
            account: String::new(),
            kind: String::new(),
            provider: String::new(),
            reference: reference.join(" "),
        });
    }
    if pending > 0 {
        errors.push(format!("{pending} vorgemerkte, noch nicht gebuchte Umsätze wurden übersprungen."));
    }
    Ok(ImportPreview { format: "camt".into(), columns: vec![], sample: vec![], mapping: None, transactions, duplicates: 0, locked: 0, errors })
}

pub fn preview(conn: &Connection, account_id: i64, bytes: &[u8], mapping: Option<CsvMapping>) -> Result<ImportPreview> {
    let acc: PaymentAccount = db::get(conn, account_id)?;
    let text = decode(bytes);
    let chart = crate::services::settings::load(conn)?.tax.chart;
    let mut p = if text.trim_start().starts_with('<') {
        parse_camt(&text)?
    } else if let Some(p) = mapping.is_none().then(|| psp::parse(&text, &crate::chart::role(&chart, "geldverkehr"), &crate::chart::role(&chart, "transit"))).flatten() {
        p
    } else {
        parse_csv(&text, mapping)
    };
    if p.transactions.iter().any(|t| !t.provider.is_empty()) && acc.kind == "bank" {
        p.errors.push(format!(
            "Die Datei ist ein Bericht eines Zahlungsdienstleisters. Sie gehört in ein eigenes Konto der Art „PayPal / Sonstige“, nicht in das Bankkonto „{}“.",
            acc.name
        ));
    }
    assign_hashes(account_id, &mut p.transactions);
    let locked_until = crate::services::settings::load(conn)?.tax.locked_until;
    for t in p.transactions.iter_mut() {
        t.duplicate = db::scalar_i64(conn, "SELECT COUNT(*) FROM transactions WHERE account_id = ?1 AND import_hash = ?2", &[v(account_id), v(t.hash.clone())])? > 0;
        t.locked = locked_until.is_some_and(|l| t.date <= l);
    }
    p.duplicates = p.transactions.iter().filter(|t| t.duplicate).count() as i64;
    p.locked = p.transactions.iter().filter(|t| t.locked && !t.duplicate).count() as i64;
    Ok(p)
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ImportSummary {
    pub imported: i64,
    pub skipped: i64,
    /// Not imported because they fall into the festgeschriebenen period.
    pub locked: i64,
    pub auto_matched: i64,
    /// Booked by a Bankregel.
    pub rules_applied: i64,
    /// Fees and transfers of provider reports, booked directly.
    pub booked: i64,
}

pub fn import(conn: &Connection, account_id: i64, mut txs: Vec<ImportedTx>, auto_match: bool) -> Result<ImportSummary> {
    let acc: PaymentAccount = db::get(conn, account_id)?;
    if acc.kind == "cash" {
        bail_user!("In ein Kassenkonto können keine Bankumsätze importiert werden.");
    }
    if txs.iter().any(|t| t.hash.is_empty()) {
        assign_hashes(account_id, &mut txs);
    }
    let settings = crate::services::settings::load(conn)?;
    let locked_until = settings.tax.locked_until;
    let transit = crate::chart::role(&settings.tax.chart, "transit");
    let rules: Vec<crate::models::BankRule> = crate::services::bankrules::list(conn)?.into_iter().filter(|r| r.active).collect();
    let (mut imported, mut skipped, mut locked, mut matched, mut ruled, mut booked) = (0, 0, 0, 0, 0, 0);
    for t in txs {
        let h = t.hash.clone();
        if db::scalar_i64(conn, "SELECT COUNT(*) FROM transactions WHERE account_id = ?1 AND import_hash = ?2", &[v(account_id), v(h.clone())])? > 0 {
            skipped += 1;
            continue;
        }
        if locked_until.is_some_and(|l| t.date <= l) {
            locked += 1;
            continue;
        }
        // Payouts from and direct debits by a provider whose report is imported into another
        // account are transfers between own accounts.
        let preset = if !t.account.is_empty() {
            t.account.clone()
        } else if t.provider.is_empty()
            && let Some(p) = psp::provider_of(&format!("{} {}", t.counterparty, t.purpose))
            && db::scalar_i64(conn, "SELECT COUNT(*) FROM transactions WHERE source = ?1 AND account_id != ?2", &[v(format!("import-{p}")), v(account_id)])? > 0
        {
            transit.clone()
        } else {
            String::new()
        };
        let tx = Transaction {
            account_id,
            date: t.date,
            amount: t.amount,
            counterparty: t.counterparty,
            counterparty_iban: t.iban,
            purpose: if t.reference.is_empty() { t.purpose } else { format!("{} {}", t.purpose, t.reference).trim().to_string() },
            import_hash: h,
            source: if t.provider.is_empty() { "import".into() } else { format!("import-{}", t.provider) },
            status: "open".into(),
            ..Default::default()
        };
        let id = finance::create(conn, TransactionInput { tx, allocations: vec![] })?;
        imported += 1;
        if !preset.is_empty() {
            let tx: Transaction = db::get(conn, id)?;
            let description = if preset == transit { "Geldtransit".to_string() } else { tx.purpose.clone() };
            let alloc = crate::models::Allocation { kind: "account".into(), account: preset, amount: tx.amount, date: tx.date, description, ..Default::default() };
            finance::update(conn, id, TransactionInput { tx, allocations: vec![alloc] })?;
            booked += 1;
            continue;
        }
        // Rücklastschriften and collections credited as one amount.
        if auto_match && (crate::services::debit::match_return(conn, id)? || crate::services::debit::match_collection(conn, id)?) {
            matched += 1;
            continue;
        }
        if auto_match {
            // Only unambiguous matches (number in purpose and exact amount) are assigned automatically.
            let sug = finance::suggestions(conn, id)?;
            if let Some(best) = sug.first()
                && best.score >= 90
                && sug.get(1).map(|s| s.score < 60).unwrap_or(true)
            {
                let tx: Transaction = db::get(conn, id)?;
                let alloc = crate::models::Allocation {
                    kind: best.kind.clone(),
                    document_id: if best.kind == "document" { Some(best.id) } else { None },
                    receipt_id: if best.kind == "receipt" { Some(best.id) } else { None },
                    amount: tx.amount,
                    date: tx.date,
                    ..Default::default()
                };
                if finance::update(conn, id, TransactionInput { tx, allocations: vec![alloc] }).is_ok() {
                    matched += 1;
                }
            }
        }
        // Movements not matched to an invoice or receipt are booked by the Bankregeln.
        if crate::services::bankrules::apply_to(conn, id, &rules)? {
            ruled += 1;
        }
    }
    Ok(ImportSummary { imported, skipped, locked, auto_matched: matched, rules_applied: ruled, booked })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn amounts() {
        assert_eq!(parse_amount("1.234,56"), Some(dec!(1234.56)));
        assert_eq!(parse_amount("-12,50 €"), Some(dec!(-12.50)));
        assert_eq!(parse_amount("1,234.56"), Some(dec!(1234.56)));
        assert_eq!(parse_amount("99.9"), Some(dec!(99.9)));
        assert_eq!(parse_amount("12,00-"), Some(dec!(-12.00)));
    }

    fn setup() -> (Connection, i64) {
        let conn = db::open_memory();
        crate::services::settings::ensure_seeded(&conn).unwrap();
        let acc = finance::save_account(&conn, PaymentAccount { name: "Bank".into(), opening_date: crate::util::ymd(2020, 1, 1), ..Default::default() }).unwrap();
        (conn, acc)
    }

    const TWO_COFFEES: &str = "Buchungstag;Name Zahlungsbeteiligter;Verwendungszweck;Betrag\n\
        03.02.2021;Café Bohne;Kartenzahlung;-4,90\n\
        03.02.2021;Café Bohne;Kartenzahlung;-4,90\n\
        04.02.2021;Kunde AG;RE-1;119,00\n";

    #[test]
    fn identical_movements_are_both_imported() {
        let (conn, acc) = setup();
        let p = preview(&conn, acc, TWO_COFFEES.as_bytes(), None).unwrap();
        assert_eq!(p.transactions.len(), 3);
        assert_ne!(p.transactions[0].hash, p.transactions[1].hash);
        // The first of identical rows keeps the hash used by earlier versions.
        let mut plain = Sha256::new();
        plain.update(format!("{acc}|2021-02-03|-4.9|Café Bohne||Kartenzahlung"));
        let plain: String = plain.finalize().iter().take(12).map(|b| format!("{b:02x}")).collect();
        assert_eq!(p.transactions[0].hash, plain);
        assert_eq!(p.duplicates, 0);

        let s = import(&conn, acc, p.transactions, false).unwrap();
        assert_eq!((s.imported, s.skipped), (3, 0));
        let again = preview(&conn, acc, TWO_COFFEES.as_bytes(), None).unwrap();
        assert_eq!(again.duplicates, 3, "re-importing the same file adds nothing");
        assert_eq!(import(&conn, acc, again.transactions, false).unwrap().imported, 0);
    }

    #[test]
    fn rows_in_locked_period_are_skipped() {
        let (conn, acc) = setup();
        crate::services::settings::lock_until(&conn, crate::util::ymd(2021, 2, 3)).unwrap();
        let p = preview(&conn, acc, TWO_COFFEES.as_bytes(), None).unwrap();
        assert_eq!((p.locked, p.transactions.iter().filter(|t| t.locked).count()), (2, 2));
        let s = import(&conn, acc, p.transactions, false).unwrap();
        assert_eq!((s.imported, s.locked), (1, 2), "import continues after locked rows");
    }

    #[test]
    fn camt_skips_pending_and_foreign_currency_and_marks_reversals() {
        let entry = |amt: &str, ccy: &str, ind: &str, sts: &str, extra: &str, text: &str| {
            format!(
                "<Ntry><Amt Ccy=\"{ccy}\">{amt}</Amt><CdtDbtInd>{ind}</CdtDbtInd>{extra}<Sts><Cd>{sts}</Cd></Sts><BookgDt><Dt>2026-03-02</Dt></BookgDt>\
                 <NtryDtls><TxDtls><RmtInf><Ustrd>{text}</Ustrd></RmtInf></TxDtls></NtryDtls></Ntry>"
            )
        };
        let xml = format!(
            "<Document><BkToCstmrStmt><Stmt>{}{}{}{}</Stmt></BkToCstmrStmt></Document>",
            entry("119.00", "EUR", "CRDT", "BOOK", "", "RE-1"),
            entry("50.00", "EUR", "DBIT", "PDNG", "", "vorgemerkt"),
            entry("20.00", "USD", "DBIT", "BOOK", "", "Abo"),
            entry("119.00", "EUR", "DBIT", "BOOK", "<RvslInd>true</RvslInd>", "Lastschrift RE-1"),
        );
        let p = parse_camt(&xml).unwrap();
        let got: Vec<(Decimal, &str)> = p.transactions.iter().map(|t| (t.amount, t.purpose.as_str())).collect();
        assert_eq!(got, vec![(dec!(119), "RE-1"), (dec!(-119), "Rückbuchung: Lastschrift RE-1")]);
        assert_eq!(p.errors.len(), 2, "{:?}", p.errors);
        assert!(p.errors[0].contains("USD") && p.errors[1].contains("1 vorgemerkte"));
    }

    #[test]
    fn provider_report_books_fees_and_transfers() {
        use crate::models::{Document, DocumentLine};
        use crate::services::documents::{self, DocumentInput};
        let (conn, bank) = setup();
        crate::services::settings::seed_test_company(&conn);
        let paypal = finance::save_account(
            &conn,
            PaymentAccount { name: "PayPal".into(), kind: "other".into(), ledger_account: "1220".into(), opening_date: crate::util::ymd(2026, 1, 1), ..Default::default() },
        )
        .unwrap();
        let doc = Document { doc_type: "invoice".into(), address: "Kunde AG\nWeg 1\n12345 Stadt".into(), tax_mode: "standard".into(), date: crate::util::ymd(2026, 3, 1), ..Default::default() };
        let line = DocumentLine { line_type: "item".into(), name: "Beratung".into(), quantity: dec!(1), unit_price: dec!(1000), tax_rate: dec!(19), ..Default::default() };
        let id = documents::create(&conn, DocumentInput { doc, lines: vec![line] }).unwrap();
        let number = documents::finalize(&conn, id).unwrap().number;

        let report = format!(
            "\"Datum\",\"Uhrzeit\",\"Zeitzone\",\"Name\",\"Typ\",\"Status\",\"Währung\",\"Brutto\",\"Gebühr\",\"Netto\",\"Transaktionscode\",\"Rechnungsnummer\",\"Auswirkung auf Guthaben\"\n\
             \"02.03.2026\",\"10:00:00\",\"CET\",\"Kunde AG\",\"Website-Zahlung\",\"Abgeschlossen\",\"EUR\",\"1.190,00\",\"-41,90\",\"1.148,10\",\"1AB\",\"{number}\",\"Haben\"\n\
             \"04.03.2026\",\"09:00:00\",\"CET\",\"\",\"Allgemeine Abbuchung – Bankkonto\",\"Abgeschlossen\",\"EUR\",\"-1.000,00\",\"0,00\",\"-1.000,00\",\"2AB\",\"\",\"Soll\"\n"
        );
        let p = preview(&conn, paypal, report.as_bytes(), None).unwrap();
        assert_eq!(p.format, "paypal");
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        let s = import(&conn, paypal, p.transactions, true).unwrap();
        assert_eq!((s.imported, s.auto_matched, s.booked), (3, 1, 2));
        let inv: Document = db::get(&conn, id).unwrap();
        assert_eq!(inv.status, "paid");

        // The same report in the bank account is flagged.
        let wrong = preview(&conn, bank, report.as_bytes(), None).unwrap();
        assert!(wrong.errors.iter().any(|e| e.contains("Zahlungsdienstleister")));

        // The payout arrives at the bank: Geldtransit as well.
        let statement = "Buchungstag;Name Zahlungsbeteiligter;Verwendungszweck;Betrag\n05.03.2026;PayPal Europe S.a.r.l. et Cie S.C.A;Abbuchung 2AB;1.000,00\n";
        let p = preview(&conn, bank, statement.as_bytes(), None).unwrap();
        let s = import(&conn, bank, p.transactions, true).unwrap();
        assert_eq!(s.booked, 1);

        let mut bal: std::collections::HashMap<String, Decimal> = Default::default();
        for l in crate::services::reports::raw_lines(&conn, None, None).unwrap() {
            *bal.entry(l.account).or_default() += l.debit - l.credit;
        }
        let get = |a: &str| bal.get(a).copied().unwrap_or_default();
        assert_eq!(get("1360"), dec!(0), "Geldtransit is cleared");
        assert_eq!(get("1220"), dec!(148.10), "PayPal balance");
        assert_eq!(get("1200"), dec!(1000), "bank");
        assert_eq!(get("4970"), dec!(41.90), "fee without VAT");
        assert!(db::scalar_i64(&conn, "SELECT COUNT(*) FROM transactions WHERE status = 'open'", &[]).unwrap() == 0);
    }

    #[test]
    fn csv_detection() {
        let csv = "Kontoinhaber;Max\n\nBuchungstag;Valutadatum;Name Zahlungsbeteiligter;IBAN Zahlungsbeteiligter;Verwendungszweck;Betrag\n01.10.2026;01.10.2026;ACME GmbH;DE02120300000000202051;RE-2026-0001;1.190,00\n02.10.2026;02.10.2026;Telekom;;Rechnung 123;-49,95\n";
        let p = parse_csv(csv, None);
        assert_eq!(p.transactions.len(), 2);
        assert_eq!(p.transactions[0].amount, dec!(1190));
        assert_eq!(p.transactions[1].counterparty, "Telekom");
        assert_eq!(p.transactions[0].purpose, "RE-2026-0001");
    }
}
