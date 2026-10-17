// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Sqonto – lokale Buchhaltungs- und Rechnungssoftware.
//!
//! The library contains the complete application: database, bookkeeping, PDF output and the
//! HTTP API including the embedded frontend. It is used by two programs:
//!
//! * `sqonto-server` – headless server for the browser or the Vite dev server,
//! * `sqonto-desktop` – native window (WebView2 / WebKit) around the same server.

mod api;
mod chart;
mod db;
mod error;
mod guard;
mod models;
mod pdf;
mod services;
mod state;
mod util;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use tower_http::compression::CompressionLayer;

use crate::state::{AppState, Inner};

/// Data directory: `SQONTO_DATA_DIR`, otherwise next to the executable for release builds
/// (portable installation, independent of where it is started from) and the working directory
/// for debug builds.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SQONTO_DATA_DIR") {
        return PathBuf::from(dir);
    }
    if !cfg!(debug_assertions)
        && let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf()))
    {
        return dir.join("data");
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.join("data")
}

/// Preferred port: `SQONTO_PORT` or 8787.
pub fn default_port() -> u16 {
    std::env::var("SQONTO_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8787)
}

/// A bound, fully initialised server that only needs to be run.
pub struct Server {
    listener: tokio::net::TcpListener,
    app: axum::Router,
    pub port: u16,
    pub url: String,
    pub data_dir: PathBuf,
}

impl Server {
    pub async fn run(self) -> anyhow::Result<()> {
        axum::serve(self.listener, self.app).await?;
        Ok(())
    }
}

pub enum Launch {
    Started(Server),
    /// Sqonto already runs with the same data (e.g. after a second double-click).
    AlreadyRunning { port: u16, url: String },
}

/// Binds the local port and initialises database, fonts and API. A port taken by anything
/// other than a Sqonto instance with the same data is skipped.
pub async fn launch(port: u16, dir: &Path) -> anyhow::Result<Launch> {
    std::fs::create_dir_all(dir.join("files"))?;
    let mut bound = None;
    for p in port..port.saturating_add(20) {
        match tokio::net::TcpListener::bind(("127.0.0.1", p)).await {
            Ok(listener) => {
                bound = Some((listener, p));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                if running_instance(p).is_some_and(|d| same_dir(Path::new(&d), dir)) {
                    return Ok(Launch::AlreadyRunning { port: p, url: format!("http://localhost:{p}") });
                }
                tracing::warn!("Port {p} ist belegt, versuche {} …", p + 1);
            }
            Err(e) => return Err(e.into()),
        }
    }
    let Some((listener, port)) = bound else {
        anyhow::bail!("Kein freier Port zwischen {port} und {} gefunden.", port.saturating_add(19));
    };
    migrate_legacy_files(dir)?;
    let conn = db::open(&dir.join("sqonto.db"))?;
    services::settings::ensure_seeded(&conn)?;
    services::posting::ensure_current_afa(&conn)?;
    tracing::info!("Lade Schriftarten …");
    let pdf = pdf::PdfEngine::new();
    tracing::info!("{} Schriftfamilien verfügbar", pdf.families().len());
    let state = AppState(Arc::new(Inner { db: Mutex::new(conn), data_dir: dir.to_path_buf(), pdf }));
    spawn_afa_check(state.clone());
    let app = api::router(state).layer(CompressionLayer::new()).layer(axum::middleware::from_fn(guard::local_only));
    Ok(Launch::Started(Server { listener, app, port, url: format!("http://localhost:{port}"), data_dir: dir.to_path_buf() }))
}

/// Books the depreciation of a new year once it has started – also when the program keeps
/// running over New Year (the check itself only does work once per year).
fn spawn_afa_check(state: AppState) {
    tokio::spawn(async move {
        let mut hourly = tokio::time::interval(Duration::from_secs(3600));
        hourly.tick().await; // the first tick is immediate; the start already checked
        loop {
            hourly.tick().await;
            if let Err(e) = state.write(services::posting::ensure_current_afa) {
                tracing::warn!("Abschreibungen des neuen Jahres: {}", e.message);
            }
        }
    });
}

/// The program was called "Kontor" before; its database is taken over unchanged.
fn migrate_legacy_files(dir: &Path) -> anyhow::Result<()> {
    if dir.join("sqonto.db").exists() || !dir.join("kontor.db").exists() {
        return Ok(());
    }
    for suffix in ["", "-wal", "-shm"] {
        let old = dir.join(format!("kontor.db{suffix}"));
        if old.exists() {
            std::fs::rename(&old, dir.join(format!("sqonto.db{suffix}")))?;
        }
    }
    let _ = std::fs::remove_file(dir.join("kontor.log"));
    tracing::info!("Datenbank kontor.db als sqonto.db übernommen");
    Ok(())
}

/// Minimal HTTP/1.0 request to a local instance; returns the parsed JSON body.
fn local_request(port: u16, method: &str, path: &str) -> Option<serde_json::Value> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let request = format!("{method} {path} HTTP/1.0\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    serde_json::from_str(response.split_once("\r\n\r\n")?.1).ok()
}

/// Data directory of a Sqonto instance listening on `port`, if there is one.
fn running_instance(port: u16) -> Option<String> {
    local_request(port, "GET", "/api/system/status")?.get("data_dir")?.as_str().map(str::to_string)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

static FOCUS_HANDLER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Registers what happens when another start of the program asks this instance to come to the
/// front (the desktop app raises its window).
pub fn set_focus_handler(f: impl Fn() + Send + Sync + 'static) {
    let _ = FOCUS_HANDLER.set(Box::new(f));
}

/// Called by `POST /api/system/focus`; false if this instance has no window.
pub(crate) fn focus_window() -> bool {
    FOCUS_HANDLER.get().map(|f| f()).is_some()
}

/// Asks the instance on `port` to raise its window; false if it has none (headless server).
pub fn request_focus(port: u16) -> bool {
    local_request(port, "POST", "/api/system/focus").and_then(|v| v.get("focused")?.as_bool()).unwrap_or(false)
}
