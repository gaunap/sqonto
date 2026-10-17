// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Belegerfassung: Eingangsrechnungen, Quittungen und sonstige Belege.

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
    if let Some(other) = duplicate_of(conn, r)? {
        bail_user!(
            "Die Rechnung {} von {} ist bereits als Beleg {} vom {} erfasst.",
            r.invoice_number.trim(),
            party_name(conn, r)?,
            other.number,
            crate::util::fmt_date(other.date)
        );
    }
    Ok(())
}

/// Another receipt of the same business partner with the same invoice number (ignoring case,
/// spaces and punctuation) – the same supplier invoice entered twice would double the Vorsteuer.
pub fn duplicate_of(conn: &Connection, r: &Receipt) -> Result<Option<Receipt>> {
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let number = norm(&r.invoice_number);
    if number.is_empty() {
        return Ok(None);
    }
    let party = norm(&party_name(conn, r)?);
    for other in db::query::<Receipt>(conn, "WHERE id != ?1 AND invoice_number != ''", &[v(r.id)])? {
        let same_party = match (r.contact_id, other.contact_id) {
            (Some(a), Some(b)) if a == b => true,
            _ => !party.is_empty() && norm(&party_name(conn, &other)?) == party,
        };
        if same_party && norm(&other.invoice_number) == number {
            return Ok(Some(other));
        }
    }
    Ok(None)
}

