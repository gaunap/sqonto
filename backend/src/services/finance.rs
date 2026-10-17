// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Zahlungsverkehr: Zahlungskonten, Kontobewegungen, Zuordnungen, Kassenbuch.

use std::collections::HashMap;

use anyhow::Result;
use chrono::NaiveDate;
use rusqlite::Connection;
use rust_decimal::Decimal;
use rust_decimal::prelude::Signed;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::*;
use crate::services::{documents, posting, receipts, settings as cfg};
use crate::util::{self, fmt_money, now_str, r2, today};

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PaymentAccountView {
    pub account: PaymentAccount,
    #[ts(type = "number")]
    pub balance: Decimal,
    pub open_count: i64,
    pub last_date: Option<NaiveDate>,
}

pub fn balance_of(conn: &Connection, acc: &PaymentAccount, until: Option<NaiveDate>) -> Result<Decimal> {
    let txs = match until {
        Some(d) => db::query::<Transaction>(conn, "WHERE account_id = ?1 AND date <= ?2", &[v(acc.id), v(d)])?,
        None => db::query::<Transaction>(conn, "WHERE account_id = ?1", &[v(acc.id)])?,
    };
    Ok(acc.opening_balance + txs.iter().map(|t| t.amount).sum::<Decimal>())
}

pub fn accounts(conn: &Connection) -> Result<Vec<PaymentAccountView>> {
    let mut out = Vec::new();
    for a in db::query::<PaymentAccount>(conn, "ORDER BY archived, kind = 'cash', id", &[])? {
        let balance = balance_of(conn, &a, None)?;
        let open_count = db::scalar_i64(conn, "SELECT COUNT(*) FROM transactions WHERE account_id = ?1 AND status = 'open'", &[v(a.id)])?;
        let last_date: Option<String> =
            conn.query_row("SELECT MAX(date) FROM transactions WHERE account_id = ?1", [a.id], |r| r.get(0)).ok().flatten();
        out.push(PaymentAccountView {
            balance,
            open_count,
            last_date: last_date.and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
            account: a,
        });
    }
    Ok(out)
}

pub fn save_account(conn: &Connection, mut a: PaymentAccount) -> Result<i64> {
    let s = cfg::load(conn)?;
    if a.name.trim().is_empty() {
        bail_user!("Bitte einen Namen angeben.");
    }
    if a.kind.is_empty() {
        a.kind = "bank".into();
    }
    let old = if a.id > 0 { Some(db::get::<PaymentAccount>(conn, a.id)?) } else { None };
    if let Some(old) = &old {
        if a.ledger_account.is_empty() {
            a.ledger_account = old.ledger_account.clone();
        }
        // Kind and ledger account determine how the existing movements are booked.
        let has_movements = db::scalar_i64(conn, "SELECT COUNT(*) FROM transactions WHERE account_id = ?1", &[v(a.id)])? > 0;
        if has_movements && (a.kind != old.kind || a.ledger_account != old.ledger_account) {
            bail_user!("Kontoart und Sachkonto können nicht mehr geändert werden, sobald Kontobewegungen erfasst sind.");
        }
        if a.opening_balance != old.opening_balance || a.opening_date != old.opening_date {
            cfg::check_unlocked(conn, old.opening_date)?;
            cfg::check_unlocked(conn, a.opening_date)?;
        }
    }
    if a.ledger_account.is_empty() {
        a.ledger_account = match a.kind.as_str() {
            "cash" => crate::chart::role(&s.tax.chart, "kasse"),
            "other" => crate::chart::role(&s.tax.chart, "paypal"),
            _ => {
                let used = db::scalar_i64(
                    conn,
                    "SELECT COUNT(*) FROM payment_accounts WHERE ledger_account = ?1 AND id != ?2",
                    &[v(crate::chart::role(&s.tax.chart, "bank")), v(a.id)],
                )?;
                crate::chart::role(&s.tax.chart, if used > 0 { "bank2" } else { "bank" })
            }
        };
    }
    a.iban = util::normalize_iban(&a.iban);
    if a.kind == "cash" && a.opening_balance < Decimal::ZERO {
        bail_user!("Der Kassenbestand darf nicht negativ sein.");
    }
    let id = if let Some(old) = old {
        a.created_at = old.created_at;
        db::update(conn, &a)?;
        a.id
    } else {
        a.created_at = now_str();
        db::insert(conn, &mut a)?
    };
    posting::rebuild(conn, posting::Source::Opening(id))?;
    Ok(id)
}

