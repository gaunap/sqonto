use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

use super::MessageResponse;
use crate::db;
use crate::error::ApiResult;
use crate::models::{Article, Contact};
use crate::services::masterdata::{self as md, ArticleListItem, ContactDefaults, ContactFilter, ContactListItem, ContactOverview};
use crate::state::AppState;

pub async fn contacts(State(st): State<AppState>, Query(f): Query<ContactFilter>) -> ApiResult<Vec<ContactListItem>> {
    st.read(|c| md::list_contacts(c, &f)).map(Json)
}

pub async fn contact(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<ContactOverview> {
    st.read(|c| md::overview(c, id)).map(Json)
}

pub async fn create_contact(State(st): State<AppState>, Json(mut contact): Json<Contact>) -> ApiResult<Contact> {
    contact.id = 0;
    st.write(|c| md::save_contact(c, contact)).map(Json)
}

pub async fn update_contact(State(st): State<AppState>, Path(id): Path<i64>, Json(mut contact): Json<Contact>) -> ApiResult<Contact> {
    contact.id = id;
    st.write(|c| md::save_contact(c, contact)).map(Json)
}

pub async fn delete_contact(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| md::delete_contact(c, id)).map(|message| Json(MessageResponse { message }))
}

#[derive(Deserialize)]
pub struct DefaultsQ {
    #[serde(default)]
    doc_type: String,
}

pub async fn contact_defaults(State(st): State<AppState>, Path(id): Path<i64>, Query(q): Query<DefaultsQ>) -> ApiResult<ContactDefaults> {
    st.read(|c| md::contact_defaults(c, id, if q.doc_type.is_empty() { "invoice" } else { &q.doc_type })).map(Json)
}

#[derive(Deserialize)]
pub struct ArticleQ {
    #[serde(default)]
    q: String,
    #[serde(default)]
    archived: bool,
}

pub async fn articles(State(st): State<AppState>, Query(q): Query<ArticleQ>) -> ApiResult<Vec<ArticleListItem>> {
    st.read(|c| md::list_articles(c, &q.q, q.archived)).map(Json)
}

pub async fn article(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Article> {
    st.read(|c| db::get(c, id)).map(Json)
}

pub async fn create_article(State(st): State<AppState>, Json(mut a): Json<Article>) -> ApiResult<Article> {
    a.id = 0;
    st.write(|c| md::save_article(c, a)).map(Json)
}

pub async fn update_article(State(st): State<AppState>, Path(id): Path<i64>, Json(mut a): Json<Article>) -> ApiResult<Article> {
    a.id = id;
    st.write(|c| md::save_article(c, a)).map(Json)
}

pub async fn delete_article(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| md::delete_article(c, id)).map(|message| Json(MessageResponse { message }))
}