pub fn recompute_paid(conn: &Connection, id: i64) -> Result<()> {
    let Some(mut r) = db::find::<Receipt>(conn, id)? else { return Ok(()) };
    let allocs = db::query::<Allocation>(conn, "WHERE receipt_id = ?1", &[v(id)])?;
    r.paid_total = allocs.iter().map(|a| a.amount).sum::<Decimal>() * r.sign();
    // Supplier credit notes have negative totals and are settled by a refund.
    r.status = if r.remaining() <= Decimal::ZERO {
        "paid".into()
    } else if r.paid_total.signum() == r.gross_total.signum() && !r.paid_total.is_zero() {
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
            p.amount = receipt.gross_total.abs();
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
            p.amount = r.remaining();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Transaction;
    use crate::services::finance::{self, PaymentRequest, TransactionInput};
    use crate::services::reports;
    use crate::util::ymd;
    use rust_decimal_macros::dec;

    fn setup() -> (Connection, i64) {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        let bank = finance::save_account(&conn, PaymentAccount { name: "Bank".into(), opening_date: ymd(2026, 1, 1), ..Default::default() }).unwrap();
        (conn, bank)
    }

    fn receipt(conn: &Connection, net: Decimal) -> i64 {
        let receipt = Receipt { supplier_name: "Lieferant GmbH".into(), receipt_type: "expense".into(), date: ymd(2026, 3, 1), prices_gross: false, ..Default::default() };
        let lines = vec![ReceiptLine { account: "4930".into(), net, tax_rate: dec!(19), ..Default::default() }];
        create(conn, ReceiptInput { receipt, lines, payment: None }).unwrap()
    }

    #[test]
    fn same_supplier_invoice_cannot_be_entered_twice() {
        let (conn, _) = setup();
        let save = |supplier: &str, invoice: &str| {
            let receipt = Receipt { supplier_name: supplier.into(), invoice_number: invoice.into(), receipt_type: "expense".into(), date: ymd(2026, 3, 1), prices_gross: true, ..Default::default() };
            let lines = vec![ReceiptLine { account: "4930".into(), gross: dec!(119), tax_rate: dec!(19), ..Default::default() }];
            create(&conn, ReceiptInput { receipt, lines, payment: None })
        };
        let first = save("Lieferant GmbH", "R-2026/001").unwrap();
        let err = save("lieferant gmbh", "r 2026 001").unwrap_err().to_string();
        assert!(err.contains("bereits als Beleg BE-"), "{err}");
        assert!(save("Anderer Lieferant", "R-2026/001").is_ok(), "same number from another supplier");
        assert!(save("Lieferant GmbH", "").is_ok(), "receipts without invoice number");
        assert!(save("Lieferant GmbH", "").is_ok());
        // Saving the receipt itself again is no duplicate.
        let full = get_full(&conn, first).unwrap();
        update(&conn, first, ReceiptInput { receipt: full.receipt, lines: full.lines, payment: None }).unwrap();
    }

    #[test]
    fn supplier_credit_note_is_settled_by_refund() {
        let (conn, bank) = setup();
        let id = receipt(&conn, dec!(-100));
        let r: Receipt = db::get(&conn, id).unwrap();
        assert_eq!((r.gross_total, r.remaining(), r.money_sign()), (dec!(-119), dec!(119), dec!(1)));
        let open = reports::open_items(&conn).unwrap();
        assert_eq!((open.payables.len(), open.payables_total), (1, dec!(-119)), "open credit note reduces payables");

        // The incoming refund is suggested for the credit note …
        let tx = Transaction { account_id: bank, date: ymd(2026, 3, 5), amount: dec!(119), counterparty: "Lieferant GmbH".into(), source: "import".into(), ..Default::default() };
        let tx = finance::create(&conn, TransactionInput { tx, allocations: vec![] }).unwrap();
        let sug = finance::suggestions(&conn, tx).unwrap();
        assert!(sug.iter().any(|s| s.kind == "receipt" && s.id == id && s.open_amount == dec!(119)), "{sug:?}");

        // … and settles it.
        finance::pay_receipt(&conn, id, PaymentRequest { transaction_id: Some(tx), amount: dec!(119), ..Default::default() }).unwrap();
        let r: Receipt = db::get(&conn, id).unwrap();
        assert_eq!((r.status.as_str(), r.remaining()), ("paid", dec!(0)));
        assert!(reports::open_items(&conn).unwrap().payables.is_empty());
        // EÜR: the Vorsteuer correction is booked with the refund.
        assert_eq!(reports::ustva(&conn, 2026, "Q1").unwrap().input_tax, dec!(-19));
        assert!(finance::pay_receipt(&conn, id, PaymentRequest { account_id: Some(bank), amount: dec!(1), ..Default::default() }).is_err());
    }

    #[test]
    fn credit_note_saved_as_refunded() {
        let (conn, bank) = setup();
        let receipt = Receipt { supplier_name: "Lieferant GmbH".into(), receipt_type: "expense".into(), date: ymd(2026, 3, 1), prices_gross: true, ..Default::default() };
        let lines = vec![ReceiptLine { account: "4930".into(), gross: dec!(-59.50), tax_rate: dec!(19), ..Default::default() }];
        let payment = Some(PaymentRequest { account_id: Some(bank), ..Default::default() });
        let id = create(&conn, ReceiptInput { receipt, lines, payment }).unwrap();
        let r: Receipt = db::get(&conn, id).unwrap();
        assert_eq!(r.status, "paid");
        let money: Decimal = db::query::<Transaction>(&conn, "", &[]).unwrap().iter().map(|t| t.amount).sum();
        assert_eq!(money, dec!(59.50), "refund received");
    }

    #[test]
    fn invoice_receipt_is_paid_out() {
        let (conn, bank) = setup();
        let id = receipt(&conn, dec!(100));
        finance::pay_receipt(&conn, id, PaymentRequest { account_id: Some(bank), amount: dec!(50), date: Some(ymd(2026, 3, 10)), ..Default::default() }).unwrap();
        assert_eq!(db::get::<Receipt>(&conn, id).unwrap().status, "partial");
        finance::pay_receipt(&conn, id, PaymentRequest { account_id: Some(bank), amount: dec!(69), date: Some(ymd(2026, 3, 20)), ..Default::default() }).unwrap();
        let r: Receipt = db::get(&conn, id).unwrap();
        assert_eq!((r.status.as_str(), r.paid_total), ("paid", dec!(119)));
        let money: Decimal = db::query::<Transaction>(&conn, "", &[]).unwrap().iter().map(|t| t.amount).sum();
        assert_eq!(money, dec!(-119));
        assert_eq!(reports::ustva(&conn, 2026, "Q1").unwrap().input_tax, dec!(19));
    }
}
