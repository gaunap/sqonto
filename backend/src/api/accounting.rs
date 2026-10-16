// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;

use super::MessageResponse;
use crate::db;
use crate::error::ApiResult;
use crate::models::{Account, Asset, ManualEntry};
use crate::services::assets::{self, AssetView};
use crate::services::reports::{self, JournalRow, Ledger};
use crate::services::{posting, settings as cfg};
use crate::state::AppState;
use crate::util::{now_str, r2, today, ymd};

pub async fn accounts(State(st): State<AppState>) -> ApiResult<Vec<Account>> {
    st.read(cfg::accounts).map(Json)
}

pub async fn save_account(State(st): State<AppState>, id: Option<Path<i64>>, Json(mut a): Json<Account>) -> ApiResult<Account> {
    st.write(|c| {
        a.number = a.number.trim().to_string();
        if a.number.is_empty() || !a.number.chars().all(|ch| ch.is_ascii_digit()) || a.number.len() > 8 {
            crate::bail_user!("Die Kontonummer muss aus Ziffern bestehen.");
        }
        if a.name.trim().is_empty() {
            crate::bail_user!("Bitte eine Kontobezeichnung angeben.");
        }
        match id {
            Some(Path(id)) => {
                let old: Account = db::get(c, id)?;
                // System accounts keep number, kind and mappings.
                let mut updated = old.clone();
                updated.name = a.name;
                updated.hint = a.hint;
                updated.active = a.active || old.system;
                if !old.system {
                    updated.kind = a.kind;
                    updated.euer = a.euer;
                    updated.guv = a.guv;
                    updated.bwa = a.bwa;
                    updated.bilanz = a.bilanz;
                    updated.tax_rate = a.tax_rate;
                    updated.pick = a.pick;
                }
                db::update(c, &updated)?;
                Ok(updated)
            }
            None => {
                if cfg::account_by_number(c, &a.number)?.is_some() {
                    crate::bail_user!("Das Konto {} existiert bereits.", a.number);
                }
                a.id = 0;
                a.system = false;
                a.active = true;
                a.role = String::new();
                if a.pick.is_empty() {
                    a.pick = match a.kind.as_str() {
                        "expense" => "expense",
                        "revenue" => "revenue",
                        "asset" => "asset",
                        "private" => "private",
                        _ => "other",
                    }
                    .into();
                }
                db::insert(c, &mut a)?;
                Ok(a)
            }
        }
    })
    .map(Json)
}

#[derive(Deserialize)]
pub struct JournalQ {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    #[serde(default)]
    account: String,
    #[serde(default)]
    q: String,
}

fn default_range(from: Option<NaiveDate>, to: Option<NaiveDate>) -> (NaiveDate, NaiveDate) {
    let y = today().year();
    (from.unwrap_or(ymd(y, 1, 1)), to.unwrap_or(ymd(y, 12, 31)))
}

pub async fn journal(State(st): State<AppState>, Query(q): Query<JournalQ>) -> ApiResult<Vec<JournalRow>> {
    let (from, to) = default_range(q.from, q.to);
    st.read(|c| reports::journal(c, from, to, &q.account, &q.q)).map(Json)
}

pub async fn ledger(State(st): State<AppState>, Path(number): Path<String>, Query(q): Query<JournalQ>) -> ApiResult<Ledger> {
    let (from, to) = default_range(q.from, q.to);
    st.read(|c| reports::ledger(c, &number, from, to)).map(Json)
}

pub async fn manual_entries(State(st): State<AppState>) -> ApiResult<Vec<ManualEntry>> {
    st.read(|c| db::query::<ManualEntry>(c, "ORDER BY date DESC, id DESC", &[])).map(Json)
}

pub async fn save_manual_entry(State(st): State<AppState>, id: Option<Path<i64>>, Json(mut m): Json<ManualEntry>) -> ApiResult<ManualEntry> {
    st.write(|c| {
        if m.amount <= rust_decimal::Decimal::ZERO {
            crate::bail_user!("Bitte einen Betrag größer 0 angeben.");
        }
        if m.debit_account == m.credit_account {
            crate::bail_user!("Soll- und Habenkonto müssen sich unterscheiden.");
        }
        for acc in [&m.debit_account, &m.credit_account] {
            if cfg::account_by_number(c, acc)?.is_none() {
                crate::bail_user!("Konto {acc} existiert nicht.");
            }
        }
        if m.description.trim().is_empty() {
            crate::bail_user!("Bitte einen Buchungstext angeben.");
        }
        cfg::check_unlocked(c, m.date)?;
        m.amount = r2(m.amount);
        match id {
            Some(Path(id)) => {
                let old: ManualEntry = db::get(c, id)?;
                cfg::check_unlocked(c, old.date)?;
                m.id = id;
                m.created_at = old.created_at;
                db::update(c, &m)?;
            }
            None => {
                m.id = 0;
                m.created_at = now_str();
                db::insert(c, &mut m)?;
            }
        }
        posting::rebuild(c, posting::Source::Manual(m.id))?;
        Ok(m)
    })
    .map(Json)
}

pub async fn delete_manual_entry(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| {
        let m: ManualEntry = db::get(c, id)?;
        cfg::check_unlocked(c, m.date)?;
        db::delete::<ManualEntry>(c, id)?;
        posting::rebuild(c, posting::Source::Manual(id))?;
        Ok(MessageResponse { message: "Buchung gelöscht".into() })
    })
    .map(Json)
}

pub async fn assets(State(st): State<AppState>) -> ApiResult<Vec<AssetView>> {
    st.read(assets::list).map(Json)
}

pub async fn asset(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<AssetView> {
    st.read(|c| assets::view(c, db::get(c, id)?)).map(Json)
}

pub async fn save_asset(State(st): State<AppState>, id: Option<Path<i64>>, Json(mut a): Json<Asset>) -> ApiResult<AssetView> {
    a.id = id.map(|Path(i)| i).unwrap_or(0);
    st.write(|c| {
        let id = assets::save(c, a)?;
        assets::view(c, db::get(c, id)?)
    })
    .map(Json)
}

pub async fn delete_asset(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| assets::delete(c, id)).map(|_| Json(MessageResponse { message: "Anlage gelöscht".into() }))
}
