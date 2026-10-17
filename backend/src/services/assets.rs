// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Anlagevermögen: Abschreibungspläne (linear pro rata temporis, GWG, Sammelposten).

use anyhow::Result;
use chrono::Datelike;
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::Serialize;
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::{Account, Asset};
use crate::services::{posting, settings as cfg};
use crate::util::{now_str, r2, today};

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AfaYear {
    pub year: i32,
    #[ts(type = "number")]
    pub opening: Decimal,
    #[ts(type = "number")]
    pub afa: Decimal,
    #[ts(type = "number")]
    pub disposal: Decimal,
    #[ts(type = "number")]
    pub closing: Decimal,
    pub months: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AssetView {
    pub asset: Asset,
    pub account_name: String,
    pub schedule: Vec<AfaYear>,
    #[ts(type = "number")]
    pub book_value: Decimal,
    #[ts(type = "number")]
    pub afa_current_year: Decimal,
    #[ts(type = "number")]
    pub accumulated: Decimal,
}

pub fn schedule(a: &Asset) -> Vec<AfaYear> {
    let mut rows = Vec::new();
    let acq_year = a.acquisition_date.year();
    let acq_month = a.acquisition_date.month() as i64;
    let cost = a.cost;
    if cost <= Decimal::ZERO {
        return rows;
    }
    let disposal = a.disposal_date;
    match a.method.as_str() {
        "gwg" => {
            rows.push(AfaYear { year: acq_year, opening: cost, afa: cost, disposal: Decimal::ZERO, closing: Decimal::ZERO, months: 12 });
        }
        "pool" => {
            let mut book = cost;
            let part = r2(cost / Decimal::from(5));
            for i in 0..5 {
                let afa = if i == 4 { book } else { part };
                rows.push(AfaYear {
                    year: acq_year + i,
                    opening: book,
                    afa,
                    disposal: Decimal::ZERO,
                    closing: book - afa,
                    months: 12,
                });
                book -= afa;
            }
        }
        "none" => {
            let end_year = disposal.map(|d| d.year()).unwrap_or(today().year()).max(acq_year);
            for y in acq_year..=end_year {
                let is_disp = disposal.map(|d| d.year() == y).unwrap_or(false);
                rows.push(AfaYear {
                    year: y,
                    opening: cost,
                    afa: Decimal::ZERO,
                    disposal: if is_disp { cost } else { Decimal::ZERO },
                    closing: if is_disp { Decimal::ZERO } else { cost },
                    months: 0,
                });
            }
        }
        _ => {
            let total = (a.useful_life_years.max(1) * 12) as i64;
            let cum = |m: i64| -> Decimal { r2(cost * Decimal::from(m.clamp(0, total)) / Decimal::from(total)) };
            let mut y = acq_year;
            let mut prev_months = 0i64;
            loop {
                let mut months_end = (i64::from(y - acq_year)) * 12 + (12 - acq_month + 1);
                let mut is_disp = false;
                if let Some(d) = disposal
                    && d.year() == y
                {
                    months_end = (i64::from(y - acq_year)) * 12 + d.month() as i64 - acq_month + 1;
                    is_disp = true;
                }
                let opening = cost - cum(prev_months);
                let afa = cum(months_end) - cum(prev_months);
                let closing_before = opening - afa;
                let disp_amount = if is_disp { closing_before } else { Decimal::ZERO };
                rows.push(AfaYear {
                    year: y,
                    opening,
                    afa,
                    disposal: disp_amount,
                    closing: closing_before - disp_amount,
                    months: (months_end.min(total) - prev_months.min(total)).max(0),
                });
                prev_months = months_end;
                if is_disp || months_end >= total || y > acq_year + 60 {
                    break;
                }
                y += 1;
            }
        }
    }
    // Note: Sammelposten are not reduced by disposals (§ 6 Abs. 2a EStG).
    rows
}

pub fn view(conn: &Connection, a: Asset) -> Result<AssetView> {
    let account_name = cfg::account_by_number(conn, &a.account)?.map(|x| x.name).unwrap_or_default();
    let sched = schedule(&a);
    let year = today().year();
    let accumulated: Decimal = sched.iter().filter(|r| r.year < year).map(|r| r.afa).sum();
    let afa_current_year = sched.iter().find(|r| r.year == year).map(|r| r.afa).unwrap_or_default();
    let book_value = sched
        .iter()
        .filter(|r| r.year <= year)
        .last()
        .map(|r| r.closing)
        .unwrap_or(if a.acquisition_date.year() > year { a.cost } else { Decimal::ZERO });
    Ok(AssetView { account_name, schedule: sched, book_value, afa_current_year, accumulated, asset: a })
}

pub fn list(conn: &Connection) -> Result<Vec<AssetView>> {
    db::query::<Asset>(conn, "ORDER BY acquisition_date DESC, id DESC", &[])?
        .into_iter()
        .map(|a| view(conn, a))
        .collect()
}

pub fn default_method(cost: Decimal, account: &Account) -> &'static str {
    if account.number == "0485" || account.number == "0675" {
        "pool"
    } else if account.number == "0480" || account.number == "0670" || cost <= Decimal::from(800) {
        "gwg"
    } else {
        "linear"
    }
}

