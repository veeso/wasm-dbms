// Rust guideline compliant 2026-02-28

//! WIT Component Model guest example for wasm-dbms.
//!
//! This crate wraps the wasm-dbms engine behind a WIT-exported `database`
//! interface. A [`FileMemoryProvider`] gives the DBMS persistent,
//! file-backed storage so that data survives across invocations.

pub mod file_provider;
pub mod schema;

use std::cell::RefCell;

use ::wasm_dbms::prelude::{DatabaseSchema as _, DbmsContext, WasmDbmsDatabase};
use wasm_dbms_api::prelude::*;

use crate::file_provider::FileMemoryProvider;
use crate::schema::{ExampleDatabaseSchema, register_tables};

wit_bindgen::generate!({
    world: "dbms",
    path: "../../../../wit/dbms.wit",
});

use crate::wasm_dbms::dbms::types as wit;

/// Database file path (relative to WASI preopened directory).
const DB_FILE_PATH: &str = "wasm-dbms.db";

thread_local! {
    static DBMS_CTX: RefCell<Option<DbmsContext<FileMemoryProvider>>> = const { RefCell::new(None) };
}

/// Runs `f` against the lazily initialised DBMS context.
fn with_dbms<F, R>(f: F) -> R
where
    F: FnOnce(&DbmsContext<FileMemoryProvider>) -> R,
{
    DBMS_CTX.with(|cell| {
        let mut ctx = cell.borrow_mut();
        if ctx.is_none() {
            let provider =
                FileMemoryProvider::new(DB_FILE_PATH).expect("Failed to open database file");
            let dbms_ctx = DbmsContext::new(provider);
            register_tables(&dbms_ctx).expect("Failed to register tables");
            *ctx = Some(dbms_ctx);
        }
        f(ctx.as_ref().unwrap())
    })
}

// ── Value conversion ────────────────────────────────────────────────

fn wit_value_to_dbms(v: wit::Value) -> Value {
    use wasm_dbms_api::prelude as t;
    match v {
        wit::Value::BoolVal(b) => Value::Boolean(t::Boolean(b)),
        wit::Value::U8Val(n) => Value::Uint8(t::Uint8(n)),
        wit::Value::U16Val(n) => Value::Uint16(t::Uint16(n)),
        wit::Value::U32Val(n) => Value::Uint32(t::Uint32(n)),
        wit::Value::U64Val(n) => Value::Uint64(t::Uint64(n)),
        wit::Value::I8Val(n) => Value::Int8(t::Int8(n)),
        wit::Value::I16Val(n) => Value::Int16(t::Int16(n)),
        wit::Value::I32Val(n) => Value::Int32(t::Int32(n)),
        wit::Value::I64Val(n) => Value::Int64(t::Int64(n)),
        wit::Value::F32Val(f) => Value::Decimal(t::Decimal(rust_decimal::Decimal::try_from(f).unwrap_or_default())),
        wit::Value::F64Val(f) => Value::Decimal(t::Decimal(rust_decimal::Decimal::try_from(f).unwrap_or_default())),
        wit::Value::TextVal(s) => Value::Text(t::Text(s)),
        wit::Value::BlobVal(b) => Value::Blob(t::Blob(b)),
        wit::Value::NullVal => Value::Null,
    }
}

fn dbms_value_to_wit(v: Value) -> wit::Value {
    match v {
        Value::Boolean(b) => wit::Value::BoolVal(b.0),
        Value::Uint8(n) => wit::Value::U8Val(n.0),
        Value::Uint16(n) => wit::Value::U16Val(n.0),
        Value::Uint32(n) => wit::Value::U32Val(n.0),
        Value::Uint64(n) => wit::Value::U64Val(n.0),
        Value::Int8(n) => wit::Value::I8Val(n.0),
        Value::Int16(n) => wit::Value::I16Val(n.0),
        Value::Int32(n) => wit::Value::I32Val(n.0),
        Value::Int64(n) => wit::Value::I64Val(n.0),
        Value::Text(s) => wit::Value::TextVal(s.0),
        Value::Blob(b) => wit::Value::BlobVal(b.0),
        Value::Null => wit::Value::NullVal,
        Value::Decimal(d) => {
            use rust_decimal::prelude::ToPrimitive as _;
            wit::Value::F64Val(d.0.to_f64().unwrap_or(0.0))
        }
        Value::Date(d) => wit::Value::TextVal(d.to_string()),
        Value::DateTime(dt) => wit::Value::TextVal(dt.to_string()),
        Value::Json(j) => wit::Value::TextVal(j.value().to_string()),
        Value::Uuid(u) => wit::Value::TextVal(u.0.to_string()),
        Value::Custom(c) => wit::Value::BlobVal(c.encoded),
    }
}

