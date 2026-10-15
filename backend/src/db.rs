//! Database layer: SQLite connection, schema migration and a small typed
//! entity mapping (`entity!` macro) so that every table row maps 1:1 onto a
//! strongly typed Rust struct that is also exported to TypeScript.

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, Row, params_from_iter};
use rust_decimal::Decimal;
use serde::Serialize;
use serde::de::DeserializeOwned;

const SCHEMA: &str = include_str!("schema.sql");
const SCHEMA_VERSION: i64 = 1;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("Datenbank {path:?} konnte nicht geöffnet werden"))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    let version: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0))
        .ok();
    if version.is_none() {
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
            [SCHEMA_VERSION.to_string()],
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Field conversion
// ---------------------------------------------------------------------------

/// Conversion between a struct field and a SQLite value.
pub trait Field: Sized {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self>;
    fn to_value(&self) -> Value;

    fn get(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        let v = row.get_ref(col)?;
        Self::from_ref(v).map_err(|e| match e {
            rusqlite::Error::FromSqlConversionFailure(i, t, err) => rusqlite::Error::FromSqlConversionFailure(
                i,
                t,
                format!("Spalte {col}: {err}").into(),
            ),
            other => other,
        })
    }
}

fn conv_err(msg: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, msg.into().into())
}

fn as_text(v: ValueRef<'_>) -> rusqlite::Result<String> {
    match v {
        ValueRef::Text(t) => Ok(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Integer(i) => Ok(i.to_string()),
        ValueRef::Real(f) => Ok(f.to_string()),
        ValueRef::Null => Ok(String::new()),
        ValueRef::Blob(b) => Ok(String::from_utf8_lossy(b).into_owned()),
    }
}

impl Field for i64 {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        match v {
            ValueRef::Integer(i) => Ok(i),
            ValueRef::Real(f) => Ok(f as i64),
            ValueRef::Null => Ok(0),
            ValueRef::Text(t) => String::from_utf8_lossy(t).trim().parse().map_err(|_| conv_err("keine Ganzzahl")),
            ValueRef::Blob(_) => Err(conv_err("Blob statt Ganzzahl")),
        }
    }
    fn to_value(&self) -> Value {
        Value::Integer(*self)
    }
}

impl Field for bool {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        Ok(i64::from_ref(v)? != 0)
    }
    fn to_value(&self) -> Value {
        Value::Integer(i64::from(*self))
    }
}

impl Field for f64 {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        match v {
            ValueRef::Integer(i) => Ok(i as f64),
            ValueRef::Real(f) => Ok(f),
            ValueRef::Null => Ok(0.0),
            ValueRef::Text(t) => String::from_utf8_lossy(t).trim().parse().map_err(|_| conv_err("keine Zahl")),
            ValueRef::Blob(_) => Err(conv_err("Blob statt Zahl")),
        }
    }
    fn to_value(&self) -> Value {
        Value::Real(*self)
    }
}

impl Field for String {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        as_text(v)
    }
    fn to_value(&self) -> Value {
        Value::Text(self.clone())
    }
}

impl Field for Decimal {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        match v {
            ValueRef::Null => Ok(Decimal::ZERO),
            ValueRef::Integer(i) => Ok(Decimal::from(i)),
            ValueRef::Real(f) => Decimal::from_str(&f.to_string()).map_err(|_| conv_err("ungültiger Betrag")),
            other => {
                let s = as_text(other)?;
                if s.trim().is_empty() {
                    return Ok(Decimal::ZERO);
                }
                Decimal::from_str(s.trim()).map_err(|_| conv_err(format!("ungültiger Betrag '{s}'")))
            }
        }
    }
    fn to_value(&self) -> Value {
        Value::Text(self.normalize().to_string())
    }
}

impl Field for NaiveDate {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        let s = as_text(v)?;
        NaiveDate::parse_from_str(s.get(..10).unwrap_or(&s), "%Y-%m-%d")
            .map_err(|_| conv_err(format!("ungültiges Datum '{s}'")))
    }
    fn to_value(&self) -> Value {
        Value::Text(self.format("%Y-%m-%d").to_string())
    }
}

impl Field for NaiveDateTime {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        let s = as_text(v)?;
        NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").map_err(|_| conv_err(format!("ungültiger Zeitstempel '{s}'")))
    }
    fn to_value(&self) -> Value {
        Value::Text(self.format("%Y-%m-%d %H:%M:%S").to_string())
    }
}

impl<T: Field> Field for Option<T> {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        match v {
            ValueRef::Null => Ok(None),
            ValueRef::Text(t) if t.is_empty() => Ok(None),
            other => T::from_ref(other).map(Some),
        }
    }
    fn to_value(&self) -> Value {
        match self {
            Some(v) => v.to_value(),
            None => Value::Null,
        }
    }
}

impl<T: Serialize + DeserializeOwned> Field for Vec<T> {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        let s = as_text(v)?;
        if s.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&s).map_err(|e| conv_err(format!("ungültiges JSON: {e}")))
    }
    fn to_value(&self) -> Value {
        Value::Text(serde_json::to_string(self).unwrap_or_else(|_| "[]".into()))
    }
}