/// Default useful life (AfA-Tabelle) by account.
pub fn default_life(account_number: &str) -> i64 {
    match account_number {
        "0027" | "0135" => 3,
        "0320" | "0520" | "0350" | "0540" => 6,
        "0420" | "0650" => 13,
        "0430" | "0640" => 8,
        "0440" | "0620" => 5,
        "0210" => 10,
        _ => 5,
    }
}

pub fn save(conn: &Connection, mut a: Asset) -> Result<i64> {
    if a.name.trim().is_empty() {
        bail_user!("Bitte eine Bezeichnung angeben.");
    }
    if a.cost <= Decimal::ZERO {
        bail_user!("Die Anschaffungskosten müssen größer als 0 sein.");
    }
    let acc = cfg::account_by_number(conn, &a.account)?;
    match acc {
        Some(acc) if acc.kind == "asset" => {}
        _ => bail_user!("Bitte ein Anlagekonto auswählen."),
    }
    if a.method.is_empty() {
        a.method = "linear".into();
    }
    if a.method == "gwg" && a.cost > Decimal::from(800) {
        bail_user!("Geringwertige Wirtschaftsgüter dürfen höchstens 800 € netto kosten.");
    }
    if a.method == "pool" && (a.cost <= Decimal::from(250) || a.cost > Decimal::from(1000)) {
        bail_user!("In den Sammelposten gehören nur Wirtschaftsgüter von 250,01 € bis 1.000 € netto.");
    }
    if a.useful_life_years <= 0 {
        a.useful_life_years = 1;
    }
    let old = if a.id > 0 { Some(db::get::<Asset>(conn, a.id)?) } else { None };
    match &old {
        Some(old) => {
            // Changes to the schedule rewrite the depreciation since acquisition; a disposal only
            // affects its own year. Name, number and notes stay editable.
            let schedule_changed = a.acquisition_date != old.acquisition_date
                || a.cost != old.cost
                || a.useful_life_years != old.useful_life_years
                || a.method != old.method
                || a.account != old.account;
            if schedule_changed {
                cfg::check_unlocked(conn, old.acquisition_date)?;
                cfg::check_unlocked(conn, a.acquisition_date)?;
            }
            if a.disposal_date != old.disposal_date || a.disposal_proceeds != old.disposal_proceeds {
                for d in [old.disposal_date, a.disposal_date].into_iter().flatten() {
                    cfg::check_unlocked(conn, d)?;
                }
            }
        }
        None => cfg::check_unlocked(conn, a.acquisition_date)?,
    }
    let id = if let Some(old) = old {
        a.created_at = old.created_at;
        a.number = if a.number.is_empty() { old.number } else { a.number };
        db::update(conn, &a)?;
        a.id
    } else {
        if a.number.is_empty() {
            a.number = cfg::next_number(conn, "asset", a.acquisition_date)?;
        }
        a.created_at = now_str();
        db::insert(conn, &mut a)?
    };
    posting::rebuild(conn, posting::Source::Asset(id))?;
    Ok(id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let a: Asset = db::get(conn, id)?;
    cfg::check_unlocked(conn, a.acquisition_date)?;
    if a.receipt_line_id.is_some() {
        conn.execute("UPDATE receipt_lines SET asset_id = NULL WHERE asset_id = ?1", [id])?;
    }
    db::delete::<Asset>(conn, id)?;
    posting::rebuild(conn, posting::Source::Asset(id))?;
    Ok(())
}

/// Creates or updates the asset belonging to a receipt line booked on an asset account.
pub fn sync_from_receipt_line(
    conn: &Connection,
    receipt_id: i64,
    line: &mut crate::models::ReceiptLine,
    date: chrono::NaiveDate,
    small_business: bool,
) -> Result<()> {
    let Some(acc) = cfg::account_by_number(conn, &line.account)? else { return Ok(()) };
    if acc.kind != "asset" {
        if let Some(aid) = line.asset_id.take() {
            db::delete::<Asset>(conn, aid)?;
            posting::rebuild(conn, posting::Source::Asset(aid))?;
        }
        return Ok(());
    }
    let cost = if small_business { line.gross } else { line.net };
    let existing = match line.asset_id {
        Some(aid) => db::find::<Asset>(conn, aid)?,
        None => None,
    };
    let mut a = existing.unwrap_or_else(|| Asset {
        method: default_method(cost, &acc).into(),
        useful_life_years: if line.asset_life_years > 0 { line.asset_life_years } else { default_life(&acc.number) },
        ..Default::default()
    });
    a.name = if line.description.trim().is_empty() { acc.name.clone() } else { line.description.clone() };
    a.account = acc.number.clone();
    a.acquisition_date = date;
    a.cost = cost;
    a.receipt_id = Some(receipt_id);
    a.receipt_line_id = if line.id > 0 { Some(line.id) } else { None };
    if line.asset_life_years > 0 {
        a.useful_life_years = line.asset_life_years;
    }
    if a.method == "gwg" && cost > Decimal::from(800) {
        a.method = "linear".into();
    }
    if a.method == "pool" && (cost <= Decimal::from(250) || cost > Decimal::from(1000)) {
        a.method = if cost <= Decimal::from(800) { "gwg".into() } else { "linear".into() };
    }
    let id = save(conn, a)?;
    line.asset_id = Some(id);
    Ok(())
}

pub fn assets_of_receipt(conn: &Connection, receipt_id: i64) -> Result<Vec<Asset>> {
    db::query::<Asset>(conn, "WHERE receipt_id = ?1", &[v(receipt_id)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ymd;
    use rust_decimal_macros::dec;

    fn asset(cost: Decimal, years: i64, method: &str, date: chrono::NaiveDate) -> Asset {
        Asset { cost, useful_life_years: years, method: method.into(), acquisition_date: date, ..Default::default() }
    }

    #[test]
    fn linear_pro_rata() {
        let a = asset(dec!(3600), 3, "linear", ymd(2025, 10, 15));
        let s = schedule(&a);
        assert_eq!(s[0].year, 2025);
        assert_eq!(s[0].afa, dec!(300)); // Oct-Dec = 3 months à 100
        assert_eq!(s[1].afa, dec!(1200));
        assert_eq!(s.last().unwrap().closing, dec!(0));
        assert_eq!(s.iter().map(|r| r.afa).sum::<Decimal>(), dec!(3600));
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn pool_and_gwg() {
        let p = schedule(&asset(dec!(999.99), 5, "pool", ymd(2026, 6, 1)));
        assert_eq!(p.len(), 5);
        assert_eq!(p.iter().map(|r| r.afa).sum::<Decimal>(), dec!(999.99));
        let g = schedule(&asset(dec!(500), 3, "gwg", ymd(2026, 6, 1)));
        assert_eq!(g[0].afa, dec!(500));
    }

    #[test]
    fn lock_protects_past_depreciation_but_allows_disposal() {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        let mut a = asset(dec!(3600), 3, "linear", ymd(2020, 1, 1));
        a.name = "Laptop".into();
        a.account = "0420".into();
        let id = save(&conn, a).unwrap();
        cfg::lock_until(&conn, ymd(2021, 12, 31)).unwrap();
        let a: Asset = db::get(&conn, id).unwrap();
        let afa = |year: &str| -> Decimal {
            db::query::<crate::models::JournalEntry>(&conn, "WHERE source_type = 'asset' AND source_id = ?1 AND description LIKE ?2", &[v(id), v(format!("AfA {year}%"))])
                .unwrap()
                .iter()
                .map(|e| db::query::<crate::models::JournalLine>(&conn, "WHERE entry_id = ?1", &[v(e.id)]).unwrap().iter().map(|l| l.debit).sum::<Decimal>())
                .sum()
        };

        assert!(save(&conn, Asset { cost: dec!(4000), ..a.clone() }).is_err(), "cost of a locked acquisition");
        assert!(save(&conn, Asset { useful_life_years: 5, ..a.clone() }).is_err(), "useful life");
        assert!(save(&conn, Asset { acquisition_date: ymd(2022, 1, 1), ..a.clone() }).is_err(), "moving it out of the locked period");
        assert!(save(&conn, Asset { name: "Notebook".into(), notes: "Inventar 7".into(), ..a.clone() }).is_ok(), "master data");
        assert!(save(&conn, Asset { disposal_date: Some(ymd(2021, 6, 30)), ..a.clone() }).is_err(), "disposal in the locked period");

        // Sold in 2022: allowed, earlier years keep their depreciation.
        save(&conn, Asset { disposal_date: Some(ymd(2022, 6, 30)), ..a }).unwrap();
        assert_eq!((afa("2020"), afa("2021"), afa("2022")), (dec!(1200), dec!(1200), dec!(600)));

        let mut b = asset(dec!(1200), 3, "linear", ymd(2022, 5, 1));
        b.name = "Monitor".into();
        b.account = "0420".into();
        let b: Asset = db::get(&conn, save(&conn, b).unwrap()).unwrap();
        assert!(save(&conn, Asset { acquisition_date: ymd(2021, 5, 1), ..b }).is_err(), "moving it into the locked period");
    }

    #[test]
    fn disposal() {
        let mut a = asset(dec!(1200), 1, "linear", ymd(2026, 1, 1));
        a.useful_life_years = 2;
        a.disposal_date = Some(ymd(2026, 6, 30));
        let s = schedule(&a);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].afa, dec!(300));
        assert_eq!(s[0].disposal, dec!(900));
        assert_eq!(s[0].closing, dec!(0));
    }
}
