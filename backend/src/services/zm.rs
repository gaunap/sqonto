// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Zusammenfassende Meldung (§ 18a UStG): innergemeinschaftliche Lieferungen and sonstige
//! Leistungen to EU customers per USt-IdNr. and Meldezeitraum, with the CSV file for the import in
//! the ELSTER form.
//!
//! The figures come from the issued documents, not from the journal: the Meldezeitraum depends on
//! the invoice and service dates, not on the payment (Ist-Versteuerung).
//! - Lieferung: month of the invoice, at the latest the month after the delivery (§ 18a Abs. 8 Nr. 1).
//! - Sonstige Leistung: month in which the service was completed (§ 18a Abs. 8 Nr. 2).
//! - Gutschriften and Stornorechnungen change the amount in the period of their own date (UStAE 18a.3).
//! - Abschlagsrechnungen are not reported; the Schlussrechnung counts with its full amount.
//!
//! Dreiecksgeschäfte (D) have no tax mode yet and are not reported.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::Connection;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::Serialize;
use ts_rs::TS;

use crate::db::{self, v};
use crate::models::{Contact, Document, ZmFiling};
use crate::services::documents;
use crate::services::reports::{next_workday, period_range};
use crate::services::settings as cfg;
use crate::util::{last_day_of_month, today, ymd};

/// Quarterly reporting is allowed while the Lieferungen stay at or below this amount per quarter
/// (§ 18a Abs. 1 S. 2 UStG).
const QUARTERLY_LIMIT: Decimal = Decimal::from_parts(50_000, 0, 0, false, 0);

