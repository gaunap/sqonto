// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! DATEV-Export (Buchungsstapel im EXTF-Format) für die Übergabe an die
//! Steuerberatung.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Datelike, Local, NaiveDate};
use rusqlite::Connection;
use rust_decimal::Decimal;
use rust_decimal::prelude::Signed;

use crate::chart;
use crate::db::{self, v};
use crate::models::{JournalEntry, JournalLine};
use crate::services::settings as cfg;

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\"").chars().take(60).collect::<String>())
}

fn amount(d: Decimal) -> String {
    format!("{:.2}", d.abs()).replace('.', ",")
}

pub fn export(conn: &Connection, from: NaiveDate, to: NaiveDate, advisor: &str, client: &str) -> Result<Vec<u8>> {
    let s = cfg::load(conn)?;
    let accounts: HashMap<String, crate::models::Account> = cfg::accounts(conn)?.into_iter().map(|a| (a.number.clone(), a)).collect();
    let skr = if s.tax.chart == "SKR04" { "04" } else { "03" };
    let role = |r: &str| chart::role(&s.tax.chart, r);
    let (vst19, vst7, ust19, ust7) = (role("vst19"), role("vst7"), role("ust19"), role("ust7"));
    let mut out = String::new();
    let now = Local::now();
    out.push_str(&format!(
        "\"EXTF\";700;21;\"Buchungsstapel\";13;{};;\"SQ\";\"Sqonto\";\"\";{};{};{};4;{};{};\"Sqonto Buchungsstapel\";\"\";1;0;0;\"EUR\";;\"\";;;\"{}\";;;\"\";\"\"\r\n",
        now.format("%Y%m%d%H%M%S000"),
        if advisor.is_empty() { "1001" } else { advisor },
        if client.is_empty() { "1" } else { client },
        format!("{}0101", from.year()),
        from.format("%Y%m%d"),
        to.format("%Y%m%d"),
        skr
    ));
    out.push_str("Umsatz (ohne Soll/Haben-Kz);Soll/Haben-Kennzeichen;WKZ Umsatz;Kurs;Basis-Umsatz;WKZ Basis-Umsatz;Konto;Gegenkonto (ohne BU-Schlüssel);BU-Schlüssel;Belegdatum;Belegfeld 1;Belegfeld 2;Skonto;Buchungstext\r\n");
    let entries = db::query::<JournalEntry>(conn, "WHERE date >= ?1 AND date <= ?2 ORDER BY date, id", &[v(from), v(to)])?;
    for e in entries {
        let lines = db::query::<JournalLine>(conn, "WHERE entry_id = ?1 ORDER BY id", &[v(e.id)])?;
        // (account, signed amount (+ = Soll), BU key)
        let mut items: Vec<(String, Decimal, String, String)> =
            lines.iter().map(|l| (l.account.clone(), l.debit - l.credit, String::new(), l.tax_code.clone())).collect();
        // Merge standard VAT lines into their base line (gross booking with BU key / Automatikkonto).
        let tax_idx: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, (acc, _, _, code))| (code == "VST" && (*acc == vst19 || *acc == vst7)) || ((code == "UST19" || code == "UST7") && (*acc == ust19 || *acc == ust7)))
            .map(|(i, _)| i)
            .collect();
        let mut remove = Vec::new();
        for ti in tax_idx {
            let (tacc, tamt, _, tcode) = items[ti].clone();
            let rate = if tacc == vst19 || tacc == ust19 { 19 } else { 7 };
            let base_line = lines.get(ti).map(|l| l.base).unwrap_or_default();
            let target = items.iter().position(|(acc, amt, _, code)| {
                *acc != tacc && !code.starts_with("UST") && code != "VST" && amt.signum() == tamt.signum() && (base_line.is_zero() || (*amt - base_line).abs() < Decimal::new(2, 2))
            });
            if let Some(bi) = target {
                items[bi].1 += tamt;
                let auto = accounts.get(&items[bi].0).map(|a| a.datev_auto).unwrap_or(false);
                if !auto {
                    items[bi].2 = match (tcode.as_str(), rate) {
                        ("VST", 19) => "9".into(),
                        ("VST", _) => "8".into(),
                        (_, 19) => "3".into(),
                        _ => "2".into(),
                    };
                }
                remove.push(ti);
            }
        }
        let items: Vec<_> = items.into_iter().enumerate().filter(|(i, _)| !remove.contains(i)).map(|(_, x)| x).collect();
        if items.len() < 2 {
            continue;
        }
        let main = items.iter().enumerate().max_by(|a, b| a.1 .1.abs().cmp(&b.1 .1.abs())).map(|(i, _)| i).unwrap_or(0);
        let main_acc = items[main].0.clone();
        for (i, (acc, amt, bu, _)) in items.iter().enumerate() {
            if i == main || amt.is_zero() {
                continue;
            }
            // The main account is debited when the counter line is a credit.
            let sh = if *amt < Decimal::ZERO { "S" } else { "H" };
            out.push_str(&format!(
                "{};\"{}\";\"EUR\";;;;{};{};{};{};{};;;{}\r\n",
                amount(*amt),
                sh,
                main_acc,
                acc,
                if bu.is_empty() { String::new() } else { q(bu) },
                e.date.format("%d%m"),
                q(&e.doc_ref.chars().take(36).collect::<String>()),
                q(&e.description)
            ));
        }
    }
    // DATEV expects Windows-1252
    let safe = cp1252_safe(&out);
    let (bytes, _, _) = encoding_rs::WINDOWS_1252.encode(&safe);
    Ok(bytes.into_owned())
}

/// Replaces characters that Windows-1252 cannot represent (encoding_rs would emit
/// HTML entities like `&#8594;` for them).
fn cp1252_safe(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut buf = [0u8; 4];
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
            continue;
        }
        let (_, _, unmappable) = encoding_rs::WINDOWS_1252.encode(c.encode_utf8(&mut buf));
        if !unmappable {
            out.push(c);
            continue;
        }
        out.push_str(match c {
            '→' | '⇒' => "->",
            '←' | '⇐' => "<-",
            '≤' => "<=",
            '≥' => ">=",
            '\u{2009}' | '\u{202F}' => " ",
            _ => "?",
        });
    }
    out
}
