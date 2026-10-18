// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Small helpers: rounding, German number/date formatting, country data.

use chrono::{Datelike, Local, NaiveDate};

use rust_decimal::{Decimal, RoundingStrategy};

pub const HUNDRED: Decimal = Decimal::ONE_HUNDRED;

/// Commercial rounding to cents.
pub fn r2(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

pub fn now_str() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn group_thousands(int_part: &str) -> String {
    let bytes: Vec<char> = int_part.chars().collect();
    let mut out = String::new();
    for (i, c) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(*c);
    }
    out
}

/// Formats a number German style with a fixed number of decimals: 1234.5 -> "1.234,50".
pub fn fmt_num(d: Decimal, decimals: u32) -> String {
    let rounded = d.round_dp_with_strategy(decimals, RoundingStrategy::MidpointAwayFromZero);
    let neg = rounded.is_sign_negative() && !rounded.is_zero();
    let s = format!("{:.*}", decimals as usize, rounded.abs());
    let (int_part, frac) = match s.split_once('.') {
        Some((a, b)) => (a.to_string(), Some(b.to_string())),
        None => (s.clone(), None),
    };
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(&group_thousands(&int_part));
    if let Some(f) = frac {
        out.push(',');
        out.push_str(&f);
    }
    out
}

pub fn fmt_money(d: Decimal) -> String {
    format!("{}\u{a0}€", fmt_num(d, 2))
}

/// Quantity: up to 3 decimals, trailing zeros trimmed, at least no decimals.
pub fn fmt_qty(d: Decimal) -> String {
    let n = d.round_dp(3).normalize();
    let scale = n.scale();
    fmt_num(n, scale.min(3))
}

/// Unit price: 2 decimals, or up to 4 if needed.
pub fn fmt_price(d: Decimal) -> String {
    let n = d.round_dp(4).normalize();
    let scale = n.scale().clamp(2, 4);
    format!("{}\u{a0}€", fmt_num(n, scale))
}

pub fn fmt_percent(d: Decimal) -> String {
    let n = d.round_dp(2).normalize();
    format!("{}\u{a0}%", fmt_num(n, n.scale()))
}

pub fn fmt_date(d: NaiveDate) -> String {
    d.format("%d.%m.%Y").to_string()
}

pub fn fmt_date_opt(d: Option<NaiveDate>) -> String {
    d.map(fmt_date).unwrap_or_default()
}

pub fn month_name(m: u32) -> &'static str {
    [
        "Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November",
        "Dezember",
    ][(m.clamp(1, 12) - 1) as usize]
}

pub fn month_short(m: u32) -> &'static str {
    ["Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"][(m.clamp(1, 12) - 1) as usize]
}

pub fn last_day_of_month(year: i32, month: u32) -> NaiveDate {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1).unwrap() - chrono::Duration::days(1)
}

pub fn add_months(d: NaiveDate, months: i32) -> NaiveDate {
    let total = d.year() * 12 + d.month0() as i32 + months;
    let y = total.div_euclid(12);
    let m = total.rem_euclid(12) as u32 + 1;
    let last = last_day_of_month(y, m).day();
    NaiveDate::from_ymd_opt(y, m, d.day().min(last)).unwrap()
}

pub fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap_or_else(|| last_day_of_month(y, m))
}

pub const EU_COUNTRIES: &[&str] = &[
    "AT", "BE", "BG", "CY", "CZ", "DE", "DK", "EE", "ES", "FI", "FR", "GR", "HR", "HU", "IE", "IT", "LT", "LU", "LV",
    "MT", "NL", "PL", "PT", "RO", "SE", "SI", "SK",
];

pub fn is_eu(country: &str) -> bool {
    EU_COUNTRIES.contains(&country.to_uppercase().as_str())
}

pub fn country_name(code: &str) -> &'static str {
    match code.to_uppercase().as_str() {
        "DE" => "Deutschland",
        "AT" => "Österreich",
        "CH" => "Schweiz",
        "BE" => "Belgien",
        "BG" => "Bulgarien",
        "CY" => "Zypern",
        "CZ" => "Tschechien",
        "DK" => "Dänemark",
        "EE" => "Estland",
        "ES" => "Spanien",
        "FI" => "Finnland",
        "FR" => "Frankreich",
        "GR" => "Griechenland",
        "HR" => "Kroatien",
        "HU" => "Ungarn",
        "IE" => "Irland",
        "IT" => "Italien",
        "LT" => "Litauen",
        "LU" => "Luxemburg",
        "LV" => "Lettland",
        "MT" => "Malta",
        "NL" => "Niederlande",
        "PL" => "Polen",
        "PT" => "Portugal",
        "RO" => "Rumänien",
        "SE" => "Schweden",
        "SI" => "Slowenien",
        "SK" => "Slowakei",
        "GB" => "Vereinigtes Königreich",
        "US" => "Vereinigte Staaten",
        "NO" => "Norwegen",
        "LI" => "Liechtenstein",
        "CN" => "China",
        "JP" => "Japan",
        "CA" => "Kanada",
        "AU" => "Australien",
        "TR" => "Türkei",
        _ => "Ausland",
    }
}

