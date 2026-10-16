// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

use axum::Json;
use axum::extract::{Multipart, Path, Query, State};
use axum::response::Response;
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::system::save_file;
use super::{MessageResponse, file_response, render_pdf};
use crate::db;
use crate::error::{ApiResult, AppError};
use crate::models::{PaymentAccount, Transaction};
use crate::pdf::data::{self, col, trow};
use crate::services::bankimport::{self, CsvMapping, ImportPreview, ImportSummary, ImportedTx};
use crate::services::einvoice::{self, ImportResult};
use crate::services::finance::{self as fin, CashEntryInput, Cashbook, MatchSuggestion, PaymentAccountView, PaymentRequest, TransactionFilter, TransactionInput, TransactionView};
use crate::services::receipts::{self as rec, ReceiptFilter, ReceiptFull, ReceiptInput, ReceiptListItem};
use crate::state::AppState;
use crate::util::{fmt_date, fmt_money, today, ymd};

// ---------------------------------------------------------------- Belege

pub async fn receipts(State(st): State<AppState>, Query(f): Query<ReceiptFilter>) -> ApiResult<Vec<ReceiptListItem>> {
    st.read(|c| rec::list(c, &f)).map(Json)
}

pub async fn receipt(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<ReceiptFull> {
    let mut full = st.read(|c| rec::get_full(c, id))?;
    // Re-read attached e-invoices so their content can be displayed again.
    if let Some(file) = &full.file
        && full.receipt.source != "manual"
        && let Ok(bytes) = std::fs::read(st.files_dir().join(&file.uuid))
    {
        let xml = if bytes.starts_with(b"%PDF") { einvoice::extract_xml_from_pdf(&bytes) } else { Some(String::from_utf8_lossy(&bytes).to_string()) };
        full.einvoice = xml.and_then(|x| einvoice::parse_xml(&x).ok());
    }
    Ok(Json(full))
}

pub async fn create_receipt(State(st): State<AppState>, Json(input): Json<ReceiptInput>) -> ApiResult<ReceiptFull> {
    st.write(|c| {
        let id = rec::create(c, input)?;
        rec::get_full(c, id)
    })
    .map(Json)
}

pub async fn update_receipt(State(st): State<AppState>, Path(id): Path<i64>, Json(input): Json<ReceiptInput>) -> ApiResult<ReceiptFull> {
    st.write(|c| {
        rec::update(c, id, input)?;
        rec::get_full(c, id)
    })
    .map(Json)
}

pub async fn delete_receipt(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| rec::delete(c, id)).map(|_| Json(MessageResponse { message: "Beleg gelöscht".into() }))
}

pub async fn pay_receipt(State(st): State<AppState>, Path(id): Path<i64>, Json(r): Json<PaymentRequest>) -> ApiResult<ReceiptFull> {
    st.write(|c| {
        fin::pay_receipt(c, id, r)?;
        rec::get_full(c, id)
    })
    .map(Json)
}

