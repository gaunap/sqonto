//! Stammdaten: Kontakte, Produkte und Dienstleistungen.

use anyhow::Result;
use chrono::Datelike;
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::*;
use crate::services::documents::{self, DocumentFilter, DocumentListItem};
use crate::services::receipts::{self, ReceiptFilter, ReceiptListItem};
use crate::services::settings as cfg;
use crate::util::{self, now_str, today};

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ContactListItem {
    pub contact: Contact,
    pub name: String,
    #[ts(type = "number")]
    pub revenue: Decimal,
    #[ts(type = "number")]
    pub open_amount: Decimal,
    pub document_count: i64,
}

#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct ContactFilter {
    pub q: String,
    /// customer | supplier
    pub role: String,
    pub archived: bool,
}

pub fn list_contacts(conn: &Connection, f: &ContactFilter) -> Result<Vec<ContactListItem>> {
    let contacts = db::query::<Contact>(conn, "WHERE archived = ?1 ORDER BY company COLLATE NOCASE, last_name COLLATE NOCASE", &[v(f.archived)])?;
    let docs = db::query::<Document>(conn, "WHERE doc_type IN ('invoice','credit_note') AND status != 'draft'", &[])?;
    let q = f.q.trim().to_lowercase();
    let mut out = Vec::new();
    for c in contacts {
        if f.role == "customer" && !c.is_customer {
            continue;
        }
        if f.role == "supplier" && !c.is_supplier {
            continue;
        }
        let name = c.display_name();
        if !q.is_empty() {
            let hay = format!("{} {} {} {} {} {}", name, c.number, c.email, c.city, c.first_name, c.last_name).to_lowercase();
            if !hay.contains(&q) {
                continue;
            }
        }
        let mine: Vec<&Document> = docs.iter().filter(|d| d.contact_id == Some(c.id)).collect();
        out.push(ContactListItem {
            revenue: mine.iter().map(|d| d.net_total * d.sign()).sum(),
            open_amount: mine.iter().map(|d| d.open_amount() * d.sign()).sum(),
            document_count: mine.len() as i64,
            name,
            contact: c,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ContactOverview {
    pub contact: Contact,
    #[ts(type = "number")]
    pub revenue_year: Decimal,
    #[ts(type = "number")]
    pub revenue_total: Decimal,
    #[ts(type = "number")]
    pub open_amount: Decimal,
    #[ts(type = "number")]
    pub expenses_total: Decimal,
    pub documents: Vec<DocumentListItem>,
    pub receipts: Vec<ReceiptListItem>,
    pub activities: Vec<Activity>,
}

pub fn overview(conn: &Connection, id: i64) -> Result<ContactOverview> {
    let contact: Contact = db::get(conn, id)?;
    let documents = documents::list(conn, &DocumentFilter { contact_id: Some(id), ..Default::default() })?;
    let receipts = receipts::list(conn, &ReceiptFilter { contact_id: Some(id), ..Default::default() })?;
    let year = today().year();
    let inv = |d: &&DocumentListItem| (d.doc_type == "invoice" || d.doc_type == "credit_note") && d.status != "draft";
    let sign = |d: &DocumentListItem| if d.doc_type == "credit_note" { Decimal::NEGATIVE_ONE } else { Decimal::ONE };
    Ok(ContactOverview {
        revenue_year: documents.iter().filter(inv).filter(|d| d.date.year() == year).map(|d| d.net_total * sign(d)).sum(),
        revenue_total: documents.iter().filter(inv).map(|d| d.net_total * sign(d)).sum(),
        open_amount: documents.iter().filter(inv).map(|d| d.open_amount * sign(d)).sum(),
        expenses_total: receipts.iter().map(|r| r.receipt.net_total).sum(),
        activities: cfg::activities(conn, "contact", id)?,
        contact,
        documents,
        receipts,
    })
}

pub fn save_contact(conn: &Connection, mut c: Contact) -> Result<Contact> {
    if c.kind.is_empty() {
        c.kind = if c.company.trim().is_empty() { "person".into() } else { "organization".into() };
    }
    if c.kind == "organization" && c.company.trim().is_empty() {
        bail_user!("Bitte einen Firmennamen angeben.");
    }
    if c.kind == "person" && c.last_name.trim().is_empty() && c.first_name.trim().is_empty() {
        bail_user!("Bitte einen Namen angeben.");
    }
    if !c.is_customer && !c.is_supplier {
        c.is_customer = true;
    }
    if c.country.is_empty() {
        c.country = "DE".into();
    }
    c.country = c.country.to_uppercase();
    c.iban = util::normalize_iban(&c.iban);
    c.vat_id = c.vat_id.replace(' ', "").to_uppercase();
    if c.einvoice_format.is_empty() {
        c.einvoice_format = "pdf".into();
    }
    if !c.persons.is_empty() && !c.persons.iter().any(|p| p.primary) {
        c.persons[0].primary = true;
    }
    c.updated_at = now_str();
    if c.id > 0 {
        let old: Contact = db::get(conn, c.id)?;
        c.created_at = old.created_at;
        if c.number.is_empty() {
            c.number = old.number;
        }
        db::update(conn, &c)?;
        cfg::log(conn, "contact", c.id, "updated", "Kontakt bearbeitet")?;
    } else {
        if c.number.is_empty() {
            c.number = cfg::next_number(conn, if c.is_customer { "customer" } else { "supplier" }, today())?;
        } else if db::scalar_i64(conn, "SELECT COUNT(*) FROM contacts WHERE number = ?1", &[v(c.number.clone())])? > 0 {
            bail_user!("Die Kontaktnummer {} ist bereits vergeben.", c.number);
        }
        c.created_at = now_str();
        db::insert(conn, &mut c)?;
        cfg::log(conn, "contact", c.id, "created", "Kontakt angelegt")?;
    }
    Ok(c)
}

/// Deletes a contact or archives it when it is referenced by documents.
pub fn delete_contact(conn: &Connection, id: i64) -> Result<String> {
    let refs = db::scalar_i64(conn, "SELECT COUNT(*) FROM documents WHERE contact_id = ?1", &[v(id)])?
        + db::scalar_i64(conn, "SELECT COUNT(*) FROM receipts WHERE contact_id = ?1", &[v(id)])?;
    if refs > 0 {
        let mut c: Contact = db::get(conn, id)?;
        c.archived = true;
        db::update(conn, &c)?;
        return Ok("archived".into());
    }
    conn.execute("DELETE FROM activities WHERE entity = 'contact' AND entity_id = ?1", [id])?;
    db::delete::<Contact>(conn, id)?;
    Ok("deleted".into())
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ContactDefaults {
    pub address: String,
    pub contact_person: String,
    pub tax_mode: String,
    pub buyer_reference: String,
    pub payment_terms_days: Option<i64>,
    pub einvoice_format: String,
    pub persons: Vec<String>,
}

pub fn contact_defaults(conn: &Connection, id: i64, doc_type: &str) -> Result<ContactDefaults> {
    let s = cfg::load(conn)?;
    let c: Contact = db::get(conn, id)?;
    let mut doc = Document { doc_type: doc_type.into(), date: today(), ..Default::default() };
    documents::apply_contact(&s, &mut doc, &c);
    let persons = c
        .persons
        .iter()
        .map(|p| format!("{} {} {}", p.salutation, p.first_name, p.last_name).split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    Ok(ContactDefaults {
        address: doc.address,
        contact_person: doc.contact_person,
        tax_mode: doc.tax_mode,
        buyer_reference: doc.buyer_reference,
        payment_terms_days: c.payment_terms_days,
        einvoice_format: if c.einvoice_format.is_empty() { s.docs.einvoice_default.clone() } else { c.einvoice_format.clone() },
        persons,
    })
}

// ---------------------------------------------------------------------------
// Artikel
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ArticleListItem {
    pub article: Article,
    #[ts(type = "number")]
    pub sold_quantity: Decimal,
    #[ts(type = "number")]
    pub revenue: Decimal,
}

pub fn list_articles(conn: &Connection, q: &str, archived: bool) -> Result<Vec<ArticleListItem>> {
    let arts = db::query::<Article>(conn, "WHERE archived = ?1 ORDER BY name COLLATE NOCASE", &[v(archived)])?;
    let mut stmt = conn.prepare(
        "SELECT l.article_id, l.quantity, l.net_amount, d.doc_type FROM document_lines l JOIN documents d ON d.id = l.document_id
         WHERE l.article_id IS NOT NULL AND d.doc_type IN ('invoice','credit_note') AND d.status NOT IN ('draft','cancelled') AND d.kind != 'cancellation'",
    )?;
    let mut stats: std::collections::HashMap<i64, (Decimal, Decimal)> = Default::default();
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?;
    for row in rows.flatten() {
        let sign = if row.3 == "credit_note" { Decimal::NEGATIVE_ONE } else { Decimal::ONE };
        let e = stats.entry(row.0).or_default();
        e.0 += row.1.parse::<Decimal>().unwrap_or_default() * sign;
        e.1 += row.2.parse::<Decimal>().unwrap_or_default() * sign;
    }
    let q = q.trim().to_lowercase();
    Ok(arts
        .into_iter()
        .filter(|a| q.is_empty() || format!("{} {} {} {}", a.name, a.number, a.description, a.category).to_lowercase().contains(&q))
        .map(|a| {
            let (sold_quantity, revenue) = stats.get(&a.id).copied().unwrap_or_default();
            ArticleListItem { article: a, sold_quantity, revenue }
        })
        .collect())
}

pub fn save_article(conn: &Connection, mut a: Article) -> Result<Article> {
    if a.name.trim().is_empty() {
        bail_user!("Bitte eine Bezeichnung angeben.");
    }
    if a.kind.is_empty() {
        a.kind = "product".into();
    }
    if a.unit.is_empty() {
        a.unit = if a.kind == "service" { "Std".into() } else { "Stk".into() };
    }
    if !a.revenue_account.is_empty() && cfg::account_by_number(conn, &a.revenue_account)?.is_none() {
        bail_user!("Erlöskonto {} existiert nicht.", a.revenue_account);
    }
    a.updated_at = now_str();
    if a.id > 0 {
        let old: Article = db::get(conn, a.id)?;
        a.created_at = old.created_at;
        if a.number.is_empty() {
            a.number = old.number;
        }
        db::update(conn, &a)?;
    } else {
        if a.number.is_empty() {
            a.number = cfg::next_number(conn, "article", today())?;
        }
        a.created_at = now_str();
        db::insert(conn, &mut a)?;
    }
    Ok(a)
}

pub fn delete_article(conn: &Connection, id: i64) -> Result<String> {
    let used = db::scalar_i64(conn, "SELECT COUNT(*) FROM document_lines WHERE article_id = ?1", &[v(id)])?;
    if used > 0 {
        let mut a: Article = db::get(conn, id)?;
        a.archived = true;
        db::update(conn, &a)?;
        return Ok("archived".into());
    }
    db::delete::<Article>(conn, id)?;
    Ok("deleted".into())
}

// ---------------------------------------------------------------------------
// Globale Suche
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SearchHit {
    /// contact | document | receipt | article
    pub kind: String,
    pub id: i64,
    pub title: String,
    pub subtitle: String,
    pub link: String,
}

pub fn search(conn: &Connection, q: &str) -> Result<Vec<SearchHit>> {
    let q = q.trim();
    if q.len() < 2 {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for c in list_contacts(conn, &ContactFilter { q: q.into(), ..Default::default() })?.into_iter().take(5) {
        out.push(SearchHit {
            kind: "contact".into(),
            id: c.contact.id,
            subtitle: format!("{} · {}", c.contact.number, c.contact.city),
            title: c.name,
            link: format!("/kontakte/{}", c.contact.id),
        });
    }
    for d in documents::list(conn, &DocumentFilter { q: q.into(), ..Default::default() })?.into_iter().take(6) {
        out.push(SearchHit {
            kind: "document".into(),
            id: d.id,
            title: format!("{} {}", documents::type_label(&d.doc_type, &d.kind), if d.number.is_empty() { "(Entwurf)".into() } else { d.number.clone() }),
            subtitle: format!("{} · {}", d.contact_name, util::fmt_money(d.gross_total)),
            link: format!("/dokumente/{}", d.id),
        });
    }
    for r in receipts::list(conn, &ReceiptFilter { q: q.into(), ..Default::default() })?.into_iter().take(5) {
        out.push(SearchHit {
            kind: "receipt".into(),
            id: r.receipt.id,
            title: format!("Beleg {} · {}", r.receipt.number, r.party),
            subtitle: format!("{} · {}", util::fmt_date(r.receipt.date), util::fmt_money(r.receipt.gross_total)),
            link: format!("/belege/{}", r.receipt.id),
        });
    }
    for a in list_articles(conn, q, false)?.into_iter().take(4) {
        out.push(SearchHit {
            kind: "article".into(),
            id: a.article.id,
            title: a.article.name.clone(),
            subtitle: format!("{} · {}", a.article.number, util::fmt_money(a.article.price)),
            link: format!("/artikel?edit={}", a.article.id),
        });
    }
    Ok(out)
}
