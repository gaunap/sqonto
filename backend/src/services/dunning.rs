// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Mahnwesen: Zahlungserinnerungen und Mahnstufen.

use anyhow::Result;
use chrono::{Duration, NaiveDate};
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::*;
use crate::services::documents::{self, DocumentFilter, DocumentListItem};
use crate::services::settings as cfg;
use crate::util::{self, HUNDRED, fmt_date, fmt_money, now_str, r2, today};

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DunningSuggestion {
    pub document: DocumentListItem,
    pub next_level: i64,
    pub level_name: String,
    #[ts(type = "number")]
    pub fee: Decimal,
    pub last_dunning: Option<Dunning>,
    pub due: bool,
}

#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct DunningRequest {
    pub document_id: i64,
    pub level: Option<i64>,
    pub date: Option<NaiveDate>,
    #[ts(type = "number | null")]
    pub fee: Option<Decimal>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DunningListItem {
    pub dunning: Dunning,
    pub document_number: String,
    pub contact_name: String,
    pub level_name: String,
}

fn level_def(s: &Settings, level: i64) -> DunningLevel {
    s.dunning
        .levels
        .iter()
        .find(|l| l.level == level)
        .cloned()
        .or_else(|| s.dunning.levels.last().cloned())
        .unwrap_or_default()
}

fn max_level(s: &Settings) -> i64 {
    s.dunning.levels.iter().map(|l| l.level).max().unwrap_or(3)
}

/// Open, overdue invoices with the next dunning level that is due (or upcoming).
pub fn suggestions_all(conn: &Connection) -> Result<Vec<DunningSuggestion>> {
    let s = cfg::load(conn)?;
    let t = today();
    let docs = documents::list(conn, &DocumentFilter { doc_type: "invoice".into(), status: "overdue".into(), ..Default::default() })?;
    // Invoices collected by direct debit are only dunned after a Rücklastschrift.
    let debit = crate::services::debit::not_dunnable(conn)?;
    let mut out = Vec::new();
    for d in docs.into_iter().filter(|d| !debit.contains(&d.id)) {
        let last = db::query_one::<Dunning>(conn, "WHERE document_id = ?1 ORDER BY level DESC, id DESC", &[v(d.id)])?;
        let next_level = match &last {
            Some(l) => l.level + 1,
            None => 0,
        };
        if next_level > max_level(&s) {
            continue;
        }
        let def = level_def(&s, next_level);
        let due = match &last {
            Some(l) => t > l.due_date + Duration::days(def.days.max(0)),
            None => d.days_overdue >= def.days,
        };
        out.push(DunningSuggestion { level_name: def.name.clone(), fee: def.fee, document: d, next_level, last_dunning: last, due });
    }
    out.sort_by(|a, b| b.document.days_overdue.cmp(&a.document.days_overdue));
    Ok(out)
}

pub fn suggestions(conn: &Connection) -> Result<Vec<DunningSuggestion>> {
    Ok(suggestions_all(conn)?.into_iter().filter(|s| s.due).collect())
}

/// Verzugspauschale for business customers (§ 288 Abs. 5 BGB).
pub const FLAT_FEE_B2B: Decimal = Decimal::from_parts(40, 0, 0, false, 0);

fn is_b2b(contact: Option<&Contact>) -> bool {
    contact.map(|c| c.kind != "person").unwrap_or(true)
}

/// Fee of a dunning. With the Verzugspauschale, a business customer pays at least 40 € from the
/// first Mahnung on; the Pauschale is offset against the fees of the levels (§ 288 Abs. 5 S. 3
/// BGB), so in total the larger of both is charged.
fn fee_for(s: &Settings, contact: Option<&Contact>, level: i64, charged_before: Decimal) -> Decimal {
    let def = level_def(s, level);
    if !s.dunning.flat_fee_b2b || !is_b2b(contact) || level < 1 {
        return def.fee;
    }
    let level_fees: Decimal = (0..=level).map(|l| level_def(s, l).fee).sum();
    (level_fees.max(FLAT_FEE_B2B) - charged_before).max(Decimal::ZERO)
}

