// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Typesetting with the embedded Typst compiler: PDF (incl. PDF/A-3b for
//! ZUGFeRD) and SVG page previews.

pub mod data;
pub mod girocode;

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use chrono::Datelike;
use rust_embed::RustEmbed;
use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_kit::fonts::FontStore;
use typst_layout::PagedDocument;

#[derive(RustEmbed)]
#[folder = "assets/fonts/"]
struct FontAssets;

pub const DOCUMENT_TEMPLATE: &str = include_str!("../../templates/document.typ");
pub const REPORT_TEMPLATE: &str = include_str!("../../templates/report.typ");

pub struct PdfEngine {
    library: LazyHash<Library>,
    fonts: FontStore,
    families: Vec<String>,
}

impl PdfEngine {
    pub fn new() -> Self {
        let mut fonts = FontStore::new();
        // The folder also holds the font licences (LICENSE-FONTS.txt).
        for name in FontAssets::iter().filter(|n| n.ends_with(".ttf") || n.ends_with(".otf")) {
            if let Some(file) = FontAssets::get(&name) {
                for font in Font::iter(Bytes::new(file.data.into_owned())) {
                    let info = font.info().clone();
                    fonts.push((font, info));
                }
            }
        }
        fonts.extend(typst_kit::fonts::embedded());
        fonts.extend(typst_kit::fonts::system());
        let mut families: Vec<String> = fonts.book().families().map(|(f, _)| f.to_string()).collect();
        families.sort_by_key(|f| f.to_lowercase());
        families.dedup();
        Self { library: LazyHash::new(Library::default()), fonts, families }
    }

    pub fn families(&self) -> &[String] {
        &self.families
    }

    pub fn bundled_families() -> Vec<&'static str> {
        vec![
            "Inter",
            "Source Sans 3",
            "Roboto",
            "Open Sans",
            "Lato",
            "IBM Plex Sans",
            "Montserrat",
            "Fira Sans",
            "Merriweather",
            "Source Serif 4",
            "Libertinus Serif",
        ]
    }

    /// Compiles a Typst source with the given virtual files.
    pub fn compile(&self, source: &str, files: HashMap<String, Vec<u8>>) -> Result<PagedDocument> {
        let world = RenderWorld::new(self, source, files);
        let result = typst::compile::<PagedDocument>(&world);
        comemo::evict(20);
        result.output.map_err(|errs| anyhow!(format_errors(&world, &errs)))
    }

    pub fn pdf(&self, doc: &PagedDocument, pdfa: bool) -> Result<Vec<u8>> {
        let mut options = typst_pdf::PdfOptions::default();
        let now = chrono::Utc::now();
        if let Some(dt) = Datetime::from_ymd_hms(
            now.year(),
            now.month() as u8,
            now.day() as u8,
            chrono::Timelike::hour(&now) as u8,
            chrono::Timelike::minute(&now) as u8,
            chrono::Timelike::second(&now) as u8,
        ) {
            options.timestamp = Some(typst_pdf::Timestamp::new_utc(dt));
        }
        if pdfa {
            options.standards = typst_pdf::PdfStandards::new(&[typst_pdf::PdfStandard::A_3b])
                .map_err(|e| anyhow!("PDF/A: {}", e.message()))?;
        }
        typst_pdf::pdf(doc, &options).map_err(|errs| anyhow!(errs.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; ")))
    }

    pub fn svgs(&self, doc: &PagedDocument) -> Vec<String> {
        let opts = typst_svg::SvgOptions::default();
        doc.pages().iter().map(|p| typst_svg::svg(p, &opts)).collect()
    }
}

impl Default for PdfEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn format_errors(world: &RenderWorld, errs: &[SourceDiagnostic]) -> String {
    errs.iter()
        .map(|e| {
            let line = world
                .range(e.span)
                .map(|r| world.main.text()[..r.start.min(world.main.text().len())].lines().count())
                .map(|l| format!(" (Zeile {l})"))
                .unwrap_or_default();
            let hints: Vec<String> = e.hints.iter().map(|h| h.v.to_string()).collect();
            if hints.is_empty() {
                format!("Typst: {}{line}", e.message)
            } else {
                format!("Typst: {}{line} – {}", e.message, hints.join("; "))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct RenderWorld<'a> {
    engine: &'a PdfEngine,
    main_id: FileId,
    main: Source,
    files: HashMap<String, Bytes>,
    today: Option<Datetime>,
}

impl<'a> RenderWorld<'a> {
    fn new(engine: &'a PdfEngine, source: &str, files: HashMap<String, Vec<u8>>) -> Self {
        let main_id = FileId::new(RootedPath::new(VirtualRoot::Project, VirtualPath::new("main.typ").expect("valid path")));
        let main = Source::new(main_id, source.to_string());
        let now = chrono::Local::now().date_naive();
        Self {
            engine,
            main_id,
            main,
            files: files.into_iter().map(|(k, v)| (k, Bytes::new(v))).collect(),
            today: Datetime::from_ymd(now.year(), now.month() as u8, now.day() as u8),
        }
    }

    fn path_of(id: FileId) -> String {
        id.get().vpath().get_without_slash().to_string()
    }
}

impl World for RenderWorld<'_> {
    fn library(&self) -> &LazyHash<Library> {
        &self.engine.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.engine.fonts.book()
    }

    fn main(&self) -> FileId {
        self.main_id
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main_id {
            Ok(self.main.clone())
        } else {
            Err(FileError::NotSource)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main_id {
            return Ok(Bytes::from_string(self.main.text().to_string()));
        }
        let path = Self::path_of(id);
        self.files.get(&path).cloned().ok_or_else(|| FileError::NotFound(path.into()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.engine.fonts.font(index)
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        self.today
    }
}
