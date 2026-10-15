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
    let mut out = Vec::new();
    for d in docs {
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

pub fn interest_for(s: &Settings, contact: Option<&Contact>, open: Decimal, due: Option<NaiveDate>, at: NaiveDate) -> Decimal {
    if !s.dunning.interest_enabled {
        return Decimal::ZERO;
    }
    let Some(due) = due else { return Decimal::ZERO };
    let days = (at - due).num_days();
    if days <= 0 {
        return Decimal::ZERO;
    }
    let b2b = contact.map(|c| c.kind != "person").unwrap_or(true);
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
        fee: req.fee.unwrap_or(def.fee),
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
