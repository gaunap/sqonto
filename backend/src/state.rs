use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};
use rusqlite::Connection;

use crate::error::AppError;
use crate::pdf::PdfEngine;

pub struct Inner {
    pub db: Mutex<Connection>,
    pub data_dir: PathBuf,
    pub pdf: PdfEngine,
}

#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);

impl std::ops::Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl AppState {
    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.0.db.lock()
    }

    /// Read-only access.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> anyhow::Result<T>) -> Result<T, AppError> {
        let conn = self.db();
        f(&conn).map_err(AppError::from)
    }

    /// Runs `f` inside a database transaction; rolls back on error.
    pub fn write<T>(&self, f: impl FnOnce(&Connection) -> anyhow::Result<T>) -> Result<T, AppError> {
        let conn = self.db();
        let tx = conn.unchecked_transaction().map_err(AppError::from)?;
        match f(&tx) {
            Ok(v) => {
                tx.commit().map_err(AppError::from)?;
                Ok(v)
            }
            Err(e) => {
                let _ = tx.rollback();
                Err(AppError::from(e))
            }
        }
    }

    pub fn files_dir(&self) -> PathBuf {
        self.data_dir.join("files")
    }
}
