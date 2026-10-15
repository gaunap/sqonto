use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{IdResponse, MessageResponse, Pages, file_response, render_pdf, render_svgs, safe_filename};
use crate::db;
use crate::error::{ApiResult, AppError};
use crate::models::{Document, Dunning};
use crate::pdf::data;
use crate::services::calc::{self, Totals};
use crate::services::documents::{self as docs, DocumentFilter, DocumentFull, DocumentInput, DocumentListItem, PartialRequest};
use crate::services::dunning::{self, DunningListItem, DunningRequest, DunningSuggestion};
use crate::services::einvoice::{self, EinvoiceCheck};
use crate::services::finance::{self, PaymentRequest};
use crate::state::AppState;
use crate::util::today;

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

pub async fn finalize(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<DocumentFull> {
    st.write(|c| {
        docs::finalize(c, id)?;
        docs::get_full(c, id)
    })
    .map(Json)
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
    st.write(|c| docs::cancel(c, id)).map(|id| Json(IdResponse { id }))
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

async fn zugferd_bytes(st: &AppState, id: i64) -> Result<(Vec<u8>, Document), AppError> {
    let (job, doc) = st.read(|c| {
        let doc: Document = db::get(c, id)?;
        let check = einvoice::check(c, &doc, false)?;
        if !check.errors.is_empty() {
            return Err(crate::error::UserError(format!("ZUGFeRD nicht möglich: {}", check.errors.join(" "))).into());
        }
        let xml = einvoice::generate_cii(c, &doc, einvoice::GUIDELINE_EN16931)?;
        let lines = docs::lines_of(c, id)?;
        let mut job = data::document_job(c, &st.data_dir, &doc, &lines, None)?;
        job.attach("factur-x.xml", xml.into_bytes());
        Ok((job, doc))
    })?;
    let pdf = render_pdf(st, job, true).await?;
    let pdf = einvoice::add_facturx_xmp(&pdf).map_err(AppError::from)?;
    Ok((pdf, doc))
}

pub async fn pdf(State(st): State<AppState>, Path(id): Path<i64>, Query(q): Query<PdfQ>) -> Result<Response, AppError> {
    let doc: Document = st.read(|c| db::get(c, id))?;
    if doc.is_invoice_like() && doc.status != "draft" && doc.einvoice_format == "zugferd" {
        if let Ok((bytes, doc)) = zugferd_bytes(&st, id).await {
            if q.download {
                let _ = st.write(|c| crate::services::settings::log(c, "document", id, "download", "PDF (ZUGFeRD) heruntergeladen"));
            }
            return Ok(file_response(bytes, "application/pdf", &pdf_name(&doc), q.download));
        }
    }
    let job = st.read(|c| {
        let lines = docs::lines_of(c, id)?;
        data::document_job(c, &st.data_dir, &doc, &lines, None)
    })?;
    let bytes = render_pdf(&st, job, false).await?;
    if q.download {
        let _ = st.write(|c| crate::services::settings::log(c, "document", id, "download", "PDF heruntergeladen"));
    }
    Ok(file_response(bytes, "application/pdf", &pdf_name(&doc), q.download))
}

pub async fn zugferd(State(st): State<AppState>, Path(id): Path<i64>) -> Result<Response, AppError> {
    let (bytes, doc) = zugferd_bytes(&st, id).await?;
    let _ = st.write(|c| crate::services::settings::log(c, "document", id, "einvoice", "ZUGFeRD-Rechnung erzeugt"));
    Ok(file_response(bytes, "application/pdf", &pdf_name(&doc).replace(".pdf", "_ZUGFeRD.pdf"), true))
}

pub async fn xrechnung(State(st): State<AppState>, Path(id): Path<i64>) -> Result<Response, AppError> {
    let (xml, doc) = st.write(|c| {
        let doc: Document = db::get(c, id)?;
        let check = einvoice::check(c, &doc, true)?;
        if !check.errors.is_empty() {
            return Err(crate::error::UserError(format!("XRechnung nicht möglich: {}", check.errors.join(" "))).into());
        }
        let xml = einvoice::generate_cii(c, &doc, einvoice::GUIDELINE_XRECHNUNG)?;
        crate::services::settings::log(c, "document", id, "einvoice", "XRechnung (CII) erzeugt")?;
        Ok((xml, doc))
    })?;
    let name = safe_filename(&format!("XRechnung_{}.xml", doc.number));
    Ok(file_response(xml.into_bytes(), "application/xml", &name, true))
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
    let job = st.read(|c| {
        let doc: Document = db::get(c, id)?;
        let lines = docs::lines_of(c, id)?;
        data::document_job(c, &st.data_dir, &doc, &lines, None)
    })?;
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