pub fn delete_account(conn: &Connection, id: i64) -> Result<()> {
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM transactions WHERE account_id = ?1", &[v(id)])? > 0 {
        bail_user!("Das Konto enthält Buchungen und kann nur archiviert werden.");
    }
    db::delete::<PaymentAccount>(conn, id)?;
    posting::rebuild(conn, posting::Source::Opening(id))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Transactions & allocations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct TransactionInput {
    pub tx: Transaction,
    pub allocations: Vec<Allocation>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AllocationView {
    pub allocation: Allocation,
    pub label: String,
    pub target_number: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TransactionView {
    pub tx: Transaction,
    pub allocations: Vec<AllocationView>,
    pub account_name: String,
    #[ts(type = "number")]
    pub allocated: Decimal,
}

#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct TransactionFilter {
    pub account_id: Option<i64>,
    pub status: String,
    pub q: String,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

pub fn allocation_label(conn: &Connection, a: &Allocation, accounts: &HashMap<String, String>) -> Result<(String, String)> {
    Ok(match a.kind.as_str() {
        "document" => match a.document_id.and_then(|id| db::find::<Document>(conn, id).ok().flatten()) {
            Some(d) => (
                format!("{} {} · {}", documents::type_label(&d.doc_type, &d.kind), d.number, d.address.lines().next().unwrap_or_default()),
                d.number,
            ),
            None => ("Dokument (gelöscht)".into(), String::new()),
        },
        "receipt" => match a.receipt_id.and_then(|id| db::find::<Receipt>(conn, id).ok().flatten()) {
            Some(r) => {
                let party = receipts::party_name(conn, &r)?;
                (format!("Beleg {} · {}", r.number, party), r.number)
            }
            None => ("Beleg (gelöscht)".into(), String::new()),
        },
        "writeoff" => ("Ausbuchung".into(), String::new()),
        _ => {
            let name = accounts.get(&a.account).cloned().unwrap_or_default();
            let tax = if a.tax_rate > Decimal::ZERO { format!(" ({} USt)", util::fmt_percent(a.tax_rate)) } else { String::new() };
            (format!("{} {}{}", a.account, name, tax), a.account.clone())
        }
    })
}

fn account_names(conn: &Connection) -> Result<HashMap<String, String>> {
    Ok(cfg::accounts(conn)?.into_iter().map(|a| (a.number, a.name)).collect())
}

pub fn view(conn: &Connection, tx: Transaction) -> Result<TransactionView> {
    let names = account_names(conn)?;
    view_with(conn, tx, &names)
}

fn view_with(conn: &Connection, tx: Transaction, names: &HashMap<String, String>) -> Result<TransactionView> {
    let allocs = db::query::<Allocation>(conn, "WHERE transaction_id = ?1 ORDER BY id", &[v(tx.id)])?;
    let allocated = allocs.iter().map(|a| a.amount).sum();
    let mut views = Vec::new();
    for a in allocs {
        let (label, target_number) = allocation_label(conn, &a, names)?;
        views.push(AllocationView { allocation: a, label, target_number });
    }
    let account_name = db::find::<PaymentAccount>(conn, tx.account_id)?.map(|a| a.name).unwrap_or_default();
    Ok(TransactionView { tx, allocations: views, account_name, allocated })
}

pub fn list(conn: &Connection, f: &TransactionFilter) -> Result<Vec<TransactionView>> {
    let mut sql = String::from("WHERE 1=1");
    let mut params = Vec::new();
    if let Some(a) = f.account_id {
        params.push(v(a));
        sql.push_str(&format!(" AND account_id = ?{}", params.len()));
    }
    if !f.status.is_empty() && f.status != "all" {
        params.push(v(f.status.clone()));
        sql.push_str(&format!(" AND status = ?{}", params.len()));
    }
    if let Some(d) = f.from {
        params.push(v(d));
        sql.push_str(&format!(" AND date >= ?{}", params.len()));
    }
    if let Some(d) = f.to {
        params.push(v(d));
        sql.push_str(&format!(" AND date <= ?{}", params.len()));
    }
    sql.push_str(" ORDER BY date DESC, id DESC");
    let names = account_names(conn)?;
    let q = f.q.trim().to_lowercase();
    let mut out = Vec::new();
    for t in db::query::<Transaction>(conn, &sql, &params)? {
        if !q.is_empty() {
            let hay = format!("{} {} {} {}", t.counterparty, t.purpose, t.amount, t.counterparty_iban).to_lowercase();
            if !hay.contains(&q) {
                continue;
            }
        }
        out.push(view_with(conn, t, &names)?);
    }
    Ok(out)
}

fn validate_allocations(conn: &Connection, tx: &Transaction, allocs: &[Allocation]) -> Result<()> {
    let sum: Decimal = allocs.iter().map(|a| a.amount).sum();
    if sum.abs() > tx.amount.abs() + Decimal::new(1, 2) || (sum.signum() != tx.amount.signum() && !sum.is_zero()) {
        bail_user!("Die zugeordneten Beträge ({}) passen nicht zum Zahlungsbetrag ({}).", fmt_money(sum), fmt_money(tx.amount));
    }
    for a in allocs {
        match a.kind.as_str() {
            "document" => {
                let Some(d) = a.document_id.and_then(|id| db::find::<Document>(conn, id).ok().flatten()) else {
                    bail_user!("Rechnung nicht gefunden.")
                };
                if !d.is_invoice_like() || d.status == "draft" || d.status == "cancelled" {
                    bail_user!("{} kann keine Zahlung zugeordnet werden.", d.number);
                }
            }
            "receipt" => {
                if a.receipt_id.and_then(|id| db::find::<Receipt>(conn, id).ok().flatten()).is_none() {
                    bail_user!("Beleg nicht gefunden.");
                }
            }
            "account" => {
                if cfg::account_by_number(conn, &a.account)?.is_none() {
                    bail_user!("Konto {} existiert nicht.", a.account);
                }
            }
            _ => bail_user!("Ungültige Zuordnung."),
        }
    }
    Ok(())
}

fn affected_targets(allocs: &[Allocation]) -> (Vec<i64>, Vec<i64>) {
    let docs = allocs.iter().filter_map(|a| a.document_id).collect();
    let recs = allocs.iter().filter_map(|a| a.receipt_id).collect();
    (docs, recs)
}

fn refresh_targets(conn: &Connection, docs: &[i64], recs: &[i64]) -> Result<()> {
    for d in docs {
        documents::recompute_paid(conn, *d)?;
    }
    for r in recs {
        receipts::recompute_paid(conn, *r)?;
    }
    Ok(())
}

fn update_status(conn: &Connection, tx_id: i64) -> Result<()> {
    let mut tx: Transaction = db::get(conn, tx_id)?;
    let allocs = db::query::<Allocation>(conn, "WHERE transaction_id = ?1", &[v(tx_id)])?;
    let sum: Decimal = allocs.iter().map(|a| a.amount).sum();
    tx.status = if (tx.amount - sum).abs() < Decimal::new(1, 2) && !allocs.is_empty() { "booked".into() } else { "open".into() };
    db::update(conn, &tx)?;
    Ok(())
}

/// Ensures the cash balance never becomes negative (Kassenbuch).
fn check_cash_balance(conn: &Connection, account_id: i64) -> Result<()> {
    let acc: PaymentAccount = db::get(conn, account_id)?;
    if acc.kind != "cash" {
        return Ok(());
    }
    let txs = db::query::<Transaction>(conn, "WHERE account_id = ?1 ORDER BY date, cash_number, id", &[v(account_id)])?;
    let mut bal = acc.opening_balance;
    for t in txs {
        bal += t.amount;
        if bal < Decimal::ZERO {
            bail_user!(
                "Der Kassenbestand würde am {} negativ werden ({}). Bitte Buchungen prüfen.",
                util::fmt_date(t.date),
                fmt_money(bal)
            );
        }
    }
    Ok(())
}

pub fn create(conn: &Connection, input: TransactionInput) -> Result<i64> {
    let TransactionInput { mut tx, allocations } = input;
    let acc: PaymentAccount = db::get(conn, tx.account_id)?;
    cfg::check_unlocked(conn, tx.date)?;
    if tx.amount.is_zero() {
        bail_user!("Bitte einen Betrag angeben.");
    }
    tx.id = 0;
    tx.amount = r2(tx.amount);
    tx.created_at = now_str();
    if tx.source.is_empty() {
        tx.source = "manual".into();
    }
    if acc.kind == "cash" {
        tx.cash_number = Some(db::scalar_i64(conn, "SELECT MAX(cash_number) FROM transactions WHERE account_id = ?1", &[v(acc.id)])? + 1);
    }
    validate_allocations(conn, &tx, &allocations)?;
    let id = db::insert(conn, &mut tx)?;
    for mut a in allocations.clone() {
        a.id = 0;
        a.transaction_id = Some(id);
        a.date = tx.date;
        a.amount = r2(a.amount);
        a.created_at = now_str();
        db::insert(conn, &mut a)?;
    }
    check_cash_balance(conn, acc.id)?;
    update_status(conn, id)?;
    let (d, r) = affected_targets(&allocations);
    refresh_targets(conn, &d, &r)?;
    posting::rebuild(conn, posting::Source::Transaction(id))?;
    for doc in d {
        cfg::log(conn, "document", doc, "payment", format!("Zahlung über {} erfasst ({})", fmt_money(tx.amount.abs()), acc.name))?;
    }
    Ok(id)
}

pub fn update(conn: &Connection, id: i64, input: TransactionInput) -> Result<()> {
    let old: Transaction = db::get(conn, id)?;
    let acc: PaymentAccount = db::get(conn, old.account_id)?;
    cfg::check_unlocked(conn, old.date)?;
    let TransactionInput { mut tx, allocations } = input;
    cfg::check_unlocked(conn, tx.date)?;
    tx.id = id;
    tx.account_id = old.account_id;
    tx.created_at = old.created_at.clone();
    tx.cash_number = old.cash_number;
    tx.import_hash = old.import_hash.clone();
    tx.source = old.source.clone();
    tx.reversal_of = old.reversal_of;
    if old.source == "import" || acc.kind == "cash" {
        // Imported bank data and cash entries are immutable – only the assignment may change.
        tx.amount = old.amount;
        tx.date = old.date;
        if old.source == "import" {
            tx.counterparty = old.counterparty.clone();
            tx.counterparty_iban = old.counterparty_iban.clone();
            tx.purpose = old.purpose.clone();
        }
    }
    tx.amount = r2(tx.amount);
    validate_allocations(conn, &tx, &allocations)?;
    let old_allocs = db::query::<Allocation>(conn, "WHERE transaction_id = ?1", &[v(id)])?;
    db::update(conn, &tx)?;
    conn.execute("DELETE FROM allocations WHERE transaction_id = ?1", [id])?;
    for mut a in allocations.clone() {
        a.id = 0;
        a.transaction_id = Some(id);
        a.date = tx.date;
        a.amount = r2(a.amount);
        a.created_at = now_str();
        db::insert(conn, &mut a)?;
    }
    check_cash_balance(conn, acc.id)?;
    update_status(conn, id)?;
    let (mut d, mut r) = affected_targets(&allocations);
    let (d2, r2_) = affected_targets(&old_allocs);
    d.extend(d2);
    r.extend(r2_);
    d.sort();
    d.dedup();
    r.sort();
    r.dedup();
    refresh_targets(conn, &d, &r)?;
    posting::rebuild(conn, posting::Source::Transaction(id))?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let tx: Transaction = db::get(conn, id)?;
    cfg::check_unlocked(conn, tx.date)?;
    let acc: PaymentAccount = db::get(conn, tx.account_id)?;
    if acc.kind == "cash" {
        let last = db::scalar_i64(conn, "SELECT MAX(cash_number) FROM transactions WHERE account_id = ?1", &[v(acc.id)])?;
        if tx.cash_number != Some(last) {
            bail_user!("Kassenbuchungen können nicht gelöscht werden (GoBD). Bitte stattdessen stornieren.");
        }
    }
    let allocs = db::query::<Allocation>(conn, "WHERE transaction_id = ?1", &[v(id)])?;
    conn.execute("DELETE FROM allocations WHERE transaction_id = ?1", [id])?;
    db::delete::<Transaction>(conn, id)?;
    let (d, r) = affected_targets(&allocs);
    refresh_targets(conn, &d, &r)?;
    posting::rebuild(conn, posting::Source::Transaction(id))?;
    check_cash_balance(conn, acc.id)?;
    Ok(())
}

/// Removes a single allocation (e.g. "Zahlung entfernen" on an invoice).
pub fn delete_allocation(conn: &Connection, alloc_id: i64) -> Result<()> {
    let a: Allocation = db::get(conn, alloc_id)?;
    cfg::check_unlocked(conn, a.date)?;
    db::delete::<Allocation>(conn, alloc_id)?;
    if let Some(tid) = a.transaction_id {
        let tx: Transaction = db::get(conn, tid)?;
        let remaining = db::scalar_i64(conn, "SELECT COUNT(*) FROM allocations WHERE transaction_id = ?1", &[v(tid)])?;
        let acc: PaymentAccount = db::get(conn, tx.account_id)?;
        if remaining == 0 && tx.source == "payment" && acc.kind != "cash" {
            db::delete::<Transaction>(conn, tid)?;
        } else {
            update_status(conn, tid)?;
        }
        posting::rebuild(conn, posting::Source::Transaction(tid))?;
    } else {
        posting::rebuild(conn, posting::Source::Writeoff(alloc_id))?;
    }
    refresh_targets(conn, &a.document_id.into_iter().collect::<Vec<_>>(), &a.receipt_id.into_iter().collect::<Vec<_>>())?;
    if let Some(d) = a.document_id {
        cfg::log(conn, "document", d, "payment_removed", format!("Zahlung über {} entfernt", fmt_money(a.amount.abs())))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct PaymentRequest {
    pub account_id: Option<i64>,
    /// Use an existing (imported) transaction instead of creating a new one.
    pub transaction_id: Option<i64>,
    pub date: Option<NaiveDate>,
    #[ts(type = "number")]
    pub amount: Decimal,
    /// Additional amount booked as dunning fees (sonstige Erträge).
    #[ts(type = "number")]
    pub fee_amount: Decimal,
    /// "" | skonto | loss – write off the remaining open amount.
    pub writeoff: String,
    pub note: String,
}

pub fn pay_document(conn: &Connection, doc_id: i64, req: PaymentRequest) -> Result<()> {
    let s = cfg::load(conn)?;
    let doc: Document = db::get(conn, doc_id)?;
    if !doc.is_invoice_like() || doc.status == "draft" || doc.status == "cancelled" {
        bail_user!("Für dieses Dokument kann keine Zahlung erfasst werden.");
    }
    let date = req.date.unwrap_or_else(today);
    // Credit notes and negative invoices are settled by a refund.
    let open = doc.remaining();
    let amount = r2(req.amount);
    if amount < Decimal::ZERO {
        bail_user!("Bitte einen positiven Betrag angeben.");
    }
    if amount > open + Decimal::new(1, 2) {
        bail_user!("Der Betrag übersteigt den offenen Betrag von {}.", fmt_money(open.max(Decimal::ZERO)));
    }
    let sign = doc.money_sign();
    if amount > Decimal::ZERO || req.fee_amount > Decimal::ZERO {
        let mut allocations = Vec::new();
        if amount > Decimal::ZERO {
            allocations.push(Allocation {
                kind: "document".into(),
                document_id: Some(doc_id),
                amount: sign * amount,
                date,
                ..Default::default()
            });
        }
        if req.fee_amount > Decimal::ZERO {
            allocations.push(Allocation {
                kind: "account".into(),
                account: crate::chart::role(&s.tax.chart, "sonst_ertr"),
                amount: r2(req.fee_amount),
                date,
                description: format!("Mahngebühren {}", doc.number),
                ..Default::default()
            });
        }
        match req.transaction_id {
            Some(tid) => {
                let tx: Transaction = db::get(conn, tid)?;
                let mut existing: Vec<Allocation> = db::query::<Allocation>(conn, "WHERE transaction_id = ?1", &[v(tid)])?;
                existing.extend(allocations);
                update(conn, tid, TransactionInput { tx, allocations: existing })?;
            }
            None => {
                let Some(account_id) = req.account_id else { bail_user!("Bitte ein Zahlungskonto wählen.") };
                let contact = doc.contact_id.and_then(|c| db::find::<Contact>(conn, c).ok().flatten());
                let total: Decimal = allocations.iter().map(|a| a.amount).sum();
                let tx = Transaction {
                    account_id,
                    date,
                    amount: total,
                    counterparty: contact.map(|c| c.display_name()).unwrap_or_default(),
                    purpose: if req.note.is_empty() { format!("{} {}", documents::type_label(&doc.doc_type, &doc.kind), doc.number) } else { req.note.clone() },
                    source: "payment".into(),
                    status: "open".into(),
                    ..Default::default()
                };
                create(conn, TransactionInput { tx, allocations })?;
            }
        }
    }
    if !req.writeoff.is_empty() {
        let doc: Document = db::get(conn, doc_id)?;
        let rest = doc.remaining();
        if rest > Decimal::ZERO && doc.gross_total > Decimal::ZERO {
            add_writeoff(conn, doc_id, &req.writeoff, rest, date)?;
        }
    }
    Ok(())
}

pub fn add_writeoff(conn: &Connection, doc_id: i64, reason: &str, amount: Decimal, date: NaiveDate) -> Result<()> {
    cfg::check_unlocked(conn, date)?;
    let doc: Document = db::get(conn, doc_id)?;
    let mut a = Allocation {
        kind: "writeoff".into(),
        document_id: Some(doc_id),
        amount: doc.sign() * r2(amount),
        date,
        writeoff_reason: reason.into(),
        description: match reason {
            "skonto" => "Skonto".into(),
            "loss" => "Forderungsverlust".into(),
            _ => "Ausbuchung".into(),
        },
        created_at: now_str(),
        ..Default::default()
    };
    let id = db::insert(conn, &mut a)?;
    documents::recompute_paid(conn, doc_id)?;
    posting::rebuild(conn, posting::Source::Writeoff(id))?;
    cfg::log(conn, "document", doc_id, "writeoff", format!("{} über {} ausgebucht", a.description, fmt_money(amount)))?;
    Ok(())
}

pub fn pay_receipt(conn: &Connection, receipt_id: i64, req: PaymentRequest) -> Result<()> {
    let r: Receipt = db::get(conn, receipt_id)?;
    let date = req.date.unwrap_or_else(today);
    let amount = r2(req.amount);
    if amount <= Decimal::ZERO {
        bail_user!("Bitte einen positiven Betrag angeben.");
    }
    if amount > r.remaining() + Decimal::new(1, 2) {
        bail_user!("Der Betrag übersteigt den offenen Betrag von {}.", fmt_money(r.remaining().max(Decimal::ZERO)));
    }
    // Paid out for invoices, received for supplier credit notes.
    let money = r.money_sign() * amount;
    let alloc = Allocation { kind: "receipt".into(), receipt_id: Some(receipt_id), amount: money, date, ..Default::default() };
    match req.transaction_id {
        Some(tid) => {
            let tx: Transaction = db::get(conn, tid)?;
            let mut existing = db::query::<Allocation>(conn, "WHERE transaction_id = ?1", &[v(tid)])?;
            existing.push(alloc);
            update(conn, tid, TransactionInput { tx, allocations: existing })?;
        }
        None => {
            let Some(account_id) = req.account_id else { bail_user!("Bitte ein Zahlungskonto wählen.") };
            let tx = Transaction {
                account_id,
                date,
                amount: money,
                counterparty: receipts::party_name(conn, &r)?,
                purpose: if r.invoice_number.is_empty() { r.description.clone() } else { format!("Rechnung {}", r.invoice_number) },
                source: "payment".into(),
                status: "open".into(),
                ..Default::default()
            };
            create(conn, TransactionInput { tx, allocations: vec![alloc] })?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Matching suggestions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct MatchSuggestion {
    /// document | receipt
    pub kind: String,
    pub id: i64,
    pub number: String,
    pub name: String,
    pub date: NaiveDate,
    #[ts(type = "number")]
    pub open_amount: Decimal,
    pub score: i64,
    pub reason: String,
}

pub fn suggestions(conn: &Connection, tx_id: i64) -> Result<Vec<MatchSuggestion>> {
    let tx: Transaction = db::get(conn, tx_id)?;
    let allocated: Decimal = db::query::<Allocation>(conn, "WHERE transaction_id = ?1", &[v(tx_id)])?.iter().map(|a| a.amount).sum();
    let rest = tx.amount - allocated;
    let purpose = format!("{} {}", tx.purpose, tx.counterparty).to_lowercase();
    let normalized_purpose: String = purpose.chars().filter(|c| c.is_alphanumeric()).collect();
    let mut out = Vec::new();
    let contacts: HashMap<i64, Contact> = db::query::<Contact>(conn, "", &[])?.into_iter().map(|c| (c.id, c)).collect();
    let name_match = |cid: Option<i64>, fallback: &str| -> (String, bool) {
        let name = cid.and_then(|c| contacts.get(&c)).map(|c| c.display_name()).unwrap_or_else(|| fallback.to_string());
        let cp = tx.counterparty.to_lowercase();
        let n = name.to_lowercase();
        let hit = !n.is_empty() && !cp.is_empty() && (cp.contains(&n) || n.contains(&cp) || n.split_whitespace().any(|w| w.len() > 3 && cp.contains(w)));
        (name, hit)
    };
    let iban_match = |cid: Option<i64>| -> bool {
        let iban = util::normalize_iban(&tx.counterparty_iban);
        !iban.is_empty() && cid.and_then(|c| contacts.get(&c)).map(|c| util::normalize_iban(&c.iban) == iban).unwrap_or(false)
    };
    if rest > Decimal::ZERO || tx.amount > Decimal::ZERO {
        for d in db::query::<Document>(conn, "WHERE doc_type = 'invoice' AND status IN ('open','partial')", &[])? {
            let open = d.open_amount();
            if open <= Decimal::ZERO {
                continue;
            }
            let mut score = 0;
            let mut reasons = Vec::new();
            let num: String = d.number.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
            if !num.is_empty() && normalized_purpose.contains(&num) {
                score += 60;
                reasons.push("Rechnungsnummer im Verwendungszweck");
            }
            if (open - rest.abs()).abs() < Decimal::new(1, 2) {
                score += 30;
                reasons.push("Betrag stimmt überein");
            }
            let (name, hit) = name_match(d.contact_id, d.address.lines().next().unwrap_or_default());
            if hit {
                score += 15;
                reasons.push("Name passt");
            }
            if iban_match(d.contact_id) {
                score += 20;
                reasons.push("IBAN bekannt");
            }
            if score > 0 {
                out.push(MatchSuggestion {
                    kind: "document".into(),
                    id: d.id,
                    number: d.number.clone(),
                    name,
                    date: d.date,
                    open_amount: open,
                    score,
                    reason: reasons.join(", "),
                });
            }
        }
    }
    if rest < Decimal::ZERO || tx.amount < Decimal::ZERO {
        for d in db::query::<Document>(conn, "WHERE doc_type = 'credit_note' AND status IN ('open','partial')", &[])? {
            let open = d.open_amount();
            if (open - rest.abs()).abs() < Decimal::new(1, 2) {
                let (name, _) = name_match(d.contact_id, "");
                out.push(MatchSuggestion {
                    kind: "document".into(),
                    id: d.id,
                    number: d.number.clone(),
                    name,
                    date: d.date,
                    open_amount: open,
                    score: 40,
                    reason: "Betrag stimmt überein".into(),
                });
            }
        }
    }
    // Receipts in the direction of their open money: invoices are paid out, supplier credit notes
    // and income receipts are received.
    let (incoming, outgoing) = (rest > Decimal::ZERO || tx.amount > Decimal::ZERO, rest < Decimal::ZERO || tx.amount < Decimal::ZERO);
    {
        for r in db::query::<Receipt>(conn, "WHERE status IN ('open','partial')", &[])? {
            let open = r.remaining();
            let receives = r.money_sign() > Decimal::ZERO;
            if open <= Decimal::ZERO || (receives && !incoming) || (!receives && !outgoing) {
                continue;
            }
            let mut score = 0;
            let mut reasons = Vec::new();
            let num: String = r.invoice_number.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
            if num.len() >= 3 && normalized_purpose.contains(&num) {
                score += 60;
                reasons.push("Rechnungsnummer im Verwendungszweck");
            }
            if (open - rest.abs()).abs() < Decimal::new(1, 2) {
                score += 30;
                reasons.push("Betrag stimmt überein");
            }
            let (name, hit) = name_match(r.contact_id, &r.supplier_name);
            if hit {
                score += 15;
                reasons.push("Name passt");
            }
            if iban_match(r.contact_id) || (!r.iban.is_empty() && util::normalize_iban(&r.iban) == util::normalize_iban(&tx.counterparty_iban)) {
                score += 20;
                reasons.push("IBAN bekannt");
            }
            if score > 0 {
                out.push(MatchSuggestion {
                    kind: "receipt".into(),
                    id: r.id,
                    number: if r.invoice_number.is_empty() { r.number.clone() } else { r.invoice_number.clone() },
                    name,
                    date: r.date,
                    open_amount: open,
                    score,
                    reason: reasons.join(", "),
                });
            }
        }
    }
    out.sort_by(|a, b| b.score.cmp(&a.score).then(b.date.cmp(&a.date)));
    out.truncate(12);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Kassenbuch
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CashbookRow {
    pub tx: TransactionView,
    #[ts(type = "number")]
    pub balance: Decimal,
    pub reversed: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Cashbook {
    pub account: PaymentAccount,
    pub from: NaiveDate,
    pub to: NaiveDate,
    #[ts(type = "number")]
    pub opening: Decimal,
    #[ts(type = "number")]
    pub closing: Decimal,
    #[ts(type = "number")]
    pub income: Decimal,
    #[ts(type = "number")]
    pub expense: Decimal,
    pub rows: Vec<CashbookRow>,
}

pub fn cashbook(conn: &Connection, account_id: i64, from: NaiveDate, to: NaiveDate) -> Result<Cashbook> {
    let acc: PaymentAccount = db::get(conn, account_id)?;
    let names = account_names(conn)?;
    let all = db::query::<Transaction>(conn, "WHERE account_id = ?1 ORDER BY date, cash_number, id", &[v(account_id)])?;
    let reversed_ids: Vec<i64> = all.iter().filter_map(|t| t.reversal_of).collect();
    let mut bal = acc.opening_balance;
    let mut opening = acc.opening_balance;
    let mut rows = Vec::new();
    let (mut income, mut expense) = (Decimal::ZERO, Decimal::ZERO);
    for t in all {
        if t.date < from {
            bal += t.amount;
            opening = bal;
            continue;
        }
        if t.date > to {
            break;
        }
        bal += t.amount;
        if t.amount > Decimal::ZERO {
            income += t.amount;
        } else {
            expense -= t.amount;
        }
        let reversed = reversed_ids.contains(&t.id);
        rows.push(CashbookRow { tx: view_with(conn, t, &names)?, balance: bal, reversed });
    }
    Ok(Cashbook { account: acc, from, to, opening, closing: bal, income, expense, rows })
}

#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct CashEntryInput {
    pub account_id: i64,
    pub date: Option<NaiveDate>,
    /// in | out
    pub direction: String,
    #[ts(type = "number")]
    pub amount: Decimal,
    pub description: String,
    pub counter_account: String,
    #[ts(type = "number")]
    pub tax_rate: Decimal,
    pub receipt_id: Option<i64>,
    pub document_id: Option<i64>,
}

pub fn create_cash_entry(conn: &Connection, input: CashEntryInput) -> Result<i64> {
    let acc: PaymentAccount = db::get(conn, input.account_id)?;
    if acc.kind != "cash" {
        bail_user!("Das gewählte Konto ist keine Kasse.");
    }
    if input.amount <= Decimal::ZERO {
        bail_user!("Bitte einen Betrag größer 0 angeben.");
    }
    if input.description.trim().is_empty() {
        bail_user!("Bitte einen Buchungstext angeben.");
    }
    let sign = if input.direction == "in" { Decimal::ONE } else { Decimal::NEGATIVE_ONE };
    let amount = sign * r2(input.amount);
    let date = input.date.unwrap_or_else(today);
    let alloc = if let Some(did) = input.document_id {
        let d: Document = db::get(conn, did)?;
        if d.sign() != sign {
            bail_user!("Rechnungen werden als Einnahme, Gutschriften als Ausgabe gebucht.");
        }
        if amount.abs() > d.open_amount() + Decimal::new(1, 2) {
            bail_user!("Der Betrag übersteigt den offenen Betrag von {}.", fmt_money(d.open_amount()));
        }
        Allocation { kind: "document".into(), document_id: Some(did), amount, ..Default::default() }
    } else if let Some(rid) = input.receipt_id {
        Allocation { kind: "receipt".into(), receipt_id: Some(rid), amount, ..Default::default() }
    } else {
        if input.counter_account.is_empty() {
            bail_user!("Bitte eine Kategorie (Gegenkonto) wählen.");
        }
        Allocation { kind: "account".into(), account: input.counter_account.clone(), tax_rate: input.tax_rate, amount, description: input.description.clone(), ..Default::default() }
    };
    create(
        conn,
        TransactionInput {
            tx: Transaction { account_id: acc.id, date, amount, purpose: input.description.clone(), source: "cash".into(), ..Default::default() },
            allocations: vec![alloc],
        },
    )
}

pub fn reverse_cash_entry(conn: &Connection, id: i64) -> Result<i64> {
    let tx: Transaction = db::get(conn, id)?;
    if tx.reversal_of.is_some() {
        bail_user!("Eine Stornobuchung kann nicht erneut storniert werden.");
    }
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM transactions WHERE reversal_of = ?1", &[v(id)])? > 0 {
        bail_user!("Die Buchung wurde bereits storniert.");
    }
    let allocs = db::query::<Allocation>(conn, "WHERE transaction_id = ?1", &[v(id)])?;
    let reversed: Vec<Allocation> = allocs
        .into_iter()
        .map(|mut a| {
            a.id = 0;
            a.amount = -a.amount;
            a
        })
        .collect();
    let new_tx = Transaction {
        account_id: tx.account_id,
        date: today().max(tx.date),
        amount: -tx.amount,
        counterparty: tx.counterparty.clone(),
        purpose: format!("Storno Nr. {}: {}", tx.cash_number.unwrap_or(0), tx.purpose),
        source: tx.source.clone(),
        reversal_of: Some(id),
        ..Default::default()
    };
    create(conn, TransactionInput { tx: new_tx, allocations: reversed })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ymd;
    use rust_decimal_macros::dec;

    #[test]
    fn payment_account_changes_respect_bookings() {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        let id = save_account(&conn, PaymentAccount { name: "Bank".into(), opening_balance: dec!(1000), opening_date: ymd(2020, 1, 1), ..Default::default() }).unwrap();
        let acc: PaymentAccount = db::get(&conn, id).unwrap();
        // Without movements the ledger account may still change.
        assert!(save_account(&conn, PaymentAccount { ledger_account: "1210".into(), ..acc.clone() }).is_ok());
        let acc: PaymentAccount = db::get(&conn, id).unwrap();
        let tx = Transaction { account_id: id, date: ymd(2020, 3, 1), amount: dec!(-10), purpose: "Gebühr".into(), ..Default::default() };
        create(&conn, TransactionInput { tx, allocations: vec![] }).unwrap();

        assert!(save_account(&conn, PaymentAccount { ledger_account: "1200".into(), ..acc.clone() }).is_err(), "ledger account fixed");
        assert!(save_account(&conn, PaymentAccount { kind: "other".into(), ..acc.clone() }).is_err(), "kind fixed");
        // An empty ledger account in an update keeps the current one.
        assert!(save_account(&conn, PaymentAccount { ledger_account: String::new(), name: "Geschäftskonto".into(), ..acc.clone() }).is_ok());
        assert_eq!(db::get::<PaymentAccount>(&conn, id).unwrap().ledger_account, "1210");

        cfg::lock_until(&conn, ymd(2020, 12, 31)).unwrap();
        let acc: PaymentAccount = db::get(&conn, id).unwrap();
        assert!(save_account(&conn, PaymentAccount { opening_balance: dec!(2000), ..acc.clone() }).is_err(), "opening balance in locked period");
        assert!(save_account(&conn, PaymentAccount { opening_date: ymd(2021, 1, 1), ..acc.clone() }).is_err(), "moving the opening out of the locked period");
        assert!(save_account(&conn, PaymentAccount { iban: "DE02120300000000202051".into(), archived: true, ..acc }).is_ok(), "master data stays editable");
    }
}