/// The Basiszinssatz changes on 1 January and 1 July (§ 247 BGB): the entered value may be outdated
/// once a change date has passed since it was confirmed.
pub fn base_rate_outdated(s: &Settings, today: NaiveDate) -> bool {
    use chrono::Datelike;
    if !s.dunning.interest_enabled {
        return false;
    }
    let last_change = crate::util::ymd(today.year(), if today.month() >= 7 { 7 } else { 1 }, 1);
    s.dunning.base_rate_since.is_none_or(|d| d < last_change)
}

pub fn interest_for(s: &Settings, contact: Option<&Contact>, open: Decimal, due: Option<NaiveDate>, at: NaiveDate) -> Decimal {
    if !s.dunning.interest_enabled {
        return Decimal::ZERO;
    }
    let Some(due) = due else { return Decimal::ZERO };
    let days = (at - due).num_days();
    if days <= 0 {
        return Decimal::ZERO;
    }
    let b2b = is_b2b(contact);
    let rate = s.dunning.base_rate + if b2b { s.dunning.surcharge_b2b } else { s.dunning.surcharge_b2c };
    r2(open * rate / HUNDRED * Decimal::from(days) / Decimal::from(365))
}

pub fn placeholders(s: &Settings, doc: &Document, contact: Option<&Contact>, dn: &Dunning) -> Vec<(&'static str, String)> {
    let mut vars = documents::placeholders(s, doc, contact);
    vars.extend([
        ("Rechnungsnummer", doc.number.clone()),
        ("Rechnungsdatum", fmt_date(doc.date)),
        ("Offener Betrag", fmt_money(dn.open_amount)),
        ("Mahngebühr", fmt_money(dn.fee)),
        ("Zinsen", fmt_money(dn.interest)),
        ("Gesamtbetrag", fmt_money(dn.open_amount + dn.fee + dn.interest)),
        ("Neue Frist", fmt_date(dn.due_date)),
    ]);
    vars
}

pub fn build(conn: &Connection, req: &DunningRequest) -> Result<(Dunning, Document, Option<Contact>)> {
    let s = cfg::load(conn)?;
    let doc: Document = db::get(conn, req.document_id)?;
    if doc.doc_type != "invoice" || doc.open_amount() <= Decimal::ZERO {
        bail_user!("Für diese Rechnung ist keine Mahnung möglich (nicht offen).");
    }
    let last = db::query_one::<Dunning>(conn, "WHERE document_id = ?1 ORDER BY level DESC, id DESC", &[v(doc.id)])?;
    let level = req.level.unwrap_or_else(|| last.as_ref().map(|l| l.level + 1).unwrap_or(0)).clamp(0, max_level(&s));
    let def = level_def(&s, level);
    let date = req.date.unwrap_or_else(today);
    let contact = doc.contact_id.and_then(|c| db::find::<Contact>(conn, c).ok().flatten());
    let open = doc.open_amount();
    let interest = if level >= 1 { interest_for(&s, contact.as_ref(), open, doc.due_date, date) } else { Decimal::ZERO };
    let mut dn = Dunning {
        id: 0,
        document_id: doc.id,
        level,
        date,
        due_date: date + Duration::days(if def.deadline_days > 0 { def.deadline_days } else { 10 }),
        fee: match req.fee {
            Some(fee) => fee,
            None => {
                let charged: Decimal = db::query::<Dunning>(conn, "WHERE document_id = ?1", &[v(doc.id)])?.iter().map(|d| d.fee).sum();
                fee_for(&s, contact.as_ref(), level, charged)
            }
        },
        interest,
        open_amount: open,
        title: def.title.clone(),
        text: String::new(),
        created_at: now_str(),
    };
    let template = req.text.clone().unwrap_or(def.text.clone());
    dn.text = util::fill_placeholders(&template, &placeholders(&s, &doc, contact.as_ref(), &dn));
    Ok((dn, doc, contact))
}

