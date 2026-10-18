// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Settings persistence, initial seeding, number ranges and the activity log.

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use crate::db;
use crate::models::{Account, Activity, Layout, NumberRange, Settings};
use crate::services::defaults;
use crate::util::now_str;
use crate::{bail_user, chart};

pub fn load(conn: &Connection) -> Result<Settings> {
    let mut s = match db::meta_get(conn, "settings")? {
        Some(raw) => match serde_json::from_str(&raw) {
            Ok(s) => s,
            Err(e) => recover(conn, &raw, &e)?,
        },
        None => Settings::default(),
    };
    // Text templates added in a later version.
    for (key, texts) in defaults::default_texts() {
        s.docs.texts.entry(key).or_insert(texts);
    }
    Ok(s)
}

/// Settings that no longer parse as a whole (e.g. after a field changed its type) are not thrown
/// away: every value that still fits is kept and only the broken ones fall back to their
/// defaults – a Festschreibung or the onboarding state must not get lost. The original JSON stays
/// in `meta` for manual recovery.
fn recover(conn: &Connection, raw: &str, err: &serde_json::Error) -> Result<Settings> {
    db::meta_set(conn, &format!("settings_unreadable_{}", now_str()), raw)?;
    let stored: serde_json::Value = serde_json::from_str(raw).unwrap_or(serde_json::Value::Null);
    let mut merged = serde_json::to_value(Settings::default())?;
    let mut lost = Vec::new();
    merge_valid(&mut merged, "", &stored, &mut lost);
    let s: Settings = serde_json::from_value(merged)?;
    save(conn, &s)?;
    tracing::warn!("Einstellungen nicht vollständig lesbar ({err}); auf Standard gesetzt: {}", lost.join(", "));
    Ok(s)
}

/// Puts `value` at the JSON pointer `path` of `merged` if the settings stay valid; otherwise
/// tries the members of an object one by one. Values that do not fit are listed in `lost`.
fn merge_valid(merged: &mut serde_json::Value, path: &str, value: &serde_json::Value, lost: &mut Vec<String>) {
    let mut trial = merged.clone();
    if set_pointer(&mut trial, path, value.clone()) && serde_json::from_value::<Settings>(trial.clone()).is_ok() {
        *merged = trial;
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                merge_valid(merged, &format!("{path}/{}", k.replace('~', "~0").replace('/', "~1")), v, lost);
            }
        }
        _ => lost.push(path.to_string()),
    }
}

fn set_pointer(doc: &mut serde_json::Value, path: &str, value: serde_json::Value) -> bool {
    if let Some(slot) = doc.pointer_mut(path) {
        *slot = value;
        return true;
    }
    // A key missing in the defaults (map entries): add it to its parent object.
    let Some((parent, key)) = path.rsplit_once('/') else { return false };
    match doc.pointer_mut(parent) {
        Some(serde_json::Value::Object(map)) => {
            map.insert(key.replace("~1", "/").replace("~0", "~"), value);
            true
        }
        _ => false,
    }
}

pub fn save(conn: &Connection, s: &Settings) -> Result<()> {
    db::meta_set(conn, "settings", &serde_json::to_string(s)?)
}

/// Seeds chart of accounts, number ranges and layouts on a fresh database.
pub fn ensure_seeded(conn: &Connection) -> Result<()> {
    let settings = load(conn)?;
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM accounts", &[])? == 0 {
        seed_accounts(conn, &settings.tax.chart)?;
    }
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM number_ranges", &[])? == 0 {
        for mut r in defaults::default_number_ranges() {
            db::insert(conn, &mut r)?;
        }
    }
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM layouts", &[])? == 0 {
        for (i, (name, config)) in defaults::default_layouts().into_iter().enumerate() {
            let mut l = Layout { id: 0, name, is_default: i == 0, config, created_at: now_str(), updated_at: now_str() };
            db::insert(conn, &mut l)?;
        }
    }
    if db::meta_get(conn, "settings")?.is_none() {
        save(conn, &settings)?;
    }
    Ok(())
}

pub fn seed_accounts(conn: &Connection, chart_name: &str) -> Result<()> {
    conn.execute("DELETE FROM accounts", [])?;
    for mut a in chart::accounts_for(chart_name) {
        db::insert(conn, &mut a)?;
    }
    Ok(())
}

/// The chart can only be switched while no bookings, receipts or payment accounts use accounts.
pub fn chart_in_use(conn: &Connection) -> Result<bool> {
    let used = db::scalar_i64(conn, "SELECT COUNT(*) FROM journal_lines", &[])?
        + db::scalar_i64(conn, "SELECT COUNT(*) FROM receipt_lines", &[])?
        + db::scalar_i64(conn, "SELECT COUNT(*) FROM payment_accounts", &[])?;
    Ok(used > 0)
}