/// UN/ECE Recommendation 20 unit code for common German unit names (E-Rechnung).
pub fn unit_code(unit: &str) -> &'static str {
    match unit.trim().to_lowercase().trim_end_matches('.') {
        "stk" | "stück" | "st" | "pcs" => "H87",
        "std" | "stunde" | "stunden" | "h" => "HUR",
        "min" | "minute" | "minuten" => "MIN",
        "tag" | "tage" | "pt" | "personentag" => "DAY",
        "woche" | "wochen" => "WEE",
        "monat" | "monate" | "mon" => "MON",
        "jahr" | "jahre" => "ANN",
        "pauschal" | "psch" | "pauschale" => "LS",
        "kg" => "KGM",
        "g" => "GRM",
        "t" => "TNE",
        "m" | "meter" => "MTR",
        "km" => "KMT",
        "m²" | "m2" | "qm" => "MTK",
        "m³" | "m3" | "cbm" => "MTQ",
        "l" | "liter" => "LTR",
        "paket" | "pkt" => "XPK",
        "set" => "SET",
        "lizenz" => "C62",
        _ => "C62",
    }
}

/// "Sehr geehrte Frau Dr. Müller," etc.
pub fn salutation_line(salutation: &str, title: &str, last_name: &str) -> String {
    let t = if title.trim().is_empty() { String::new() } else { format!("{} ", title.trim()) };
    match salutation.trim() {
        "Frau" if !last_name.trim().is_empty() => format!("Sehr geehrte Frau {t}{},", last_name.trim()),
        "Herr" if !last_name.trim().is_empty() => format!("Sehr geehrter Herr {t}{},", last_name.trim()),
        _ => "Sehr geehrte Damen und Herren,".into(),
    }
}

/// Replaces `{{Name}}` placeholders.
pub fn fill_placeholders(text: &str, vars: &[(&str, String)]) -> String {
    let mut out = text.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

pub fn normalize_iban(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase()
}

/// Format and check digits (ISO 13616, mod 97) of an IBAN.
pub fn iban_valid(s: &str) -> bool {
    let n = normalize_iban(s);
    if n.len() < 15 || n.len() > 34 || !n.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    let (head, rest) = n.split_at(4);
    if !head[..2].chars().all(|c| c.is_ascii_uppercase()) || !head[2..].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let mut rem: u32 = 0;
    for c in rest.chars().chain(head.chars()) {
        let d = c.to_digit(36).unwrap_or(0);
        rem = if d < 10 { (rem * 10 + d) % 97 } else { (rem * 100 + d) % 97 };
    }
    rem == 1
}

/// BIC with 8 or 11 characters (ISO 9362).
pub fn bic_valid(s: &str) -> bool {
    let b = normalize_iban(s);
    (b.len() == 8 || b.len() == 11) && b[..6].chars().all(|c| c.is_ascii_uppercase()) && b[6..].chars().all(|c| c.is_ascii_alphanumeric())
}

pub fn format_iban(s: &str) -> String {
    let n = normalize_iban(s);
    n.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).to_string()).collect::<Vec<_>>().join(" ")
}

/// IBAN with all but the country/check digits and the last four characters masked, as printed in
/// a pre-notification ("DE89 XXXX XXXX XXXX XX30 00").
pub fn mask_iban(s: &str) -> String {
    let n = normalize_iban(s);
    let len = n.chars().count();
    let masked: String = n.chars().enumerate().map(|(i, c)| if i < 4 || i + 4 >= len { c } else { 'X' }).collect();
    format_iban(&masked)
}

/// Gläubiger-Identifikationsnummer (SEPA creditor identifier): country, two check digits, a
/// three-character business code (not part of the check) and the national identifier; the check
/// digits are computed like an IBAN's over national identifier and country.
pub fn creditor_id_valid(s: &str) -> bool {
    let n = normalize_iban(s);
    if n.len() < 8 || n.len() > 35 || !n.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    if !n[..2].chars().all(|c| c.is_ascii_uppercase()) || !n[2..4].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if n.starts_with("DE") && (n.len() != 18 || !n[7..].chars().all(|c| c.is_ascii_digit())) {
        return false;
    }
    let mut rem: u32 = 0;
    for c in n[7..].chars().chain(n[..4].chars()) {
        let d = c.to_digit(36).unwrap_or(0);
        rem = if d < 10 { (rem * 10 + d) % 97 } else { (rem * 100 + d) % 97 };
    }
    rem == 1
}