const CSV_HEADER: &str = "#v3.0\r\n#ve3.2.1\r\nUmsatzsteuer-Identifikationsnummer (USt-IdNr.),Summe (Euro),Art der Leistung\r\n";

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ZmDocument {
    pub id: i64,
    pub number: String,
    pub doc_type: String,
    pub kind: String,
    pub date: NaiveDate,
    /// Date that decides the Meldezeitraum.
    pub report_date: NaiveDate,
    pub contact_name: String,
    /// Net amount in euros, negative for Gutschriften and Stornos.
    #[ts(type = "number")]
    pub amount: Decimal,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ZmRow {
    pub vat_id: String,
    pub country: String,
    /// L = innergemeinschaftliche Lieferung, S = sonstige Leistung
    pub kind: String,
    pub contact_name: String,
    #[ts(type = "number")]
    pub amount: Decimal,
    /// Amount as reported: whole euros.
    pub reported: i64,
    pub documents: Vec<ZmDocument>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ZmIssue {
    pub document: ZmDocument,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ZmReport {
    pub year: i32,
    pub period: String,
    pub period_label: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub due_date: NaiveDate,
    pub rows: Vec<ZmRow>,
    /// Documents that cannot be reported (missing or invalid USt-IdNr. of the customer).
    pub issues: Vec<ZmIssue>,
    pub total_deliveries: i64,
    pub total_services: i64,
    pub filing: Option<ZmFiling>,
    /// The rows differ from the ones marked as submitted (berichtigte Meldung needed).
    pub changed: bool,
    /// Quarterly reporting although the Lieferungen of this or one of the four previous quarters
    /// exceed 50.000 €.
    pub monthly_required: bool,
    pub small_business: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ZmPeriod {
    pub period: String,
    pub label: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub due_date: NaiveDate,
    pub rows: i64,
    pub total: i64,
    pub issues: i64,
    pub filing: Option<ZmFiling>,
    pub changed: bool,
    pub is_current: bool,
    pub is_future: bool,
}

/// The ZM is due on the 25th day after the end of the Meldezeitraum; there is no
/// Dauerfristverlängerung.
pub fn due_date(to: NaiveDate) -> NaiveDate {
    next_workday(to + Duration::days(25))
}

/// USt-IdNr. in the reported form: upper case without blanks and separators, Greece as "EL".
pub fn normalize_vat_id(raw: &str) -> String {
    let s = raw.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '*').collect::<String>().to_uppercase();
    match s.strip_prefix("GR") {
        Some(rest) => format!("EL{rest}"),
        None => s,
    }
}

fn vat_id_problem(vat_id: &str) -> Option<&'static str> {
    if vat_id.is_empty() {
        return Some("Beim Kunden ist keine USt-IdNr. hinterlegt.");
    }
    if vat_id.len() < 7 || !vat_id.chars().take(2).all(|c| c.is_ascii_uppercase()) {
        return Some("Die USt-IdNr. des Kunden ist ungültig (Länderkennzeichen und mindestens 7 Zeichen).");
    }
    if vat_id.starts_with("DE") {
        return Some("Die USt-IdNr. des Kunden ist deutsch – gemeldet werden nur Kunden in anderen EU-Staaten.");
    }
    None
}

/// Date that decides the Meldezeitraum of a document.
fn report_date(d: &Document) -> NaiveDate {
    if d.doc_type == "credit_note" || d.kind == "cancellation" {
        return d.date;
    }
    let service = d.service_date_end.or(d.service_date);
    if d.tax_mode == "eu_service" {
        return service.unwrap_or(d.date);
    }
    match service {
        Some(s) => {
            let next = ymd(s.year(), s.month(), 1) + Duration::days(32);
            d.date.min(last_day_of_month(next.year(), next.month()))
        }
        None => d.date,
    }
}

struct Item {
    vat_id: String,
    kind: &'static str,
    doc: ZmDocument,
}

/// Reportable documents (issued invoices, Gutschriften and Stornos to EU customers), whatever
/// their period.
fn items(conn: &Connection) -> Result<Vec<Item>> {
    let docs = db::query::<Document>(
        conn,
        "WHERE doc_type IN ('invoice','credit_note') AND status != 'draft' AND tax_mode IN ('eu_delivery','eu_service') ORDER BY date, id",
        &[],
    )?;
    let partials: Vec<i64> = docs.iter().filter(|d| d.kind == "partial").map(|d| d.id).collect();
    let contacts: HashMap<i64, Contact> = db::query::<Contact>(conn, "", &[])?.into_iter().map(|c| (c.id, c)).collect();
    let mut out = Vec::new();
    for d in docs {
        // Abschläge (and their Stornos) are part of the Schlussrechnung.
        if d.kind == "partial" || d.cancels_id.is_some_and(|c| partials.contains(&c)) {
            continue;
        }
        // The Schlussrechnung reports the whole delivery, not only the rest after the Abschläge.
        let deducted: Decimal = documents::lines_of(conn, d.id)?.iter().filter(|l| l.line_type == "deduction").map(|l| l.net_amount).sum();
        let contact = d.contact_id.and_then(|id| contacts.get(&id));
        out.push(Item {
            vat_id: contact.map(|c| normalize_vat_id(&c.vat_id)).unwrap_or_default(),
            kind: if d.tax_mode == "eu_delivery" { "L" } else { "S" },
            doc: ZmDocument {
                id: d.id,
                number: d.number.clone(),
                doc_type: d.doc_type.clone(),
                kind: d.kind.clone(),
                date: d.date,
                report_date: report_date(&d),
                contact_name: contact.map(|c| c.display_name()).unwrap_or_else(|| d.address.lines().next().unwrap_or_default().to_string()),
                amount: d.sign() * (d.net_total - deducted),
            },
        });
    }
    Ok(out)
}

fn whole_euros(d: Decimal) -> i64 {
    d.round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero).to_i64().unwrap_or(0)
}

/// Rows of a Meldezeitraum, grouped by USt-IdNr. and type; rows that round to 0 € are left out.
fn rows_and_issues(items: &[Item], from: NaiveDate, to: NaiveDate) -> (Vec<ZmRow>, Vec<ZmIssue>) {
    let mut groups: BTreeMap<(String, &str), ZmRow> = BTreeMap::new();
    let mut issues = Vec::new();
    for it in items.iter().filter(|i| i.doc.report_date >= from && i.doc.report_date <= to) {
        if let Some(problem) = vat_id_problem(&it.vat_id) {
            issues.push(ZmIssue { document: it.doc.clone(), message: problem.into() });
            continue;
        }
        let row = groups.entry((it.vat_id.clone(), it.kind)).or_insert_with(|| ZmRow {
            vat_id: it.vat_id.clone(),
            country: it.vat_id[..2].to_string(),
            kind: it.kind.into(),
            contact_name: it.doc.contact_name.clone(),
            amount: Decimal::ZERO,
            reported: 0,
            documents: Vec::new(),
        });
        row.amount += it.doc.amount;
        row.documents.push(it.doc.clone());
    }
    let rows = groups
        .into_values()
        .map(|mut r| {
            r.reported = whole_euros(r.amount);
            r
        })
        .filter(|r| r.reported != 0)
        .collect();
    (rows, issues)
}

/// CSV file for the import in the ELSTER form "Zusammenfassende Meldung".
pub fn csv(rows: &[ZmRow]) -> String {
    let mut out = String::from(CSV_HEADER);
    for r in rows {
        out.push_str(&format!("{},{},{}\r\n", r.vat_id, r.reported, r.kind));
    }
    out
}

/// Lieferungen of a calendar quarter (for the 50.000 € limit).
fn quarter_deliveries(items: &[Item], year: i32, quarter: u32) -> Decimal {
    let (from, to, _) = period_range(year, &format!("Q{quarter}"));
    items.iter().filter(|i| i.kind == "L" && i.doc.report_date >= from && i.doc.report_date <= to).map(|i| i.doc.amount).sum()
}

fn limit_exceeded(items: &[Item], to: NaiveDate) -> bool {
    let (mut y, mut q) = (to.year(), to.month0() / 3 + 1);
    for _ in 0..5 {
        if quarter_deliveries(items, y, q) > QUARTERLY_LIMIT {
            return true;
        }
        (y, q) = if q == 1 { (y - 1, 4) } else { (y, q - 1) };
    }
    false
}

fn build(conn: &Connection, items: &[Item], year: i32, period: &str) -> Result<ZmReport> {
    let s = cfg::load(conn)?;
    let (from, to, label) = period_range(year, period);
    let (rows, issues) = rows_and_issues(items, from, to);
    let filing = db::query_one::<ZmFiling>(conn, "WHERE year = ?1 AND period = ?2", &[v(year as i64), v(period.to_string())])?;
    let changed = filing.as_ref().is_some_and(|f| f.content != csv(&rows));
    Ok(ZmReport {
        year,
        period: period.into(),
        period_label: label,
        from,
        to,
        due_date: due_date(to),
        total_deliveries: rows.iter().filter(|r| r.kind == "L").map(|r| r.reported).sum(),
        total_services: rows.iter().filter(|r| r.kind == "S").map(|r| r.reported).sum(),
        rows,
        issues,
        filing,
        changed,
        monthly_required: period.starts_with('Q') && limit_exceeded(items, to),
        small_business: s.tax.small_business,
    })
}

pub fn report(conn: &Connection, year: i32, period: &str) -> Result<ZmReport> {
    build(conn, &items(conn)?, year, period)
}

pub fn periods(conn: &Connection, year: i32) -> Result<Vec<ZmPeriod>> {
    let s = cfg::load(conn)?;
    let keys: Vec<String> = if s.tax.zm_period == "monthly" { (1..=12).map(|m| format!("{m:02}")).collect() } else { (1..=4).map(|q| format!("Q{q}")).collect() };
    let items = items(conn)?;
    let t = today();
    let mut out = Vec::new();
    for p in keys {
        let r = build(conn, &items, year, &p)?;
        out.push(ZmPeriod {
            is_current: r.from <= t && t <= r.to,
            is_future: r.from > t,
            period: p,
            label: r.period_label,
            from: r.from,
            to: r.to,
            due_date: r.due_date,
            rows: r.rows.len() as i64,
            total: r.total_deliveries + r.total_services,
            issues: r.issues.len() as i64,
            changed: r.changed,
            filing: r.filing,
        });
    }
    Ok(out)
}

/// Marks a Meldezeitraum as submitted; the reported rows are kept to detect later changes.
pub fn file(conn: &Connection, year: i32, period: &str, submitted_at: NaiveDate, note: String) -> Result<ZmReport> {
    let r = report(conn, year, period)?;
    conn.execute("DELETE FROM zm_filings WHERE year = ?1 AND period = ?2", rusqlite::params![year, period])?;
    let mut f = ZmFiling {
        id: 0,
        year: year as i64,
        period: period.into(),
        submitted_at: submitted_at.to_string(),
        amount: Decimal::from(r.total_deliveries + r.total_services),
        content: csv(&r.rows),
        note,
    };
    db::insert(conn, &mut f)?;
    report(conn, year, period)
}

pub fn unfile(conn: &Connection, year: i32, period: &str) -> Result<ZmReport> {
    conn.execute("DELETE FROM zm_filings WHERE year = ?1 AND period = ?2", rusqlite::params![year, period])?;
    report(conn, year, period)
}

/// (ended periods of this and the previous year with something to report but not marked as
/// submitted, submitted periods that changed since) – for the dashboard.
pub fn todos(conn: &Connection, t: NaiveDate) -> Result<(i64, i64)> {
    if cfg::load(conn)?.tax.small_business {
        return Ok((0, 0));
    }
    let (mut unfiled, mut changed) = (0, 0);
    for year in [t.year() - 1, t.year()] {
        for p in periods(conn, year)? {
            if p.changed {
                changed += 1;
            } else if p.filing.is_none() && p.to < t && (p.rows > 0 || p.issues > 0) {
                unfiled += 1;
            }
        }
    }
    Ok((unfiled, changed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DocumentLine;
    use crate::services::documents::DocumentInput;
    use rust_decimal_macros::dec;

    fn setup() -> Connection {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        cfg::seed_test_company(&conn);
        conn
    }

    fn customer(conn: &Connection, name: &str, vat_id: &str) -> i64 {
        let mut c = Contact { company: name.into(), street: "Rue 1".into(), zip: "75001".into(), city: "Paris".into(), country: "FR".into(), vat_id: vat_id.into(), ..Default::default() };
        db::insert(conn, &mut c).unwrap()
    }

    fn doc(conn: &Connection, contact: i64, doc_type: &str, mode: &str, date: NaiveDate, service: Option<NaiveDate>, net: Decimal) -> i64 {
        let d = Document {
            doc_type: doc_type.into(),
            contact_id: Some(contact),
            address: "Client SARL\nRue 1\n75001 Paris".into(),
            tax_mode: mode.into(),
            date,
            service_date: service,
            ..Default::default()
        };
        let line = DocumentLine { line_type: "item".into(), name: "Leistung".into(), quantity: dec!(1), unit_price: net, tax_rate: dec!(0), ..Default::default() };
        let id = documents::create(conn, DocumentInput { doc: d, lines: vec![line] }).unwrap();
        documents::finalize(conn, id).unwrap();
        id
    }

    fn rows(conn: &Connection, year: i32, period: &str) -> Vec<(String, String, i64)> {
        report(conn, year, period).unwrap().rows.into_iter().map(|r| (r.vat_id, r.kind, r.reported)).collect()
    }

    #[test]
    fn groups_by_vat_id_and_kind() {
        let conn = setup();
        let fr = customer(&conn, "Client SARL", "fr 12 345678901");
        let at = customer(&conn, "Kunde GmbH", "ATU12345678");
        doc(&conn, fr, "invoice", "eu_delivery", ymd(2026, 2, 3), None, dec!(1000.40));
        doc(&conn, fr, "invoice", "eu_delivery", ymd(2026, 3, 3), None, dec!(500.20));
        doc(&conn, fr, "invoice", "eu_service", ymd(2026, 3, 9), None, dec!(300));
        doc(&conn, at, "invoice", "eu_service", ymd(2026, 1, 9), None, dec!(99.50));
        doc(&conn, at, "invoice", "standard", ymd(2026, 1, 9), None, dec!(5000));
        assert_eq!(
            rows(&conn, 2026, "Q1"),
            vec![("ATU12345678".into(), "S".into(), 100), ("FR12345678901".into(), "L".into(), 1501), ("FR12345678901".into(), "S".into(), 300)]
        );
        let r = report(&conn, 2026, "Q1").unwrap();
        assert_eq!((r.total_deliveries, r.total_services, r.due_date), (1501, 400, ymd(2026, 4, 27)));
        assert_eq!(
            csv(&r.rows),
            "#v3.0\r\n#ve3.2.1\r\nUmsatzsteuer-Identifikationsnummer (USt-IdNr.),Summe (Euro),Art der Leistung\r\nATU12345678,100,S\r\nFR12345678901,1501,L\r\nFR12345678901,300,S\r\n"
        );
        assert!(rows(&conn, 2026, "Q2").is_empty());
    }

    #[test]
    fn meldezeitraum_follows_invoice_and_service_dates() {
        let conn = setup();
        let fr = customer(&conn, "Client SARL", "FR12345678901");
        // Lieferung in March, invoiced in April: April.
        doc(&conn, fr, "invoice", "eu_delivery", ymd(2026, 4, 10), Some(ymd(2026, 3, 20)), dec!(100));
        // Lieferung in January, invoiced only in May: at the latest February.
        doc(&conn, fr, "invoice", "eu_delivery", ymd(2026, 5, 4), Some(ymd(2026, 1, 15)), dec!(200));
        // Service completed in March, invoiced in April: March.
        doc(&conn, fr, "invoice", "eu_service", ymd(2026, 4, 2), Some(ymd(2026, 3, 31)), dec!(300));
        assert_eq!(rows(&conn, 2026, "Q1"), vec![("FR12345678901".into(), "L".into(), 200), ("FR12345678901".into(), "S".into(), 300)]);
        assert_eq!(rows(&conn, 2026, "Q2"), vec![("FR12345678901".into(), "L".into(), 100)]);
    }

    #[test]
    fn credit_notes_and_cancellations_reduce_their_own_period() {
        let conn = setup();
        let fr = customer(&conn, "Client SARL", "FR12345678901");
        doc(&conn, fr, "invoice", "eu_delivery", ymd(2026, 3, 2), None, dec!(1000));
        let other = doc(&conn, fr, "invoice", "eu_delivery", ymd(2026, 3, 5), None, dec!(400));
        doc(&conn, fr, "credit_note", "eu_delivery", ymd(2026, 3, 20), None, dec!(150));
        assert_eq!(rows(&conn, 2026, "Q1"), vec![("FR12345678901".into(), "L".into(), 1250)]);
        // A Storno is dated today: it reduces the current period, the original one stays.
        documents::cancel(&conn, other).unwrap();
        let (y, q) = (today().year(), format!("Q{}", today().month0() / 3 + 1));
        let current = rows(&conn, y, &q);
        assert!(current.contains(&("FR12345678901".into(), "L".into(), -400)), "{current:?}");
        assert_eq!(rows(&conn, 2026, "Q1"), vec![("FR12345678901".into(), "L".into(), 1250)]);
    }

    #[test]
    fn final_invoice_reports_the_full_amount_and_partials_none() {
        let conn = setup();
        let fr = customer(&conn, "Client SARL", "FR12345678901");
        let order = Document { doc_type: "order".into(), contact_id: Some(fr), address: "Client SARL\nRue 1\n75001 Paris".into(), tax_mode: "eu_service".into(), date: ymd(2026, 1, 5), ..Default::default() };
        let line = DocumentLine { line_type: "item".into(), name: "Projekt".into(), quantity: dec!(1), unit_price: dec!(10000), ..Default::default() };
        let order = documents::create(&conn, DocumentInput { doc: order, lines: vec![line] }).unwrap();
        documents::finalize(&conn, order).unwrap();
        let p = documents::create_partial(&conn, order, documents::PartialRequest { mode: "percent".into(), value: dec!(30) }).unwrap();
        let mut pd: Document = db::get(&conn, p).unwrap();
        pd.date = ymd(2026, 1, 20);
        db::update(&conn, &pd).unwrap();
        documents::finalize(&conn, p).unwrap();
        let f = documents::create_final(&conn, order).unwrap();
        let mut fd: Document = db::get(&conn, f).unwrap();
        fd.date = ymd(2026, 4, 15);
        fd.service_date = Some(ymd(2026, 4, 10));
        db::update(&conn, &fd).unwrap();
        documents::finalize(&conn, f).unwrap();
        assert!(rows(&conn, 2026, "Q1").is_empty());
        assert_eq!(rows(&conn, 2026, "Q2"), vec![("FR12345678901".into(), "S".into(), 10000)]);
    }

    #[test]
    fn invalid_vat_ids_are_issues() {
        let conn = setup();
        let changed = customer(&conn, "Client SARL", "FR12345678901");
        let de = customer(&conn, "Inland", "DE123456789");
        let gr = customer(&conn, "Pelatis", "GR 123456789");
        let a = doc(&conn, changed, "invoice", "eu_service", ymd(2026, 2, 1), None, dec!(100));
        let b = doc(&conn, de, "invoice", "eu_service", ymd(2026, 2, 2), None, dec!(100));
        doc(&conn, gr, "invoice", "eu_service", ymd(2026, 2, 3), None, dec!(100));
        // Finalizing checks that a USt-IdNr. exists; it can still be wrong or be changed later.
        let mut c: Contact = db::get(&conn, changed).unwrap();
        c.vat_id = "FR123".into();
        db::update(&conn, &c).unwrap();
        let r = report(&conn, 2026, "Q1").unwrap();
        assert_eq!(r.rows.iter().map(|r| r.vat_id.as_str()).collect::<Vec<_>>(), vec!["EL123456789"]);
        assert_eq!(r.issues.iter().map(|i| i.document.id).collect::<Vec<_>>(), vec![a, b]);
    }

    #[test]
    fn quarterly_limit_and_filing_changes() {
        let conn = setup();
        let fr = customer(&conn, "Client SARL", "FR12345678901");
        doc(&conn, fr, "invoice", "eu_delivery", ymd(2025, 11, 3), None, dec!(50000.01));
        doc(&conn, fr, "invoice", "eu_service", ymd(2026, 2, 3), None, dec!(100));
        assert!(report(&conn, 2026, "Q1").unwrap().monthly_required, "previous quarters count");
        assert!(!report(&conn, 2027, "Q1").unwrap().monthly_required, "only four previous quarters");

        let t = ymd(2026, 4, 30);
        assert_eq!(todos(&conn, t).unwrap(), (2, 0), "Q4/2025 and Q1/2026");
        file(&conn, 2026, "Q1", ymd(2026, 4, 20), String::new()).unwrap();
        file(&conn, 2025, "Q4", ymd(2026, 1, 20), String::new()).unwrap();
        assert_eq!(todos(&conn, t).unwrap(), (0, 0));
        doc(&conn, fr, "invoice", "eu_service", ymd(2026, 3, 3), None, dec!(1));
        assert!(report(&conn, 2026, "Q1").unwrap().changed);
        assert_eq!(todos(&conn, t).unwrap(), (0, 1));
        unfile(&conn, 2026, "Q1").unwrap();
        assert_eq!(todos(&conn, t).unwrap(), (1, 0));
    }

    #[test]
    fn due_dates_and_vat_ids() {
        assert_eq!(due_date(ymd(2026, 3, 31)), ymd(2026, 4, 27), "25 April 2026 is a Saturday");
        assert_eq!(due_date(ymd(2026, 1, 31)), ymd(2026, 2, 25));
        assert_eq!(normalize_vat_id(" at u-123.456 78"), "ATU12345678");
        assert_eq!(normalize_vat_id("GR123456789"), "EL123456789");
    }
}