// ── Error conversion ────────────────────────────────────────────────

fn dbms_error_to_wit(e: DbmsError) -> wit::DbmsError {
    match e {
        DbmsError::Memory(m) => wit::DbmsError::MemoryError(m.to_string()),
        DbmsError::Query(QueryError::TableNotFound(t)) => wit::DbmsError::TableNotFound(t),
        DbmsError::Query(q) => wit::DbmsError::IntegrityError(q.to_string()),
        DbmsError::Table(t) => wit::DbmsError::TableNotFound(t.to_string()),
        DbmsError::Transaction(t) => wit::DbmsError::TransactionError(t.to_string()),
        DbmsError::Sanitize(s) => wit::DbmsError::ValidationError(s),
        DbmsError::Validation(v) => wit::DbmsError::ValidationError(v),
    }
}

// ── Query conversion ────────────────────────────────────────────────

fn wit_query_to_dbms(q: wit::Query) -> Query {
    let mut builder = Query::builder();

    if let Some(filter_json) = q.filter {
        if let Ok(filter) = serde_json::from_str::<Filter>(&filter_json) {
            builder = builder.filter(Some(filter));
        }
    }

    if let Some(ref order_col) = q.order_by {
        builder = match q.order_dir {
            Some(wit::OrderDirection::Desc) => builder.order_by_desc(order_col),
            _ => builder.order_by_asc(order_col),
        };
    }

    if let Some(limit) = q.limit {
        builder = builder.limit(limit as usize);
    }

    if let Some(offset) = q.offset {
        builder = builder.offset(offset as usize);
    }

    builder.build()
}

// ── Row conversion ──────────────────────────────────────────────────

fn wit_row_to_named_values(row: Vec<wit::ColumnValue>) -> Vec<(String, Value)> {
    row.into_iter()
        .map(|cv| (cv.name, wit_value_to_dbms(cv.value)))
        .collect()
}

fn dbms_row_to_wit(row: Vec<(ColumnDef, Value)>) -> Vec<wit::ColumnValue> {
    row.into_iter()
        .map(|(col, val)| wit::ColumnValue {
            name: col.name.to_string(),
            value: dbms_value_to_wit(val),
        })
        .collect()
}

// ── Column matching ─────────────────────────────────────────────────

/// Matches named string values against `ColumnDef` entries for a table.
fn match_column_defs(
    table: &str,
    named_values: Vec<(String, Value)>,
) -> DbmsResult<Vec<(ColumnDef, Value)>> {
    let columns = table_columns(table)?;
    let mut result = Vec::with_capacity(named_values.len());
    for (name, value) in named_values {
        let col_def = columns
            .iter()
            .find(|c| c.name == name)
            .ok_or_else(|| DbmsError::Query(QueryError::UnknownColumn(name.clone())))?;
        result.push((*col_def, value));
    }
    Ok(result)
}

/// Returns column definitions for a known table.
fn table_columns(table: &str) -> DbmsResult<&'static [ColumnDef]> {
    match table {
        "users" => Ok(schema::User::columns()),
        "posts" => Ok(schema::Post::columns()),
        _ => Err(DbmsError::Query(QueryError::TableNotFound(
            table.to_string(),
        ))),
    }
}

/// Leaks a string into `&'static str`.
///
/// `DatabaseSchema` methods require `&'static str` table names, but the
/// WIT boundary delivers owned `String`s. Leaking is acceptable in a WASM
/// component where the process lifetime is bounded and the leaked strings
/// are small table names.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