/// Business day of the TARGET2 system, on which SEPA direct debits are settled: not on weekends,
/// New Year, Good Friday, Easter Monday, 1 May and 25/26 December.
pub fn is_target_day(d: NaiveDate) -> bool {
    if matches!(d.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) || matches!((d.month(), d.day()), (1, 1) | (5, 1) | (12, 25) | (12, 26)) {
        return false;
    }
    let easter = easter_sunday(d.year());
    d != easter - chrono::Duration::days(2) && d != easter + chrono::Duration::days(1)
}

/// The date itself if it is a TARGET2 business day, otherwise the next one.
pub fn target_day_from(mut d: NaiveDate) -> NaiveDate {
    while !is_target_day(d) {
        d += chrono::Duration::days(1);
    }
    d
}

/// Easter Sunday (Gregorian calendar, anonymous algorithm).
pub fn easter_sunday(year: i32) -> NaiveDate {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    ymd(year, month as u32, day as u32)
}

/// Public holiday in all German states (regional holidays such as Fronleichnam are not included).
pub fn is_national_holiday(d: NaiveDate) -> bool {
    if matches!((d.month(), d.day()), (1, 1) | (5, 1) | (10, 3) | (12, 25) | (12, 26)) {
        return true;
    }
    let easter = easter_sunday(d.year());
    [-2, 1, 39, 50].iter().any(|offset| d == easter + chrono::Duration::days(*offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn easter_and_holidays() {
        assert_eq!(easter_sunday(2024), ymd(2024, 3, 31));
        assert_eq!(easter_sunday(2025), ymd(2025, 4, 20));
        assert_eq!(easter_sunday(2026), ymd(2026, 4, 5));
        assert_eq!(easter_sunday(2038), ymd(2038, 4, 25));
        for d in [ymd(2020, 4, 10), ymd(2020, 4, 13), ymd(2018, 5, 10), ymd(2019, 6, 10), ymd(2026, 10, 3), ymd(2026, 12, 26)] {
            assert!(is_national_holiday(d), "{d}");
        }
        for d in [ymd(2020, 4, 14), ymd(2026, 6, 4), ymd(2026, 11, 1)] {
            assert!(!is_national_holiday(d), "{d}");
        }
    }

    #[test]
    fn formats_german_numbers() {
        assert_eq!(fmt_money(dec!(1234.5)), "1.234,50\u{a0}€");
        assert_eq!(fmt_money(dec!(-0.004)), "0,00\u{a0}€");
        assert_eq!(fmt_money(dec!(1234567.891)), "1.234.567,89\u{a0}€");
        assert_eq!(fmt_qty(dec!(2.500)), "2,5");
        assert_eq!(fmt_qty(dec!(3)), "3");
        assert_eq!(fmt_price(dec!(12.3456)), "12,3456\u{a0}€");
        assert_eq!(fmt_price(dec!(12.3)), "12,30\u{a0}€");
        assert_eq!(fmt_percent(dec!(19)), "19\u{a0}%");
        assert_eq!(r2(dec!(2.345)), dec!(2.35));
        assert_eq!(r2(dec!(-2.345)), dec!(-2.35));
    }

    #[test]
    fn creditor_ids_and_target_days() {
        assert!(creditor_id_valid("DE98ZZZ09999999999"));
        assert!(creditor_id_valid("de98 zzz 0999 9999 999"));
        assert!(!creditor_id_valid("DE97ZZZ09999999999"));
        assert!(!creditor_id_valid("DE98ZZZ0999999999"));
        assert_eq!(mask_iban("DE89 3704 0044 0532 0130 00"), "DE89 XXXX XXXX XXXX XX30 00");
        // Good Friday and Easter Monday 2026, then a weekend.
        assert!(!is_target_day(ymd(2026, 4, 3)) && !is_target_day(ymd(2026, 4, 6)) && is_target_day(ymd(2026, 4, 7)));
        assert!(is_target_day(ymd(2025, 10, 3)), "German holidays are TARGET days");
        assert_eq!(target_day_from(ymd(2026, 12, 24)), ymd(2026, 12, 24));
        assert_eq!(target_day_from(ymd(2026, 12, 25)), ymd(2026, 12, 28));
    }

    #[test]
    fn month_math() {
        assert_eq!(add_months(ymd(2026, 1, 31), 1), ymd(2026, 2, 28));
        assert_eq!(add_months(ymd(2026, 11, 15), 3), ymd(2027, 2, 15));
    }
}
