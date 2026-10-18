// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Abo-Rechnungen: an invoice serves as template; when a period is due, a copy with that period's
//! dates (and prices, if a price change applies) is created – as draft for review, or finalized
//! right away and optionally sent by e-mail (`api::documents::deliver_recurring`).

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::Connection;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::Serialize;
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::{Contact, Document, DocumentLine, RecurringInvoice};
use crate::services::calc;
use crate::services::documents::{self, DocumentInput};
use crate::services::settings as cfg;
use crate::util::{HUNDRED, add_months, fill_placeholders, fmt_date, fmt_percent, last_day_of_month, now_str, ymd};

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

/// How often the price change applies to the period starting at `start`: once from
/// `price_change_from`, and with `price_change_yearly` once more on every anniversary.
fn price_steps(r: &RecurringInvoice, start: NaiveDate) -> u32 {
    let Some(from) = r.price_change_from.filter(|_| !r.price_change_percent.is_zero()) else { return 0 };
    if start < from {
        return 0;
    }
    let mut steps = 1;
    while r.price_change_yearly && add_months(from, 12 * steps as i32) <= start {
        steps += 1;
    }
    steps
}

/// A price after `steps` changes by `percent`; each change is rounded like a price on an invoice
/// (cents, or the precision of the original price).
pub fn changed_price(price: Decimal, percent: Decimal, steps: u32) -> Decimal {
    let dp = price.scale().max(2);
    let mut p = price;
    for _ in 0..steps {
        p = (p * (Decimal::ONE + percent / HUNDRED)).round_dp_with_strategy(dp, RoundingStrategy::MidpointAwayFromZero);
    }
    p
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RecurringView {
    pub recurring: RecurringInvoice,
    pub template_number: String,
    pub contact_name: String,
    pub contact_email: String,
    pub title: String,
    #[ts(type = "number")]
    pub gross_total: Decimal,
    /// Gross total of the next invoice (differs from the template after a price change).
    #[ts(type = "number")]
    pub next_gross_total: Decimal,
    pub last_number: String,
}

pub fn list(conn: &Connection) -> Result<Vec<RecurringView>> {
    let mut out = Vec::new();
    for r in db::query::<RecurringInvoice>(conn, "ORDER BY active DESC, next_date, id", &[])? {
        let Some(t) = db::find::<Document>(conn, r.template_id)? else { continue };
        let contact = match t.contact_id {
            Some(c) => db::find::<Contact>(conn, c)?,
            None => None,
        };
        let contact_name = match &contact {
            Some(c) => c.display_name(),
            None => t.address.lines().next().unwrap_or_default().to_string(),
        };
        let last_number = match r.last_document_id {
            Some(id) => db::find::<Document>(conn, id)?.map(|d| if d.number.is_empty() { "Entwurf".into() } else { d.number }).unwrap_or_default(),
            None => String::new(),
        };
        let (next, _) = period_copy(conn, &r, &t)?;
        out.push(RecurringView {
            template_number: t.number.clone(),
            contact_name,
            contact_email: contact.map(|c| c.email).unwrap_or_default(),
            title: t.title.clone(),
            gross_total: t.gross_total,
            next_gross_total: next.doc.gross_total,
            last_number,
            recurring: r,
        });
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
    if r.price_change_percent.is_zero() {
        r.price_change_from = None;
        r.price_change_yearly = false;
    } else {
        if r.price_change_percent <= Decimal::from(-50) || r.price_change_percent > HUNDRED {
            bail_user!("Bitte eine Preisanpassung zwischen −50 % und +100 % angeben.");
        }
        if r.price_change_from.is_none() {
            bail_user!("Bitte angeben, ab wann die Preisanpassung gilt.");
        }
    }
    if r.auto_send {
        r.auto_finalize = true;
        if !cfg::load(conn)?.email.enabled {
            bail_user!("Für den automatischen Versand bitte zuerst den E-Mail-Versand einrichten (Einstellungen → E-Mail).");
        }
        let email = match t.contact_id {
            Some(c) => db::find::<Contact>(conn, c)?.map(|c| c.email).unwrap_or_default(),
            None => String::new(),
        };
        if email.trim().is_empty() {
            bail_user!("Für den automatischen Versand fehlt die E-Mail-Adresse des Kunden.");
        }
    }
    if r.id > 0 {
        let old: RecurringInvoice = db::get(conn, r.id)?;
        r.created_at = old.created_at;
        r.last_document_id = old.last_document_id;
        r.template_id = old.template_id;
        r.last_error = old.last_error;
        db::update(conn, &r)?;
    } else {
        if db::scalar_i64(conn, "SELECT COUNT(*) FROM recurring_invoices WHERE template_id = ?1", &[v(r.template_id)])? > 0 {
            bail_user!("Für diese Rechnung besteht bereits eine Abo-Rechnung.");
        }
        r.created_at = now_str();
        r.last_document_id = None;
        r.last_error = String::new();
        db::insert(conn, &mut r)?;
    }
    Ok(r)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    db::delete::<RecurringInvoice>(conn, id)
}

/// Records why the automatic finalizing or sending failed (shown with the Abo-Rechnung).
pub fn set_error(conn: &Connection, id: i64, message: &str) -> Result<()> {
    conn.execute("UPDATE recurring_invoices SET last_error = ?1 WHERE id = ?2", rusqlite::params![message, id])?;
    Ok(())
}

/// The invoice of the period starting at `r.next_date`: the template's texts and positions with
/// that period's dates and prices; the period placeholders of title and positions are filled in.
/// Also returns how often the price change was applied.
fn period_copy(conn: &Connection, r: &RecurringInvoice, t: &Document) -> Result<(DocumentInput, u32)> {
    let s = cfg::load(conn)?;
    let mut d = t.clone();
    d.id = 0;
    d.kind = "standard".into();
    d.status = "draft".into();
    d.number = String::new();
    d.source_id = None;
    d.base_id = None;
    d.cancels_id = None;
    d.dunning_level = 0;
    d.sent_at = String::new();
    d.paid_at = None;
    d.paid_total = Decimal::ZERO;
    d.valid_until = None;
    d.date = r.next_date;
    let days = t.payment_terms_days.unwrap_or(s.docs.payment_terms_days);
    d.payment_terms_days = Some(days);
    d.due_date = Some(d.date + Duration::days(days));
    d.service_date = Some(r.next_date);
    d.service_date_end = Some(advance(r.next_date, &r.cycle, r.day) - Duration::days(1));
    let vars = documents::period_placeholders(&d);
    d.title = fill_placeholders(&d.title, &vars);
    let steps = price_steps(r, r.next_date);
    let mut lines: Vec<DocumentLine> = documents::lines_of(conn, t.id)?
        .into_iter()
        .filter(|l| l.line_type != "deduction")
        .map(|l| DocumentLine {
            id: 0,
            document_id: 0,
            name: fill_placeholders(&l.name, &vars),
            description: fill_placeholders(&l.description, &vars),
            unit_price: if l.line_type == "item" { changed_price(l.unit_price, r.price_change_percent, steps) } else { l.unit_price },
            ..l
        })
        .collect();
    let totals = calc::calculate(&d, &mut lines);
    calc::apply_totals(&mut d, &totals);
    Ok((DocumentInput { doc: d, lines }, steps))
}

fn create_copy(conn: &Connection, r: &RecurringInvoice) -> Result<i64> {
    let t: Document = db::get(conn, r.template_id)?;
    let (input, steps) = period_copy(conn, r, &t)?;
    let end = input.doc.service_date_end.unwrap_or(r.next_date);
    let id = documents::create(conn, input)?;
    let mut note = format!("Aus Abo-Rechnung erstellt ({}, {} – {})", cycle_label(&r.cycle), fmt_date(r.next_date), fmt_date(end));
    if steps > 0 {
        let total = changed_price(HUNDRED, r.price_change_percent, steps) - HUNDRED;
        note.push_str(&format!(", Preise angepasst um {}", fmt_percent(total)));
    }
    cfg::log(conn, "document", id, "created", note)?;
    Ok(id)
}

/// An invoice created for a due period.
#[derive(Debug, Clone)]
pub struct Created {
    pub recurring_id: i64,
    pub document_id: i64,
    /// Finalized automatically (otherwise a draft, also when finalizing failed).
    pub finalized: bool,
    /// To be sent by e-mail.
    pub send: bool,
}

/// Finalizes a created invoice; a failure (e.g. missing invoice data) leaves the draft and returns
/// the reason.
fn try_finalize(conn: &Connection, r: &RecurringInvoice, id: i64) -> Result<Option<String>> {
    conn.execute_batch("SAVEPOINT recurring_finalize")?;
    match documents::finalize(conn, id) {
        Ok(_) => {
            conn.execute_batch("RELEASE recurring_finalize")?;
            Ok(None)
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK TO recurring_finalize; RELEASE recurring_finalize")?;
            cfg::log(conn, "document", id, "finalize_failed", format!("Automatisches Abschließen fehlgeschlagen: {e}"))?;
            Ok(Some(format!("Die Rechnung vom {} ist ein Entwurf geblieben: {e}", fmt_date(r.next_date))))
        }
    }
}

/// Creates the invoices of all periods due up to `today`.
pub fn create_due(conn: &Connection, today: NaiveDate) -> Result<Vec<Created>> {
    let mut created = Vec::new();
    for mut r in db::query::<RecurringInvoice>(conn, "WHERE active = 1 AND next_date <= ?1", &[v(today)])? {
        if db::find::<Document>(conn, r.template_id)?.is_none() {
            r.active = false;
            db::update(conn, &r)?;
            continue;
        }
        let mut n = 0;
        let mut error = None;
        while r.next_date <= today && n < MAX_CATCH_UP && r.end_date.is_none_or(|e| r.next_date <= e) {
            let id = create_copy(conn, &r)?;
            let failed = if r.auto_finalize { try_finalize(conn, &r, id)? } else { None };
            let finalized = r.auto_finalize && failed.is_none();
            error = failed.or(error);
            created.push(Created { recurring_id: r.id, document_id: id, finalized, send: finalized && r.auto_send });
            r.last_document_id = Some(id);
            r.next_date = advance(r.next_date, &r.cycle, r.day);
            n += 1;
        }
        if r.end_date.is_some_and(|e| r.next_date > e) {
            r.active = false;
        }
        if n > 0 {
            r.last_error = error.unwrap_or_default();
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

    fn setup() -> Connection {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        cfg::seed_test_company(&conn);
        conn
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

    fn ids(created: &[Created]) -> Vec<i64> {
        created.iter().map(|c| c.document_id).collect()
    }

    #[test]
    fn due_periods_become_drafts() {
        let conn = setup();
        let t = template(&conn);
        let r = save(&conn, RecurringInvoice { template_id: t, cycle: "monthly".into(), next_date: ymd(2026, 1, 31), end_date: Some(ymd(2026, 4, 30)), active: true, ..Default::default() }).unwrap();
        assert_eq!(r.day, 31);
        assert!(save(&conn, RecurringInvoice { template_id: t, cycle: "monthly".into(), next_date: ymd(2026, 5, 1), active: true, ..Default::default() }).is_err(), "one schedule per invoice");

        // Three periods are due by 20 March (31.01., 28.02.; 31.03. not yet).
        let ids = ids(&create_due(&conn, ymd(2026, 3, 20)).unwrap());
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

    #[test]
    fn period_placeholders_are_filled() {
        let conn = setup();
        let doc = Document { doc_type: "invoice".into(), address: "Kunde GmbH\nWeg 1\n12345 Stadt".into(), date: ymd(2026, 1, 1), title: "Miete {{Monat}}".into(), ..Default::default() };
        let line = DocumentLine { line_type: "item".into(), name: "Stellplatz {{Quartal}}".into(), description: "Zeitraum {{Leistungszeitraum}} ({{Jahr}})".into(), quantity: dec!(1), unit_price: dec!(50), tax_rate: dec!(19), ..Default::default() };
        let t = documents::create(&conn, DocumentInput { doc, lines: vec![line] }).unwrap();
        save(&conn, RecurringInvoice { template_id: t, cycle: "quarterly".into(), next_date: ymd(2026, 4, 1), active: true, ..Default::default() }).unwrap();
        let id = create_due(&conn, ymd(2026, 4, 1)).unwrap()[0].document_id;
        let d: Document = db::get(&conn, id).unwrap();
        let l = &documents::lines_of(&conn, id).unwrap()[0];
        assert_eq!(d.title, "Miete April 2026");
        assert_eq!((l.name.as_str(), l.description.as_str()), ("Stellplatz 2. Quartal 2026", "Zeitraum 01.04.2026 – 30.06.2026 (2026)"));
        // The template keeps its placeholders.
        assert_eq!(documents::lines_of(&conn, t).unwrap()[0].name, "Stellplatz {{Quartal}}");
    }

    #[test]
    fn prices_change_from_a_date_and_yearly() {
        let r = RecurringInvoice { price_change_percent: dec!(3), price_change_from: Some(ymd(2027, 1, 1)), price_change_yearly: true, ..Default::default() };
        assert_eq!(price_steps(&r, ymd(2026, 12, 1)), 0);
        assert_eq!(price_steps(&r, ymd(2027, 1, 1)), 1);
        assert_eq!(price_steps(&r, ymd(2027, 12, 31)), 1);
        assert_eq!(price_steps(&r, ymd(2029, 1, 1)), 3);
        assert_eq!(price_steps(&RecurringInvoice { price_change_yearly: false, ..r.clone() }, ymd(2029, 1, 1)), 1);
        // Rounded after every change: 100 → 103 → 106,09 → 109,27.
        assert_eq!(changed_price(dec!(100), dec!(3), 3), dec!(109.27));
        assert_eq!(changed_price(dec!(0.0125), dec!(10), 1), dec!(0.0138));
        assert_eq!(changed_price(dec!(80), dec!(-5), 1), dec!(76));

        let conn = setup();
        let t = template(&conn);
        let bad = save(&conn, RecurringInvoice { template_id: t, cycle: "monthly".into(), next_date: ymd(2026, 11, 30), price_change_percent: dec!(5), active: true, ..Default::default() });
        assert!(bad.unwrap_err().to_string().contains("ab wann"));
        save(&conn, RecurringInvoice { template_id: t, cycle: "monthly".into(), next_date: ymd(2026, 11, 30), price_change_percent: dec!(5), price_change_from: Some(ymd(2027, 1, 1)), active: true, ..Default::default() }).unwrap();
        assert_eq!(list(&conn).unwrap()[0].next_gross_total, dec!(119));
        let created = ids(&create_due(&conn, ymd(2027, 1, 31)).unwrap());
        let totals: Vec<Decimal> = created.iter().map(|id| db::get::<Document>(&conn, *id).unwrap().gross_total).collect();
        assert_eq!(totals, vec![dec!(119), dec!(119), dec!(124.95)], "November and December unchanged, January +5 %");
        assert_eq!(list(&conn).unwrap()[0].next_gross_total, dec!(124.95));
        assert_eq!(documents::lines_of(&conn, t).unwrap()[0].unit_price, dec!(100), "template unchanged");
    }

    #[test]
    fn invoices_can_be_finalized_automatically() {
        let conn = setup();
        let t = template(&conn);
        let r = save(&conn, RecurringInvoice { template_id: t, cycle: "monthly".into(), next_date: ymd(2026, 2, 28), auto_finalize: true, active: true, ..Default::default() }).unwrap();
        let created = create_due(&conn, ymd(2026, 3, 31)).unwrap();
        assert_eq!(created.len(), 2);
        assert!(created.iter().all(|c| c.finalized && !c.send));
        let numbers: Vec<String> = created.iter().map(|c| db::get::<Document>(&conn, c.document_id).unwrap().number).collect();
        assert!(numbers.iter().all(|n| !n.is_empty()) && numbers[0] != numbers[1], "{numbers:?}");

        // Without the company's data the invoice stays a draft and the reason is kept.
        let mut s = cfg::load(&conn).unwrap();
        s.company.street = String::new();
        cfg::save(&conn, &s).unwrap();
        let created = create_due(&conn, ymd(2026, 4, 30)).unwrap();
        assert!(!created[0].finalized);
        let d: Document = db::get(&conn, created[0].document_id).unwrap();
        assert_eq!((d.status.as_str(), d.number.as_str()), ("draft", ""));
        let r: RecurringInvoice = db::get(&conn, r.id).unwrap();
        assert!(r.last_error.contains("Entwurf geblieben") && r.last_error.contains("Anschrift"), "{}", r.last_error);

        // Sending needs SMTP.
        let send = save(&conn, RecurringInvoice { auto_send: true, ..r });
        assert!(send.unwrap_err().to_string().contains("E-Mail-Versand"));
    }
}
