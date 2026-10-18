// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

use std::io::Write;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::Deserialize;
use ts_rs::TS;

use super::{file_response, render_pdf, safe_filename};
use crate::db::{self, v};
use crate::error::{ApiResult, AppError};
use crate::models::{FileMeta, Receipt, UstvaFiling};
use crate::pdf::data::{self, ReportSection, col, trow};
use crate::services::reports::{self, Analytics, Bwa, Dashboard, OpenItems, Statement, SusaRow, UstvaPeriod, UstvaReport};
use crate::services::zm::{self, ZmPeriod, ZmReport};
use crate::services::{assets, datev, gdpdu, settings as cfg};
use crate::state::AppState;
use crate::util::{fmt_date, fmt_money, fmt_num, today, ymd};

#[derive(Deserialize)]
pub struct YearQ {
    year: Option<i32>,
    date: Option<NaiveDate>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    period: Option<String>,
}

fn year(q: &YearQ) -> i32 {
    q.year.unwrap_or_else(|| today().year())
}

pub async fn dashboard(State(st): State<AppState>) -> ApiResult<Dashboard> {
    st.read(reports::dashboard).map(Json)
}

pub async fn analytics(State(st): State<AppState>, Query(q): Query<YearQ>) -> ApiResult<Analytics> {
    st.read(|c| reports::analytics(c, year(&q))).map(Json)
}

pub async fn euer(State(st): State<AppState>, Query(q): Query<YearQ>) -> ApiResult<Statement> {
    st.read(|c| reports::euer(c, year(&q))).map(Json)
}

pub async fn guv(State(st): State<AppState>, Query(q): Query<YearQ>) -> ApiResult<Statement> {
    st.read(|c| reports::guv(c, year(&q))).map(Json)
}

pub async fn bilanz(State(st): State<AppState>, Query(q): Query<YearQ>) -> ApiResult<Statement> {
    let date = q.date.unwrap_or_else(|| ymd(year(&q), 12, 31));
    st.read(|c| reports::bilanz(c, date)).map(Json)
}

pub async fn bwa(State(st): State<AppState>, Query(q): Query<YearQ>) -> ApiResult<Bwa> {
    st.read(|c| reports::bwa(c, year(&q))).map(Json)
}

pub async fn susa(State(st): State<AppState>, Query(q): Query<YearQ>) -> ApiResult<Vec<SusaRow>> {
    let y = year(&q);
    st.read(|c| reports::susa(c, q.from.unwrap_or(ymd(y, 1, 1)), q.to.unwrap_or(ymd(y, 12, 31)))).map(Json)
}

pub async fn open_items(State(st): State<AppState>) -> ApiResult<OpenItems> {
    st.read(reports::open_items).map(Json)
}

pub async fn ustva_periods(State(st): State<AppState>, Query(q): Query<YearQ>) -> ApiResult<Vec<UstvaPeriod>> {
    st.read(|c| reports::ustva_periods(c, year(&q))).map(Json)
}