// ── Guest implementation ────────────────────────────────────────────

struct GuestDbms;

export!(GuestDbms);

impl exports::wasm_dbms::dbms::database::Guest for GuestDbms {
    fn select(
        table: String,
        query: wit::Query,
    ) -> Result<Vec<wit::Row>, wit::DbmsError> {
        let query = wit_query_to_dbms(query);
        with_dbms(|ctx| {
            let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
            db.select_raw(&table, query)
                .map(|rows| rows.into_iter().map(dbms_row_to_wit).collect())
                .map_err(dbms_error_to_wit)
        })
    }

    fn insert(
        table: String,
        values: wit::Row,
        tx: Option<wit::TransactionId>,
    ) -> Result<(), wit::DbmsError> {
        let named_values = wit_row_to_named_values(values);
        with_dbms(|ctx| {
            let col_values =
                match_column_defs(&table, named_values).map_err(dbms_error_to_wit)?;
            let table_name = leak_str(&table);

            if let Some(tx_id) = tx {
                let db =
                    WasmDbmsDatabase::from_transaction(ctx, ExampleDatabaseSchema, tx_id);
                ExampleDatabaseSchema
                    .insert(&db, table_name, &col_values)
                    .map_err(dbms_error_to_wit)
            } else {
                let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
                ExampleDatabaseSchema
                    .insert(&db, table_name, &col_values)
                    .map_err(dbms_error_to_wit)
            }
        })
    }

    fn update(
        table: String,
        values: wit::Row,
        tx: Option<wit::TransactionId>,
    ) -> Result<u64, wit::DbmsError> {
        let named_values = wit_row_to_named_values(values);
        with_dbms(|ctx| {
            let col_values =
                match_column_defs(&table, named_values).map_err(dbms_error_to_wit)?;
            let table_name = leak_str(&table);

            if let Some(tx_id) = tx {
                let db =
                    WasmDbmsDatabase::from_transaction(ctx, ExampleDatabaseSchema, tx_id);
                ExampleDatabaseSchema
                    .update(&db, table_name, &col_values, None)
                    .map_err(dbms_error_to_wit)
            } else {
                let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
                ExampleDatabaseSchema
                    .update(&db, table_name, &col_values, None)
                    .map_err(dbms_error_to_wit)
            }
        })
    }

    fn delete(
        table: String,
        filter: Option<String>,
        tx: Option<wit::TransactionId>,
    ) -> Result<u64, wit::DbmsError> {
        let filter = filter.and_then(|f| serde_json::from_str::<Filter>(&f).ok());
        with_dbms(|ctx| {
            let table_name = leak_str(&table);

            if let Some(tx_id) = tx {
                let db =
                    WasmDbmsDatabase::from_transaction(ctx, ExampleDatabaseSchema, tx_id);
                ExampleDatabaseSchema
                    .delete(&db, table_name, DeleteBehavior::Restrict, filter)
                    .map_err(dbms_error_to_wit)
            } else {
                let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
                ExampleDatabaseSchema
                    .delete(&db, table_name, DeleteBehavior::Restrict, filter)
                    .map_err(dbms_error_to_wit)
            }
        })
    }

    fn begin_transaction() -> Result<wit::TransactionId, wit::DbmsError> {
        with_dbms(|ctx| Ok(ctx.begin_transaction(vec![0u8])))
    }

    fn commit(tx: wit::TransactionId) -> Result<(), wit::DbmsError> {
        with_dbms(|ctx| {
            let mut db = WasmDbmsDatabase::from_transaction(ctx, ExampleDatabaseSchema, tx);
            db.commit().map_err(dbms_error_to_wit)
        })
    }

    fn rollback(tx: wit::TransactionId) -> Result<(), wit::DbmsError> {
        with_dbms(|ctx| {
            let mut db = WasmDbmsDatabase::from_transaction(ctx, ExampleDatabaseSchema, tx);
            db.rollback().map_err(dbms_error_to_wit)
        })
    }
}