#[derive(Deserialize)]
pub struct SuggestQ {
    contact_id: Option<i64>,
    #[serde(default)]
    name: String,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct AccountSuggestion {
    pub account: String,
    #[ts(type = "number")]
    pub tax_rate: rust_decimal::Decimal,
}

pub async fn suggest_account(State(st): State<AppState>, Query(q): Query<SuggestQ>) -> ApiResult<Option<AccountSuggestion>> {
    st.read(|c| Ok(rec::suggest_account(c, q.contact_id, &q.name)?.map(|(account, tax_rate)| AccountSuggestion { account, tax_rate }))).map(Json)
}

/// Upload of a receipt file. XML / ZUGFeRD-PDF files are parsed as e-invoices.
pub async fn import_einvoice(State(st): State<AppState>, mut mp: Multipart) -> ApiResult<ImportResult> {
    let mut file: Option<(String, String, Vec<u8>)> = None;
    while let Some(field) = mp.next_field().await.map_err(|e| AppError::bad(e.to_string()))? {
        if field.name() == Some("file") {
            let name = field.file_name().unwrap_or("beleg").to_string();
            let mime = field.content_type().map(|m| m.to_string()).unwrap_or_else(|| mime_guess::from_path(&name).first_or_octet_stream().to_string());
            let bytes = field.bytes().await.map_err(|e| AppError::bad(e.to_string()))?.to_vec();
            file = Some((name, mime, bytes));
        }
    }
    let (name, mime, bytes) = file.ok_or_else(|| AppError::bad("Keine Datei übermittelt"))?;
    let is_pdf = bytes.starts_with(b"%PDF");
    let text = String::from_utf8_lossy(&bytes);
    let is_xml = !is_pdf && text.trim_start_matches('\u{feff}').trim_start().starts_with('<');
    let (xml, source) = if is_pdf {
        (einvoice::extract_xml_from_pdf(&bytes), "zugferd")
    } else if is_xml {
        (Some(text.to_string()), "xrechnung")
    } else {
        (None, "manual")
    };
    let mime = if is_pdf { "application/pdf".to_string() } else if is_xml { "application/xml".to_string() } else { mime };
    let meta = save_file(&st, &name, &mime, &bytes)?;
    match xml {
        Some(xml) => {
            let parsed = einvoice::parse_xml(&xml).map_err(AppError::from)?;
            let (mut draft, contact_match, warnings) = st.read(|c| einvoice::to_receipt(c, &parsed, source))?;
            draft.receipt.file_id = Some(meta.id);
            Ok(Json(ImportResult { parsed, draft, contact_match, warnings, file_id: Some(meta.id), file: Some(meta) }))
        }
        None => {
            let draft = ReceiptInput {
                receipt: crate::models::Receipt { file_id: Some(meta.id), date: today(), receipt_type: "expense".into(), tax_mode: "standard".into(), prices_gross: true, ..Default::default() },
                lines: vec![],
                payment: None,
            };
            Ok(Json(ImportResult { parsed: Default::default(), draft, contact_match: None, warnings: vec![], file_id: Some(meta.id), file: Some(meta) }))
        }
    }
}

// ---------------------------------------------------------------- Zahlungskonten

pub async fn payment_accounts(State(st): State<AppState>) -> ApiResult<Vec<PaymentAccountView>> {
    st.read(fin::accounts).map(Json)
}

pub async fn create_payment_account(State(st): State<AppState>, Json(mut a): Json<PaymentAccount>) -> ApiResult<Vec<PaymentAccountView>> {
    a.id = 0;
    st.write(|c| {
        fin::save_account(c, a)?;
        fin::accounts(c)
    })
    .map(Json)
}

pub async fn update_payment_account(State(st): State<AppState>, Path(id): Path<i64>, Json(mut a): Json<PaymentAccount>) -> ApiResult<Vec<PaymentAccountView>> {
    a.id = id;
    st.write(|c| {
        fin::save_account(c, a)?;
        fin::accounts(c)
    })
    .map(Json)
}

pub async fn delete_payment_account(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Vec<PaymentAccountView>> {
    st.write(|c| {
        fin::delete_account(c, id)?;
        fin::accounts(c)
    })
    .map(Json)
}

// ---------------------------------------------------------------- Transaktionen

pub async fn transactions(State(st): State<AppState>, Query(f): Query<TransactionFilter>) -> ApiResult<Vec<TransactionView>> {
    st.read(|c| fin::list(c, &f)).map(Json)
}

pub async fn create_transaction(State(st): State<AppState>, Json(input): Json<TransactionInput>) -> ApiResult<TransactionView> {
    st.write(|c| {
        let id = fin::create(c, input)?;
        fin::view(c, db::get::<Transaction>(c, id)?)
    })
    .map(Json)
}

pub async fn update_transaction(State(st): State<AppState>, Path(id): Path<i64>, Json(input): Json<TransactionInput>) -> ApiResult<TransactionView> {
    st.write(|c| {
        fin::update(c, id, input)?;
        fin::view(c, db::get::<Transaction>(c, id)?)
    })
    .map(Json)
}

pub async fn delete_transaction(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| fin::delete(c, id)).map(|_| Json(MessageResponse { message: "Buchung gelöscht".into() }))
}

pub async fn suggestions(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Vec<MatchSuggestion>> {
    st.read(|c| fin::suggestions(c, id)).map(Json)
}

pub async fn delete_allocation(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| fin::delete_allocation(c, id)).map(|_| Json(MessageResponse { message: "Zuordnung entfernt".into() }))
}

pub async fn import_preview(State(st): State<AppState>, mut mp: Multipart) -> ApiResult<ImportPreview> {
    let mut bytes = None;
    let mut account_id = 0i64;
    let mut mapping: Option<CsvMapping> = None;
    while let Some(field) = mp.next_field().await.map_err(|e| AppError::bad(e.to_string()))? {
        match field.name() {
            Some("file") => bytes = Some(field.bytes().await.map_err(|e| AppError::bad(e.to_string()))?.to_vec()),
            Some("account_id") => account_id = field.text().await.unwrap_or_default().parse().unwrap_or(0),
            Some("mapping") => {
                let t = field.text().await.unwrap_or_default();
                if !t.trim().is_empty() && t != "null" {
                    mapping = serde_json::from_str(&t).ok();
                }
            }
            _ => {}
        }
    }
    let bytes = bytes.ok_or_else(|| AppError::bad("Keine Datei übermittelt"))?;
    st.read(|c| bankimport::preview(c, account_id, &bytes, mapping)).map(Json)
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct ImportRequest {
    pub account_id: i64,
    pub transactions: Vec<ImportedTx>,
    pub auto_match: bool,
}

pub async fn import_transactions(State(st): State<AppState>, Json(r): Json<ImportRequest>) -> ApiResult<ImportSummary> {
    st.write(|c| bankimport::import(c, r.account_id, r.transactions.into_iter().filter(|t| !t.duplicate).collect(), r.auto_match)).map(Json)
}

// ---------------------------------------------------------------- Kassenbuch

#[derive(Deserialize)]
pub struct CashQ {
    account_id: Option<i64>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

fn cash_account(c: &rusqlite::Connection, id: Option<i64>) -> anyhow::Result<i64> {
    match id {
        Some(i) => Ok(i),
        None => c
            .query_row("SELECT id FROM payment_accounts WHERE kind = 'cash' ORDER BY archived, id LIMIT 1", [], |r| r.get(0))
            .map_err(|_| crate::error::UserError("Es ist noch keine Kasse angelegt.".into()).into()),
    }
}

pub async fn cashbook(State(st): State<AppState>, Query(q): Query<CashQ>) -> ApiResult<Cashbook> {
    st.read(|c| {
        let id = cash_account(c, q.account_id)?;
        let t = today();
        fin::cashbook(c, id, q.from.unwrap_or(ymd(t.year(), t.month(), 1)), q.to.unwrap_or(crate::util::last_day_of_month(t.year(), t.month())))
    })
    .map(Json)
}

pub async fn create_cash_entry(State(st): State<AppState>, Json(mut input): Json<CashEntryInput>) -> ApiResult<TransactionView> {
    st.write(|c| {
        input.account_id = cash_account(c, if input.account_id > 0 { Some(input.account_id) } else { None })?;
        let id = fin::create_cash_entry(c, input)?;
        fin::view(c, db::get::<Transaction>(c, id)?)
    })
    .map(Json)
}

pub async fn reverse_cash_entry(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<TransactionView> {
    st.write(|c| {
        let nid = fin::reverse_cash_entry(c, id)?;
        fin::view(c, db::get::<Transaction>(c, nid)?)
    })
    .map(Json)
}

pub async fn cashbook_pdf(State(st): State<AppState>, Query(q): Query<CashQ>) -> Result<Response, AppError> {
    let (job, name) = st.read(|c| {
        let id = cash_account(c, q.account_id)?;
        let t = today();
        let from = q.from.unwrap_or(ymd(t.year(), t.month(), 1));
        let to = q.to.unwrap_or(crate::util::last_day_of_month(t.year(), t.month()));
        let cb = fin::cashbook(c, id, from, to)?;
        let mut rows = vec![trow(vec!["".into(), fmt_date(from), "Anfangsbestand".into(), "".into(), "".into(), "".into(), fmt_money(cb.opening)], "muted")];
        for r in &cb.rows {
            let cat = r.tx.allocations.first().map(|a| a.label.clone()).unwrap_or_default();
            let (inc, out) = if r.tx.tx.amount > rust_decimal::Decimal::ZERO { (fmt_money(r.tx.tx.amount), String::new()) } else { (String::new(), fmt_money(-r.tx.tx.amount)) };
            rows.push(trow(vec![r.tx.tx.cash_number.map(|n| n.to_string()).unwrap_or_default(), fmt_date(r.tx.tx.date), r.tx.tx.purpose.clone(), cat, inc, out, fmt_money(r.balance)], if r.reversed { "muted" } else { "normal" }));
        }
        rows.push(trow(vec!["".into(), fmt_date(to), "Endbestand".into(), "".into(), fmt_money(cb.income), fmt_money(cb.expense), fmt_money(cb.closing)], "total"));
        let section = data::ReportSection {
            title: String::new(),
            columns: vec![col("Nr.", "left", "10mm"), col("Datum", "left", "18mm"), col("Buchungstext", "left", "1fr"), col("Kategorie", "left", "1fr"), col("Einnahme", "right", "auto"), col("Ausgabe", "right", "auto"), col("Bestand", "right", "auto")],
            rows,
        };
        let job = data::report_job(
            c,
            &format!("Kassenbuch {}", cb.account.name),
            &format!("{} bis {}", fmt_date(from), fmt_date(to)),
            false,
            vec![section],
            vec![("Anfangsbestand".into(), fmt_money(cb.opening)), ("Einnahmen".into(), fmt_money(cb.income)), ("Ausgaben".into(), fmt_money(cb.expense)), ("Endbestand".into(), fmt_money(cb.closing))],
            vec!["Kassenbuchungen werden fortlaufend nummeriert und können nur durch Stornobuchungen korrigiert werden.".into()],
        )?;
        Ok((job, format!("Kassenbuch_{}_{}.pdf", from, to)))
    })?;
    let bytes = render_pdf(&st, job, false).await?;
    Ok(file_response(bytes, "application/pdf", &name, true))
}