pub async fn ustva(State(st): State<AppState>, Path((y, period)): Path<(i32, String)>) -> ApiResult<UstvaReport> {
    st.read(|c| reports::ustva(c, y, &period)).map(Json)
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct FilingRequest {
    pub submitted_at: Option<NaiveDate>,
    pub note: String,
}

pub async fn file_ustva(State(st): State<AppState>, Path((y, period)): Path<(i32, String)>, Json(r): Json<FilingRequest>) -> ApiResult<UstvaReport> {
    st.write(|c| {
        let rep = reports::ustva(c, y, &period)?;
        c.execute("DELETE FROM ustva_filings WHERE year = ?1 AND period = ?2", rusqlite::params![y, period])?;
        let mut f = UstvaFiling {
            id: 0,
            year: y as i64,
            period: period.clone(),
            submitted_at: r.submitted_at.unwrap_or_else(today).to_string(),
            amount: rep.zahllast,
            note: r.note,
        };
        db::insert(c, &mut f)?;
        reports::ustva(c, y, &period)
    })
    .map(Json)
}

pub async fn unfile_ustva(State(st): State<AppState>, Path((y, period)): Path<(i32, String)>) -> ApiResult<UstvaReport> {
    st.write(|c| {
        c.execute("DELETE FROM ustva_filings WHERE year = ?1 AND period = ?2", rusqlite::params![y, period])?;
        reports::ustva(c, y, &period)
    })
    .map(Json)
}

pub async fn zm_periods(State(st): State<AppState>, Query(q): Query<YearQ>) -> ApiResult<Vec<ZmPeriod>> {
    st.read(|c| zm::periods(c, year(&q))).map(Json)
}

pub async fn zm_report(State(st): State<AppState>, Path((y, period)): Path<(i32, String)>) -> ApiResult<ZmReport> {
    st.read(|c| zm::report(c, y, &period)).map(Json)
}

pub async fn file_zm(State(st): State<AppState>, Path((y, period)): Path<(i32, String)>, Json(r): Json<FilingRequest>) -> ApiResult<ZmReport> {
    st.write(|c| zm::file(c, y, &period, r.submitted_at.unwrap_or_else(today), r.note)).map(Json)
}

pub async fn unfile_zm(State(st): State<AppState>, Path((y, period)): Path<(i32, String)>) -> ApiResult<ZmReport> {
    st.write(|c| zm::unfile(c, y, &period)).map(Json)
}

/// CSV file for the import in the ELSTER form.
pub async fn zm_csv(State(st): State<AppState>, Path((y, period)): Path<(i32, String)>) -> Result<Response, AppError> {
    let r = st.read(|c| zm::report(c, y, &period))?;
    if r.rows.is_empty() {
        return Err(AppError::bad("Für diesen Zeitraum gibt es nichts zu melden."));
    }
    Ok(file_response(zm::csv(&r.rows).into_bytes(), "text/csv; charset=utf-8", &format!("ZM_{y}_{period}.csv"), true))
}

fn statement_sections(s: &Statement, with_prev: bool, prev_label: &str) -> Vec<ReportSection> {
    let mut cols = vec![col("Position", "left", "1fr"), col("Betrag", "right", "32mm")];
    if with_prev {
        cols.push(col(prev_label, "right", "32mm"));
    }
    let cells = |label: String, a: Decimal, p: Decimal| {
        let mut v = vec![label, fmt_money(a)];
        if with_prev {
            v.push(fmt_money(p));
        }
        v
    };
    s.sections
        .iter()
        .map(|sec| {
            let mut rows = Vec::new();
            for r in &sec.rows {
                if r.amount.is_zero() && r.prev.is_zero() && r.style == "normal" {
                    continue;
                }
                let style = match r.style.as_str() {
                    "subtotal" | "total" => r.style.as_str(),
                    "info" => "muted",
                    _ => "bold",
                };
                rows.push(trow(cells(r.label.clone(), r.amount, r.prev), style));
                for a in &r.accounts {
                    rows.push(trow(cells(format!("{} {}", a.number, a.name), a.amount, a.prev), "indent"));
                }
            }
            if !sec.total_label.is_empty() {
                rows.push(trow(cells(sec.total_label.clone(), sec.total, sec.prev_total), "total"));
            }
            ReportSection { title: sec.title.clone(), columns: cols.clone(), rows }
        })
        .collect()
}

/// PDF exports of all reports: /reports/pdf/{kind}
pub async fn report_pdf(State(st): State<AppState>, Path(kind): Path<String>, Query(q): Query<YearQ>) -> Result<Response, AppError> {
    let y = year(&q);
    let (job, name) = st.read(|c| {
        let s = cfg::load(c)?;
        let job;
        let name;
        match kind.as_str() {
            "euer" | "guv" => {
                let st_ = if kind == "euer" { reports::euer(c, y)? } else { reports::guv(c, y)? };
                let mut sections = statement_sections(&st_, true, &format!("Vorjahr {}", y - 1));
                sections.push(ReportSection {
                    title: String::new(),
                    columns: vec![col("Ergebnis", "left", "1fr"), col("Betrag", "right", "32mm"), col(&format!("Vorjahr {}", y - 1), "right", "32mm")],
                    rows: vec![trow(vec![st_.result_label.clone(), fmt_money(st_.result), fmt_money(st_.prev_result)], "total")],
                });
                job = data::report_job(c, &st_.title, &st_.subtitle, false, sections, vec![(st_.result_label.clone(), fmt_money(st_.result))], st_.notes.clone())?;
                name = format!("{}_{y}.pdf", if kind == "euer" { "EUER" } else { "GuV" });
            }
            "bilanz" => {
                let date = q.date.unwrap_or(ymd(y, 12, 31));
                let b = reports::bilanz(c, date)?;
                let sections = statement_sections(&b, true, "Vorjahr");
                let summary = b.sections.iter().map(|s| (s.total_label.clone(), fmt_money(s.total))).collect();
                job = data::report_job(c, &b.title, &b.subtitle, false, sections, summary, b.notes.clone())?;
                name = format!("Bilanz_{date}.pdf");
            }
            "bwa" => {
                let b = reports::bwa(c, y)?;
                let mut columns = vec![col("Position", "left", "1fr")];
                for m in &b.months {
                    columns.push(col(m, "right", "auto"));
                }
                columns.push(col("Gesamt", "right", "auto"));
                let rows = b
                    .rows
                    .iter()
                    .map(|r| {
                        let mut cells = vec![r.label.clone()];
                        cells.extend(r.values.iter().map(|v| fmt_num(*v, 0)));
                        cells.push(fmt_num(r.total, 0));
                        trow(cells, if r.style == "normal" { "normal" } else { r.style.as_str() })
                    })
                    .collect();
                job = data::report_job(c, "Betriebswirtschaftliche Auswertung", &format!("Kurzfristige Erfolgsrechnung {y} (Beträge in €)"), true, vec![ReportSection { title: String::new(), columns, rows }], vec![], vec![])?;
                name = format!("BWA_{y}.pdf");
            }
            "susa" => {
                let from = q.from.unwrap_or(ymd(y, 1, 1));
                let to = q.to.unwrap_or(ymd(y, 12, 31));
                let rows_data = reports::susa(c, from, to)?;
                let mut rows: Vec<_> = rows_data
                    .iter()
                    .map(|r| trow(vec![r.number.clone(), r.name.clone(), fmt_money(r.opening), fmt_money(r.debit), fmt_money(r.credit), fmt_money(r.closing)], "normal"))
                    .collect();
                let (sd, sc): (Decimal, Decimal) = (rows_data.iter().map(|r| r.debit).sum(), rows_data.iter().map(|r| r.credit).sum());
                rows.push(trow(vec!["".into(), "Summe".into(), "".into(), fmt_money(sd), fmt_money(sc), "".into()], "total"));
                job = data::report_job(
                    c,
                    "Summen- und Saldenliste",
                    &format!("{} bis {} · {}", fmt_date(from), fmt_date(to), s.tax.chart),
                    true,
                    vec![ReportSection {
                        title: String::new(),
                        columns: vec![col("Konto", "left", "16mm"), col("Bezeichnung", "left", "1fr"), col("Saldovortrag", "right", "auto"), col("Soll", "right", "auto"), col("Haben", "right", "auto"), col("Saldo", "right", "auto")],
                        rows,
                    }],
                    vec![],
                    vec![],
                )?;
                name = format!("SuSa_{from}_{to}.pdf");
            }
            "journal" => {
                let from = q.from.unwrap_or(ymd(y, 1, 1));
                let to = q.to.unwrap_or(ymd(y, 12, 31));
                let mut entries = reports::journal(c, from, to, "", "")?;
                entries.reverse();
                let mut rows = Vec::new();
                for e in &entries {
                    for (i, l) in e.lines.iter().enumerate() {
                        rows.push(trow(
                            vec![
                                if i == 0 { fmt_date(e.entry.date) } else { String::new() },
                                if i == 0 { e.entry.doc_ref.clone() } else { String::new() },
                                if i == 0 { e.entry.description.clone() } else { String::new() },
                                l.account.clone(),
                                if l.debit.is_zero() { String::new() } else { fmt_money(l.debit) },
                                if l.credit.is_zero() { String::new() } else { fmt_money(l.credit) },
                            ],
                            if i == 0 { "normal" } else { "muted" },
                        ));
                    }
                }
                job = data::report_job(
                    c,
                    "Buchungsjournal",
                    &format!("{} bis {}", fmt_date(from), fmt_date(to)),
                    true,
                    vec![ReportSection {
                        title: String::new(),
                        columns: vec![col("Datum", "left", "18mm"), col("Beleg", "left", "26mm"), col("Buchungstext", "left", "1fr"), col("Konto", "left", "14mm"), col("Soll", "right", "auto"), col("Haben", "right", "auto")],
                        rows,
                    }],
                    vec![("Buchungssätze".into(), entries.len().to_string())],
                    vec![],
                )?;
                name = format!("Journal_{from}_{to}.pdf");
            }
            "ustva" => {
                let period = q.period.clone().unwrap_or_else(|| "Q1".into());
                let r = reports::ustva(c, y, &period)?;
                let mut sections = Vec::new();
                for sec in &r.sections {
                    let rows: Vec<_> = sec
                        .fields
                        .iter()
                        .map(|f| {
                            trow(
                                vec![
                                    f.kz.clone(),
                                    f.label.clone(),
                                    f.base.map(|b| fmt_num(b.trunc(), 0)).unwrap_or_default(),
                                    if f.tax_kz.is_empty() { String::new() } else { f.tax_kz.clone() },
                                    f.tax.map(fmt_money).unwrap_or_default(),
                                ],
                                "normal",
                            )
                        })
                        .collect();
                    sections.push(ReportSection {
                        title: sec.title.clone(),
                        columns: vec![col("Kz.", "left", "10mm"), col("Bezeichnung", "left", "1fr"), col("Bemessungsgrundlage (volle €)", "right", "34mm"), col("Kz.", "left", "10mm"), col("Steuer", "right", "26mm")],
                        rows,
                    });
                }
                sections.push(ReportSection {
                    title: String::new(),
                    columns: vec![col("Kz.", "left", "10mm"), col("Ergebnis", "left", "1fr"), col("Betrag", "right", "32mm")],
                    rows: vec![trow(vec!["83".into(), "Verbleibende Umsatzsteuer-Vorauszahlung (+) / Überschuss (−)".into(), fmt_money(r.zahllast)], "total")],
                });
                job = data::report_job(
                    c,
                    "Umsatzsteuer-Voranmeldung",
                    &format!("{} · {} · fällig am {}", r.period_label, r.basis, fmt_date(r.due_date)),
                    false,
                    sections,
                    vec![("Umsatzsteuer".into(), fmt_money(r.output_tax)), ("Vorsteuer".into(), fmt_money(r.input_tax)), ("Zahllast / Erstattung".into(), fmt_money(r.zahllast))],
                    vec!["Dieser Ausdruck dient der Kontrolle. Die Übermittlung erfolgt über ELSTER.".into()],
                )?;
                name = format!("UStVA_{y}_{period}.pdf");
            }
            "zm" => {
                let period = q.period.clone().unwrap_or_else(|| "Q1".into());
                let r = zm::report(c, y, &period)?;
                let kind = |k: &str| if k == "L" { "Lieferung (L)" } else { "Sonstige Leistung (S)" };
                let rows: Vec<_> = r.rows.iter().map(|x| trow(vec![x.vat_id.clone(), x.contact_name.clone(), kind(&x.kind).into(), fmt_num(Decimal::from(x.reported), 0)], "normal")).collect();
                let mut sections = vec![ReportSection {
                    title: String::new(),
                    columns: vec![col("USt-IdNr.", "left", "34mm"), col("Kunde", "left", "1fr"), col("Art der Leistung", "left", "40mm"), col("Summe (volle €)", "right", "30mm")],
                    rows,
                }];
                if !r.issues.is_empty() {
                    sections.push(ReportSection {
                        title: "Nicht gemeldet – bitte prüfen".into(),
                        columns: vec![col("Beleg", "left", "26mm"), col("Kunde", "left", "1fr"), col("Hinweis", "left", "1fr"), col("Netto", "right", "26mm")],
                        rows: r.issues.iter().map(|i| trow(vec![i.document.number.clone(), i.document.contact_name.clone(), i.message.clone(), fmt_money(i.document.amount)], "normal")).collect(),
                    });
                }
                job = data::report_job(
                    c,
                    "Zusammenfassende Meldung",
                    &format!("{} · fällig am {}", r.period_label, fmt_date(r.due_date)),
                    false,
                    sections,
                    vec![("Innergemeinschaftliche Lieferungen".into(), format!("{} €", fmt_num(Decimal::from(r.total_deliveries), 0))), ("Sonstige Leistungen".into(), format!("{} €", fmt_num(Decimal::from(r.total_services), 0)))],
                    vec!["Dieser Ausdruck dient der Kontrolle. Die Übermittlung erfolgt über ELSTER.".into()],
                )?;
                name = format!("ZM_{y}_{period}.pdf");
            }
            "open-items" => {
                let oi = reports::open_items(c)?;
                let rows: Vec<_> = oi
                    .receivables
                    .iter()
                    .map(|d| trow(vec![d.number.clone(), d.contact_name.clone(), fmt_date(d.date), d.due_date.map(fmt_date).unwrap_or_default(), if d.days_overdue > 0 { d.days_overdue.to_string() } else { String::new() }, fmt_money(d.gross_total), fmt_money(d.open_amount)], "normal"))
                    .collect();
                let prow: Vec<_> = oi
                    .payables
                    .iter()
                    .map(|r| trow(vec![if r.receipt.invoice_number.is_empty() { r.receipt.number.clone() } else { r.receipt.invoice_number.clone() }, r.party.clone(), fmt_date(r.receipt.date), r.receipt.due_date.map(fmt_date).unwrap_or_default(), String::new(), fmt_money(r.receipt.gross_total), fmt_money(r.open_amount)], "normal"))
                    .collect();
                let cols = vec![col("Nummer", "left", "auto"), col("Name", "left", "1fr"), col("Datum", "left", "auto"), col("Fällig", "left", "auto"), col("Tage überf.", "right", "auto"), col("Betrag", "right", "auto"), col("Offen", "right", "auto")];
                job = data::report_job(
                    c,
                    "Offene Posten",
                    &format!("Stand {}", fmt_date(today())),
                    true,
                    vec![ReportSection { title: "Forderungen".into(), columns: cols.clone(), rows }, ReportSection { title: "Verbindlichkeiten".into(), columns: cols, rows: prow }],
                    vec![("Offene Forderungen".into(), fmt_money(oi.receivables_total)), ("Offene Verbindlichkeiten".into(), fmt_money(oi.payables_total))],
                    vec![],
                )?;
                name = "Offene_Posten.pdf".into();
            }
            "assets" => {
                let list = assets::list(c)?;
                let mut rows: Vec<_> = list
                    .iter()
                    .map(|a| {
                        trow(
                            vec![
                                a.asset.number.clone(),
                                a.asset.name.clone(),
                                a.asset.account.clone(),
                                fmt_date(a.asset.acquisition_date),
                                fmt_money(a.asset.cost),
                                match a.asset.method.as_str() {
                                    "gwg" => "GWG".into(),
                                    "pool" => "Sammelposten".into(),
                                    "none" => "keine".into(),
                                    _ => format!("linear {} J.", a.asset.useful_life_years),
                                },
                                fmt_money(a.afa_current_year),
                                fmt_money(a.book_value),
                            ],
                            "normal",
                        )
                    })
                    .collect();
                let afa_total: Decimal = list.iter().map(|a| a.afa_current_year).sum();
                let book_total: Decimal = list.iter().map(|a| a.book_value).sum();
                if !list.is_empty() {
                    rows.push(trow(
                        vec![
                            "Summe".into(),
                            String::new(),
                            String::new(),
                            String::new(),
                            fmt_money(list.iter().map(|a| a.asset.cost).sum()),
                            String::new(),
                            fmt_money(afa_total),
                            fmt_money(book_total),
                        ],
                        "total",
                    ));
                }
                job = data::report_job(
                    c,
                    "Anlagenverzeichnis",
                    &format!("Stand {} · AfA {}", fmt_date(today()), today().year()),
                    true,
                    vec![ReportSection {
                        title: String::new(),
                        columns: vec![col("Inv.-Nr.", "left", "auto"), col("Bezeichnung", "left", "1fr"), col("Konto", "left", "auto"), col("Anschaffung", "left", "auto"), col("AK netto", "right", "auto"), col("Methode", "left", "auto"), col("AfA lfd. Jahr", "right", "auto"), col("Buchwert", "right", "auto")],
                        rows,
                    }],
                    vec![("Anlagegüter".into(), list.len().to_string()), (format!("AfA {}", today().year()), fmt_money(afa_total)), ("Buchwert gesamt".into(), fmt_money(book_total))],
                    vec![],
                )?;
                name = "Anlagenverzeichnis.pdf".into();
            }
            _ => return Err(crate::error::NotFound("Unbekannter Bericht".into()).into()),
        }
        Ok((job, name))
    })?;
    let bytes = render_pdf(&st, job, false).await?;
    Ok(file_response(bytes, "application/pdf", &safe_filename(&name), true))
}

#[derive(Deserialize)]
pub struct DatevQ {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    #[serde(default)]
    advisor: String,
    #[serde(default)]
    client: String,
}

pub async fn datev(State(st): State<AppState>, Query(q): Query<DatevQ>) -> Result<Response, AppError> {
    let y = today().year();
    let from = q.from.unwrap_or(ymd(y, 1, 1));
    let to = q.to.unwrap_or(ymd(y, 12, 31));
    let bytes = st.read(|c| datev::export(c, from, to, &q.advisor, &q.client))?;
    Ok(file_response(bytes, "text/csv; charset=windows-1252", &format!("EXTF_Buchungsstapel_{from}_{to}.csv"), true))
}

/// Datenträgerüberlassung for tax audits (GoBD) of one calendar year.
pub async fn gdpdu_export(State(st): State<AppState>, Query(q): Query<YearQ>) -> Result<Response, AppError> {
    let y = year(&q);
    let bytes = st.read(|c| gdpdu::export(c, y))?;
    Ok(file_response(bytes, "application/zip", &format!("GoBD-Datenexport_{y}.zip"), true))
}

pub async fn receipts_zip(State(st): State<AppState>, Query(q): Query<DatevQ>) -> Result<Response, AppError> {
    let y = today().year();
    let from = q.from.unwrap_or(ymd(y, 1, 1));
    let to = q.to.unwrap_or(ymd(y, 12, 31));
    let files: Vec<(Receipt, FileMeta)> = st.read(|c| {
        let mut out = Vec::new();
        for r in db::query::<Receipt>(c, "WHERE date >= ?1 AND date <= ?2 AND file_id IS NOT NULL ORDER BY date", &[v(from), v(to)])? {
            if let Some(f) = db::find::<FileMeta>(c, r.file_id.unwrap_or(0))? {
                out.push((r, f));
            }
        }
        Ok(out)
    })?;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        for (r, f) in &files {
            if let Ok(bytes) = std::fs::read(st.files_dir().join(&f.uuid)) {
                let ext = std::path::Path::new(&f.filename).extension().and_then(|e| e.to_str()).unwrap_or("pdf");
                let name = safe_filename(&format!("{}_{}_{}.{ext}", r.date, r.number, r.supplier_name));
                let opts = match zip::DateTime::from_date_and_time(r.date.year() as u16, r.date.month() as u8, r.date.day() as u8, 12, 0, 0) {
                    Ok(t) => opts.last_modified_time(t),
                    Err(_) => opts,
                };
                zip.start_file(name, opts).map_err(AppError::from)?;
                zip.write_all(&bytes).map_err(AppError::from)?;
            }
        }
        zip.finish().map_err(AppError::from)?;
    }
    Ok(file_response(buf.into_inner(), "application/zip", &format!("Belege_{from}_{to}.zip"), true))
}