pub fn create(conn: &Connection, req: DunningRequest) -> Result<Dunning> {
    let (mut dn, mut doc, _) = build(conn, &req)?;
    db::insert(conn, &mut dn)?;
    doc.dunning_level = doc.dunning_level.max(dn.level + 1);
    db::update(conn, &doc)?;
    cfg::log(conn, "document", doc.id, "dunning", format!("{} erstellt (Frist {})", dn.title, fmt_date(dn.due_date)))?;
    Ok(dn)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let dn: Dunning = db::get(conn, id)?;
    db::delete::<Dunning>(conn, id)?;
    let mut doc: Document = db::get(conn, dn.document_id)?;
    doc.dunning_level = db::scalar_i64(conn, "SELECT MAX(level) + 1 FROM dunnings WHERE document_id = ?1", &[v(doc.id)])?;
    db::update(conn, &doc)?;
    cfg::log(conn, "document", doc.id, "dunning", format!("{} gelöscht", dn.title))?;
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<DunningListItem>> {
    let s = cfg::load(conn)?;
    let mut out = Vec::new();
    for dn in db::query::<Dunning>(conn, "ORDER BY date DESC, id DESC", &[])? {
        let Some(doc) = db::find::<Document>(conn, dn.document_id)? else { continue };
        let contact_name = doc
            .contact_id
            .and_then(|c| db::find::<Contact>(conn, c).ok().flatten())
            .map(|c| c.display_name())
            .unwrap_or_else(|| doc.address.lines().next().unwrap_or_default().to_string());
        out.push(DunningListItem { level_name: level_def(&s, dn.level).name, document_number: doc.number, contact_name, dunning: dn });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ymd;
    use rust_decimal_macros::dec;

    fn settings(fees: &[Decimal], flat_fee: bool) -> Settings {
        let mut s = Settings::default();
        s.dunning.levels = fees.iter().enumerate().map(|(i, f)| DunningLevel { level: i as i64, fee: *f, ..Default::default() }).collect();
        s.dunning.flat_fee_b2b = flat_fee;
        s
    }

    #[test]
    fn flat_fee_is_offset_against_level_fees() {
        let company = Contact { kind: "organization".into(), company: "Kunde GmbH".into(), ..Default::default() };
        let person = Contact { kind: "person".into(), last_name: "Muster".into(), ..Default::default() };
        let s = settings(&[dec!(0), dec!(5), dec!(10), dec!(30)], true);
        // Zahlungserinnerung: no Pauschale yet; 1. Mahnung: 40 €; then nothing until the level fees exceed 40 €.
        assert_eq!(fee_for(&s, Some(&company), 0, dec!(0)), dec!(0));
        assert_eq!(fee_for(&s, Some(&company), 1, dec!(0)), dec!(40));
        assert_eq!(fee_for(&s, Some(&company), 2, dec!(40)), dec!(0));
        assert_eq!(fee_for(&s, Some(&company), 3, dec!(40)), dec!(5), "0 + 5 + 10 + 30 = 45 > 40");
        // Consumers and disabled Pauschale: the fees of the levels.
        assert_eq!(fee_for(&s, Some(&person), 1, dec!(0)), dec!(5));
        assert_eq!(fee_for(&settings(&[dec!(0), dec!(5)], false), Some(&company), 1, dec!(0)), dec!(5));
    }

    #[test]
    fn base_rate_must_be_confirmed_after_each_change_date() {
        let mut s = Settings::default();
        assert!(!base_rate_outdated(&s, ymd(2026, 10, 17)), "no interest, no warning");
        s.dunning.interest_enabled = true;
        assert!(base_rate_outdated(&s, ymd(2026, 10, 17)), "never confirmed");
        s.dunning.base_rate_since = Some(ymd(2026, 1, 1));
        assert!(base_rate_outdated(&s, ymd(2026, 10, 17)), "1 July has passed");
        assert!(!base_rate_outdated(&s, ymd(2026, 6, 30)));
        s.dunning.base_rate_since = Some(ymd(2026, 7, 1));
        assert!(!base_rate_outdated(&s, ymd(2026, 12, 31)));
        assert!(base_rate_outdated(&s, ymd(2027, 1, 2)));
    }
}
