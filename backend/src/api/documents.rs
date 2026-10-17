// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{IdResponse, MessageResponse, Pages, file_response, render_pdf, render_pdf_and_svgs, render_svgs, safe_filename};
use crate::db;
use crate::error::{ApiResult, AppError};
use crate::models::{Document, Dunning, FileMeta};
use crate::pdf::data;
use crate::services::archive;
use crate::services::calc::{self, Totals};
use crate::services::documents::{self as docs, DocumentFilter, DocumentFull, DocumentInput, DocumentListItem, PartialRequest};
use crate::services::dunning::{self, DunningListItem, DunningRequest, DunningSuggestion};
use crate::services::einvoice::{self, EinvoiceCheck};
use crate::services::finance::{self, PaymentRequest};
use crate::state::AppState;
use crate::util::{now_str, today};

pub async fn list(State(st): State<AppState>, Query(f): Query<DocumentFilter>) -> ApiResult<Vec<DocumentListItem>> {
    st.read(|c| docs::list(c, &f)).map(Json)
}

#[derive(Deserialize)]
pub struct TypeQ {
    #[serde(default)]
    doc_type: String,
    contact_id: Option<i64>,
}

pub async fn counts(State(st): State<AppState>, Query(q): Query<TypeQ>) -> ApiResult<BTreeMap<String, i64>> {
    st.read(|c| {
        let items = docs::list(c, &DocumentFilter { doc_type: q.doc_type.clone(), ..Default::default() })?;
        let mut m = BTreeMap::new();
        m.insert("all".to_string(), items.len() as i64);
        for i in &items {
            *m.entry(i.status.clone()).or_insert(0) += 1;
            if i.days_overdue > 0 {
                *m.entry("overdue".into()).or_insert(0) += 1;
            }
            if i.open_amount > Decimal::ZERO {
                *m.entry("unpaid".into()).or_insert(0) += 1;
            }
        }
        Ok(m)
    })
    .map(Json)
}

pub async fn new_template(State(st): State<AppState>, Query(q): Query<TypeQ>) -> ApiResult<DocumentInput> {
    st.read(|c| docs::new_template(c, if q.doc_type.is_empty() { "invoice" } else { &q.doc_type }, q.contact_id)).map(Json)
}

pub async fn create(State(st): State<AppState>, Json(input): Json<DocumentInput>) -> ApiResult<DocumentFull> {
    st.write(|c| {
        let id = docs::create(c, input)?;
        docs::get_full(c, id)
    })
    .map(Json)
}

pub async fn get(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<DocumentFull> {
    st.read(|c| docs::get_full(c, id)).map(Json)
}

pub async fn update(State(st): State<AppState>, Path(id): Path<i64>, Json(input): Json<DocumentInput>) -> ApiResult<DocumentFull> {
    st.write(|c| {
        docs::update(c, id, input)?;
        docs::get_full(c, id)
    })
    .map(Json)
}

pub async fn delete(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| docs::delete(c, id)).map(|_| Json(MessageResponse { message: "Dokument gelöscht".into() }))
}

/// Archives an issued document right away; a failure is retried on its first download or view.
async fn archive_now(st: &AppState, id: i64) {
    let doc = match st.read(|c| db::get::<Document>(c, id)) {
        Ok(d) if archive::is_issued(&d) => d,
        _ => return,
    };
    if let Err(e) = ensure_issued(st, &doc).await {
        tracing::warn!("{} konnte nicht archiviert werden: {}", doc.number, e.message);
    }
}

