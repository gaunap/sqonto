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
    match db::meta_get(conn, "settings")? {
        Some(s) => Ok(serde_json::from_str(&s).unwrap_or_default()),
        None => Ok(Settings::default()),
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

/// Switching the chart is only allowed while no bookings reference accounts.
pub fn change_chart(conn: &Connection, chart_name: &str) -> Result<()> {
    let used = db::scalar_i64(conn, "SELECT COUNT(*) FROM journal_lines", &[])?
        + db::scalar_i64(conn, "SELECT COUNT(*) FROM receipt_lines", &[])?
        + db::scalar_i64(conn, "SELECT COUNT(*) FROM payment_accounts", &[])?;
    if used > 0 {
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
    if r.reset_yearly && r.year != year {
        // Start a new year; never go backwards when documents are created for an older year.
        if r.year < year {
            r.year = year;
            r.next = 1;
        }
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

fn number_in_use(conn: &Connection, key: &str, number: &str) -> Result<bool> {
    let sql = match key {
        "invoice" | "credit_note" | "cancellation" | "quote" | "order" | "delivery" => {
            "SELECT COUNT(*) FROM documents WHERE number = ?1"
        }
        "receipt" => "SELECT COUNT(*) FROM receipts WHERE number = ?1",
        "customer" | "supplier" => "SELECT COUNT(*) FROM contacts WHERE number = ?1",
        "article" => "SELECT COUNT(*) FROM articles WHERE number = ?1",
        "asset" => "SELECT COUNT(*) FROM assets WHERE number = ?1",
        _ => return Ok(false),
    };
    Ok(db::scalar_i64(conn, sql, &[db::v(number.to_string())])? > 0)
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
    fn number_format() {
        assert_eq!(format_number("RE-{YYYY}-{NNNN}", 7, ymd(2026, 3, 1)), "RE-2026-0007");
        assert_eq!(format_number("{N}", 10001, ymd(2026, 3, 1)), "10001");
        assert_eq!(format_number("{YY}{MM}-{NNN}", 12, ymd(2026, 3, 1)), "2603-012");
    }
}
