// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Abo-Rechnungen: an invoice serves as template; when a period is due, a draft copy with that
//! period's dates is created for review and finalizing.

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::Serialize;
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::{Contact, Document, DocumentLine, RecurringInvoice};
use crate::services::documents::{self, DocumentInput};
use crate::services::settings as cfg;
use crate::util::{add_months, fmt_date, last_day_of_month, now_str, ymd};

/// At most this many overdue periods are created at once (e.g. after a long break).
const MAX_CATCH_UP: usize = 12;

pub fn months(cycle: &str) -> Option<i32> {
    Some(match cycle {
        "monthly" => 1,
        "quarterly" => 3,
        "halfyearly" => 6,
        "yearly" => 12,
        _ => return None,
    })
}

pub fn cycle_label(cycle: &str) -> &'static str {
    match cycle {
        "quarterly" => "vierteljährlich",
        "halfyearly" => "halbjährlich",
        "yearly" => "jährlich",
        _ => "monatlich",
    }
}

/// The date one cycle later, on the anchor day (short months use their last day).
fn advance(date: NaiveDate, cycle: &str, day: i64) -> NaiveDate {
    let d = add_months(date, months(cycle).unwrap_or(1));
    let last = last_day_of_month(d.year(), d.month()).day();
    ymd(d.year(), d.month(), (day.clamp(1, 31) as u32).min(last))
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RecurringView {
    pub recurring: RecurringInvoice,
    pub template_number: String,
    pub contact_name: String,
    pub title: String,
    #[ts(type = "number")]
    pub gross_total: Decimal,
    pub last_number: String,
}

pub fn list(conn: &Connection) -> Result<Vec<RecurringView>> {
    let mut out = Vec::new();
    for r in db::query::<RecurringInvoice>(conn, "ORDER BY active DESC, next_date, id", &[])? {
        let Some(t) = db::find::<Document>(conn, r.template_id)? else { continue };
        let contact_name = match t.contact_id {
            Some(c) => db::find::<Contact>(conn, c)?.map(|c| c.display_name()).unwrap_or_default(),
            None => t.address.lines().next().unwrap_or_default().to_string(),
        };
        let last_number = match r.last_document_id {
            Some(id) => db::find::<Document>(conn, id)?.map(|d| if d.number.is_empty() { "Entwurf".into() } else { d.number }).unwrap_or_default(),
            None => String::new(),
        };
        out.push(RecurringView { template_number: t.number.clone(), contact_name, title: t.title.clone(), gross_total: t.gross_total, last_number, recurring: r });
    }
    Ok(out)
}

pub fn save(conn: &Connection, mut r: RecurringInvoice) -> Result<RecurringInvoice> {
    let Some(t) = db::find::<Document>(conn, r.template_id)? else { bail_user!("Die Vorlage wurde nicht gefunden.") };
    if t.doc_type != "invoice" || t.kind != "standard" || t.status == "cancelled" {
        bail_user!("Als Abo-Rechnung lassen sich nur normale, nicht stornierte Rechnungen wiederholen.");
    }
    if months(&r.cycle).is_none() {
        bail_user!("Unbekannter Rhythmus.");
    }
    if r.end_date.is_some_and(|e| e < r.next_date) {
        bail_user!("Das Enddatum liegt vor der nächsten Rechnung.");
    }
    if r.day < 1 || r.day > 31 {
        r.day = i64::from(r.next_date.day());
    }
    if r.id > 0 {
        let old: RecurringInvoice = db::get(conn, r.id)?;
        r.created_at = old.created_at;
        r.last_document_id = old.last_document_id;
        r.template_id = old.template_id;
        db::update(conn, &r)?;
    } else {
        if db::scalar_i64(conn, "SELECT COUNT(*) FROM recurring_invoices WHERE template_id = ?1", &[v(r.template_id)])? > 0 {
            bail_user!("Für diese Rechnung besteht bereits eine Abo-Rechnung.");
        }
        r.created_at = now_str();
        r.last_document_id = None;
        db::insert(conn, &mut r)?;
    }
    Ok(r)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    db::delete::<RecurringInvoice>(conn, id)
}

/// Draft of the period starting at `r.next_date`, with the template's texts and positions.
fn create_draft(conn: &Connection, r: &RecurringInvoice) -> Result<i64> {
    let s = cfg::load(conn)?;
    let t: Document = db::get(conn, r.template_id)?;
    let mut d = t.clone();
    d.id = 0;
    d.kind = "standard".into();
    d.source_id = None;
    d.base_id = None;
    d.cancels_id = None;
    d.dunning_level = 0;
    d.sent_at = String::new();
    d.paid_at = None;
    d.valid_until = None;
    d.date = r.next_date;
    let days = t.payment_terms_days.unwrap_or(s.docs.payment_terms_days);
    d.payment_terms_days = Some(days);
    d.due_date = Some(d.date + Duration::days(days));
    d.service_date = Some(r.next_date);
    d.service_date_end = Some(advance(r.next_date, &r.cycle, r.day) - Duration::days(1));
    let lines: Vec<DocumentLine> = documents::lines_of(conn, t.id)?
        .into_iter()
        .filter(|l| l.line_type != "deduction")
        .map(|l| DocumentLine { id: 0, document_id: 0, ..l })
        .collect();
    let id = documents::create(conn, DocumentInput { doc: d.clone(), lines })?;
    let period = format!("{} – {}", fmt_date(r.next_date), fmt_date(d.service_date_end.unwrap_or(r.next_date)));
    cfg::log(conn, "document", id, "created", format!("Aus Abo-Rechnung erstellt ({}, {period})", cycle_label(&r.cycle)))?;
    Ok(id)
}

/// Creates the drafts of all periods due up to `today`; returns their ids.
pub fn create_due(conn: &Connection, today: NaiveDate) -> Result<Vec<i64>> {
    let mut created = Vec::new();
    for mut r in db::query::<RecurringInvoice>(conn, "WHERE active = 1 AND next_date <= ?1", &[v(today)])? {
        if db::find::<Document>(conn, r.template_id)?.is_none() {
            r.active = false;
            db::update(conn, &r)?;
            continue;
        }
        let mut n = 0;
        while r.next_date <= today && n < MAX_CATCH_UP && r.end_date.is_none_or(|e| r.next_date <= e) {
            let id = create_draft(conn, &r)?;
            created.push(id);
            r.last_document_id = Some(id);
            r.next_date = advance(r.next_date, &r.cycle, r.day);
            n += 1;
        }
        if r.end_date.is_some_and(|e| r.next_date > e) {
            r.active = false;
        }
        db::update(conn, &r)?;
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn periods_keep_their_day() {
        assert_eq!(advance(ymd(2026, 1, 31), "monthly", 31), ymd(2026, 2, 28));
        assert_eq!(advance(ymd(2026, 2, 28), "monthly", 31), ymd(2026, 3, 31));
        assert_eq!(advance(ymd(2026, 11, 15), "quarterly", 15), ymd(2027, 2, 15));
        assert_eq!(advance(ymd(2024, 2, 29), "yearly", 29), ymd(2025, 2, 28));
    }

    fn template(conn: &Connection) -> i64 {
        let doc = Document {
            doc_type: "invoice".into(),
            address: "Kunde GmbH\nWeg 1\n12345 Stadt".into(),
            tax_mode: "standard".into(),
            date: ymd(2026, 1, 31),
            title: "Wartungsvertrag".into(),
            intro_text: "Für die Wartung im Zeitraum berechnen wir:".into(),
            payment_terms_days: Some(10),
            ..Default::default()
        };
        let line = DocumentLine { line_type: "item".into(), name: "Wartung".into(), quantity: dec!(1), unit_price: dec!(100), tax_rate: dec!(19), ..Default::default() };
        let id = documents::create(conn, DocumentInput { doc, lines: vec![line] }).unwrap();
        documents::finalize(conn, id).unwrap();
        id
    }

    #[test]
    fn due_periods_become_drafts() {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        cfg::seed_test_company(&conn);
        let t = template(&conn);
        let r = save(&conn, RecurringInvoice { template_id: t, cycle: "monthly".into(), next_date: ymd(2026, 1, 31), end_date: Some(ymd(2026, 4, 30)), active: true, ..Default::default() }).unwrap();
        assert_eq!(r.day, 31);
        assert!(save(&conn, RecurringInvoice { template_id: t, cycle: "monthly".into(), next_date: ymd(2026, 5, 1), active: true, ..Default::default() }).is_err(), "one schedule per invoice");

        // Three periods are due by 20 March (31.01., 28.02.; 31.03. not yet).
        let ids = create_due(&conn, ymd(2026, 3, 20)).unwrap();
        assert_eq!(ids.len(), 2);
        let feb: Document = db::get(&conn, ids[1]).unwrap();
        assert_eq!((feb.status.as_str(), feb.date, feb.due_date, feb.service_date, feb.service_date_end), ("draft", ymd(2026, 2, 28), Some(ymd(2026, 3, 10)), Some(ymd(2026, 2, 28)), Some(ymd(2026, 3, 30))));
        assert_eq!((feb.title.as_str(), feb.intro_text.as_str(), feb.gross_total), ("Wartungsvertrag", "Für die Wartung im Zeitraum berechnen wir:", dec!(119)));
        assert!(create_due(&conn, ymd(2026, 3, 20)).unwrap().is_empty(), "nothing twice");

        // The end date stops the schedule after the last period.
        assert_eq!(create_due(&conn, ymd(2026, 12, 31)).unwrap().len(), 2, "31.03. and 30.04., then the end date is reached");
        let r: RecurringInvoice = db::get(&conn, r.id).unwrap();
        assert!(!r.active);
        assert!(documents::delete(&conn, ids[0]).is_ok(), "generated drafts can be deleted");
    }

    #[test]
    fn draft_templates_cannot_be_deleted() {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        let doc = Document { doc_type: "invoice".into(), address: "Kunde".into(), tax_mode: "standard".into(), date: ymd(2026, 1, 1), ..Default::default() };
        let t = documents::create(&conn, DocumentInput { doc, lines: vec![] }).unwrap();
        save(&conn, RecurringInvoice { template_id: t, cycle: "yearly".into(), next_date: ymd(2027, 1, 1), active: true, ..Default::default() }).unwrap();
        assert!(documents::delete(&conn, t).unwrap_err().to_string().contains("Abo-Rechnung"));
    }
}
