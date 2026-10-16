// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Belegerfassung: Eingangsrechnungen, Quittungen und sonstige Belege.

use anyhow::Result;
use chrono::NaiveDate;
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::*;
use crate::services::finance::{self, PaymentRequest};
use crate::services::{assets, posting, settings as cfg};
use crate::util::{HUNDRED, now_str, r2};

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct ReceiptInput {
    pub receipt: Receipt,
    pub lines: Vec<ReceiptLine>,
    /// Optional: mark as paid immediately.
    pub payment: Option<PaymentRequest>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReceiptListItem {
    pub receipt: Receipt,
    pub party: String,
    pub categories: Vec<String>,
    pub file_name: String,
    #[ts(type = "number")]
    pub open_amount: Decimal,
    pub overdue: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReceiptFull {
    pub receipt: Receipt,
    pub lines: Vec<ReceiptLine>,
    pub file: Option<FileMeta>,
    pub contact: Option<Contact>,
    pub payments: Vec<crate::services::documents::PaymentInfo>,
    pub assets: Vec<Asset>,
    #[ts(type = "number")]
    pub open_amount: Decimal,
    /// Parsed content of an attached e-invoice (XRechnung / ZUGFeRD), if any.
    pub einvoice: Option<crate::services::einvoice::ParsedInvoice>,
}

#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct ReceiptFilter {
    pub status: String,
    pub receipt_type: String,
    pub q: String,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub contact_id: Option<i64>,
}

pub fn party_name(conn: &Connection, r: &Receipt) -> Result<String> {
    if let Some(cid) = r.contact_id
        && let Some(c) = db::find::<Contact>(conn, cid)?
    {
        return Ok(c.display_name());
    }
    Ok(r.supplier_name.clone())
}

pub fn lines_of(conn: &Connection, id: i64) -> Result<Vec<ReceiptLine>> {
    db::query::<ReceiptLine>(conn, "WHERE receipt_id = ?1 ORDER BY position, id", &[v(id)])
}

pub fn list(conn: &Connection, f: &ReceiptFilter) -> Result<Vec<ReceiptListItem>> {
    let mut sql = String::from("WHERE 1=1");
    let mut params = Vec::new();
    if !f.receipt_type.is_empty() {
        params.push(v(f.receipt_type.clone()));
        sql.push_str(&format!(" AND receipt_type = ?{}", params.len()));
    }
    if let Some(d) = f.from {
        params.push(v(d));
        sql.push_str(&format!(" AND date >= ?{}", params.len()));
    }
    if let Some(d) = f.to {
        params.push(v(d));
        sql.push_str(&format!(" AND date <= ?{}", params.len()));
    }
    if let Some(c) = f.contact_id {
        params.push(v(c));
        sql.push_str(&format!(" AND contact_id = ?{}", params.len()));
    }
    sql.push_str(" ORDER BY date DESC, id DESC");
    let accounts: std::collections::HashMap<String, String> =
        cfg::accounts(conn)?.into_iter().map(|a| (a.number, a.name)).collect();
    let q = f.q.trim().to_lowercase();
    let today = crate::util::today();
    let mut out = Vec::new();
    for r in db::query::<Receipt>(conn, &sql, &params)? {
        let party = party_name(conn, &r)?;
        let lines = lines_of(conn, r.id)?;
        let mut categories: Vec<String> = lines.iter().map(|l| accounts.get(&l.account).cloned().unwrap_or(l.account.clone())).collect();
        categories.dedup();
        if !q.is_empty() {
            let hay = format!("{} {} {} {} {}", r.number, r.invoice_number, party, r.description, categories.join(" ")).to_lowercase();
            if !hay.contains(&q) {
                continue;
            }
        }
        let overdue = r.status != "paid" && r.due_date.map(|d| d < today).unwrap_or(false);
        let ok = match f.status.as_str() {
            "" | "all" => true,
            "unpaid" => r.status != "paid",
            "overdue" => overdue,
            s => r.status == s,
        };
        if !ok {
            continue;
        }
        let file_name = match r.file_id {
            Some(fid) => db::find::<FileMeta>(conn, fid)?.map(|f| f.filename).unwrap_or_default(),
            None => String::new(),
        };
        out.push(ReceiptListItem { open_amount: r.open_amount(), receipt: r, party, categories, file_name, overdue });
    }
    Ok(out)
}

pub fn get_full(conn: &Connection, id: i64) -> Result<ReceiptFull> {
    let receipt: Receipt = db::get(conn, id)?;
    let lines = lines_of(conn, id)?;
    let file = match receipt.file_id {
        Some(f) => db::find::<FileMeta>(conn, f)?,
        None => None,
    };
    let contact = match receipt.contact_id {
        Some(c) => db::find::<Contact>(conn, c)?,
        None => None,
    };
    let mut payments = Vec::new();
    for a in db::query::<Allocation>(conn, "WHERE receipt_id = ?1 ORDER BY date, id", &[v(id)])? {
        let tx = match a.transaction_id {
            Some(t) => db::find::<Transaction>(conn, t)?,
            None => None,
        };
        let account_name = match &tx {
            Some(t) => db::find::<PaymentAccount>(conn, t.account_id)?.map(|p| p.name).unwrap_or_default(),
            None => String::new(),
        };
        payments.push(crate::services::documents::PaymentInfo { allocation: a, transaction: tx, account_name });
    }
    let assets = assets::assets_of_receipt(conn, id)?;
    Ok(ReceiptFull { open_amount: receipt.open_amount(), receipt, lines, file, contact, payments, assets, einvoice: None })
}

/// Computes line and header amounts.
pub fn calculate(r: &mut Receipt, lines: &mut [ReceiptLine]) {
    let reverse_charge = r.tax_mode != "standard";
    for (i, l) in lines.iter_mut().enumerate() {
        l.position = i as i64 + 1;
        if reverse_charge {
            // Supplier invoices net; VAT is owed by us (§ 13b / igE).
            if r.prices_gross {
                l.net = r2(l.gross);
            }
            l.net = r2(l.net);
            l.tax = Decimal::ZERO;
            l.gross = l.net;
        } else if r.prices_gross {
            l.gross = r2(l.gross);
            l.net = r2(l.gross / (Decimal::ONE + l.tax_rate / HUNDRED));
            l.tax = l.gross - l.net;
        } else {
            l.net = r2(l.net);
            l.tax = r2(l.net * l.tax_rate / HUNDRED);
            l.gross = l.net + l.tax;
        }
    }
    r.net_total = lines.iter().map(|l| l.net).sum();
    r.tax_total = lines.iter().map(|l| l.tax).sum();
    r.gross_total = lines.iter().map(|l| l.gross).sum();
}

fn validate(conn: &Connection, r: &Receipt, lines: &[ReceiptLine]) -> Result<()> {
    if lines.is_empty() {
        bail_user!("Bitte mindestens eine Position (Kategorie und Betrag) erfassen.");
    }
    if r.contact_id.is_none() && r.supplier_name.trim().is_empty() {
        bail_user!("Bitte einen Lieferanten bzw. Geschäftspartner angeben.");
    }
    for l in lines {
        if l.account.is_empty() {
            bail_user!("Bitte für jede Position eine Kategorie wählen.");
        }
        if cfg::account_by_number(conn, &l.account)?.is_none() {
            bail_user!("Konto {} existiert nicht.", l.account);
        }
        if l.gross.is_zero() && l.net.is_zero() {
            bail_user!("Bitte für jede Position einen Betrag angeben.");
        }
    }
    Ok(())
}

pub fn recompute_paid(conn: &Connection, id: i64) -> Result<()> {
    let Some(mut r) = db::find::<Receipt>(conn, id)? else { return Ok(()) };
    let allocs = db::query::<Allocation>(conn, "WHERE receipt_id = ?1", &[v(id)])?;
    r.paid_total = allocs.iter().map(|a| a.amount).sum::<Decimal>() * r.sign();
    r.status = if r.gross_total.abs() > Decimal::ZERO && r.paid_total >= r.gross_total {
        "paid".into()
    } else if r.paid_total > Decimal::ZERO {
        "partial".into()
    } else {
        "open".into()
    };
    db::update(conn, &r)?;
    Ok(())
}

fn save_lines(conn: &Connection, r: &Receipt, lines: &mut [ReceiptLine], small_business: bool) -> Result<()> {
    let old = lines_of(conn, r.id)?;
    conn.execute("DELETE FROM receipt_lines WHERE receipt_id = ?1", [r.id])?;
    for l in lines.iter_mut() {
        l.id = 0;
        l.receipt_id = r.id;
        db::insert(conn, l)?;
        assets::sync_from_receipt_line(conn, r.id, l, r.date, small_business)?;
        db::update(conn, l)?;
    }
    // Remove assets of deleted lines.
    for o in old {
        if let Some(aid) = o.asset_id
            && !lines.iter().any(|l| l.asset_id == Some(aid))
        {
            db::delete::<Asset>(conn, aid)?;
            posting::rebuild(conn, posting::Source::Asset(aid))?;
        }
    }
    Ok(())
}

pub fn create(conn: &Connection, input: ReceiptInput) -> Result<i64> {
    let s = cfg::load(conn)?;
    let ReceiptInput { mut receipt, mut lines, payment } = input;
    cfg::check_unlocked(conn, receipt.date)?;
    receipt.id = 0;
    if receipt.receipt_type.is_empty() {
        receipt.receipt_type = "expense".into();
    }
    if receipt.tax_mode.is_empty() {
        receipt.tax_mode = "standard".into();
    }
    if receipt.source.is_empty() {
        receipt.source = "manual".into();
    }
    calculate(&mut receipt, &mut lines);
    validate(conn, &receipt, &lines)?;
    receipt.number = cfg::next_number(conn, "receipt", receipt.date)?;
    receipt.paid_total = Decimal::ZERO;
    receipt.status = "open".into();
    receipt.created_at = now_str();
    receipt.updated_at = now_str();
    let id = db::insert(conn, &mut receipt)?;
    save_lines(conn, &receipt, &mut lines, s.tax.small_business)?;
    posting::rebuild_receipt_with_payments(conn, id)?;
    if let Some(mut p) = payment
        && (p.account_id.is_some() || p.transaction_id.is_some())
    {
        if p.amount.is_zero() {
            p.amount = receipt.gross_total;
        }
        if p.date.is_none() {
            p.date = Some(receipt.date);
        }
        finance::pay_receipt(conn, id, p)?;
    }
    cfg::log(conn, "receipt", id, "created", "Beleg erfasst")?;
    Ok(id)
}

pub fn update(conn: &Connection, id: i64, input: ReceiptInput) -> Result<()> {
    let s = cfg::load(conn)?;
    let old: Receipt = db::get(conn, id)?;
    cfg::check_unlocked(conn, old.date)?;
    let ReceiptInput { mut receipt, mut lines, payment } = input;
    cfg::check_unlocked(conn, receipt.date)?;
    receipt.id = id;
    receipt.number = old.number.clone();
    receipt.paid_total = old.paid_total;
    receipt.status = old.status.clone();
    receipt.created_at = old.created_at.clone();
    receipt.updated_at = now_str();
    calculate(&mut receipt, &mut lines);
    validate(conn, &receipt, &lines)?;
    if receipt.gross_total < old.paid_total {
        bail_user!("Der Belegbetrag ist kleiner als die bereits zugeordneten Zahlungen.");
    }
    db::update(conn, &receipt)?;
    save_lines(conn, &receipt, &mut lines, s.tax.small_business)?;
    recompute_paid(conn, id)?;
    posting::rebuild_receipt_with_payments(conn, id)?;
    if let Some(mut p) = payment
        && (p.account_id.is_some() || p.transaction_id.is_some())
    {
        let r: Receipt = db::get(conn, id)?;
        if p.amount.is_zero() {
            p.amount = r.open_amount();
        }
        if p.amount > Decimal::ZERO {
            finance::pay_receipt(conn, id, p)?;
        }
    }
    cfg::log(conn, "receipt", id, "updated", "Beleg bearbeitet")?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let r: Receipt = db::get(conn, id)?;
    cfg::check_unlocked(conn, r.date)?;
    let allocs = db::query::<Allocation>(conn, "WHERE receipt_id = ?1", &[v(id)])?;
    for a in allocs {
        finance::delete_allocation(conn, a.id)?;
    }
    for a in assets::assets_of_receipt(conn, id)? {
        db::delete::<Asset>(conn, a.id)?;
        posting::rebuild(conn, posting::Source::Asset(a.id))?;
    }
    db::delete::<Receipt>(conn, id)?;
    posting::rebuild(conn, posting::Source::Receipt(id))?;
    Ok(())
}

/// Most recently used category for a supplier (smart default).
pub fn suggest_account(conn: &Connection, contact_id: Option<i64>, name: &str) -> Result<Option<(String, Decimal)>> {
    let row: Option<(String, String)> = if let Some(c) = contact_id {
        conn.query_row(
            "SELECT l.account, l.tax_rate FROM receipt_lines l JOIN receipts r ON r.id = l.receipt_id WHERE r.contact_id = ?1 ORDER BY r.date DESC, r.id DESC LIMIT 1",
            [c],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
    } else if !name.trim().is_empty() {
        conn.query_row(
            "SELECT l.account, l.tax_rate FROM receipt_lines l JOIN receipts r ON r.id = l.receipt_id WHERE lower(r.supplier_name) = lower(?1) ORDER BY r.date DESC, r.id DESC LIMIT 1",
            [name.trim()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
    } else {
        None
    };
    Ok(row.map(|(a, t)| (a, t.parse().unwrap_or_default())))
}