pub fn change_chart(conn: &Connection, chart_name: &str) -> Result<()> {
    if chart_in_use(conn)? {
        bail_user!("Der Kontenrahmen kann nur geändert werden, solange noch keine Buchungen, Belege oder Zahlungskonten existieren.");
    }
    seed_accounts(conn, chart_name)
}

pub fn accounts(conn: &Connection) -> Result<Vec<Account>> {
    db::query::<Account>(conn, "ORDER BY number", &[])
}

pub fn account_by_number(conn: &Connection, number: &str) -> Result<Option<Account>> {
    db::query_one::<Account>(conn, "WHERE number = ?1", &[db::v(number.to_string())])
}

/// Number ranges ------------------------------------------------------------

pub fn format_number(format: &str, n: i64, date: NaiveDate) -> String {
    let mut out = format.replace("{YYYY}", &format!("{:04}", date.year()));
    out = out.replace("{YY}", &format!("{:02}", date.year() % 100));
    out = out.replace("{MM}", &format!("{:02}", date.month()));
    // {NNNN...} => zero padded
    while let Some(start) = out.find("{N") {
        let Some(end_rel) = out[start..].find('}') else { break };
        let token = &out[start..start + end_rel + 1];
        let width = token.chars().filter(|c| *c == 'N').count();
        let rep = if width <= 1 { n.to_string() } else { format!("{:0width$}", n, width = width) };
        out = out.replacen(token, &rep, 1);
    }
    out
}

/// Consume the next number of a range (gap-free, per year when configured).
pub fn next_number(conn: &Connection, key: &str, date: NaiveDate) -> Result<String> {
    let mut r = db::query_one::<NumberRange>(conn, "WHERE key = ?1", &[db::v(key.to_string())])?
        .ok_or_else(|| anyhow::anyhow!("Nummernkreis {key} fehlt"))?;
    let year = i64::from(date.year());
    if r.reset_yearly && r.year > year {
        // A document dated in an earlier year continues that year's sequence after its highest
        // number; the counter of the current year stays untouched.
        let used = numbers_in_use(conn, key)?;
        let highest = (1..=used.len() as i64 + r.next + 10_000).filter(|n| used.contains(&format_number(&r.format, *n, date))).max().unwrap_or(0);
        return Ok(format_number(&r.format, highest + 1, date));
    }
    if r.reset_yearly && r.year < year {
        r.year = year;
        r.next = 1;
    }
    let mut n = r.next;
    // Guard against duplicates (e.g. after manual edits of the range).
    loop {
        let candidate = format_number(&r.format, n, date);
        if !number_in_use(conn, key, &candidate)? {
            r.next = n + 1;
            db::update(conn, &r)?;
            return Ok(candidate);
        }
        n += 1;
    }
}

/// Table holding the numbers of a number range.
fn number_table(key: &str) -> Option<&'static str> {
    Some(match key {
        "invoice" | "credit_note" | "cancellation" | "quote" | "order" | "delivery" => "documents",
        "receipt" => "receipts",
        "customer" | "supplier" => "contacts",
        "article" => "articles",
        "asset" => "assets",
        _ => return None,
    })
}

fn number_in_use(conn: &Connection, key: &str, number: &str) -> Result<bool> {
    let Some(table) = number_table(key) else { return Ok(false) };
    Ok(db::scalar_i64(conn, &format!("SELECT COUNT(*) FROM {table} WHERE number = ?1"), &[db::v(number.to_string())])? > 0)
}