/// Implements [`Field`] for a struct that is stored as JSON text.
#[macro_export]
macro_rules! json_field {
    ($t:ty) => {
        impl $crate::db::Field for $t {
            fn from_ref(v: rusqlite::types::ValueRef<'_>) -> rusqlite::Result<Self> {
                let s = <String as $crate::db::Field>::from_ref(v)?;
                if s.trim().is_empty() {
                    return Ok(Default::default());
                }
                serde_json::from_str(&s).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
                })
            }
            fn to_value(&self) -> rusqlite::types::Value {
                rusqlite::types::Value::Text(serde_json::to_string(self).unwrap_or_default())
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

pub trait Entity: Sized {
    const TABLE: &'static str;
    const COLUMNS: &'static [&'static str];
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self>;
    fn values(&self) -> Vec<Value>;
    fn id(&self) -> i64;
    fn set_id(&mut self, id: i64);
}

/// Declares a table-backed struct. The first field must be `id: i64`.
#[macro_export]
macro_rules! entity {
    (
        $(#[$smeta:meta])*
        pub struct $name:ident in $table:literal {
            $( $(#[$fmeta:meta])* pub $field:ident : $ty:ty ),* $(,)?
        }
    ) => {
        $(#[$smeta])*
        #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, ts_rs::TS)]
        #[serde(default)]
        #[ts(export)]
        pub struct $name { $( $(#[$fmeta])* pub $field: $ty ),* }

        impl $crate::db::Entity for $name {
            const TABLE: &'static str = $table;
            const COLUMNS: &'static [&'static str] = &[$(stringify!($field)),*];
            fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
                Ok(Self { $( $field: <$ty as $crate::db::Field>::get(row, stringify!($field))? ),* })
            }
            fn values(&self) -> Vec<rusqlite::types::Value> {
                vec![$( $crate::db::Field::to_value(&self.$field) ),*]
            }
            fn id(&self) -> i64 { self.id }
            fn set_id(&mut self, id: i64) { self.id = id; }
        }
    };
}

pub fn insert<T: Entity>(conn: &Connection, item: &mut T) -> Result<i64> {
    let values = item.values();
    let (cols, vals): (Vec<&str>, Vec<Value>) = if item.id() > 0 {
        (T::COLUMNS.to_vec(), values)
    } else {
        (T::COLUMNS[1..].to_vec(), values.into_iter().skip(1).collect())
    };
    let placeholders = (1..=cols.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
    let sql = format!("INSERT INTO {} ({}) VALUES ({})", T::TABLE, cols.join(", "), placeholders);
    conn.execute(&sql, params_from_iter(vals.iter()))
        .with_context(|| format!("Einfügen in {} fehlgeschlagen", T::TABLE))?;
    let id = conn.last_insert_rowid();
    item.set_id(id);
    Ok(id)
}

pub fn update<T: Entity>(conn: &Connection, item: &T) -> Result<()> {
    let values = item.values();
    let sets = T::COLUMNS[1..]
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{c} = ?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("UPDATE {} SET {} WHERE id = ?{}", T::TABLE, sets, T::COLUMNS.len());
    let mut params: Vec<Value> = values.into_iter().skip(1).collect();
    params.push(Value::Integer(item.id()));
    conn.execute(&sql, params_from_iter(params.iter()))
        .with_context(|| format!("Aktualisieren von {} fehlgeschlagen", T::TABLE))?;
    Ok(())
}

pub fn find<T: Entity>(conn: &Connection, id: i64) -> Result<Option<T>> {
    let sql = format!("SELECT * FROM {} WHERE id = ?1", T::TABLE);
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(row) => Ok(Some(T::from_row(row)?)),
        None => Ok(None),
    }
}

pub fn get<T: Entity>(conn: &Connection, id: i64) -> Result<T> {
    find(conn, id)?.ok_or_else(|| crate::error::NotFound(format!("{} #{id} nicht gefunden", T::TABLE)).into())
}

/// `SELECT * FROM table <suffix>` with positional parameters.
pub fn query<T: Entity>(conn: &Connection, suffix: &str, params: &[Value]) -> Result<Vec<T>> {
    let sql = format!("SELECT * FROM {} {}", T::TABLE, suffix);
    let mut stmt = conn.prepare_cached(&sql).with_context(|| format!("SQL: {sql}"))?;
    let rows = stmt.query_map(params_from_iter(params.iter()), |r| T::from_row(r))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn query_one<T: Entity>(conn: &Connection, suffix: &str, params: &[Value]) -> Result<Option<T>> {
    Ok(query::<T>(conn, suffix, params)?.into_iter().next())
}

pub fn delete<T: Entity>(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(&format!("DELETE FROM {} WHERE id = ?1", T::TABLE), [id])?;
    Ok(())
}

/// Helper for building parameter lists.
pub fn v<T: Field>(x: T) -> Value {
    x.to_value()
}

pub fn scalar_i64(conn: &Connection, sql: &str, params: &[Value]) -> Result<i64> {
    Ok(conn.query_row(sql, params_from_iter(params.iter()), |r| r.get::<_, Option<i64>>(0))?.unwrap_or(0))
}

pub fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).ok())
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}