pub async fn finalize(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<DocumentFull> {
    st.write(|c| docs::finalize(c, id))?;
    archive_now(&st, id).await;
    st.read(|c| docs::get_full(c, id)).map(Json)
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct StatusRequest {
    pub status: String,
}

pub async fn set_status(State(st): State<AppState>, Path(id): Path<i64>, Json(r): Json<StatusRequest>) -> ApiResult<DocumentFull> {
    st.write(|c| {
        docs::set_status(c, id, &r.status)?;
        docs::get_full(c, id)
    })
    .map(Json)
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct SentRequest {
    pub via: String,
}

pub async fn mark_sent(State(st): State<AppState>, Path(id): Path<i64>, Json(r): Json<SentRequest>) -> ApiResult<DocumentFull> {
    st.write(|c| {
        docs::mark_sent(c, id, if r.via.is_empty() { "E-Mail" } else { &r.via })?;
        docs::get_full(c, id)
    })
    .map(Json)
}

pub async fn copy(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<IdResponse> {
    st.write(|c| docs::copy(c, id)).map(|id| Json(IdResponse { id }))
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct ConvertRequest {
    pub target: String,
}

pub async fn convert(State(st): State<AppState>, Path(id): Path<i64>, Json(r): Json<ConvertRequest>) -> ApiResult<IdResponse> {
    st.write(|c| docs::convert(c, id, &r.target)).map(|id| Json(IdResponse { id }))
}

pub async fn cancel(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<IdResponse> {
    let storno = st.write(|c| docs::cancel(c, id))?;
    archive_now(&st, storno).await;
    Ok(Json(IdResponse { id: storno }))
}

pub async fn credit_note(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<IdResponse> {
    st.write(|c| docs::credit_note_from(c, id)).map(|id| Json(IdResponse { id }))
}

pub async fn partial(State(st): State<AppState>, Path(id): Path<i64>, Json(r): Json<PartialRequest>) -> ApiResult<IdResponse> {
    st.write(|c| docs::create_partial(c, id, r)).map(|id| Json(IdResponse { id }))
}

pub async fn final_invoice(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<IdResponse> {
    st.write(|c| docs::create_final(c, id)).map(|id| Json(IdResponse { id }))
}

pub async fn pay(State(st): State<AppState>, Path(id): Path<i64>, Json(r): Json<PaymentRequest>) -> ApiResult<DocumentFull> {
    st.write(|c| {
        finance::pay_document(c, id, r)?;
        docs::get_full(c, id)
    })
    .map(Json)
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct WriteoffRequest {
    pub reason: String,
    #[ts(type = "number")]
    pub amount: Decimal,
    pub date: Option<NaiveDate>,
}

pub async fn writeoff(State(st): State<AppState>, Path(id): Path<i64>, Json(r): Json<WriteoffRequest>) -> ApiResult<DocumentFull> {
    st.write(|c| {
        let doc: Document = db::get(c, id)?;
        let amount = if r.amount.is_zero() { doc.open_amount() } else { r.amount };
        if amount <= Decimal::ZERO || amount > doc.open_amount() {
            crate::bail_user!("Ungültiger Betrag für die Ausbuchung.");
        }
        finance::add_writeoff(c, id, &r.reason, amount, r.date.unwrap_or_else(today))?;
        docs::get_full(c, id)
    })
    .map(Json)
}

fn pdf_name(doc: &Document) -> String {
    let label = docs::type_label(&doc.doc_type, &doc.kind);
    let number = if doc.number.is_empty() { "Entwurf".to_string() } else { doc.number.clone() };
    safe_filename(&format!("{label}_{number}.pdf"))
}

#[derive(Deserialize)]
pub struct PdfQ {
    #[serde(default)]
    download: bool,
}

/// ZUGFeRD PDF/A-3 (with embedded factur-x.xml) and its pages, rendered from current data.
async fn zugferd_render(st: &AppState, doc: &Document) -> Result<(Vec<u8>, Vec<String>), AppError> {
    let job = st.read(|c| {
        let check = einvoice::check(c, doc, false)?;
        if !check.errors.is_empty() {
            return Err(crate::error::UserError(format!("ZUGFeRD nicht möglich: {}", check.errors.join(" "))).into());
        }
        let xml = einvoice::generate_cii(c, doc, einvoice::GUIDELINE_EN16931)?;
        let lines = docs::lines_of(c, doc.id)?;
        let mut job = data::document_job(c, &st.data_dir, doc, &lines, None)?;
        job.attach("factur-x.xml", xml.into_bytes());
        Ok(job)
    })?;
    let (pdf, pages) = render_pdf_and_svgs(st, job, true).await?;
    Ok((einvoice::add_facturx_xmp(&pdf).map_err(AppError::from)?, pages))
}

/// Renders an issued document as it is archived: the ZUGFeRD PDF if that is its format (a plain
/// PDF if the e-invoice data is incomplete), otherwise a plain PDF. Returns (pdf, pages, zugferd).
async fn render_issued(st: &AppState, doc: &Document) -> Result<(Vec<u8>, Vec<String>, bool), AppError> {
    if doc.einvoice_format == "zugferd" {
        match zugferd_render(st, doc).await {
            Ok((pdf, pages)) => return Ok((pdf, pages, true)),
            Err(e) => tracing::warn!("{}: ZUGFeRD nicht möglich, archiviere PDF ohne E-Rechnung ({})", doc.number, e.message),
        }
    }
    let job = st.read(|c| data::document_job(c, &st.data_dir, doc, &docs::lines_of(c, doc.id)?, None))?;
    let (pdf, pages) = render_pdf_and_svgs(st, job, false).await?;
    Ok((pdf, pages, false))
}

/// An archived file of an issued document.
fn archived(st: &AppState, id: i64, kind: &str) -> Result<Option<(FileMeta, Vec<u8>)>, AppError> {
    let Some(meta) = st.read(|c| archive::find(c, id, kind))? else { return Ok(None) };
    match std::fs::read(st.files_dir().join(&meta.uuid)) {
        Ok(bytes) => Ok(Some((meta, bytes))),
        Err(_) => Err(AppError::not_found(format!("Die archivierte Datei „{}“ fehlt im Datenordner.", meta.filename))),
    }
}

fn archived_required(st: &AppState, id: i64, kind: &str) -> Result<(FileMeta, Vec<u8>), AppError> {
    archived(st, id, kind)?.ok_or_else(|| AppError::not_found("Archivierte Datei nicht gefunden"))
}

/// Stores rendered files (each under one or more archive kinds). A kind archived in the meantime
/// by another request keeps its first version; files that end up unused are removed again.
fn archive_files(st: &AppState, id: i64, files: Vec<(Vec<&'static str>, String, &'static str, Vec<u8>)>) -> Result<(), AppError> {
    std::fs::create_dir_all(st.files_dir()).map_err(AppError::from)?;
    let mut metas = Vec::new();
    for (kinds, filename, mime, bytes) in files {
        let uuid = uuid::Uuid::new_v4().to_string();
        std::fs::write(st.files_dir().join(&uuid), &bytes).map_err(AppError::from)?;
        metas.push((kinds, FileMeta { id: 0, uuid, filename, mime: mime.into(), size: bytes.len() as i64, created_at: now_str() }));
    }
    let result = st.write(|c| {
        let mut used = Vec::new();
        for (kinds, meta) in metas.iter_mut() {
            let mut any = false;
            for kind in kinds.iter() {
                any |= archive::insert(c, id, kind, meta)?;
            }
            used.push(any);
        }
        Ok(used)
    });
    let used = match &result {
        Ok(used) => used.clone(),
        Err(_) => vec![false; metas.len()],
    };
    for ((_, meta), used) in metas.iter().zip(used) {
        if !used {
            let _ = std::fs::remove_file(st.files_dir().join(&meta.uuid));
        }
    }
    result.map(|_| ())
}

/// Makes sure PDF and preview pages of an issued document are archived (rendered on first use;
/// documents issued before archiving existed are archived on their first download or view).
async fn ensure_issued(st: &AppState, doc: &Document) -> Result<(), AppError> {
    let done = st.read(|c| Ok(archive::find(c, doc.id, archive::PDF)?.is_some() && archive::find(c, doc.id, archive::PAGES)?.is_some()))?;
    if done {
        return Ok(());
    }
    let (pdf, pages, zugferd) = render_issued(st, doc).await?;
    let pdf_kinds = if zugferd { vec![archive::PDF, archive::ZUGFERD] } else { vec![archive::PDF] };
    let pages = serde_json::to_vec(&pages).map_err(AppError::from)?;
    archive_files(
        st,
        doc.id,
        vec![
            (pdf_kinds, pdf_name(doc), "application/pdf", pdf),
            (vec![archive::PAGES], pdf_name(doc).replace(".pdf", "_Vorschau.json"), "application/json", pages),
        ],
    )
}

pub async fn pdf(State(st): State<AppState>, Path(id): Path<i64>, Query(q): Query<PdfQ>) -> Result<Response, AppError> {
    let doc: Document = st.read(|c| db::get(c, id))?;
    let (bytes, message) = if archive::is_issued(&doc) {
        ensure_issued(&st, &doc).await?;
        let (meta, bytes) = archived_required(&st, id, archive::PDF)?;
        let zugferd = archived(&st, id, archive::ZUGFERD)?.is_some_and(|(z, _)| z.id == meta.id);
        (bytes, if zugferd { "PDF (ZUGFeRD) heruntergeladen" } else { "PDF heruntergeladen" })
    } else {
        let job = st.read(|c| data::document_job(c, &st.data_dir, &doc, &docs::lines_of(c, id)?, None))?;
        (render_pdf(&st, job, false).await?, "PDF heruntergeladen")
    };
    if q.download {
        let _ = st.write(|c| crate::services::settings::log(c, "document", id, "download", message));
    }
    Ok(file_response(bytes, "application/pdf", &pdf_name(&doc), q.download))
}

pub async fn zugferd(State(st): State<AppState>, Path(id): Path<i64>) -> Result<Response, AppError> {
    let doc: Document = st.read(|c| db::get(c, id))?;
    let name = pdf_name(&doc).replace(".pdf", "_ZUGFeRD.pdf");
    let bytes = if archive::is_issued(&doc) {
        // Archives the ZUGFeRD PDF along with the main PDF when it is the document's format.
        ensure_issued(&st, &doc).await?;
        match archived(&st, id, archive::ZUGFERD)? {
            Some((_, bytes)) => bytes,
            None => {
                let (pdf, _) = zugferd_render(&st, &doc).await?;
                archive_files(&st, id, vec![(vec![archive::ZUGFERD], name.clone(), "application/pdf", pdf)])?;
                archived_required(&st, id, archive::ZUGFERD)?.1
            }
        }
    } else {
        zugferd_render(&st, &doc).await?.0
    };
    let _ = st.write(|c| crate::services::settings::log(c, "document", id, "einvoice", "ZUGFeRD-Rechnung heruntergeladen"));
    Ok(file_response(bytes, "application/pdf", &name, true))
}

pub async fn xrechnung(State(st): State<AppState>, Path(id): Path<i64>) -> Result<Response, AppError> {
    let doc: Document = st.read(|c| db::get(c, id))?;
    let xml = xrechnung_bytes(&st, &doc)?;
    let _ = st.write(|c| crate::services::settings::log(c, "document", id, "einvoice", "XRechnung (CII) heruntergeladen"));
    Ok(file_response(xml, "application/xml", &xrechnung_name(&doc), true))
}

fn xrechnung_name(doc: &Document) -> String {
    safe_filename(&format!("XRechnung_{}.xml", doc.number))
}

/// XRechnung of a document – archived for issued invoices.
fn xrechnung_bytes(st: &AppState, doc: &Document) -> Result<Vec<u8>, AppError> {
    let id = doc.id;
    let name = xrechnung_name(doc);
    let issued = archive::is_issued(doc);
    let existing = if issued { archived(st, id, archive::XRECHNUNG)? } else { None };
    let xml = match existing {
        Some((_, bytes)) => bytes,
        None => {
            let xml = st.read(|c| {
                let check = einvoice::check(c, doc, true)?;
                if !check.errors.is_empty() {
                    return Err(crate::error::UserError(format!("XRechnung nicht möglich: {}", check.errors.join(" "))).into());
                }
                einvoice::generate_cii(c, doc, einvoice::GUIDELINE_XRECHNUNG)
            })?;
            if issued {
                archive_files(st, id, vec![(vec![archive::XRECHNUNG], name.clone(), "application/xml", xml.into_bytes())])?;
                archived_required(st, id, archive::XRECHNUNG)?.1
            } else {
                xml.into_bytes()
            }
        }
    };
    Ok(xml)
}

/// PDF of a document: archived for issued invoices, rendered for everything else.
async fn document_pdf(st: &AppState, doc: &Document) -> Result<Vec<u8>, AppError> {
    if archive::is_issued(doc) {
        ensure_issued(st, doc).await?;
        return Ok(archived_required(st, doc.id, archive::PDF)?.1);
    }
    let job = st.read(|c| data::document_job(c, &st.data_dir, doc, &docs::lines_of(c, doc.id)?, None))?;
    render_pdf(st, job, false).await
}

// ---------------------------------------------------------------- E-Mail

#[derive(Serialize, TS)]
#[ts(export)]
pub struct EmailDraft {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<String>,
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct EmailRequest {
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
}

/// Recipient, texts (from the templates) and attachments for sending a document.
fn email_draft_of(c: &rusqlite::Connection, doc: &Document) -> anyhow::Result<EmailDraft> {
    let s = crate::services::settings::load(c)?;
    let contact = doc.contact_id.and_then(|id| db::find::<crate::models::Contact>(c, id).ok().flatten());
    let mut vars = docs::placeholders(&s, doc, contact.as_ref());
    vars.push(("Dokument", docs::type_label(&doc.doc_type, &doc.kind).to_string()));
    let mut attachments = vec![pdf_name(doc)];
    if doc.is_invoice_like() && doc.einvoice_format == "xrechnung" {
        attachments.push(xrechnung_name(doc));
    }
    Ok(EmailDraft {
        to: contact.map(|k| k.email).unwrap_or_default(),
        subject: crate::util::fill_placeholders(&s.email.subject, &vars),
        body: crate::util::fill_placeholders(&s.email.body, &vars),
        attachments,
    })
}

pub async fn email_draft(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<EmailDraft> {
    st.read(|c| email_draft_of(c, &db::get(c, id)?)).map(Json)
}

/// Sends the document as PDF (plus XRechnung where that is its format) and marks it as sent.
pub async fn send_email(State(st): State<AppState>, Path(id): Path<i64>, Json(r): Json<EmailRequest>) -> ApiResult<DocumentFull> {
    let (doc, s) = st.read(|c| Ok((db::get::<Document>(c, id)?, crate::services::settings::load(c)?)))?;
    if doc.status == "draft" {
        return Err(AppError::bad("Bitte das Dokument zuerst abschließen."));
    }
    let mut attachments = vec![(pdf_name(&doc), "application/pdf".to_string(), document_pdf(&st, &doc).await?)];
    if doc.is_invoice_like() && doc.einvoice_format == "xrechnung" {
        attachments.push((xrechnung_name(&doc), "application/xml".to_string(), xrechnung_bytes(&st, &doc)?));
    }
    let msg = crate::services::email::Outgoing {
        to: crate::services::email::addresses(&r.to),
        cc: crate::services::email::addresses(&r.cc),
        subject: r.subject,
        body: r.body,
        attachments,
    };
    let to = msg.to.join(", ");
    tokio::task::spawn_blocking(move || crate::services::email::send(&s.email, &msg)).await.map_err(|e| AppError::from(anyhow::anyhow!(e)))??;
    st.write(|c| {
        docs::mark_sent(c, id, &format!("E-Mail an {to}"))?;
        docs::get_full(c, id)
    })
    .map(Json)
}

#[derive(Deserialize)]
pub struct CheckQ {
    #[serde(default)]
    format: String,
}

pub async fn einvoice_check(State(st): State<AppState>, Path(id): Path<i64>, Query(q): Query<CheckQ>) -> ApiResult<EinvoiceCheck> {
    st.read(|c| {
        let doc: Document = db::get(c, id)?;
        einvoice::check(c, &doc, q.format != "zugferd")
    })
    .map(Json)
}

pub async fn preview(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Pages> {
    let doc: Document = st.read(|c| db::get(c, id))?;
    if archive::is_issued(&doc) {
        ensure_issued(&st, &doc).await?;
        let (_, raw) = archived_required(&st, id, archive::PAGES)?;
        return Ok(Json(Pages { pages: serde_json::from_slice(&raw).map_err(AppError::from)? }));
    }
    let job = st.read(|c| data::document_job(c, &st.data_dir, &doc, &docs::lines_of(c, id)?, None))?;
    Ok(Json(Pages { pages: render_svgs(&st, job).await? }))
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct PreviewResult {
    pub pages: Vec<String>,
    pub totals: Totals,
}

/// Live preview of an unsaved document (editor).
pub async fn preview_input(State(st): State<AppState>, Json(input): Json<DocumentInput>) -> ApiResult<PreviewResult> {
    let (job, totals) = st.read(|c| {
        let mut doc = input.doc.clone();
        let mut lines = input.lines.clone();
        if doc.status.is_empty() {
            doc.status = "draft".into();
        }
        if doc.id > 0
            && let Some(old) = db::find::<Document>(c, doc.id)?
        {
            doc.number = old.number;
            doc.status = old.status;
        }
        let s = crate::services::settings::load(c)?;
        if s.tax.small_business {
            doc.tax_mode = "small_business".into();
        }
        let totals = calc::calculate(&doc, &mut lines);
        calc::apply_totals(&mut doc, &totals);
        Ok((data::document_job(c, &st.data_dir, &doc, &lines, None)?, totals))
    })?;
    Ok(Json(PreviewResult { pages: render_svgs(&st, job).await?, totals }))
}

// ---------------------------------------------------------------- Abo-Rechnungen

pub async fn recurring(State(st): State<AppState>) -> ApiResult<Vec<crate::services::recurring::RecurringView>> {
    st.read(crate::services::recurring::list).map(Json)
}

/// Saves an Abo-Rechnung; periods already due are created right away.
pub async fn save_recurring(State(st): State<AppState>, id: Option<Path<i64>>, Json(mut r): Json<crate::models::RecurringInvoice>) -> ApiResult<Vec<crate::services::recurring::RecurringView>> {
    r.id = id.map(|Path(i)| i).unwrap_or(0);
    st.write(|c| {
        crate::services::recurring::save(c, r)?;
        crate::services::recurring::create_due(c, today())?;
        crate::services::recurring::list(c)
    })
    .map(Json)
}

pub async fn delete_recurring(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| crate::services::recurring::delete(c, id)).map(|_| Json(MessageResponse { message: "Abo-Rechnung gelöscht".into() }))
}

// ---------------------------------------------------------------- Mahnwesen

pub async fn dunnings(State(st): State<AppState>) -> ApiResult<Vec<DunningListItem>> {
    st.read(dunning::list).map(Json)
}

#[derive(Deserialize)]
pub struct SuggestQ {
    #[serde(default)]
    all: bool,
}

pub async fn dunning_suggestions(State(st): State<AppState>, Query(q): Query<SuggestQ>) -> ApiResult<Vec<DunningSuggestion>> {
    st.read(|c| if q.all { dunning::suggestions_all(c) } else { dunning::suggestions(c) }).map(Json)
}

pub async fn create_dunning(State(st): State<AppState>, Json(r): Json<DunningRequest>) -> ApiResult<Dunning> {
    st.write(|c| dunning::create(c, r)).map(Json)
}

pub async fn dunning_preview(State(st): State<AppState>, Json(r): Json<DunningRequest>) -> ApiResult<Pages> {
    let job = st.read(|c| {
        let (dn, _, _) = dunning::build(c, &r)?;
        data::dunning_job(c, &st.data_dir, &dn)
    })?;
    Ok(Json(Pages { pages: render_svgs(&st, job).await? }))
}

pub async fn delete_dunning(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| dunning::delete(c, id)).map(|_| Json(MessageResponse { message: "Mahnung gelöscht".into() }))
}

pub async fn dunning_pdf(State(st): State<AppState>, Path(id): Path<i64>, Query(q): Query<PdfQ>) -> Result<Response, AppError> {
    let (job, name) = st.read(|c| {
        let dn: Dunning = db::get(c, id)?;
        let doc: Document = db::get(c, dn.document_id)?;
        Ok((data::dunning_job(c, &st.data_dir, &dn)?, safe_filename(&format!("{}_{}.pdf", dn.title, doc.number))))
    })?;
    let bytes = render_pdf(&st, job, false).await?;
    Ok(file_response(bytes, "application/pdf", &name, q.download))
}