fn numbers_in_use(conn: &Connection, key: &str) -> Result<std::collections::HashSet<String>> {
    let Some(table) = number_table(key) else { return Ok(Default::default()) };
    let mut stmt = conn.prepare(&format!("SELECT number FROM {table} WHERE number != ''"))?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Activity log ---------------------------------------------------------------

pub fn log(conn: &Connection, entity: &str, entity_id: i64, action: &str, message: impl Into<String>) -> Result<()> {
    let mut a = Activity {
        id: 0,
        entity: entity.into(),
        entity_id,
        action: action.into(),
        message: message.into(),
        created_at: now_str(),
    };
    db::insert(conn, &mut a)?;
    Ok(())
}

pub fn activities(conn: &Connection, entity: &str, entity_id: i64) -> Result<Vec<Activity>> {
    db::query::<Activity>(
        conn,
        "WHERE entity = ?1 AND entity_id = ?2 ORDER BY id DESC",
        &[db::v(entity.to_string()), db::v(entity_id)],
    )
}

/// Festschreibung: extends the locked period up to and including `until`. A lock can never be
/// shortened or lifted, so bookings of a locked period stay unchanged (GoBD).
pub fn lock_until(conn: &Connection, until: NaiveDate) -> Result<Settings> {
    let mut s = load(conn)?;
    if until > crate::util::today() {
        bail_user!("Festschreiben ist höchstens bis zum heutigen Tag möglich.");
    }
    if let Some(locked) = s.tax.locked_until
        && until <= locked
    {
        bail_user!(
            "Der Zeitraum bis {} ist bereits festgeschrieben. Eine Festschreibung kann weder verkürzt noch aufgehoben werden.",
            crate::util::fmt_date(locked)
        );
    }
    s.tax.locked_until = Some(until);
    save(conn, &s)?;
    Ok(s)
}

/// Company master data for tests that issue invoices.
#[cfg(test)]
pub fn seed_test_company(conn: &Connection) {
    let mut s = load(conn).unwrap();
    s.company.name = "Test GmbH".into();
    s.company.street = "Hauptstraße 1".into();
    s.company.zip = "10115".into();
    s.company.city = "Berlin".into();
    s.company.tax_number = "12/345/67890".into();
    save(conn, &s).unwrap();
}

/// Rejects changes to bookings within a locked (festgeschriebenen) period.
pub fn check_unlocked(conn: &Connection, date: NaiveDate) -> Result<()> {
    let s = load(conn)?;
    if let Some(locked) = s.tax.locked_until
        && date <= locked
    {
        bail_user!(
            "Der Zeitraum bis {} ist festgeschrieben (GoBD). Änderungen sind nicht mehr möglich.",
            crate::util::fmt_date(locked)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ymd;

    #[test]
    fn unreadable_settings_keep_what_still_fits() {
        let conn = db::open_memory();
        let raw = r#"{"onboarded":true,"company":{"name":"Lichtwerk","city":"Köln"},"tax":{"bookkeeping":"bilanz","locked_until":"2020-12-31","ustva_period":5},"docs":{"payment_terms_days":"vierzehn","texts":{"invoice":{"title":"Rechnung"}}},"hide_getting_started":"ja"}"#;
        db::meta_set(&conn, "settings", raw).unwrap();
        let s = load(&conn).unwrap();
        assert!(s.onboarded);
        assert_eq!((s.company.name.as_str(), s.company.city.as_str()), ("Lichtwerk", "Köln"));
        assert_eq!((s.tax.bookkeeping.as_str(), s.tax.locked_until), ("bilanz", Some(ymd(2020, 12, 31))));
        let d = Settings::default();
        assert_eq!((s.tax.ustva_period.clone(), s.docs.payment_terms_days, s.hide_getting_started), (d.tax.ustva_period, d.docs.payment_terms_days, d.hide_getting_started));
        assert_eq!(s.docs.texts.get("invoice").map(|t| t.title.as_str()), Some("Rechnung"));
        // The repaired settings are stored, the original is kept for manual recovery.
        assert!(serde_json::from_str::<Settings>(&db::meta_get(&conn, "settings").unwrap().unwrap()).is_ok());
        let kept: i64 = conn.query_row("SELECT COUNT(*) FROM meta WHERE key LIKE 'settings_unreadable_%' AND value = ?1", [raw], |r| r.get(0)).unwrap();
        assert_eq!(kept, 1);
    }

    #[test]
    fn locked_period_refuses_every_change() {
        use crate::models::{Asset, Document, DocumentLine, PaymentAccount, Receipt, ReceiptLine, Transaction};
        use crate::services::{assets, documents, finance, receipts};
        use rust_decimal_macros::dec;
        let conn = db::open_memory();
        ensure_seeded(&conn).unwrap();
        seed_test_company(&conn);
        let bank = finance::save_account(&conn, PaymentAccount { name: "Bank".into(), opening_date: ymd(2020, 1, 1), ..Default::default() }).unwrap();
        let receipt = |date: NaiveDate| receipts::ReceiptInput {
            receipt: Receipt { supplier_name: "Lieferant".into(), receipt_type: "expense".into(), date, prices_gross: true, ..Default::default() },
            lines: vec![ReceiptLine { account: "4930".into(), gross: dec!(119), tax_rate: dec!(19), ..Default::default() }],
            payment: None,
        };
        let tx = |date: NaiveDate| finance::TransactionInput { tx: Transaction { account_id: bank, date, amount: dec!(-10), purpose: "Gebühr".into(), ..Default::default() }, allocations: vec![] };
        let doc = Document { doc_type: "invoice".into(), address: "Kunde GmbH\nWeg 1\n12345 Stadt".into(), tax_mode: "standard".into(), date: ymd(2020, 3, 1), ..Default::default() };
        let line = DocumentLine { line_type: "item".into(), name: "Leistung".into(), quantity: dec!(1), unit_price: dec!(500), tax_rate: dec!(19), ..Default::default() };
        let draft = documents::create(&conn, documents::DocumentInput { doc, lines: vec![line] }).unwrap();
        let r = receipts::create(&conn, receipt(ymd(2020, 3, 1))).unwrap();
        let t = finance::create(&conn, tx(ymd(2020, 3, 1))).unwrap();

        lock_until(&conn, ymd(2020, 12, 31)).unwrap();
        let locked = |res: anyhow::Result<()>, what: &str| {
            let err = res.expect_err(what).to_string();
            assert!(err.contains("festgeschrieben"), "{what}: {err}");
        };
        locked(documents::finalize(&conn, draft).map(|_| ()), "finalize an invoice dated in the period");
        locked(receipts::create(&conn, receipt(ymd(2020, 6, 1))).map(|_| ()), "new receipt");
        let full = receipts::get_full(&conn, r).unwrap();
        locked(receipts::update(&conn, r, receipts::ReceiptInput { receipt: full.receipt, lines: full.lines, payment: None }), "change receipt");
        locked(receipts::delete(&conn, r), "delete receipt");
        locked(finance::create(&conn, tx(ymd(2020, 5, 1))).map(|_| ()), "new bank movement");
        let old = db::get::<Transaction>(&conn, t).unwrap();
        locked(finance::update(&conn, t, finance::TransactionInput { tx: old, allocations: vec![] }), "change bank movement");
        locked(finance::delete(&conn, t), "delete bank movement");
        let asset = Asset { name: "Laptop".into(), account: "0420".into(), acquisition_date: ymd(2020, 4, 1), cost: dec!(1200), useful_life_years: 3, method: "linear".into(), ..Default::default() };
        locked(assets::save(&conn, asset).map(|_| ()), "new asset");
        // The next period is open.
        assert!(receipts::create(&conn, receipt(ymd(2021, 1, 4))).is_ok());
        assert!(finance::create(&conn, tx(ymd(2021, 1, 4))).is_ok());
    }

    #[test]
    fn lock_only_moves_forward() {
        let conn = db::open_memory();
        ensure_seeded(&conn).unwrap();
        let t = crate::util::today();
        assert!(lock_until(&conn, t + chrono::Duration::days(1)).is_err(), "no lock into the future");
        let first = t - chrono::Duration::days(30);
        assert_eq!(lock_until(&conn, first).unwrap().tax.locked_until, Some(first));
        assert!(lock_until(&conn, first).is_err(), "same date again");
        assert!(lock_until(&conn, first - chrono::Duration::days(1)).is_err(), "no shortening");
        assert_eq!(load(&conn).unwrap().tax.locked_until, Some(first));
        assert!(check_unlocked(&conn, first).is_err());
        assert!(check_unlocked(&conn, first + chrono::Duration::days(1)).is_ok());
        assert_eq!(lock_until(&conn, t).unwrap().tax.locked_until, Some(t));
    }

    #[test]
    fn back_dated_documents_continue_their_years_sequence() {
        let conn = db::open_memory();
        ensure_seeded(&conn).unwrap();
        conn.execute("UPDATE number_ranges SET format = 'RE-{YYYY}-{NNNN}', reset_yearly = 1, year = 2025, next = 1 WHERE key = 'invoice'", []).unwrap();
        let issue = |date: NaiveDate| {
            let n = next_number(&conn, "invoice", date).unwrap();
            conn.execute("INSERT INTO documents (doc_type, number, date) VALUES ('invoice', ?1, ?2)", [&n, &date.to_string()]).unwrap();
            n
        };
        for _ in 0..3 {
            issue(ymd(2025, 12, 1));
        }
        assert_eq!(issue(ymd(2026, 1, 5)), "RE-2026-0001");
        assert_eq!(issue(ymd(2026, 1, 6)), "RE-2026-0002");
        // A late invoice for December continues 2025 without touching the 2026 counter.
        assert_eq!(issue(ymd(2025, 12, 31)), "RE-2025-0004");
        assert_eq!(issue(ymd(2026, 1, 7)), "RE-2026-0003");
    }

    #[test]
    fn number_format() {
        assert_eq!(format_number("RE-{YYYY}-{NNNN}", 7, ymd(2026, 3, 1)), "RE-2026-0007");
        assert_eq!(format_number("{N}", 10001, ymd(2026, 3, 1)), "10001");
        assert_eq!(format_number("{YY}{MM}-{NNN}", 12, ymd(2026, 3, 1)), "2603-012");
    }
}
