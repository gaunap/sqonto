// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Bankregeln: recurring bank movements (e.g. "Telekom → 4920, 19 %") are booked automatically
//! when they are imported – or later for movements that are still open.

use anyhow::Result;
use rusqlite::Connection;
use rust_decimal::Decimal;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::{Allocation, BankRule, PaymentAccount, Transaction};
use crate::services::finance::{self, TransactionInput};
use crate::services::settings as cfg;
use crate::util::{self, now_str};

pub fn list(conn: &Connection) -> Result<Vec<BankRule>> {
    db::query::<BankRule>(conn, "ORDER BY id", &[])
}

pub fn save(conn: &Connection, mut r: BankRule) -> Result<BankRule> {
    r.counterparty = r.counterparty.trim().to_string();
    r.purpose = r.purpose.trim().to_string();
    r.iban = util::normalize_iban(&r.iban);
    if r.counterparty.is_empty() && r.purpose.is_empty() && r.iban.is_empty() {
        bail_user!("Bitte mindestens ein Merkmal angeben (Name, IBAN oder Text im Verwendungszweck).");
    }
    if !["", "in", "out"].contains(&r.direction.as_str()) {
        bail_user!("Ungültige Richtung.");
    }
    if cfg::account_by_number(conn, &r.account)?.is_none() {
        bail_user!("Bitte ein gültiges Konto wählen.");
    }
    if r.tax_rate < Decimal::ZERO || r.tax_rate > Decimal::from(100) {
        bail_user!("Ungültiger Steuersatz.");
    }
    if r.name.trim().is_empty() {
        r.name = [r.counterparty.as_str(), r.iban.as_str(), r.purpose.as_str()].into_iter().find(|s| !s.is_empty()).unwrap_or_default().to_string();
    }
    if r.id > 0 {
        let old: BankRule = db::get(conn, r.id)?;
        r.created_at = old.created_at;
        db::update(conn, &r)?;
    } else {
        r.created_at = now_str();
        db::insert(conn, &mut r)?;
    }
    Ok(r)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    db::delete::<BankRule>(conn, id)
}

/// All filled-in criteria must match (texts case-insensitively as part, IBAN exactly).
pub fn matches(r: &BankRule, tx: &Transaction) -> bool {
    let contains = |hay: &str, needle: &str| needle.is_empty() || hay.to_lowercase().contains(&needle.to_lowercase());
    let direction = match r.direction.as_str() {
        "in" => tx.amount > Decimal::ZERO,
        "out" => tx.amount < Decimal::ZERO,
        _ => true,
    };
    r.active
        && !(r.counterparty.is_empty() && r.purpose.is_empty() && r.iban.is_empty())
        && direction
        && contains(&tx.counterparty, &r.counterparty)
        && contains(&tx.purpose, &r.purpose)
        && (r.iban.is_empty() || util::normalize_iban(&tx.counterparty_iban) == r.iban)
}

/// Books an unassigned bank movement with the first matching rule; true if a rule applied.
/// A failure leaves the movement unassigned (savepoint) instead of aborting e.g. an import.
pub fn apply_to(conn: &Connection, tx_id: i64, rules: &[BankRule]) -> Result<bool> {
    let tx: Transaction = db::get(conn, tx_id)?;
    let assigned = db::scalar_i64(conn, "SELECT COUNT(*) FROM allocations WHERE transaction_id = ?1", &[v(tx_id)])? > 0;
    if tx.status != "open" || assigned || cfg::check_unlocked(conn, tx.date).is_err() {
        return Ok(false);
    }
    if db::get::<PaymentAccount>(conn, tx.account_id)?.kind == "cash" {
        return Ok(false);
    }
    let Some(rule) = rules.iter().find(|r| matches(r, &tx)) else { return Ok(false) };
    let alloc = Allocation {
        kind: "account".into(),
        account: rule.account.clone(),
        tax_rate: rule.tax_rate,
        amount: tx.amount,
        date: tx.date,
        description: rule.description.clone(),
        ..Default::default()
    };
    conn.execute_batch("SAVEPOINT bank_rule")?;
    match finance::update(conn, tx_id, TransactionInput { tx, allocations: vec![alloc] }) {
        Ok(()) => {
            conn.execute_batch("RELEASE bank_rule")?;
            Ok(true)
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK TO bank_rule; RELEASE bank_rule")?;
            tracing::warn!("Bankregel „{}“ nicht angewendet: {e:#}", rule.name);
            Ok(false)
        }
    }
}

