// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Archive of issued invoices and credit notes (GoBD, § 14b UStG).
//!
//! The PDF, its preview pages and generated e-invoices of an issued document are rendered once and
//! stored as files. Later downloads serve the stored files, so changes to company data, layouts or
//! templates never alter a document that has already been issued.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db;
use crate::models::{Document, FileMeta};
use crate::util::now_str;

/// The PDF as issued (ZUGFeRD PDF/A-3 if that is the document's format).
pub const PDF: &str = "pdf";
/// SVG pages of the issued PDF (JSON array) for the preview.
pub const PAGES: &str = "pages";
/// ZUGFeRD PDF from the e-invoice download.
pub const ZUGFERD: &str = "zugferd";
/// XRechnung XML from the e-invoice download.
pub const XRECHNUNG: &str = "xrechnung";

/// Documents whose output is archived: invoices and credit notes once they have been issued.
pub fn is_issued(doc: &Document) -> bool {
    doc.is_invoice_like() && doc.status != "draft"
}

pub fn find(conn: &Connection, document_id: i64, kind: &str) -> Result<Option<FileMeta>> {
    let file_id: Option<i64> = conn
        .query_row("SELECT file_id FROM document_archive WHERE document_id = ?1 AND kind = ?2", params![document_id, kind], |r| r.get(0))
        .optional()?;
    match file_id {
        Some(id) => db::find::<FileMeta>(conn, id),
        None => Ok(None),
    }
}

/// Registers an already written file; returns false if the kind was archived before (the first
/// archived version always stays authoritative).
pub fn insert(conn: &Connection, document_id: i64, kind: &str, file: &mut FileMeta) -> Result<bool> {
    if find(conn, document_id, kind)?.is_some() {
        return Ok(false);
    }
    if file.id == 0 {
        db::insert(conn, file)?;
    }
    conn.execute(
        "INSERT INTO document_archive (document_id, kind, file_id, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![document_id, kind, file.id, now_str()],
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> FileMeta {
        FileMeta { id: 0, uuid: uuid::Uuid::new_v4().to_string(), filename: name.into(), mime: "application/pdf".into(), size: 1, created_at: now_str() }
    }

    #[test]
    fn first_version_stays() {
        let conn = db::open_memory();
        conn.execute("INSERT INTO documents (id, doc_type, date) VALUES (1, 'invoice', '2026-10-01')", []).unwrap();
        let mut a = file("a.pdf");
        assert!(insert(&conn, 1, PDF, &mut a).unwrap());
        let mut b = file("b.pdf");
        assert!(!insert(&conn, 1, PDF, &mut b).unwrap());
        assert_eq!(b.id, 0, "the rejected file is not registered");
        assert_eq!(find(&conn, 1, PDF).unwrap().unwrap().filename, "a.pdf");
        // One file can be archived under several kinds (ZUGFeRD as main PDF).
        assert!(insert(&conn, 1, ZUGFERD, &mut a).unwrap());
        assert_eq!(find(&conn, 1, ZUGFERD).unwrap().unwrap().id, a.id);
        assert!(find(&conn, 1, XRECHNUNG).unwrap().is_none());
    }
}
