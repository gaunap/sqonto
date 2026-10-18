// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Auswertungen: UStVA, EÜR, GuV, Bilanz, BWA, SuSa, Journal, Kontenblätter,
//! Offene Posten, Kennzahlen und Dashboard.

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::Serialize;
use ts_rs::TS;

use crate::chart::{BILANZ_AKTIVA, BILANZ_PASSIVA, BWA_LINES, EUER_EXPENSE, EUER_INCOME, EUER_NABZ, GUV_LINES};
use crate::db::{self, v};
use crate::models::*;
use crate::services::documents::{self, DocumentFilter, DocumentListItem};
use crate::services::finance::{self, PaymentAccountView};
use crate::services::receipts::{self, ReceiptFilter, ReceiptListItem};
use crate::services::settings as cfg;
use crate::util::{last_day_of_month, month_name, month_short, today, ymd};

// ---------------------------------------------------------------------------
// Raw journal access
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RawLine {
    pub entry_id: i64,
    pub date: NaiveDate,
    pub account: String,
    pub debit: Decimal,
    pub credit: Decimal,
    pub tax_code: String,
    pub base: Decimal,
}

fn d(s: String) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or_default()
}

pub fn raw_lines(conn: &Connection, from: Option<NaiveDate>, to: Option<NaiveDate>) -> Result<Vec<RawLine>> {
    let from = from.unwrap_or(ymd(1900, 1, 1));
    let to = to.unwrap_or(ymd(2999, 12, 31));
    let mut stmt = conn.prepare_cached(
        "SELECT l.entry_id, e.date, l.account, l.debit, l.credit, l.tax_code, l.base
         FROM journal_lines l JOIN journal_entries e ON e.id = l.entry_id
         WHERE e.date >= ?1 AND e.date <= ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![from.to_string(), to.to_string()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (entry_id, date, account, debit, credit, tax_code, base) = r?;
        out.push(RawLine {
            entry_id,
            date: NaiveDate::parse_from_str(&date, "%Y-%m-%d").unwrap_or(from),
            account,
            debit: d(debit),
            credit: d(credit),
            tax_code,
            base: d(base),
        });
    }
    Ok(out)
}

/// Debit minus credit per account.
fn saldo_by_account(lines: &[RawLine]) -> HashMap<String, Decimal> {
    let mut m: HashMap<String, Decimal> = HashMap::new();
    for l in lines {
        *m.entry(l.account.clone()).or_default() += l.debit - l.credit;
    }
    m
}

fn account_map(conn: &Connection) -> Result<HashMap<String, Account>> {
    Ok(cfg::accounts(conn)?.into_iter().map(|a| (a.number.clone(), a)).collect())
}

// ---------------------------------------------------------------------------
// Generic statement structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AccountAmount {
    pub number: String,
    pub name: String,
    #[ts(type = "number")]
    pub amount: Decimal,
    #[ts(type = "number")]
    pub prev: Decimal,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReportRow {
    pub key: String,
    pub label: String,
    #[ts(type = "number")]
    pub amount: Decimal,
    #[ts(type = "number")]
    pub prev: Decimal,
    pub accounts: Vec<AccountAmount>,
    /// normal | subtotal | total | info
    pub style: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct StatementSection {
    pub title: String,
    pub rows: Vec<ReportRow>,
    pub total_label: String,
    #[ts(type = "number")]
    pub total: Decimal,
    #[ts(type = "number")]
    pub prev_total: Decimal,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Statement {
    pub title: String,
    pub subtitle: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub sections: Vec<StatementSection>,
    pub result_label: String,
    #[ts(type = "number")]
    pub result: Decimal,
    #[ts(type = "number")]
    pub prev_result: Decimal,
    pub notes: Vec<String>,
    pub applicable: bool,
}

fn grouped_rows(
    keys: &[(&str, &str)],
    accounts: &HashMap<String, Account>,
    saldo: &HashMap<String, Decimal>,
    prev_saldo: &HashMap<String, Decimal>,
    key_of: impl Fn(&Account) -> &str,
    sign: Decimal,
) -> Vec<ReportRow> {
    let mut rows = Vec::new();
    for (key, label) in keys {
        let mut accs = Vec::new();
        let mut numbers: Vec<&String> = accounts.values().filter(|a| key_of(a) == *key).map(|a| &a.number).collect();
        numbers.sort();
        for n in numbers {
            let amount = saldo.get(n).copied().unwrap_or_default() * sign;
            let prev = prev_saldo.get(n).copied().unwrap_or_default() * sign;
            if !amount.is_zero() || !prev.is_zero() {
                accs.push(AccountAmount { number: n.clone(), name: accounts[n].name.clone(), amount, prev });
            }
        }
        let amount = accs.iter().map(|a| a.amount).sum();
        let prev = accs.iter().map(|a| a.prev).sum();
        rows.push(ReportRow { key: key.to_string(), label: label.to_string(), amount, prev, accounts: accs, style: "normal".into() });
    }
    rows
}

fn year_range(year: i32) -> (NaiveDate, NaiveDate) {
    (ymd(year, 1, 1), ymd(year, 12, 31))
}

// ---------------------------------------------------------------------------
// EÜR
// ---------------------------------------------------------------------------

pub fn euer(conn: &Connection, year: i32) -> Result<Statement> {
    let s = cfg::load(conn)?;
    let accounts = account_map(conn)?;
    let (from, to) = year_range(year);
    let saldo = saldo_by_account(&raw_lines(conn, Some(from), Some(to))?);
    let (pf, pt) = year_range(year - 1);
    let prev = saldo_by_account(&raw_lines(conn, Some(pf), Some(pt))?);
    let income = grouped_rows(EUER_INCOME, &accounts, &saldo, &prev, |a| a.euer.as_str(), Decimal::NEGATIVE_ONE);
    let expense = grouped_rows(EUER_EXPENSE, &accounts, &saldo, &prev, |a| a.euer.as_str(), Decimal::ONE);
    let nabz = grouped_rows(&[EUER_NABZ], &accounts, &saldo, &prev, |a| a.euer.as_str(), Decimal::ONE);
    let ti: Decimal = income.iter().map(|r| r.amount).sum();
    let te: Decimal = expense.iter().map(|r| r.amount).sum();
    let pti: Decimal = income.iter().map(|r| r.prev).sum();
    let pte: Decimal = expense.iter().map(|r| r.prev).sum();
    let mut notes = vec![
        "Gewinnermittlung nach § 4 Abs. 3 EStG (Zufluss-/Abflussprinzip).".to_string(),
        "Die Zuordnung orientiert sich an der Anlage EÜR; bitte vor Übermittlung mit den amtlichen Zeilen abgleichen.".to_string(),
    ];
    let mut nabz_rows = nabz;
    if let Some(r) = nabz_rows.first_mut() {
        r.style = "info".into();
        if !r.amount.is_zero() {
            notes.push("Nicht abziehbare Betriebsausgaben mindern den steuerlichen Gewinn nicht und sind im Ergebnis nicht enthalten.".into());
        }
    }
    Ok(Statement {
        title: "Einnahmen-Überschuss-Rechnung".into(),
        subtitle: format!("Wirtschaftsjahr {year}"),
        from,
        to,
        sections: vec![
            StatementSection { title: "Betriebseinnahmen".into(), rows: income, total_label: "Summe Betriebseinnahmen".into(), total: ti, prev_total: pti },
            StatementSection { title: "Betriebsausgaben".into(), rows: expense, total_label: "Summe Betriebsausgaben".into(), total: te, prev_total: pte },
            StatementSection { title: "Nachrichtlich".into(), rows: nabz_rows, total_label: String::new(), total: Decimal::ZERO, prev_total: Decimal::ZERO },
        ],
        result_label: if ti - te >= Decimal::ZERO { "Gewinn".into() } else { "Verlust".into() },
        result: ti - te,
        prev_result: pti - pte,
        notes,
        applicable: s.tax.bookkeeping != "bilanz",
    })
}

// ---------------------------------------------------------------------------
// GuV
// ---------------------------------------------------------------------------

fn guv_result(rows: &[ReportRow], keys: &[&str], prev: bool) -> Decimal {
    rows.iter()
        .filter(|r| keys.contains(&r.key.as_str()))
        .map(|r| {
            let val = if prev { r.prev } else { r.amount };
            let revenue = ["g01", "g04", "g11"].contains(&r.key.as_str());
            if revenue { val } else { -val }
        })
        .sum()
}

pub fn guv(conn: &Connection, year: i32) -> Result<Statement> {
    let s = cfg::load(conn)?;
    let accounts = account_map(conn)?;
    let (from, to) = year_range(year);
    let saldo = saldo_by_account(&raw_lines(conn, Some(from), Some(to))?);
    let (pf, pt) = year_range(year - 1);
    let prev = saldo_by_account(&raw_lines(conn, Some(pf), Some(pt))?);
    let mut rows = Vec::new();
    for (key, label) in GUV_LINES {
        let revenue = ["g01", "g04", "g11"].contains(key);
        let sign = if revenue { Decimal::NEGATIVE_ONE } else { Decimal::ONE };
        let mut r = grouped_rows(&[(key, label)], &accounts, &saldo, &prev, |a| if a.kind == "revenue" || a.kind == "expense" { a.guv.as_str() } else { "" }, sign);
        rows.append(&mut r);
        let sub = |label: &str, keys: &[&str], rows: &[ReportRow]| ReportRow {
            key: String::new(),
            label: label.into(),
            amount: guv_result(rows, keys, false),
            prev: guv_result(rows, keys, true),
            accounts: vec![],
            style: "subtotal".into(),
        };
        let op = ["g01", "g04", "g05a", "g05b", "g06a", "g06b", "g07", "g08"];
        if *key == "g08" {
            let r = sub("Betriebsergebnis", &op, &rows);
            rows.push(r);
        }
        if *key == "g13" {
            let r = sub("Finanzergebnis", &["g11", "g13"], &rows);
            rows.push(r);
        }
        if *key == "g14" {
            let mut keys = op.to_vec();
            keys.extend(["g11", "g13", "g14"]);
            let r = sub("15. Ergebnis nach Steuern", &keys, &rows);
            rows.push(r);
        }
    }
    let all: Vec<&str> = GUV_LINES.iter().map(|(k, _)| *k).collect();
    let result = guv_result(&rows, &all, false);
    let prev_result = guv_result(&rows, &all, true);
    Ok(Statement {
        title: "Gewinn- und Verlustrechnung".into(),
        subtitle: format!("Geschäftsjahr {year} · Gesamtkostenverfahren (§ 275 Abs. 2 HGB)"),
        from,
        to,
        sections: vec![StatementSection { title: String::new(), rows, total_label: String::new(), total: result, prev_total: prev_result }],
        result_label: if result >= Decimal::ZERO { "17. Jahresüberschuss".into() } else { "17. Jahresfehlbetrag".into() },
        result,
        prev_result,
        notes: if s.tax.bookkeeping != "bilanz" {
            vec!["Hinweis: Bei Einnahmen-Überschuss-Rechnung basiert die GuV auf Zahlungsflüssen.".into()]
        } else {
            vec![]
        },
        applicable: true,
    })
}

// ---------------------------------------------------------------------------
// Bilanz
// ---------------------------------------------------------------------------

pub fn bilanz(conn: &Connection, date: NaiveDate) -> Result<Statement> {
    let s = cfg::load(conn)?;
    let accounts = account_map(conn)?;
    let all = raw_lines(conn, None, Some(date))?;
    let saldo = saldo_by_account(&all);
    let prev_date = ymd(date.year() - 1, 12, 31);
    let prev_lines = raw_lines(conn, None, Some(prev_date))?;
    let prev_saldo = saldo_by_account(&prev_lines);
    let year_start = ymd(date.year(), 1, 1);

    let pnl = |lines: &[RawLine], from: NaiveDate, to: NaiveDate| -> Decimal {
        lines
            .iter()
            .filter(|l| l.date >= from && l.date <= to)
            .filter(|l| accounts.get(&l.account).map(|a| a.kind == "revenue" || a.kind == "expense").unwrap_or(false))
            .map(|l| l.credit - l.debit)
            .sum()
    };
    let result_year = pnl(&all, year_start, date);
    let result_before = pnl(&all, ymd(1900, 1, 1), year_start - Duration::days(1));
    let prev_result_year = pnl(&prev_lines, ymd(date.year() - 1, 1, 1), prev_date);
    let prev_result_before = pnl(&prev_lines, ymd(1900, 1, 1), ymd(date.year() - 1, 1, 1) - Duration::days(1));

    // Classification with sign-dependent positions.
    let classify = |a: &Account, bal: Decimal| -> Option<(&'static str, bool)> {
        // returns (key, is_aktiva)
        match a.kind.as_str() {
            "revenue" | "expense" => None,
            "tax" => Some(if bal >= Decimal::ZERO { ("a_sonst_vg", true) } else { ("p_vb_sonst", false) }),
            _ => {
                let k = a.bilanz.as_str();
                if k == "a_liquide" && bal < Decimal::ZERO {
                    return Some(("p_vb_kredit", false));
                }
                if k.starts_with("a_") {
                    Some((static_key(k), true))
                } else if k.starts_with("p_") {
                    Some((static_key(k), false))
                } else {
                    None
                }
            }
        }
    };
    let mut aktiva: BTreeMap<&str, Vec<AccountAmount>> = BTreeMap::new();
    let mut passiva: BTreeMap<&str, Vec<AccountAmount>> = BTreeMap::new();
    let mut numbers: Vec<&String> = accounts.keys().collect();
    numbers.sort();
    // Tax accounts are netted into a single position.
    let mut tax_net = Decimal::ZERO;
    let mut tax_prev = Decimal::ZERO;
    for n in numbers {
        let a = &accounts[n];
        let bal = saldo.get(n).copied().unwrap_or_default();
        let pbal = prev_saldo.get(n).copied().unwrap_or_default();
        if a.kind == "tax" {
            tax_net += bal;
            tax_prev += pbal;
            continue;
        }
        if bal.is_zero() && pbal.is_zero() {
            continue;
        }
        if let Some((key, is_akt)) = classify(a, bal) {
            let (amount, prev) = if is_akt { (bal, pbal) } else { (-bal, -pbal) };
            let target = if is_akt { &mut aktiva } else { &mut passiva };
            target.entry(key).or_default().push(AccountAmount { number: n.clone(), name: a.name.clone(), amount, prev });
        }
    }
    if !tax_net.is_zero() || !tax_prev.is_zero() {
        if tax_net >= Decimal::ZERO {
            aktiva.entry("a_sonst_vg").or_default().push(AccountAmount { number: "USt".into(), name: "Umsatzsteuerforderung".into(), amount: tax_net, prev: tax_prev });
        } else {
            passiva.entry("p_vb_sonst").or_default().push(AccountAmount { number: "USt".into(), name: "Umsatzsteuerverbindlichkeit".into(), amount: -tax_net, prev: -tax_prev });
        }
    }
    let ek = passiva.entry("p_ek").or_default();
    ek.push(AccountAmount { number: String::new(), name: "Gewinn-/Verlustvortrag".into(), amount: result_before, prev: prev_result_before });
    ek.push(AccountAmount { number: String::new(), name: "Jahresüberschuss / -fehlbetrag".into(), amount: result_year, prev: prev_result_year });

    let build = |keys: &[(&str, &str)], map: &BTreeMap<&str, Vec<AccountAmount>>| -> Vec<ReportRow> {
        keys.iter()
            .map(|(k, label)| {
                let accs = map.get(k).cloned().unwrap_or_default();
                ReportRow {
                    key: k.to_string(),
                    label: label.to_string(),
                    amount: accs.iter().map(|a| a.amount).sum(),
                    prev: accs.iter().map(|a| a.prev).sum(),
                    accounts: accs,
                    style: "normal".into(),
                }
            })
            .collect()
    };
    let ar = build(BILANZ_AKTIVA, &aktiva);
    let pr = build(BILANZ_PASSIVA, &passiva);
    let ta: Decimal = ar.iter().map(|r| r.amount).sum();
    let tp: Decimal = pr.iter().map(|r| r.amount).sum();
    let pta: Decimal = ar.iter().map(|r| r.prev).sum();
    let ptp: Decimal = pr.iter().map(|r| r.prev).sum();
    let mut notes = Vec::new();
    if (ta - tp).abs() > Decimal::new(1, 2) {
        notes.push(format!("Achtung: Differenz zwischen Aktiva und Passiva von {}.", crate::util::fmt_money(ta - tp)));
    }
    Ok(Statement {
        title: "Bilanz".into(),
        subtitle: format!("zum {} (§ 266 HGB, verkürzt)", crate::util::fmt_date(date)),
        from: year_start,
        to: date,
        sections: vec![
            StatementSection { title: "Aktiva".into(), rows: ar, total_label: "Summe Aktiva".into(), total: ta, prev_total: pta },
            StatementSection { title: "Passiva".into(), rows: pr, total_label: "Summe Passiva".into(), total: tp, prev_total: ptp },
        ],
        result_label: "Jahresergebnis".into(),
        result: result_year,
        prev_result: prev_result_year,
        notes,
        applicable: s.tax.bookkeeping == "bilanz",
    })
}

fn static_key(k: &str) -> &'static str {
    for (key, _) in BILANZ_AKTIVA.iter().chain(BILANZ_PASSIVA.iter()) {
        if *key == k {
            return key;
        }
    }
    "a_sonst_vg"
}

// ---------------------------------------------------------------------------
// BWA
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BwaRow {
    pub key: String,
    pub label: String,
    #[ts(type = "number[]")]
    pub values: Vec<Decimal>,
    #[ts(type = "number")]
    pub total: Decimal,
    #[ts(type = "number")]
    pub share: Decimal,
    pub style: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Bwa {
    pub year: i32,
    pub months: Vec<String>,
    pub rows: Vec<BwaRow>,
}

pub fn bwa(conn: &Connection, year: i32) -> Result<Bwa> {
    let accounts = account_map(conn)?;
    let (from, to) = year_range(year);
    let lines = raw_lines(conn, Some(from), Some(to))?;
    let mut vals: HashMap<&str, [Decimal; 12]> = HashMap::new();
    for l in &lines {
        let Some(a) = accounts.get(&l.account) else { continue };
        if a.kind != "revenue" && a.kind != "expense" {
            continue;
        }
        let key = BWA_LINES.iter().find(|(k, _)| *k == a.bwa).map(|(k, _)| *k).unwrap_or(if a.kind == "revenue" { "neutral_ertr" } else { "sonstige" });
        let amt = if a.kind == "revenue" { l.credit - l.debit } else { l.debit - l.credit };
        vals.entry(key).or_insert([Decimal::ZERO; 12])[l.date.month0() as usize] += amt;
    }
    let get = |k: &str| vals.get(k).copied().unwrap_or([Decimal::ZERO; 12]);
    let add = |a: [Decimal; 12], b: [Decimal; 12], sign: Decimal| -> [Decimal; 12] {
        let mut r = a;
        for i in 0..12 {
            r[i] += b[i] * sign;
        }
        r
    };
    let zero = [Decimal::ZERO; 12];
    let umsatz = get("umsatz");
    let material = add(get("material"), get("fremd"), Decimal::ONE);
    let rohertrag = add(umsatz, material, Decimal::NEGATIVE_ONE);
    let betr_rohertrag = add(rohertrag, get("sonst_erloese"), Decimal::ONE);
    let cost_keys = ["personal", "raum", "steuern_betr", "versich", "kfz", "werbe_reise", "afa", "reparatur", "sonstige"];
    let mut kosten = zero;
    for k in cost_keys {
        kosten = add(kosten, get(k), Decimal::ONE);
    }
    let betriebsergebnis = add(betr_rohertrag, kosten, Decimal::NEGATIVE_ONE);
    let neutral_aufw = add(get("zinsaufw"), get("neutral_aufw"), Decimal::ONE);
    let neutral_ertr = add(get("zinsertr"), get("neutral_ertr"), Decimal::ONE);
    let vor_steuern = add(add(betriebsergebnis, neutral_aufw, Decimal::NEGATIVE_ONE), neutral_ertr, Decimal::ONE);
    let ergebnis = add(vor_steuern, get("steuern_ee"), Decimal::NEGATIVE_ONE);
    let total_umsatz: Decimal = umsatz.iter().copied().sum();
    let mk = |key: &str, label: &str, v: [Decimal; 12], style: &str| {
        let total: Decimal = v.iter().copied().sum();
        let share = if total_umsatz.is_zero() { Decimal::ZERO } else { (total / total_umsatz * Decimal::ONE_HUNDRED).round_dp(1) };
        BwaRow { key: key.into(), label: label.into(), values: v.to_vec(), total, share, style: style.into() }
    };
    let label = |k: &str| -> &'static str { BWA_LINES.iter().find(|(x, _)| *x == k).map(|(_, l)| *l).unwrap_or("") };
    let mut rows = vec![
        mk("umsatz", "Umsatzerlöse", umsatz, "normal"),
        mk("gesamtleistung", "Gesamtleistung", umsatz, "subtotal"),
        mk("material", "Material- / Wareneinkauf, Fremdleistungen", material, "normal"),
        mk("rohertrag", "Rohertrag", rohertrag, "subtotal"),
        mk("sonst_erloese", "Sonstige betriebliche Erlöse", get("sonst_erloese"), "normal"),
        mk("betr_rohertrag", "Betrieblicher Rohertrag", betr_rohertrag, "subtotal"),
    ];
    for k in cost_keys {
        rows.push(mk(k, label(k), get(k), "normal"));
    }
    rows.extend([
        mk("kosten", "Gesamtkosten", kosten, "subtotal"),
        mk("betriebsergebnis", "Betriebsergebnis", betriebsergebnis, "total"),
        mk("neutral_aufw", "Zinsaufwand / sonstiger neutraler Aufwand", neutral_aufw, "normal"),
        mk("neutral_ertr", "Zinserträge / sonstiger neutraler Ertrag", neutral_ertr, "normal"),
        mk("vor_steuern", "Ergebnis vor Steuern", vor_steuern, "subtotal"),
        mk("steuern_ee", "Steuern vom Einkommen und Ertrag", get("steuern_ee"), "normal"),
        mk("ergebnis", "Vorläufiges Ergebnis", ergebnis, "total"),
    ]);
    Ok(Bwa { year, months: (1..=12).map(|m| month_short(m).to_string()).collect(), rows })
}

// ---------------------------------------------------------------------------
// Summen- und Saldenliste, Journal, Kontenblatt
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SusaRow {
    pub number: String,
    pub name: String,
    pub kind: String,
    #[ts(type = "number")]
    pub opening: Decimal,
    #[ts(type = "number")]
    pub debit: Decimal,
    #[ts(type = "number")]
    pub credit: Decimal,
    #[ts(type = "number")]
    pub closing: Decimal,
}

pub fn susa(conn: &Connection, from: NaiveDate, to: NaiveDate) -> Result<Vec<SusaRow>> {
    let accounts = account_map(conn)?;
    let lines = raw_lines(conn, None, Some(to))?;
    let mut m: BTreeMap<String, (Decimal, Decimal, Decimal)> = BTreeMap::new();
    for l in lines {
        let e = m.entry(l.account.clone()).or_default();
        // P&L accounts start at zero each fiscal year.
        let is_pnl = accounts.get(&l.account).map(|a| a.kind == "revenue" || a.kind == "expense").unwrap_or(false);
        if l.date < from {
            if !is_pnl || l.date.year() == from.year() {
                e.0 += l.debit - l.credit;
            }
        } else {
            e.1 += l.debit;
            e.2 += l.credit;
        }
    }
    Ok(m.into_iter()
        .filter(|(_, (o, dd, c))| !o.is_zero() || !dd.is_zero() || !c.is_zero())
        .map(|(n, (o, dd, c))| SusaRow {
            name: accounts.get(&n).map(|a| a.name.clone()).unwrap_or_default(),
            kind: accounts.get(&n).map(|a| a.kind.clone()).unwrap_or_default(),
            number: n,
            opening: o,
            debit: dd,
            credit: c,
            closing: o + dd - c,
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct JournalRow {
    pub entry: JournalEntry,
    pub lines: Vec<JournalLine>,
    #[ts(type = "number")]
    pub amount: Decimal,
}

pub fn journal(conn: &Connection, from: NaiveDate, to: NaiveDate, account: &str, q: &str) -> Result<Vec<JournalRow>> {
    let entries = db::query::<JournalEntry>(conn, "WHERE date >= ?1 AND date <= ?2 ORDER BY date DESC, id DESC", &[v(from), v(to)])?;
    let amounts = search_amounts(q);
    let q = q.trim().to_lowercase();
    let mut out = Vec::new();
    let mut stmt_lines: HashMap<i64, Vec<JournalLine>> = HashMap::new();
    {
        let lines = db::query::<JournalLine>(
            conn,
            "WHERE entry_id IN (SELECT id FROM journal_entries WHERE date >= ?1 AND date <= ?2) ORDER BY id",
            &[v(from), v(to)],
        )?;
        for l in lines {
            stmt_lines.entry(l.entry_id).or_default().push(l);
        }
    }
    for e in entries {
        let lines = stmt_lines.remove(&e.id).unwrap_or_default();
        if !account.is_empty() && !lines.iter().any(|l| l.account == account) {
            continue;
        }
        let amount: Decimal = lines.iter().map(|l| l.debit).sum();
        if !q.is_empty()
            && !format!("{} {}", e.description, e.doc_ref).to_lowercase().contains(&q)
            && !lines.iter().flat_map(|l| [l.debit, l.credit]).chain([amount]).any(|a| amounts.contains(&a.abs()))
        {
            continue;
        }
        out.push(JournalRow { entry: e, lines, amount });
    }
    Ok(out)
}

/// Amounts a search term may stand for. „1.450“ is read as 1,45 and, with German digit grouping, as 1.450,00.
fn search_amounts(q: &str) -> Vec<Decimal> {
    let mut out: Vec<Decimal> = crate::services::bankimport::parse_amount(q).map(|d| d.abs()).into_iter().collect();
    let groups: Vec<&str> = q.trim().split('.').collect();
    if groups.len() > 1
        && groups.iter().all(|g| !g.is_empty() && g.chars().all(|c| c.is_ascii_digit()))
        && groups[0].len() <= 3
        && groups[1..].iter().all(|g| g.len() == 3)
        && let Ok(d) = Decimal::from_str(&groups.concat())
    {
        out.push(d);
    }
    out
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LedgerRow {
    pub entry_id: i64,
    pub date: NaiveDate,
    pub doc_ref: String,
    pub description: String,
    pub contra: String,
    pub source_type: String,
    pub source_id: i64,
    #[ts(type = "number")]
    pub debit: Decimal,
    #[ts(type = "number")]
    pub credit: Decimal,
    #[ts(type = "number")]
    pub balance: Decimal,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Ledger {
    pub account: Account,
    #[ts(type = "number")]
    pub opening: Decimal,
    pub rows: Vec<LedgerRow>,
    #[ts(type = "number")]
    pub debit_total: Decimal,
    #[ts(type = "number")]
    pub credit_total: Decimal,
    #[ts(type = "number")]
    pub closing: Decimal,
}

pub fn ledger(conn: &Connection, number: &str, from: NaiveDate, to: NaiveDate) -> Result<Ledger> {
    let account = cfg::account_by_number(conn, number)?.ok_or_else(|| crate::error::NotFound(format!("Konto {number} nicht gefunden")))?;
    let is_pnl = account.kind == "revenue" || account.kind == "expense";
    let all = raw_lines(conn, None, Some(to))?;
    let opening: Decimal = all
        .iter()
        .filter(|l| l.account == number && l.date < from && (!is_pnl || l.date.year() == from.year()))
        .map(|l| l.debit - l.credit)
        .sum();
    let entries: HashMap<i64, JournalEntry> = db::query::<JournalEntry>(conn, "WHERE date >= ?1 AND date <= ?2", &[v(from), v(to)])?
        .into_iter()
        .map(|e| (e.id, e))
        .collect();
    let mut by_entry: HashMap<i64, Vec<&RawLine>> = HashMap::new();
    for l in all.iter().filter(|l| l.date >= from) {
        by_entry.entry(l.entry_id).or_default().push(l);
    }
    let mut own: Vec<&RawLine> = all.iter().filter(|l| l.account == number && l.date >= from).collect();
    own.sort_by_key(|l| (l.date, l.entry_id));
    let mut bal = opening;
    let mut rows = Vec::new();
    let (mut dt, mut ct) = (Decimal::ZERO, Decimal::ZERO);
    for l in own {
        bal += l.debit - l.credit;
        dt += l.debit;
        ct += l.credit;
        let e = entries.get(&l.entry_id);
        let mut contras: Vec<String> = by_entry
            .get(&l.entry_id)
            .map(|ls| ls.iter().filter(|x| x.account != number).map(|x| x.account.clone()).collect())
            .unwrap_or_default();
        contras.dedup();
        rows.push(LedgerRow {
            entry_id: l.entry_id,
            date: l.date,
            doc_ref: e.map(|e| e.doc_ref.clone()).unwrap_or_default(),
            description: e.map(|e| e.description.clone()).unwrap_or_default(),
            contra: contras.join(", "),
            source_type: e.map(|e| e.source_type.clone()).unwrap_or_default(),
            source_id: e.map(|e| e.source_id).unwrap_or_default(),
            debit: l.debit,
            credit: l.credit,
            balance: bal,
        });
    }
    Ok(Ledger { account, opening, rows, debit_total: dt, credit_total: ct, closing: bal })
}

// ---------------------------------------------------------------------------
// Umsatzsteuer-Voranmeldung
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UstvaField {
    pub kz: String,
    pub tax_kz: String,
    pub label: String,
    #[ts(type = "number | null")]
    pub base: Option<Decimal>,
    #[ts(type = "number | null")]
    pub tax: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UstvaSection {
    pub title: String,
    pub fields: Vec<UstvaField>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UstvaReport {
    pub year: i32,
    pub period: String,
    pub period_label: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub due_date: NaiveDate,
    pub sections: Vec<UstvaSection>,
    #[ts(type = "number")]
    pub output_tax: Decimal,
    #[ts(type = "number")]
    pub input_tax: Decimal,
    #[ts(type = "number")]
    pub zahllast: Decimal,
    pub filing: Option<UstvaFiling>,
    pub small_business: bool,
    pub basis: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UstvaPeriod {
    pub period: String,
    pub label: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub due_date: NaiveDate,
    #[ts(type = "number")]
    pub zahllast: Decimal,
    pub filing: Option<UstvaFiling>,
    pub is_current: bool,
    pub is_future: bool,
}

pub fn period_range(year: i32, period: &str) -> (NaiveDate, NaiveDate, String) {
    if let Some(q) = period.strip_prefix('Q') {
        let q: u32 = q.parse().unwrap_or(1).clamp(1, 4);
        let start = ymd(year, (q - 1) * 3 + 1, 1);
        (start, last_day_of_month(year, q * 3), format!("{q}. Quartal {year}"))
    } else if period == "Y" {
        (ymd(year, 1, 1), ymd(year, 12, 31), format!("Jahr {year}"))
    } else {
        let m: u32 = period.parse().unwrap_or(1).clamp(1, 12);
        (ymd(year, m, 1), last_day_of_month(year, m), format!("{} {year}", month_name(m)))
    }
}

/// Deadlines falling on a weekend or public holiday move to the next working day (§ 108 Abs. 3 AO).
pub fn next_workday(d: NaiveDate) -> NaiveDate {
    let mut d = d;
    while d.weekday() == Weekday::Sat || d.weekday() == Weekday::Sun || crate::util::is_national_holiday(d) {
        d += Duration::days(1);
    }
    d
}

pub fn ustva_due(to: NaiveDate, extension: bool) -> NaiveDate {
    let next = to + Duration::days(1);
    let base = ymd(next.year(), next.month(), 10);
    let base = if extension { crate::util::add_months(base, 1) } else { base };
    next_workday(base)
}

fn sum_code(lines: &[RawLine], codes: &[&str], credit_positive: bool) -> Decimal {
    lines
        .iter()
        .filter(|l| codes.contains(&l.tax_code.as_str()))
        .map(|l| if credit_positive { l.credit - l.debit } else { l.debit - l.credit })
        .sum()
}

fn sum_base(lines: &[RawLine], codes: &[&str]) -> Decimal {
    lines.iter().filter(|l| codes.contains(&l.tax_code.as_str())).map(|l| l.base).sum()
}

pub fn ustva(conn: &Connection, year: i32, period: &str) -> Result<UstvaReport> {
    let s = cfg::load(conn)?;
    let (from, to, label) = period_range(year, period);
    let lines = raw_lines(conn, Some(from), Some(to))?;
    let f = |kz: &str, tax_kz: &str, label: &str, base: Option<Decimal>, tax: Option<Decimal>| UstvaField {
        kz: kz.into(),
        tax_kz: tax_kz.into(),
        label: label.into(),
        base,
        tax,
    };
    let b = |codes: &[&str]| Some(sum_code(&lines, codes, true));
    let t_out = |codes: &[&str]| sum_code(&lines, codes, true);
    let t_in = |codes: &[&str]| sum_code(&lines, codes, false);

    let ust19 = t_out(&["UST19"]);
    let ust7 = t_out(&["UST7"]);
    let ustx = t_out(&["USTX"]);
    let ige19 = t_out(&["UST_IGE19"]);
    let ige7 = t_out(&["UST_IGE7"]);
    let rc_eu = t_out(&["UST_RC_EU"]);
    let rc_de = t_out(&["UST_RC_DE"]);
    let vst = t_in(&["VST"]);
    let vst_ige = t_in(&["VST_IGE"]);
    let eust = t_in(&["EUST"]);
    let vst13b = t_in(&["VST13B"]);

    let sections = vec![
        UstvaSection {
            title: "Lieferungen und sonstige Leistungen (einschließlich unentgeltlicher Wertabgaben)".into(),
            fields: vec![
                f("81", "", "Steuerpflichtige Umsätze zum Steuersatz von 19 %", b(&["S19"]), Some(ust19)),
                f("86", "", "Steuerpflichtige Umsätze zum Steuersatz von 7 %", b(&["S7"]), Some(ust7)),
                f("35", "36", "Umsätze, die anderen Steuersätzen unterliegen", b(&["SX"]), Some(ustx)),
            ],
        },
        UstvaSection {
            title: "Steuerfreie Lieferungen und sonstige Leistungen".into(),
            fields: vec![
                f("41", "", "Innergemeinschaftliche Lieferungen an Abnehmer mit USt-IdNr.", b(&["IGL"]), None),
                f("43", "", "Weitere steuerfreie Umsätze mit Vorsteuerabzug (z. B. Ausfuhrlieferungen)", b(&["EXP"]), None),
                f("48", "", "Steuerfreie Umsätze ohne Vorsteuerabzug", b(&["FREI"]), None),
            ],
        },
        UstvaSection {
            title: "Innergemeinschaftliche Erwerbe".into(),
            fields: vec![
                f("89", "", "Steuerpflichtige innergemeinschaftliche Erwerbe zum Steuersatz von 19 %", Some(sum_base(&lines, &["UST_IGE19"])), Some(ige19)),
                f("93", "", "Steuerpflichtige innergemeinschaftliche Erwerbe zum Steuersatz von 7 %", Some(sum_base(&lines, &["UST_IGE7"])), Some(ige7)),
            ],
        },
        UstvaSection {
            title: "Leistungsempfänger als Steuerschuldner (§ 13b UStG)".into(),
            fields: vec![
                f("46", "47", "Im Inland steuerpflichtige sonstige Leistungen von im übrigen Gemeinschaftsgebiet ansässigen Unternehmern", Some(sum_base(&lines, &["UST_RC_EU"])), Some(rc_eu)),
                f("84", "85", "Andere Leistungen (§ 13b Abs. 2 Nr. 1, 2, 4 bis 12 UStG)", Some(sum_base(&lines, &["UST_RC_DE"])), Some(rc_de)),
            ],
        },
        UstvaSection {
            title: "Ergänzende Angaben zu Umsätzen".into(),
            fields: vec![
                f("60", "", "Steuerpflichtige Umsätze, für die der Leistungsempfänger die Steuer nach § 13b UStG schuldet", b(&["RC13B_OUT"]), None),
                f("21", "", "Nicht steuerbare sonstige Leistungen gem. § 18b Satz 1 Nr. 2 UStG (EU)", b(&["EURC"]), None),
                f("45", "", "Übrige nicht steuerbare Umsätze (Leistungsort nicht im Inland)", b(&["NSTB"]), None),
            ],
        },
        UstvaSection {
            title: "Abziehbare Vorsteuerbeträge".into(),
            fields: vec![
                f("", "66", "Vorsteuerbeträge aus Rechnungen von anderen Unternehmern", None, Some(vst)),
                f("", "61", "Vorsteuerbeträge aus dem innergemeinschaftlichen Erwerb von Gegenständen", None, Some(vst_ige)),
                f("", "62", "Entstandene Einfuhrumsatzsteuer", None, Some(eust)),
                f("", "67", "Vorsteuerbeträge aus Leistungen im Sinne des § 13b UStG", None, Some(vst13b)),
            ],
        },
    ];
    let output_tax = ust19 + ust7 + ustx + ige19 + ige7 + rc_eu + rc_de;
    let input_tax = vst + vst_ige + eust + vst13b;
    let filing = db::query_one::<UstvaFiling>(conn, "WHERE year = ?1 AND period = ?2", &[v(year as i64), v(period.to_string())])?;
    Ok(UstvaReport {
        year,
        period: period.into(),
        period_label: label,
        from,
        to,
        due_date: ustva_due(to, s.tax.permanent_extension),
        sections,
        output_tax,
        input_tax,
        zahllast: output_tax - input_tax,
        filing,
        small_business: s.tax.small_business,
        basis: if s.tax.bookkeeping == "bilanz" { "Soll-Versteuerung (vereinbarte Entgelte)".into() } else { "Ist-Versteuerung (vereinnahmte Entgelte)".into() },
    })
}

pub fn ustva_periods(conn: &Connection, year: i32) -> Result<Vec<UstvaPeriod>> {
    let s = cfg::load(conn)?;
    let periods: Vec<String> = match s.tax.ustva_period.as_str() {
        "monthly" => (1..=12).map(|m| format!("{m:02}")).collect(),
        "yearly" => vec!["Y".into()],
        _ => (1..=4).map(|q| format!("Q{q}")).collect(),
    };
    let t = today();
    let mut out = Vec::new();
    for p in periods {
        let r = ustva(conn, year, &p)?;
        out.push(UstvaPeriod {
            is_current: r.from <= t && t <= r.to,
            is_future: r.from > t,
            period: p,
            label: r.period_label,
            from: r.from,
            to: r.to,
            due_date: r.due_date,
            zahllast: r.zahllast,
            filing: r.filing,
        });
    }
    Ok(out)
}

pub fn current_ustva_period(conn: &Connection) -> Result<(i32, String)> {
    let s = cfg::load(conn)?;
    let t = today();
    Ok(match s.tax.ustva_period.as_str() {
        "monthly" => (t.year(), format!("{:02}", t.month())),
        "yearly" => (t.year(), "Y".into()),
        _ => (t.year(), format!("Q{}", (t.month() - 1) / 3 + 1)),
    })
}

// ---------------------------------------------------------------------------
// Offene Posten
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AgingBucket {
    pub key: String,
    pub label: String,
    #[ts(type = "number")]
    pub amount: Decimal,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct OpenItems {
    pub receivables: Vec<DocumentListItem>,
    pub payables: Vec<ReceiptListItem>,
    pub receivable_buckets: Vec<AgingBucket>,
    pub payable_buckets: Vec<AgingBucket>,
    #[ts(type = "number")]
    pub receivables_total: Decimal,
    #[ts(type = "number")]
    pub payables_total: Decimal,
}

fn buckets(items: &[(i64, Decimal)]) -> Vec<AgingBucket> {
    let defs = [
        ("current", "Nicht fällig", i64::MIN, 0),
        ("d30", "1–30 Tage", 1, 30),
        ("d60", "31–60 Tage", 31, 60),
        ("d90", "61–90 Tage", 61, 90),
        ("d90p", "über 90 Tage", 91, i64::MAX),
    ];
    defs.iter()
        .map(|(k, l, lo, hi)| {
            let sel: Vec<&(i64, Decimal)> = items.iter().filter(|(d, _)| *d >= *lo && *d <= *hi).collect();
            AgingBucket { key: k.to_string(), label: l.to_string(), amount: sel.iter().map(|(_, a)| *a).sum(), count: sel.len() as i64 }
        })
        .collect()
}

pub fn open_items(conn: &Connection) -> Result<OpenItems> {
    let mut receivables: Vec<DocumentListItem> = documents::list(conn, &DocumentFilter { status: "unpaid".into(), ..Default::default() })?
        .into_iter()
        .filter(|d| d.doc_type == "invoice" || d.doc_type == "credit_note")
        .collect();
    receivables.sort_by_key(|d| (d.due_date, d.id));
    let payables: Vec<ReceiptListItem> = receipts::list(conn, &ReceiptFilter { status: "unpaid".into(), ..Default::default() })?
        .into_iter()
        // Open supplier credit notes (negative) are listed too and reduce the total.
        .filter(|r| r.receipt.remaining() > Decimal::ZERO)
        .collect();
    let t = today();
    let rb: Vec<(i64, Decimal)> = receivables
        .iter()
        .filter(|d| d.doc_type == "invoice" && d.open_amount > Decimal::ZERO)
        .map(|d| (d.due_date.map(|x| (t - x).num_days()).unwrap_or(0), d.open_amount))
        .collect();
    let pb: Vec<(i64, Decimal)> = payables
        .iter()
        .filter(|r| r.open_amount > Decimal::ZERO)
        .map(|r| (r.receipt.due_date.map(|x| (t - x).num_days()).unwrap_or(0), r.open_amount))
        .collect();
    Ok(OpenItems {
        receivables_total: receivables.iter().map(|d| if d.doc_type == "credit_note" { -d.open_amount } else { d.open_amount }).sum(),
        payables_total: payables.iter().map(|r| r.open_amount).sum(),
        receivable_buckets: buckets(&rb),
        payable_buckets: buckets(&pb),
        receivables,
        payables,
    })
}

// ---------------------------------------------------------------------------
// Kennzahlen / Auswertungen
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct MonthPoint {
    pub month: u32,
    pub label: String,
    /// Fakturierter Nettoumsatz (Rechnungen abzgl. Gutschriften)
    #[ts(type = "number")]
    pub invoiced: Decimal,
    #[ts(type = "number")]
    pub invoiced_prev: Decimal,
    /// Gebuchte Erlöse / Aufwendungen laut Buchhaltung
    #[ts(type = "number")]
    pub revenue: Decimal,
    #[ts(type = "number")]
    pub expenses: Decimal,
    #[ts(type = "number")]
    pub profit: Decimal,
    #[ts(type = "number")]
    pub liquidity: Decimal,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NamedAmount {
    pub id: Option<i64>,
    pub key: String,
    pub name: String,
    #[ts(type = "number")]
    pub amount: Decimal,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Analytics {
    pub year: i32,
    pub months: Vec<MonthPoint>,
    pub top_customers: Vec<NamedAmount>,
    pub top_articles: Vec<NamedAmount>,
    pub expense_categories: Vec<NamedAmount>,
    pub revenue_categories: Vec<NamedAmount>,
    #[ts(type = "number")]
    pub invoiced_total: Decimal,
    #[ts(type = "number")]
    pub invoiced_prev_total: Decimal,
    #[ts(type = "number")]
    pub revenue_total: Decimal,
    #[ts(type = "number")]
    pub expense_total: Decimal,
    #[ts(type = "number")]
    pub profit_total: Decimal,
    pub invoice_count: i64,
    #[ts(type = "number")]
    pub avg_invoice: Decimal,
    #[ts(type = "number")]
    pub avg_days_to_pay: Decimal,
    pub quote_count: i64,
    #[ts(type = "number")]
    pub quote_conversion: Decimal,
}

fn invoiced_by_month(conn: &Connection, year: i32) -> Result<([Decimal; 12], Vec<Document>)> {
    let (from, to) = year_range(year);
    let docs = db::query::<Document>(
        conn,
        "WHERE doc_type IN ('invoice','credit_note') AND status != 'draft' AND date >= ?1 AND date <= ?2",
        &[v(from), v(to)],
    )?;
    let mut m = [Decimal::ZERO; 12];
    for d in &docs {
        m[d.date.month0() as usize] += d.net_total * d.sign();
    }
    Ok((m, docs))
}

pub fn analytics(conn: &Connection, year: i32) -> Result<Analytics> {
    let accounts = account_map(conn)?;
    let (from, to) = year_range(year);
    let (inv, docs) = invoiced_by_month(conn, year)?;
    let (inv_prev, _) = invoiced_by_month(conn, year - 1)?;
    let lines = raw_lines(conn, Some(from), Some(to))?;
    let mut rev = [Decimal::ZERO; 12];
    let mut exp = [Decimal::ZERO; 12];
    let mut exp_cat: HashMap<String, Decimal> = HashMap::new();
    let mut rev_cat: HashMap<String, Decimal> = HashMap::new();
    for l in &lines {
        let Some(a) = accounts.get(&l.account) else { continue };
        let m = l.date.month0() as usize;
        match a.kind.as_str() {
            "revenue" => {
                rev[m] += l.credit - l.debit;
                *rev_cat.entry(a.number.clone()).or_default() += l.credit - l.debit;
            }
            "expense" => {
                exp[m] += l.debit - l.credit;
                *exp_cat.entry(a.number.clone()).or_default() += l.debit - l.credit;
            }
            _ => {}
        }
    }
    let pay_accounts = db::query::<PaymentAccount>(conn, "", &[])?;
    let mut months = Vec::new();
    for i in 0..12u32 {
        let month_end = last_day_of_month(year, i + 1);
        let mut liq = Decimal::ZERO;
        for a in &pay_accounts {
            if a.opening_date <= month_end {
                liq += finance::balance_of(conn, a, Some(month_end))?;
            }
        }
        months.push(MonthPoint {
            month: i + 1,
            label: month_short(i + 1).to_string(),
            invoiced: inv[i as usize],
            invoiced_prev: inv_prev[i as usize],
            revenue: rev[i as usize],
            expenses: exp[i as usize],
            profit: rev[i as usize] - exp[i as usize],
            liquidity: liq,
        });
    }
    // Top customers
    let contacts: HashMap<i64, String> = db::query::<Contact>(conn, "", &[])?.into_iter().map(|c| (c.id, c.display_name())).collect();
    let mut cust: HashMap<i64, (Decimal, i64)> = HashMap::new();
    for d in &docs {
        if let Some(c) = d.contact_id {
            let e = cust.entry(c).or_default();
            e.0 += d.net_total * d.sign();
            if d.doc_type == "invoice" && d.kind != "cancellation" {
                e.1 += 1;
            }
        }
    }
    let mut top_customers: Vec<NamedAmount> = cust
        .into_iter()
        .map(|(id, (amount, count))| NamedAmount { id: Some(id), key: id.to_string(), name: contacts.get(&id).cloned().unwrap_or_default(), amount, count })
        .filter(|n| !n.amount.is_zero())
        .collect();
    top_customers.sort_by(|a, b| b.amount.cmp(&a.amount));
    top_customers.truncate(10);
    // Top articles
    // Partial invoices that are settled by a final invoice are skipped – the final invoice
    // repeats all positions, counting both would double the article revenue.
    let settled: std::collections::HashSet<i64> = db::query::<Document>(conn, "WHERE kind = 'final' AND status NOT IN ('draft','cancelled')", &[])?
        .into_iter()
        .filter_map(|d| d.base_id)
        .collect();
    // Group by article where possible, so credit notes ("Gutschrift: …") reduce the article they refer to.
    let article_names: HashMap<i64, String> = db::query::<crate::models::Article>(conn, "", &[])?.into_iter().map(|a| (a.id, a.name)).collect();
    let mut art: HashMap<String, (String, Decimal, Decimal, Option<i64>)> = HashMap::new();
    for d in docs.iter().filter(|d| d.status != "cancelled" && !(d.kind == "partial" && d.base_id.is_some_and(|b| settled.contains(&b)))) {
        for l in documents::lines_of(conn, d.id)?.into_iter().filter(|l| l.line_type == "item") {
            let name = l.article_id.and_then(|id| article_names.get(&id).cloned()).unwrap_or_else(|| if l.name.is_empty() { "(ohne Bezeichnung)".to_string() } else { l.name.clone() });
            let key = l.article_id.filter(|id| article_names.contains_key(id)).map(|id| format!("a{id}")).unwrap_or_else(|| name.clone());
            let e = art.entry(key).or_insert_with(|| (name, Decimal::ZERO, Decimal::ZERO, l.article_id));
            e.1 += l.net_amount * d.sign();
            e.2 += l.quantity * d.sign();
        }
    }
    let mut top_articles: Vec<NamedAmount> = art
        .into_iter()
        .map(|(key, (name, amount, qty, id))| NamedAmount { id, key, name, amount, count: qty.round().try_into().unwrap_or(0) })
        .filter(|n| n.amount > Decimal::ZERO)
        .collect();
    top_articles.sort_by(|a, b| b.amount.cmp(&a.amount));
    top_articles.truncate(10);
    let cat = |m: HashMap<String, Decimal>| -> Vec<NamedAmount> {
        let mut v: Vec<NamedAmount> = m
            .into_iter()
            .filter(|(_, a)| !a.is_zero())
            .map(|(n, a)| NamedAmount { id: None, name: accounts.get(&n).map(|x| x.name.clone()).unwrap_or_default(), key: n, amount: a, count: 0 })
            .collect();
        v.sort_by(|a, b| b.amount.cmp(&a.amount));
        v
    };
    let invoices: Vec<&Document> = docs.iter().filter(|d| d.doc_type == "invoice" && d.kind != "cancellation" && d.status != "cancelled").collect();
    let invoice_count = invoices.len() as i64;
    let invoiced_total: Decimal = inv.iter().copied().sum();
    let avg_invoice = if invoice_count > 0 { (invoices.iter().map(|d| d.net_total).sum::<Decimal>() / Decimal::from(invoice_count)).round_dp(2) } else { Decimal::ZERO };
    let paid: Vec<i64> = invoices.iter().filter_map(|d| d.paid_at.map(|p| (p - d.date).num_days())).collect();
    let avg_days_to_pay = if paid.is_empty() { Decimal::ZERO } else { (Decimal::from(paid.iter().sum::<i64>()) / Decimal::from(paid.len() as i64)).round_dp(1) };
    let quotes = db::query::<Document>(conn, "WHERE doc_type = 'quote' AND status != 'draft' AND date >= ?1 AND date <= ?2", &[v(from), v(to)])?;
    let accepted = quotes.iter().filter(|q| q.status == "accepted").count() as i64;
    let decided = quotes.iter().filter(|q| q.status == "accepted" || q.status == "rejected").count() as i64;
    let revenue_total: Decimal = rev.iter().copied().sum();
    let expense_total: Decimal = exp.iter().copied().sum();
    Ok(Analytics {
        year,
        months,
        top_customers,
        top_articles,
        expense_categories: cat(exp_cat),
        revenue_categories: cat(rev_cat),
        invoiced_total,
        invoiced_prev_total: inv_prev.iter().copied().sum(),
        revenue_total,
        expense_total,
        profit_total: revenue_total - expense_total,
        invoice_count,
        avg_invoice,
        avg_days_to_pay,
        quote_count: quotes.len() as i64,
        quote_conversion: if decided > 0 { (Decimal::from(accepted) / Decimal::from(decided) * Decimal::ONE_HUNDRED).round_dp(0) } else { Decimal::ZERO },
    })
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Todo {
    pub kind: String,
    pub label: String,
    pub count: i64,
    pub link: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SetupProgress {
    pub company_complete: bool,
    pub has_logo: bool,
    pub has_contacts: bool,
    pub has_articles: bool,
    pub has_invoices: bool,
    pub has_receipts: bool,
    pub has_bank_data: bool,
    /// Checklist was hidden by the user (Einstellungen → Übersicht).
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Dashboard {
    pub setup: SetupProgress,
    pub company_name: String,
    pub year: i32,
    #[ts(type = "number")]
    pub invoiced_month: Decimal,
    #[ts(type = "number")]
    pub invoiced_prev_month: Decimal,
    #[ts(type = "number")]
    pub invoiced_year: Decimal,
    #[ts(type = "number")]
    pub invoiced_prev_year_to_date: Decimal,
    #[ts(type = "number")]
    pub revenue_year: Decimal,
    #[ts(type = "number")]
    pub expense_year: Decimal,
    #[ts(type = "number")]
    pub profit_year: Decimal,
    #[ts(type = "number")]
    pub open_receivables: Decimal,
    pub open_receivables_count: i64,
    #[ts(type = "number")]
    pub overdue_amount: Decimal,
    pub overdue_count: i64,
    #[ts(type = "number")]
    pub open_payables: Decimal,
    pub open_payables_count: i64,
    #[ts(type = "number")]
    pub liquidity: Decimal,
    #[ts(type = "number")]
    pub ustva_zahllast: Decimal,
    pub ustva_label: String,
    pub ustva_due: NaiveDate,
    pub months: Vec<MonthPoint>,
    pub overdue: Vec<DocumentListItem>,
    pub open_quotes: Vec<DocumentListItem>,
    pub open_receipts: Vec<ReceiptListItem>,
    pub accounts: Vec<PaymentAccountView>,
    pub expense_categories: Vec<NamedAmount>,
    pub todos: Vec<Todo>,
    pub recent: Vec<Activity>,
    pub small_business: bool,
}

pub fn dashboard(conn: &Connection) -> Result<Dashboard> {
    let s = cfg::load(conn)?;
    let t = today();
    let year = t.year();
    let a = analytics(conn, year)?;
    let m = t.month0() as usize;
    let invoiced_month = a.months[m].invoiced;
    // Same period of the previous month (1st up to today's day), so a running month is not
    // compared with a complete one.
    let (py, pm) = if t.month() == 1 { (year - 1, 12) } else { (year, t.month() - 1) };
    let prev_to = ymd(py, pm, t.day().min(last_day_of_month(py, pm).day()));
    let invoiced_prev_month: Decimal = db::query::<Document>(
        conn,
        "WHERE doc_type IN ('invoice','credit_note') AND status != 'draft' AND date >= ?1 AND date <= ?2",
        &[v(ymd(py, pm, 1)), v(prev_to)],
    )?
    .iter()
    .map(|d| d.net_total * d.sign())
    .sum();
    // Previous year up to the same day.
    let prev_docs = db::query::<Document>(
        conn,
        "WHERE doc_type IN ('invoice','credit_note') AND status != 'draft' AND date >= ?1 AND date <= ?2",
        &[v(ymd(year - 1, 1, 1)), v(same_day_last_year(t))],
    )?;
    let invoiced_prev_year_to_date = prev_docs.iter().map(|d| d.net_total * d.sign()).sum();
    let oi = open_items(conn)?;
    let overdue: Vec<DocumentListItem> = oi.receivables.iter().filter(|d| d.days_overdue > 0).cloned().collect();
    let open_quotes: Vec<DocumentListItem> = documents::list(conn, &DocumentFilter { doc_type: "quote".into(), status: "open".into(), ..Default::default() })?;
    let accounts = finance::accounts(conn)?;
    let liquidity = accounts.iter().filter(|a| !a.account.archived).map(|a| a.balance).sum();
    let (uy, up) = current_ustva_period(conn)?;
    let u = ustva(conn, uy, &up)?;
    let unbooked = db::scalar_i64(conn, "SELECT COUNT(*) FROM transactions WHERE status = 'open'", &[])?;
    let drafts = db::scalar_i64(conn, "SELECT COUNT(*) FROM documents WHERE status = 'draft' AND doc_type = 'invoice'", &[])?;
    let dunning_due = crate::services::dunning::suggestions(conn)?.len() as i64;
    let mut todos = Vec::new();
    if !overdue.is_empty() {
        todos.push(Todo { kind: "overdue".into(), label: plural(overdue.len() as i64, "überfällige Rechnung", "überfällige Rechnungen"), count: overdue.len() as i64, link: "/offene-posten".into(), severity: "red".into() });
    }
    if dunning_due > 0 {
        todos.push(Todo { kind: "dunning".into(), label: plural(dunning_due, "Mahnung fällig", "Mahnungen fällig"), count: dunning_due, link: "/offene-posten?tab=mahnwesen".into(), severity: "orange".into() });
    }
    if unbooked > 0 {
        todos.push(Todo { kind: "transactions".into(), label: plural(unbooked, "nicht zugeordnete Kontobewegung", "nicht zugeordnete Kontobewegungen"), count: unbooked, link: "/bank".into(), severity: "blue".into() });
    }
    let (debits_due, debits_returned) = crate::services::debit::due_count(conn)?;
    if debits_returned > 0 {
        todos.push(Todo { kind: "debit_returned".into(), label: plural(debits_returned, "Rücklastschrift", "Rücklastschriften"), count: debits_returned, link: "/offene-posten?tab=lastschriften".into(), severity: "red".into() });
    }
    if debits_due > 0 {
        todos.push(Todo { kind: "debits".into(), label: plural(debits_due, "Lastschrift einzuziehen", "Lastschriften einzuziehen"), count: debits_due, link: "/offene-posten?tab=lastschriften".into(), severity: "blue".into() });
    }
    if drafts > 0 {
        todos.push(Todo { kind: "drafts".into(), label: plural(drafts, "Rechnungsentwurf", "Rechnungsentwürfe"), count: drafts, link: "/rechnungen?status=draft".into(), severity: "gray".into() });
    }
    if crate::services::dunning::base_rate_outdated(&s, t) {
        todos.push(Todo { kind: "base_rate".into(), label: "Basiszinssatz für Verzugszinsen prüfen".into(), count: 1, link: "/einstellungen/mahnwesen".into(), severity: "orange".into() });
    }
    let changed = ustva_changed_after_filing(conn, t)?;
    if changed > 0 {
        todos.push(Todo { kind: "ustva_changed".into(), label: plural(changed, "Voranmeldung nach Übermittlung geändert", "Voranmeldungen nach Übermittlung geändert"), count: changed, link: "/steuern/ustva".into(), severity: "red".into() });
    }
    let unfiled = unfiled_ustva(conn, t)?;
    if unfiled > 0 {
        todos.push(Todo { kind: "ustva".into(), label: plural(unfiled, "offene Umsatzsteuer-Voranmeldung", "offene Umsatzsteuer-Voranmeldungen"), count: unfiled, link: "/steuern/ustva".into(), severity: "violet".into() });
    }
    let (zm_unfiled, zm_changed) = crate::services::zm::todos(conn, t)?;
    if zm_changed > 0 {
        todos.push(Todo { kind: "zm_changed".into(), label: plural(zm_changed, "Zusammenfassende Meldung nach Übermittlung geändert", "Zusammenfassende Meldungen nach Übermittlung geändert"), count: zm_changed, link: "/steuern/zm".into(), severity: "red".into() });
    }
    if zm_unfiled > 0 {
        todos.push(Todo { kind: "zm".into(), label: plural(zm_unfiled, "offene Zusammenfassende Meldung", "offene Zusammenfassende Meldungen"), count: zm_unfiled, link: "/steuern/zm".into(), severity: "violet".into() });
    }
    let recent = db::query::<Activity>(conn, "ORDER BY id DESC LIMIT 8", &[])?;
    let mut open_receipts = oi.payables.clone();
    open_receipts.sort_by_key(|r| r.receipt.due_date);
    open_receipts.truncate(5);
    let mut expense_categories = a.expense_categories.clone();
    if expense_categories.len() > 6 {
        let rest: Decimal = expense_categories[5..].iter().map(|c| c.amount).sum();
        expense_categories.truncate(5);
        expense_categories.push(NamedAmount { id: None, key: "other".into(), name: "Sonstige".into(), amount: rest, count: 0 });
    }
    let count = |sql: &str| db::scalar_i64(conn, sql, &[]).unwrap_or(0);
    let c = &s.company;
    let setup = SetupProgress {
        company_complete: !c.name.is_empty() && !c.street.is_empty() && !c.zip.is_empty() && !c.city.is_empty() && !c.iban.is_empty() && (!c.tax_number.is_empty() || !c.vat_id.is_empty()),
        has_logo: c.logo_file_id.is_some() || db::query::<Layout>(conn, "", &[])?.iter().any(|l| l.config.logo_file_id.is_some()),
        has_contacts: count("SELECT COUNT(*) FROM contacts") > 0,
        has_articles: count("SELECT COUNT(*) FROM articles") > 0,
        has_invoices: count("SELECT COUNT(*) FROM documents WHERE doc_type = 'invoice'") > 0,
        has_receipts: count("SELECT COUNT(*) FROM receipts") > 0,
        has_bank_data: count("SELECT COUNT(*) FROM transactions") > 0,
        hidden: s.hide_getting_started,
    };
    Ok(Dashboard {
        setup,
        company_name: s.company.name.clone(),
        year,
        invoiced_month,
        invoiced_prev_month,
        invoiced_year: a.invoiced_total,
        invoiced_prev_year_to_date,
        revenue_year: a.revenue_total,
        expense_year: a.expense_total,
        profit_year: a.profit_total,
        open_receivables: oi.receivables_total,
        open_receivables_count: oi.receivables.len() as i64,
        overdue_amount: overdue.iter().map(|d| d.open_amount).sum(),
        overdue_count: overdue.len() as i64,
        open_payables: oi.payables_total,
        open_payables_count: oi.payables.len() as i64,
        liquidity,
        ustva_zahllast: u.zahllast,
        ustva_label: u.period_label,
        ustva_due: u.due_date,
        months: a.months,
        overdue: overdue.into_iter().take(6).collect(),
        open_quotes: open_quotes.into_iter().take(5).collect(),
        open_receipts,
        accounts,
        expense_categories,
        todos,
        recent,
        small_business: s.tax.small_business,
    })
}

/// The same calendar day one year earlier (29 February becomes 28 February).
fn same_day_last_year(t: NaiveDate) -> NaiveDate {
    let y = t.year() - 1;
    ymd(y, t.month(), t.day().min(last_day_of_month(y, t.month()).day()))
}

/// Submitted returns of this and the previous year whose figures changed afterwards – they need a
/// berichtigte Voranmeldung.
fn ustva_changed_after_filing(conn: &Connection, t: NaiveDate) -> Result<i64> {
    let mut changed = 0;
    for year in [t.year() - 1, t.year()] {
        for p in ustva_periods(conn, year)? {
            if p.filing.as_ref().is_some_and(|f| (f.amount - p.zahllast).abs() > Decimal::new(5, 3)) {
                changed += 1;
            }
        }
    }
    Ok(changed)
}

/// Ended Voranmeldungszeiträume of this and the previous year that are not marked as submitted –
/// the previous year matters in January / February, when its last return is due. Only periods
/// with actual bookings count (no nagging for months before the company used the software).
fn unfiled_ustva(conn: &Connection, t: NaiveDate) -> Result<i64> {
    let s = cfg::load(conn)?;
    if s.tax.small_business || s.tax.ustva_period == "yearly" {
        return Ok(0);
    }
    let mut unfiled = 0i64;
    for year in [t.year() - 1, t.year()] {
        for p in ustva_periods(conn, year)? {
            if p.filing.is_some() || p.to >= t {
                continue;
            }
            let entries = db::scalar_i64(conn, "SELECT COUNT(*) FROM journal_entries WHERE date >= ?1 AND date <= ?2", &[v(p.from), v(p.to)])?;
            if entries > 0 || !p.zahllast.is_zero() {
                unfiled += 1;
            }
        }
    }
    Ok(unfiled)
}

fn plural(n: i64, one: &str, many: &str) -> String {
    if n == 1 { one.into() } else { many.into() }
}


#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn booking(conn: &Connection, date: NaiveDate) {
        let mut m = ManualEntry { date, description: "Miete".into(), debit_account: "4210".into(), credit_account: "1200".into(), amount: dec!(500), ..Default::default() };
        let id = db::insert(conn, &mut m).unwrap();
        crate::services::posting::rebuild(conn, crate::services::posting::Source::Manual(id)).unwrap();
    }

    #[test]
    fn ustva_due_dates_skip_weekends_and_holidays() {
        // 10 April 2020 was Karfreitag, 13 April Ostermontag.
        assert_eq!(ustva_due(ymd(2020, 3, 31), false), ymd(2020, 4, 14));
        // 10 May 2018: Christi Himmelfahrt.
        assert_eq!(ustva_due(ymd(2018, 4, 30), false), ymd(2018, 5, 11));
        // Dauerfristverlängerung: one month later; 10 January 2026 is a Saturday → Monday.
        assert_eq!(ustva_due(ymd(2025, 11, 30), true), ymd(2026, 1, 12));
        assert_eq!(ustva_due(ymd(2026, 9, 30), false), ymd(2026, 10, 12));
    }

    #[test]
    fn journal_search_matches_amounts() {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        for (description, amount) in [("Erlös", dec!(1450)), ("Porto", dec!(1.45))] {
            let mut m = ManualEntry { date: ymd(2026, 2, 1), description: description.into(), debit_account: "1200".into(), credit_account: "8400".into(), amount, tax_key: "UST19".into(), ..Default::default() };
            let id = db::insert(&conn, &mut m).unwrap();
            crate::services::posting::rebuild(&conn, crate::services::posting::Source::Manual(id)).unwrap();
        }
        let hits = |q: &str| journal(&conn, ymd(2026, 1, 1), ymd(2026, 12, 31), "", q).unwrap().into_iter().map(|r| r.entry.description).collect::<Vec<_>>();
        assert_eq!(hits("1450"), ["Erlös"]);
        assert_eq!(hits("1.450,00 €"), ["Erlös"]);
        assert_eq!(hits("-1450"), ["Erlös"]);
        assert_eq!(hits("231,51"), ["Erlös"]);
        assert_eq!(hits("1,45"), ["Porto"]);
        assert_eq!(hits("1.450"), ["Porto", "Erlös"]);
        assert_eq!(hits("porto"), ["Porto"]);
        assert!(hits("999").is_empty());
    }

    #[test]
    fn changes_after_filing_are_reported() {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        let mut m = ManualEntry { date: ymd(2026, 2, 1), description: "Erlös".into(), debit_account: "1200".into(), credit_account: "8400".into(), amount: dec!(119), tax_key: "UST19".into(), ..Default::default() };
        let id = db::insert(&conn, &mut m).unwrap();
        crate::services::posting::rebuild(&conn, crate::services::posting::Source::Manual(id)).unwrap();
        let mut f = UstvaFiling { year: 2026, period: "Q1".into(), submitted_at: "2026-04-10".into(), amount: dec!(19), ..Default::default() };
        db::insert(&conn, &mut f).unwrap();
        assert_eq!(ustva_changed_after_filing(&conn, ymd(2026, 5, 1)).unwrap(), 0);
        conn.execute("UPDATE manual_entries SET amount = '238'", []).unwrap();
        crate::services::posting::rebuild(&conn, crate::services::posting::Source::Manual(id)).unwrap();
        assert_eq!(ustva_changed_after_filing(&conn, ymd(2026, 5, 1)).unwrap(), 1);
    }

    #[test]
    fn previous_year_comparison_day() {
        assert_eq!(same_day_last_year(ymd(2026, 3, 31)), ymd(2025, 3, 31));
        assert_eq!(same_day_last_year(ymd(2026, 10, 17)), ymd(2025, 10, 17));
        assert_eq!(same_day_last_year(ymd(2028, 2, 29)), ymd(2027, 2, 28));
    }

    #[test]
    fn last_years_final_return_is_outstanding_in_january() {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        booking(&conn, ymd(2025, 11, 15));
        booking(&conn, ymd(2026, 1, 5));
        // Q4/2025 ended and was not submitted; Q1/2026 is still running.
        assert_eq!(unfiled_ustva(&conn, ymd(2026, 1, 20)).unwrap(), 1);
        let mut f = UstvaFiling { year: 2025, period: "Q4".into(), submitted_at: "2026-01-09".into(), ..Default::default() };
        db::insert(&conn, &mut f).unwrap();
        assert_eq!(unfiled_ustva(&conn, ymd(2026, 1, 20)).unwrap(), 0);
        assert_eq!(unfiled_ustva(&conn, ymd(2026, 4, 2)).unwrap(), 1, "Q1/2026 after its end");
    }
}