/// Applies the rules to all bank movements that are still open; returns how many were booked.
pub fn apply_open(conn: &Connection) -> Result<i64> {
    let rules: Vec<BankRule> = list(conn)?.into_iter().filter(|r| r.active).collect();
    if rules.is_empty() {
        return Ok(0);
    }
    let mut stmt = conn.prepare("SELECT id FROM transactions WHERE status = 'open' ORDER BY date, id")?;
    let ids: Vec<i64> = stmt.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
    drop(stmt);
    let mut applied = 0;
    for id in ids {
        if apply_to(conn, id, &rules)? {
            applied += 1;
        }
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ymd;
    use rust_decimal_macros::dec;

    fn setup() -> (Connection, i64) {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        let bank = finance::save_account(&conn, PaymentAccount { name: "Bank".into(), opening_date: ymd(2020, 1, 1), ..Default::default() }).unwrap();
        (conn, bank)
    }

    fn movement(conn: &Connection, bank: i64, amount: Decimal, party: &str, purpose: &str) -> i64 {
        let tx = Transaction { account_id: bank, date: ymd(2026, 3, 3), amount, counterparty: party.into(), purpose: purpose.into(), source: "import".into(), ..Default::default() };
        finance::create(conn, TransactionInput { tx, allocations: vec![] }).unwrap()
    }

    #[test]
    fn rules_book_matching_open_movements() {
        let (conn, bank) = setup();
        save(&conn, BankRule { counterparty: "telekom".into(), direction: "out".into(), account: "4920".into(), tax_rate: dec!(19), active: true, ..Default::default() }).unwrap();
        let phone = movement(&conn, bank, dec!(-59.50), "Telekom Deutschland GmbH", "Rechnung 03/2026");
        let refund = movement(&conn, bank, dec!(10), "Telekom Deutschland GmbH", "Gutschrift");
        let other = movement(&conn, bank, dec!(-20), "Bäckerei", "Brötchen");
        assert_eq!(apply_open(&conn).unwrap(), 1);
        let booked = db::query::<Allocation>(&conn, "WHERE transaction_id = ?1", &[v(phone)]).unwrap();
        assert_eq!((booked.len(), booked[0].account.as_str(), booked[0].tax_rate), (1, "4920", dec!(19)));
        assert_eq!(db::get::<Transaction>(&conn, phone).unwrap().status, "booked");
        for id in [refund, other] {
            assert_eq!(db::get::<Transaction>(&conn, id).unwrap().status, "open", "direction / name do not match");
        }
        assert_eq!(apply_open(&conn).unwrap(), 0, "already booked movements are left alone");
    }

    #[test]
    fn rules_need_a_criterion_and_a_valid_account() {
        let (conn, _) = setup();
        assert!(save(&conn, BankRule { account: "4920".into(), active: true, ..Default::default() }).is_err());
        assert!(save(&conn, BankRule { counterparty: "X".into(), account: "9999".into(), active: true, ..Default::default() }).is_err());
        let r = save(&conn, BankRule { iban: "de02 1203 0000 0000 2020 51".into(), account: "4920".into(), active: true, ..Default::default() }).unwrap();
        assert_eq!((r.iban.as_str(), r.name.as_str()), ("DE02120300000000202051", "DE02120300000000202051"));
    }

    #[test]
    fn imports_apply_rules() {
        let (conn, bank) = setup();
        save(&conn, BankRule { purpose: "Miete".into(), account: "4210".into(), active: true, ..Default::default() }).unwrap();
        let csv = "Buchungstag;Name Zahlungsbeteiligter;Verwendungszweck;Betrag\n01.03.2026;Vermieter;Miete März;-800,00\n02.03.2026;Kunde;RE-1;119,00\n";
        let p = crate::services::bankimport::preview(&conn, bank, csv.as_bytes(), None).unwrap();
        let s = crate::services::bankimport::import(&conn, bank, p.transactions, true).unwrap();
        assert_eq!((s.imported, s.rules_applied), (2, 1));
    }
}
