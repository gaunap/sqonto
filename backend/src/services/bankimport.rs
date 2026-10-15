//! Import von Kontoumsätzen: CSV (automatische Spaltenerkennung für gängige
//! deutsche Banken) und CAMT.053 (ISO 20022).

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

fn hash(account_id: i64, t: &ImportedTx) -> String {
    let mut h = Sha256::new();
    h.update(format!("{account_id}|{}|{}|{}|{}|{}", t.date, t.amount.normalize(), t.counterparty.trim(), t.iban.trim(), t.purpose.trim()));
    let out = h.finalize();
    out.iter().take(12).map(|b| format!("{b:02x}")).collect()
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

fn split_rows(text: &str, delimiter: u8) -> Vec<Vec<String>> {
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
                });
            }
            (cols, sample)
        }
        None => {
            errors.push("Spalten konnten nicht automatisch erkannt werden. Bitte die Zuordnung manuell festlegen.".into());
            (rows.first().cloned().unwrap_or_default(), rows.iter().skip(1).take(5).cloned().collect())
        }
    };
    ImportPreview { format: "csv".into(), columns, sample, mapping, transactions, duplicates: 0, errors }
}

fn by_name<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.descendants().find(|d| d.is_element() && d.tag_name().name() == name)
}

fn parse_camt(xml: &str) -> Result<ImportPreview> {
    let doc = roxmltree::Document::parse(xml)?;
    let mut transactions = Vec::new();
    for ntry in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Ntry") {
        let amount = by_name(ntry, "Amt").and_then(|a| a.text()).and_then(|t| Decimal::from_str(t.trim()).ok()).unwrap_or_default();
        let credit = by_name(ntry, "CdtDbtInd").and_then(|a| a.text()).map(|t| t.trim() == "CRDT").unwrap_or(true);
        let date_node = ntry.children().find(|c| c.tag_name().name() == "BookgDt").or_else(|| ntry.children().find(|c| c.tag_name().name() == "ValDt"));
        let date = date_node.and_then(|d| by_name(d, "Dt").or_else(|| by_name(d, "DtTm"))).and_then(|d| d.text()).and_then(parse_date);
        let Some(date) = date else { continue };
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
        transactions.push(ImportedTx {
            date,
            amount: if credit { amount } else { -amount },
            counterparty,
            iban,
            purpose: purpose.join(" "),
            hash: String::new(),
            duplicate: false,
        });
    }
    Ok(ImportPreview { format: "camt".into(), columns: vec![], sample: vec![], mapping: None, transactions, duplicates: 0, errors: vec![] })
}

pub fn preview(conn: &Connection, account_id: i64, bytes: &[u8], mapping: Option<CsvMapping>) -> Result<ImportPreview> {
    let _acc: PaymentAccount = db::get(conn, account_id)?;
    let text = decode(bytes);
    let mut p = if text.trim_start().starts_with('<') { parse_camt(&text)? } else { parse_csv(&text, mapping) };
    let mut dups = 0;
    for t in p.transactions.iter_mut() {
        t.hash = hash(account_id, t);
        let exists = db::scalar_i64(conn, "SELECT COUNT(*) FROM transactions WHERE account_id = ?1 AND import_hash = ?2", &[v(account_id), v(t.hash.clone())])? > 0;
        t.duplicate = exists;
        if exists {
            dups += 1;
        }
    }
    p.duplicates = dups;
    Ok(p)
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ImportSummary {
    pub imported: i64,
    pub skipped: i64,
    pub auto_matched: i64,
}

pub fn import(conn: &Connection, account_id: i64, txs: Vec<ImportedTx>, auto_match: bool) -> Result<ImportSummary> {
    let acc: PaymentAccount = db::get(conn, account_id)?;
    if acc.kind == "cash" {
        bail_user!("In ein Kassenkonto können keine Bankumsätze importiert werden.");
    }
    let (mut imported, mut skipped, mut matched) = (0, 0, 0);
    for t in txs {
        let h = if t.hash.is_empty() { hash(account_id, &t) } else { t.hash.clone() };
        if db::scalar_i64(conn, "SELECT COUNT(*) FROM transactions WHERE account_id = ?1 AND import_hash = ?2", &[v(account_id), v(h.clone())])? > 0 {
            skipped += 1;
            continue;
        }
        let tx = Transaction {
            account_id,
            date: t.date,
            amount: t.amount,
            counterparty: t.counterparty,
            counterparty_iban: t.iban,
            purpose: t.purpose,
            import_hash: h,
            source: "import".into(),
            status: "open".into(),
            ..Default::default()
        };
        let id = finance::create(conn, TransactionInput { tx, allocations: vec![] })?;
        imported += 1;
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
    }
    Ok(ImportSummary { imported, skipped, auto_matched: matched })
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
