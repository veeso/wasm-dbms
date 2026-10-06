// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! WIT Component Model guest example for wasm-dbms.
//!
//! This crate wraps the wasm-dbms engine behind a WIT-exported `database`
//! interface. A [`FileMemoryProvider`] gives the DBMS persistent,
//! file-backed storage so that data survives across invocations.

pub mod file_provider;
pub mod schema;

use std::cell::RefCell;
use std::path::Path;

use ::wasm_dbms::prelude::{DatabaseSchema as _, DbmsContext, WasmDbmsDatabase};
use wasm_dbms_api::prelude::*;

use crate::file_provider::FileMemoryProvider;
use crate::schema::ExampleDatabaseSchema;

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

/// Opens the database file at `path` and registers the example tables.
///
/// # Errors
///
/// Returns [`wit::DbmsError::MemoryError`] when the file cannot be opened or
/// created, and the converted [`DbmsError`] when table registration fails.
fn open_dbms(path: &Path) -> Result<DbmsContext<FileMemoryProvider>, wit::DbmsError> {
    let provider =
        FileMemoryProvider::new(path).map_err(|e| wit::DbmsError::MemoryError(e.to_string()))?;
    let dbms_ctx = DbmsContext::new(provider);
    ExampleDatabaseSchema::register_tables(&dbms_ctx).map_err(dbms_error_to_wit)?;
    Ok(dbms_ctx)
}

/// Runs `f` against the lazily initialised DBMS context.
///
/// The context is created on first use with [`open_dbms`]. A failed
/// initialization is not cached, so the next call retries it.
///
/// # Errors
///
/// Returns the initialization error from [`open_dbms`] without calling `f`.
fn with_dbms<F, R>(f: F) -> Result<R, wit::DbmsError>
where
    F: FnOnce(&DbmsContext<FileMemoryProvider>) -> R,
{
    DBMS_CTX.with(|cell| {
        let mut ctx = cell.borrow_mut();
        let dbms_ctx = match ctx.take() {
            Some(dbms_ctx) => dbms_ctx,
            None => open_dbms(Path::new(DB_FILE_PATH))?,
        };
        Ok(f(ctx.insert(dbms_ctx)))
    })
}

// ── Value conversion ────────────────────────────────────────────────

/// Converts a WIT [`wit::Value`] into a wasm-dbms [`Value`].
///
/// String-encoded variants are parsed strictly so that every value produced by
/// [`dbms_value_to_wit`] converts back to an equal [`Value`]:
///
/// - `decimal-val`: exact decimal notation, e.g. `-12.3450`.
/// - `date-val`: `YYYY-MM-DD`, validated against the calendar.
/// - `datetime-val`: `YYYY-MM-DDTHH:MM:SS[.ffffff](Z|±HH:MM)`, the RFC 3339
///   subset emitted by [`DateTime`]'s `Display` implementation.
/// - `json-val`: any well-formed JSON document.
/// - `uuid-val`: the hyphenated 36-character form.
///
/// # Errors
///
/// Returns [`wit::DbmsError::InvalidQuery`] when a string-encoded value is
/// malformed, so invalid input never reaches the database as `NULL` or as a
/// default value.
fn wit_value_to_dbms(v: wit::Value) -> Result<Value, wit::DbmsError> {
    use wasm_dbms_api::prelude as t;
    let value = match v {
        wit::Value::BoolVal(b) => Value::Boolean(t::Boolean(b)),
        wit::Value::U8Val(n) => Value::Uint8(t::Uint8(n)),
        wit::Value::U16Val(n) => Value::Uint16(t::Uint16(n)),
        wit::Value::U32Val(n) => Value::Uint32(t::Uint32(n)),
        wit::Value::U64Val(n) => Value::Uint64(t::Uint64(n)),
        wit::Value::I8Val(n) => Value::Int8(t::Int8(n)),
        wit::Value::I16Val(n) => Value::Int16(t::Int16(n)),
        wit::Value::I32Val(n) => Value::Int32(t::Int32(n)),
        wit::Value::I64Val(n) => Value::Int64(t::Int64(n)),
        wit::Value::TextVal(s) => Value::Text(t::Text(s)),
        wit::Value::BlobVal(b) => Value::Blob(t::Blob(b)),
        wit::Value::DecimalVal(s) => rust_decimal::Decimal::from_str_exact(&s)
            .map(|d| Value::Decimal(t::Decimal(d)))
            .map_err(|e| invalid_value("decimal", &s, &e.to_string()))?,
        wit::Value::DateVal(s) => parse_date(&s)
            .map(Value::Date)
            .ok_or_else(|| invalid_value("date", &s, "expected a calendar date as YYYY-MM-DD"))?,
        wit::Value::DatetimeVal(s) => parse_datetime(&s).map(Value::DateTime).ok_or_else(|| {
            invalid_value(
                "date-time",
                &s,
                "expected YYYY-MM-DDTHH:MM:SS[.ffffff] followed by Z or ±HH:MM",
            )
        })?,
        wit::Value::JsonVal(s) => s
            .parse::<t::Json>()
            .map(Value::Json)
            .map_err(|e| invalid_value("json", &s, &e.to_string()))?,
        wit::Value::UuidVal(s) => parse_uuid(&s).map(Value::Uuid).ok_or_else(|| {
            invalid_value("uuid", &s, "expected the hyphenated 36-character form")
        })?,
        wit::Value::CustomVal(c) => Value::Custom(t::CustomValue {
            type_tag: c.type_tag,
            encoded: c.encoded,
            display: c.display,
        }),
        wit::Value::NullVal => Value::Null,
    };
    Ok(value)
}

/// Builds the error returned for a malformed string-encoded WIT value.
fn invalid_value(kind: &str, raw: &str, reason: &str) -> wit::DbmsError {
    wit::DbmsError::InvalidQuery(format!("invalid {kind} value `{raw}`: {reason}"))
}

/// Parses `s` as an unsigned decimal number made of `min_len..=max_len` ASCII digits.
fn parse_digits<T>(s: &str, min_len: usize, max_len: usize) -> Option<T>
where
    T: std::str::FromStr,
{
    let well_formed =
        (min_len..=max_len).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    well_formed.then(|| s.parse().ok()).flatten()
}

/// Returns the number of days in `month` of `year`, or `None` for an invalid month.
fn days_in_month(year: u16, month: u8) -> Option<u8> {
    let leap_year =
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if leap_year => Some(29),
        2 => Some(28),
        _ => None,
    }
}

/// Parses a `YYYY-MM-DD` date string into a calendar-valid [`Date`].
///
/// Returns `None` when the string is not in the `Display` form of [`Date`] or
/// names a day that does not exist, such as `2025-02-30`.
fn parse_date(s: &str) -> Option<Date> {
    let mut parts = s.split('-');
    let year = parse_digits::<u16>(parts.next()?, 4, 5)?;
    let month = parse_digits::<u8>(parts.next()?, 2, 2)?;
    let day = parse_digits::<u8>(parts.next()?, 2, 2)?;
    if parts.next().is_some() {
        return None;
    }
    days_in_month(year, month)
        .is_some_and(|max_day| (1..=max_day).contains(&day))
        .then_some(Date { year, month, day })
}

/// Parses an RFC 3339 date-time string into a calendar-valid [`DateTime`].
///
/// Accepts `YYYY-MM-DDTHH:MM:SS`, an optional fraction of one to six digits,
/// and either `Z` or a `±HH:MM` offset smaller than one day, which covers the
/// `Display` form of [`DateTime`].
fn parse_datetime(s: &str) -> Option<DateTime> {
    let (date, time) = s.split_once('T')?;
    let Date { year, month, day } = parse_date(date)?;

    let (clock, timezone_offset_minutes) = match time.strip_suffix('Z') {
        Some(clock) => (clock, 0),
        None => {
            let offset_start = time.rfind(['+', '-'])?;
            let (clock, offset) = time.split_at(offset_start);
            (clock, parse_offset(offset)?)
        }
    };

    let (hms, fraction) = match clock.split_once('.') {
        Some((hms, fraction)) => (hms, Some(fraction)),
        None => (clock, None),
    };
    let mut hms = hms.split(':');
    let hour = parse_digits::<u8>(hms.next()?, 2, 2).filter(|h| *h < 24)?;
    let minute = parse_digits::<u8>(hms.next()?, 2, 2).filter(|m| *m < 60)?;
    let second = parse_digits::<u8>(hms.next()?, 2, 2).filter(|s| *s < 60)?;
    if hms.next().is_some() {
        return None;
    }
    let microsecond = match fraction {
        Some(fraction) => {
            let digits = parse_digits::<u32>(fraction, 1, 6)?;
            digits * 10_u32.pow(6 - fraction.len() as u32)
        }
        None => 0,
    };

    Some(DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        microsecond,
        timezone_offset_minutes,
    })
}

/// Parses a `±HH:MM` timezone offset smaller than one day into minutes.
fn parse_offset(offset: &str) -> Option<i16> {
    let (sign, rest) = match offset.split_at_checked(1)? {
        ("+", rest) => (1, rest),
        ("-", rest) => (-1, rest),
        _ => return None,
    };
    let (hours, minutes) = rest.split_once(':')?;
    let hours = parse_digits::<i16>(hours, 2, 2).filter(|h| *h < 24)?;
    let minutes = parse_digits::<i16>(minutes, 2, 2).filter(|m| *m < 60)?;
    Some(sign * (hours * 60 + minutes))
}

/// Parses a hyphenated UUID string such as `550e8400-e29b-41d4-a716-446655440000`.
///
/// Hex digits may be upper or lower case.
fn parse_uuid(s: &str) -> Option<Uuid> {
    let well_formed = s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    if !well_formed {
        return None;
    }
    let hex = s.replace('-', "");
    let bits = u128::from_str_radix(&hex, 16).ok()?;
    Uuid::decode(std::borrow::Cow::Owned(bits.to_be_bytes().to_vec())).ok()
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
        Value::Decimal(d) => wit::Value::DecimalVal(d.0.to_string()),
        Value::Date(d) => wit::Value::DateVal(d.to_string()),
        Value::DateTime(dt) => wit::Value::DatetimeVal(dt.to_string()),
        Value::Json(j) => wit::Value::JsonVal(j.value().to_string()),
        Value::Uuid(u) => wit::Value::UuidVal(u.0.to_string()),
        Value::Custom(c) => wit::Value::CustomVal(wit::CustomValue {
            type_tag: c.type_tag,
            encoded: c.encoded,
            display: c.display,
        }),
    }
}

// ── Error conversion ────────────────────────────────────────────────

fn dbms_error_to_wit(e: DbmsError) -> wit::DbmsError {
    match e {
        DbmsError::Memory(m) => wit::DbmsError::MemoryError(m.to_string()),
        DbmsError::Migration(m) => wit::DbmsError::MigrationError(m.to_string()),
        DbmsError::Query(q) => query_error_to_wit(q),
        DbmsError::Table(t) => wit::DbmsError::TableNotFound(t.to_string()),
        DbmsError::Transaction(_) => wit::DbmsError::TransactionNotFound,
        DbmsError::Sanitize(s) => wit::DbmsError::SanitizationError(s),
        DbmsError::Validation(v) => wit::DbmsError::ValidationError(v),
    }
}

fn query_error_to_wit(q: QueryError) -> wit::DbmsError {
    match q {
        QueryError::PrimaryKeyConflict => wit::DbmsError::PrimaryKeyConflict,
        QueryError::UniqueConstraintViolation { field } => {
            wit::DbmsError::UniqueConstraintViolation(field)
        }
        QueryError::BrokenForeignKeyReference { table, key } => {
            wit::DbmsError::BrokenForeignKeyReference(format!("{table}: {key:?}"))
        }
        QueryError::ForeignKeyConstraintViolation {
            referencing_table,
            field,
        } => wit::DbmsError::ForeignKeyConstraintViolation(format!("{referencing_table}.{field}")),
        QueryError::UnknownColumn(c) => wit::DbmsError::UnknownColumn(c),
        QueryError::MissingNonNullableField(f) => wit::DbmsError::MissingNonNullableField(f),
        QueryError::TransactionNotFound => wit::DbmsError::TransactionNotFound,
        QueryError::InvalidQuery(msg) => wit::DbmsError::InvalidQuery(msg),
        QueryError::JoinInsideTypedSelect => wit::DbmsError::JoinInsideTypedSelect,
        QueryError::AggregateClauseInSelect => wit::DbmsError::AggregateClauseInSelect,
        QueryError::ConstraintViolation(msg) => wit::DbmsError::ConstraintViolation(msg),
        QueryError::MemoryError(m) => wit::DbmsError::MemoryError(m.to_string()),
        QueryError::TableNotFound(t) => wit::DbmsError::TableNotFound(t),
        QueryError::RecordNotFound => wit::DbmsError::InternalError("record not found".into()),
        QueryError::SerializationError(s) => wit::DbmsError::InternalError(s),
        QueryError::Internal(s) => wit::DbmsError::InternalError(s),
    }
}

// ── Query conversion ────────────────────────────────────────────────

fn wit_query_to_dbms(q: wit::Query) -> Result<Query, String> {
    use wasm_dbms_api::prelude::Join;

    let mut builder = Query::builder();

    if let Some(filter_json) = q.filter {
        let filter = serde_json::from_str::<Filter>(&filter_json)
            .map_err(|e| format!("invalid filter JSON: {e}"))?;
        builder = builder.filter(Some(filter));
    }

    if !q.distinct_by.is_empty() {
        builder = builder.distinct(&q.distinct_by);
    }

    for relation in &q.eager_relations {
        builder = builder.with(relation);
    }

    for join_json in &q.joins {
        let join = serde_json::from_str::<Join>(join_json)
            .map_err(|e| format!("invalid join JSON: {e}"))?;
        builder = match join.join_type {
            wasm_dbms_api::prelude::JoinType::Inner => {
                builder.inner_join(&join.table, &join.left_column, &join.right_column)
            }
            wasm_dbms_api::prelude::JoinType::Left => {
                builder.left_join(&join.table, &join.left_column, &join.right_column)
            }
            wasm_dbms_api::prelude::JoinType::Right => {
                builder.right_join(&join.table, &join.left_column, &join.right_column)
            }
            wasm_dbms_api::prelude::JoinType::Full => {
                builder.full_join(&join.table, &join.left_column, &join.right_column)
            }
        };
    }

    if !q.group_by.is_empty() {
        builder = builder.group_by(&q.group_by);
    }

    if let Some(having_json) = q.having {
        let filter = serde_json::from_str::<Filter>(&having_json)
            .map_err(|e| format!("invalid having JSON: {e}"))?;
        builder = builder.having(filter);
    }

    for key in q.order_by {
        builder = match key.direction {
            wit::OrderDirection::Asc => builder.order_by_asc(&key.column),
            wit::OrderDirection::Desc => builder.order_by_desc(&key.column),
        };
    }

    if let Some(limit) = q.limit {
        builder = builder.limit(limit as usize);
    }

    if let Some(offset) = q.offset {
        builder = builder.offset(offset as usize);
    }

    Ok(builder.build())
}

fn wit_aggregate_to_dbms(a: wit::AggregateFunction) -> AggregateFunction {
    match a {
        wit::AggregateFunction::Count(col) => AggregateFunction::Count(col),
        wit::AggregateFunction::Sum(c) => AggregateFunction::Sum(c),
        wit::AggregateFunction::Avg(c) => AggregateFunction::Avg(c),
        wit::AggregateFunction::Min(c) => AggregateFunction::Min(c),
        wit::AggregateFunction::Max(c) => AggregateFunction::Max(c),
    }
}

fn aggregated_value_to_wit(v: AggregatedValue) -> wit::AggregatedValue {
    match v {
        AggregatedValue::Count(n) => wit::AggregatedValue::Count(n),
        AggregatedValue::Sum(v) => wit::AggregatedValue::Sum(dbms_value_to_wit(v)),
        AggregatedValue::Avg(v) => wit::AggregatedValue::Avg(dbms_value_to_wit(v)),
        AggregatedValue::Min(v) => wit::AggregatedValue::Min(dbms_value_to_wit(v)),
        AggregatedValue::Max(v) => wit::AggregatedValue::Max(dbms_value_to_wit(v)),
    }
}

fn aggregated_row_to_wit(row: AggregatedRow) -> wit::AggregatedRow {
    wit::AggregatedRow {
        group_keys: row.group_keys.into_iter().map(dbms_value_to_wit).collect(),
        values: row
            .values
            .into_iter()
            .map(aggregated_value_to_wit)
            .collect(),
    }
}

fn parse_filter_json(filter: Option<String>) -> Result<Option<Filter>, wit::DbmsError> {
    filter
        .map(|f| {
            serde_json::from_str::<Filter>(&f)
                .map_err(|e| wit::DbmsError::InvalidQuery(format!("invalid filter JSON: {e}")))
        })
        .transpose()
}

fn wit_delete_behavior(b: wit::DeleteBehavior) -> DeleteBehavior {
    match b {
        wit::DeleteBehavior::Restrict => DeleteBehavior::Restrict,
        wit::DeleteBehavior::Cascade => DeleteBehavior::Cascade,
    }
}

// ── Row conversion ──────────────────────────────────────────────────

/// Converts a WIT row into named wasm-dbms values.
///
/// # Errors
///
/// Returns the first conversion error reported by [`wit_value_to_dbms`].
fn wit_row_to_named_values(
    row: Vec<wit::ColumnValue>,
) -> Result<Vec<(String, Value)>, wit::DbmsError> {
    row.into_iter()
        .map(|cv| wit_value_to_dbms(cv.value).map(|value| (cv.name, value)))
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

/// Returns the names of every table registered by [`ExampleDatabaseSchema`].
fn registered_table_names() -> [&'static str; 2] {
    [schema::User::table_name(), schema::Post::table_name()]
}

/// Resolves a caller-provided table name to the `&'static str` name of a
/// registered table.
///
/// `DatabaseSchema` methods require `&'static str` table names, while the WIT
/// boundary delivers owned `String`s. Looking the name up in the finite set of
/// registered tables yields the schema's own static name, so no
/// caller-provided string is ever leaked or retained.
///
/// # Errors
///
/// Returns [`wit::DbmsError::TableNotFound`] when no registered table has the
/// given name.
fn resolve_table_name(name: &str) -> Result<&'static str, wit::DbmsError> {
    registered_table_names()
        .into_iter()
        .find(|table| *table == name)
        .ok_or_else(|| wit::DbmsError::TableNotFound(name.to_string()))
}

// ── Migration conversion ────────────────────────────────────────────

fn data_type_to_wit(t: DataTypeSnapshot) -> wit::DataTypeSnapshot {
    match t {
        DataTypeSnapshot::Int8 => wit::DataTypeSnapshot::Int8,
        DataTypeSnapshot::Int16 => wit::DataTypeSnapshot::Int16,
        DataTypeSnapshot::Int32 => wit::DataTypeSnapshot::Int32,
        DataTypeSnapshot::Int64 => wit::DataTypeSnapshot::Int64,
        DataTypeSnapshot::Uint8 => wit::DataTypeSnapshot::Uint8,
        DataTypeSnapshot::Uint16 => wit::DataTypeSnapshot::Uint16,
        DataTypeSnapshot::Uint32 => wit::DataTypeSnapshot::Uint32,
        DataTypeSnapshot::Uint64 => wit::DataTypeSnapshot::Uint64,
        DataTypeSnapshot::Float32 => wit::DataTypeSnapshot::Float32,
        DataTypeSnapshot::Float64 => wit::DataTypeSnapshot::Float64,
        DataTypeSnapshot::Decimal => wit::DataTypeSnapshot::Decimal,
        DataTypeSnapshot::Boolean => wit::DataTypeSnapshot::Boolean,
        DataTypeSnapshot::Date => wit::DataTypeSnapshot::Date,
        DataTypeSnapshot::Datetime => wit::DataTypeSnapshot::Datetime,
        DataTypeSnapshot::Blob => wit::DataTypeSnapshot::Blob,
        DataTypeSnapshot::Text => wit::DataTypeSnapshot::Text,
        DataTypeSnapshot::Uuid => wit::DataTypeSnapshot::Uuid,
        DataTypeSnapshot::Json => wit::DataTypeSnapshot::Json,
        DataTypeSnapshot::Custom(meta) => {
            wit::DataTypeSnapshot::Custom(wit::CustomDataTypeSnapshot {
                tag: meta.tag.clone(),
                wire_size: match meta.wire_size {
                    wasm_dbms_api::prelude::WireSize::Fixed(n) => wit::WireSize::Fixed(n),
                    wasm_dbms_api::prelude::WireSize::LengthPrefixed => {
                        wit::WireSize::LengthPrefixed
                    }
                },
            })
        }
    }
}

fn on_delete_to_wit(d: OnDeleteSnapshot) -> wit::OnDeleteSnapshot {
    match d {
        OnDeleteSnapshot::Restrict => wit::OnDeleteSnapshot::Restrict,
        OnDeleteSnapshot::Cascade => wit::OnDeleteSnapshot::Cascade,
    }
}

fn fk_snapshot_to_wit(fk: ForeignKeySnapshot) -> wit::ForeignKeySnapshot {
    wit::ForeignKeySnapshot {
        table: fk.table,
        column: fk.column,
        on_delete: on_delete_to_wit(fk.on_delete),
    }
}

fn column_snapshot_to_wit(c: ColumnSnapshot) -> wit::ColumnSnapshot {
    wit::ColumnSnapshot {
        name: c.name,
        data_type: data_type_to_wit(c.data_type),
        nullable: c.nullable,
        auto_increment: c.auto_increment,
        unique: c.unique,
        primary_key: c.primary_key,
        foreign_key: c.foreign_key.map(fk_snapshot_to_wit),
        default: c.default.map(dbms_value_to_wit),
    }
}

fn index_snapshot_to_wit(i: IndexSnapshot) -> wit::IndexSnapshot {
    wit::IndexSnapshot {
        columns: i.columns,
        unique: i.unique,
    }
}

fn table_snapshot_to_wit(s: TableSchemaSnapshot) -> wit::TableSchemaSnapshot {
    wit::TableSchemaSnapshot {
        version: s.version,
        name: s.name,
        primary_key: s.primary_key,
        alignment: s.alignment,
        columns: s.columns.into_iter().map(column_snapshot_to_wit).collect(),
        indexes: s.indexes.into_iter().map(index_snapshot_to_wit).collect(),
    }
}

fn column_changes_to_wit(c: ColumnChanges) -> wit::ColumnChanges {
    wit::ColumnChanges {
        nullable: c.nullable,
        unique: c.unique,
        auto_increment: c.auto_increment,
        primary_key: c.primary_key,
        foreign_key: c.foreign_key.map(|fk| match fk {
            None => wit::ForeignKeyChange::Drop,
            Some(fk) => wit::ForeignKeyChange::Set(fk_snapshot_to_wit(fk)),
        }),
    }
}

fn migration_op_to_wit(op: MigrationOp) -> wit::MigrationOp {
    match op {
        MigrationOp::CreateTable { name, schema } => {
            wit::MigrationOp::CreateTable(wit::CreateTableOp {
                name,
                schema: table_snapshot_to_wit(schema),
            })
        }
        MigrationOp::DropTable { name } => wit::MigrationOp::DropTable(name),
        MigrationOp::AddColumn { table, column } => wit::MigrationOp::AddColumn(wit::AddColumnOp {
            table,
            column: column_snapshot_to_wit(column),
        }),
        MigrationOp::DropColumn { table, column } => {
            wit::MigrationOp::DropColumn(wit::DropColumnOp { table, column })
        }
        MigrationOp::RenameColumn { table, old, new } => {
            wit::MigrationOp::RenameColumn(wit::RenameColumnOp { table, old, new })
        }
        MigrationOp::AlterColumn {
            table,
            column,
            changes,
        } => wit::MigrationOp::AlterColumn(wit::AlterColumnOp {
            table,
            column,
            changes: column_changes_to_wit(changes),
        }),
        MigrationOp::WidenColumn {
            table,
            column,
            old_type,
            new_type,
        } => wit::MigrationOp::WidenColumn(wit::TypeChangeOp {
            table,
            column,
            old_type: data_type_to_wit(old_type),
            new_type: data_type_to_wit(new_type),
        }),
        MigrationOp::TransformColumn {
            table,
            column,
            old_type,
            new_type,
        } => wit::MigrationOp::TransformColumn(wit::TypeChangeOp {
            table,
            column,
            old_type: data_type_to_wit(old_type),
            new_type: data_type_to_wit(new_type),
        }),
        MigrationOp::AddIndex { table, index } => wit::MigrationOp::AddIndex(wit::IndexOp {
            table,
            index: index_snapshot_to_wit(index),
        }),
        MigrationOp::DropIndex { table, index } => wit::MigrationOp::DropIndex(wit::IndexOp {
            table,
            index: index_snapshot_to_wit(index),
        }),
    }
}

fn wit_migration_policy(p: wit::MigrationPolicy) -> MigrationPolicy {
    MigrationPolicy {
        allow_destructive: p.allow_destructive,
    }
}

// ── Guest implementation ────────────────────────────────────────────

struct GuestDbms;

export!(GuestDbms);

impl exports::wasm_dbms::dbms::database::Guest for GuestDbms {
    fn select(table: String, query: wit::Query) -> Result<Vec<wit::Row>, wit::DbmsError> {
        let query = wit_query_to_dbms(query).map_err(wit::DbmsError::InvalidQuery)?;
        with_dbms(|ctx| {
            let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
            db.select_raw(&table, query)
                .map(|rows| rows.into_iter().map(dbms_row_to_wit).collect())
                .map_err(dbms_error_to_wit)
        })?
    }

    fn insert(
        table: String,
        values: wit::Row,
        tx: Option<wit::TransactionId>,
    ) -> Result<(), wit::DbmsError> {
        let named_values = wit_row_to_named_values(values)?;
        let table_name = resolve_table_name(&table)?;
        with_dbms(|ctx| {
            let col_values =
                match_column_defs(table_name, named_values).map_err(dbms_error_to_wit)?;

            if let Some(tx_id) = tx {
                let db = WasmDbmsDatabase::from_transaction(ctx, ExampleDatabaseSchema, tx_id);
                ExampleDatabaseSchema
                    .insert(&db, table_name, &col_values)
                    .map_err(dbms_error_to_wit)
            } else {
                let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
                ExampleDatabaseSchema
                    .insert(&db, table_name, &col_values)
                    .map_err(dbms_error_to_wit)
            }
        })?
    }

    fn aggregate(
        table: String,
        query: wit::Query,
        aggregates: Vec<wit::AggregateFunction>,
    ) -> Result<Vec<wit::AggregatedRow>, wit::DbmsError> {
        let query = wit_query_to_dbms(query).map_err(wit::DbmsError::InvalidQuery)?;
        let aggs: Vec<AggregateFunction> =
            aggregates.into_iter().map(wit_aggregate_to_dbms).collect();
        let table_name = resolve_table_name(&table)?;
        with_dbms(|ctx| {
            let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
            ExampleDatabaseSchema
                .aggregate(&db, table_name, query, &aggs)
                .map(|rows| rows.into_iter().map(aggregated_row_to_wit).collect())
                .map_err(dbms_error_to_wit)
        })?
    }

    fn update(
        table: String,
        values: wit::Row,
        filter: Option<String>,
        tx: Option<wit::TransactionId>,
    ) -> Result<u64, wit::DbmsError> {
        let filter = parse_filter_json(filter)?;
        let named_values = wit_row_to_named_values(values)?;
        let table_name = resolve_table_name(&table)?;
        with_dbms(|ctx| {
            let col_values =
                match_column_defs(table_name, named_values).map_err(dbms_error_to_wit)?;

            if let Some(tx_id) = tx {
                let db = WasmDbmsDatabase::from_transaction(ctx, ExampleDatabaseSchema, tx_id);
                ExampleDatabaseSchema
                    .update(&db, table_name, &col_values, filter)
                    .map_err(dbms_error_to_wit)
            } else {
                let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
                ExampleDatabaseSchema
                    .update(&db, table_name, &col_values, filter)
                    .map_err(dbms_error_to_wit)
            }
        })?
    }

    fn delete(
        table: String,
        behavior: wit::DeleteBehavior,
        filter: Option<String>,
        tx: Option<wit::TransactionId>,
    ) -> Result<u64, wit::DbmsError> {
        let filter = parse_filter_json(filter)?;
        let behavior = wit_delete_behavior(behavior);
        let table_name = resolve_table_name(&table)?;
        with_dbms(|ctx| {
            if let Some(tx_id) = tx {
                let db = WasmDbmsDatabase::from_transaction(ctx, ExampleDatabaseSchema, tx_id);
                ExampleDatabaseSchema
                    .delete(&db, table_name, behavior, filter)
                    .map_err(dbms_error_to_wit)
            } else {
                let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
                ExampleDatabaseSchema
                    .delete(&db, table_name, behavior, filter)
                    .map_err(dbms_error_to_wit)
            }
        })?
    }

    fn begin_transaction() -> Result<wit::TransactionId, wit::DbmsError> {
        with_dbms(|ctx| ctx.begin_transaction(vec![0u8]))
    }

    fn commit(tx: wit::TransactionId) -> Result<(), wit::DbmsError> {
        with_dbms(|ctx| {
            let mut db = WasmDbmsDatabase::from_transaction(ctx, ExampleDatabaseSchema, tx);
            db.commit().map_err(dbms_error_to_wit)
        })?
    }

    fn rollback(tx: wit::TransactionId) -> Result<(), wit::DbmsError> {
        with_dbms(|ctx| {
            let mut db = WasmDbmsDatabase::from_transaction(ctx, ExampleDatabaseSchema, tx);
            db.rollback().map_err(dbms_error_to_wit)
        })?
    }

    fn has_drift() -> Result<bool, wit::DbmsError> {
        with_dbms(|ctx| {
            let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
            db.has_drift().map_err(dbms_error_to_wit)
        })?
    }

    fn pending_migrations() -> Result<Vec<wit::MigrationOp>, wit::DbmsError> {
        with_dbms(|ctx| {
            let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
            db.pending_migrations()
                .map(|ops| ops.into_iter().map(migration_op_to_wit).collect())
                .map_err(dbms_error_to_wit)
        })?
    }

    fn migrate(policy: wit::MigrationPolicy) -> Result<(), wit::DbmsError> {
        let policy = wit_migration_policy(policy);
        with_dbms(|ctx| {
            let mut db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
            db.migrate(policy).map_err(dbms_error_to_wit)
        })?
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    /// Returns a path under the system temp directory that no other test or process uses.
    fn unique_temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!(
            "wasm-dbms-guest-{label}-{pid}-{nanos}-{count}",
            pid = std::process::id(),
            count = COUNTER.fetch_add(1, Ordering::Relaxed),
        ))
    }

    #[test]
    fn test_open_dbms_reports_unusable_database_path_as_wit_error() {
        let dir = unique_temp_dir("dir-at-db-path");
        std::fs::create_dir(&dir).expect("create test dir");
        let db_path = dir.join(DB_FILE_PATH);
        std::fs::create_dir(&db_path).expect("create directory at the database path");

        let outcome = std::panic::catch_unwind(|| open_dbms(&db_path).map(|_| ()));
        let _ = std::fs::remove_dir_all(&dir);

        let result = outcome.expect("guest initialization must not panic");
        assert!(
            matches!(result, Err(wit::DbmsError::MemoryError(_))),
            "expected a memory error, got {result:?}"
        );
    }

    #[test]
    fn test_open_dbms_registers_tables_on_a_fresh_file() {
        let dir = unique_temp_dir("fresh-db");
        std::fs::create_dir(&dir).expect("create test dir");

        let rows = open_dbms(&dir.join(DB_FILE_PATH)).map(|ctx| {
            WasmDbmsDatabase::oneshot(&ctx, ExampleDatabaseSchema)
                .select_raw("users", Query::builder().build())
                .map(|rows| rows.len())
        });
        let _ = std::fs::remove_dir_all(&dir);

        assert!(matches!(rows, Ok(Ok(0))), "got {rows:?}");
    }

    fn sample_uuid() -> Value {
        let bytes = 0x550e_8400_e29b_41d4_a716_4466_5544_0000_u128.to_be_bytes();
        Value::Uuid(Uuid::decode(Cow::Owned(bytes.to_vec())).expect("valid UUID bytes"))
    }

    fn sample_datetime(timezone_offset_minutes: i16) -> Value {
        Value::DateTime(DateTime {
            year: 2025,
            month: 1,
            day: 2,
            hour: 3,
            minute: 4,
            second: 5,
            microsecond: 123_456,
            timezone_offset_minutes,
        })
    }

    fn assert_invalid(value: wit::Value) {
        let display = format!("{value:?}");
        let outcome = wit_value_to_dbms(value);
        assert!(
            matches!(outcome, Err(wit::DbmsError::InvalidQuery(_))),
            "expected a conversion error for {display}, got {outcome:?}"
        );
    }

    #[test]
    fn test_wit_typed_values_round_trip() {
        let values = [
            sample_uuid(),
            sample_datetime(0),
            sample_datetime(150),
            sample_datetime(-300),
            Value::Date(Date {
                year: 2024,
                month: 2,
                day: 29,
            }),
            Value::Decimal(Decimal(
                rust_decimal::Decimal::from_str_exact("-12.3450").expect("valid decimal"),
            )),
            Value::Json(r#"{"a":[1,2]}"#.parse::<Json>().expect("valid JSON")),
        ];

        for value in values {
            let round_tripped = wit_value_to_dbms(dbms_value_to_wit(value.clone()))
                .unwrap_or_else(|e| panic!("{value:?} must round-trip, got {e:?}"));
            assert_eq!(round_tripped, value);
        }
    }

    #[test]
    fn test_wit_datetime_accepts_rfc3339_utc_designator() {
        let value = wit_value_to_dbms(wit::Value::DatetimeVal("2025-01-02T03:04:05Z".into()))
            .expect("RFC 3339 date-time must parse");
        assert_eq!(
            value,
            Value::DateTime(DateTime {
                year: 2025,
                month: 1,
                day: 2,
                hour: 3,
                minute: 4,
                second: 5,
                microsecond: 0,
                timezone_offset_minutes: 0,
            })
        );
    }

    #[test]
    fn test_wit_malformed_values_are_errors() {
        assert_invalid(wit::Value::DecimalVal("not-a-decimal".into()));
        assert_invalid(wit::Value::DecimalVal(String::new()));
        assert_invalid(wit::Value::DateVal("2025-02-30".into()));
        assert_invalid(wit::Value::DateVal("2023-02-29".into()));
        assert_invalid(wit::Value::DateVal("2025-13-01".into()));
        assert_invalid(wit::Value::DateVal("2025-1-1".into()));
        assert_invalid(wit::Value::DateVal("2025-01-01-01".into()));
        assert_invalid(wit::Value::JsonVal("{".into()));
        assert_invalid(wit::Value::UuidVal("not-a-uuid".into()));
        assert_invalid(wit::Value::UuidVal(
            "550e8400-e29b-41d4-a716-44665544000g".into(),
        ));
        assert_invalid(wit::Value::DatetimeVal("garbage".into()));
        assert_invalid(wit::Value::DatetimeVal(
            "2025-02-30T00:00:00.000000+00:00".into(),
        ));
        assert_invalid(wit::Value::DatetimeVal(
            "2025-01-02T24:00:00.000000+00:00".into(),
        ));
        assert_invalid(wit::Value::DatetimeVal(
            "2025-01-02T03:04:05.1234567+00:00".into(),
        ));
        assert_invalid(wit::Value::DatetimeVal(
            "2025-01-02T03:04:05.000000+24:00".into(),
        ));
        assert_invalid(wit::Value::DatetimeVal("2025-01-02T03:04:05".into()));
    }

    #[test]
    fn test_wit_row_conversion_propagates_malformed_values() {
        let row = vec![
            wit::ColumnValue {
                name: "id".into(),
                value: wit::Value::U32Val(1),
            },
            wit::ColumnValue {
                name: "payload".into(),
                value: wit::Value::JsonVal("{".into()),
            },
        ];

        assert!(matches!(
            wit_row_to_named_values(row),
            Err(wit::DbmsError::InvalidQuery(_))
        ));
    }

    #[test]
    fn test_resolve_table_name_rejects_many_distinct_unknown_names() {
        for n in 0..1_000 {
            let name = format!("missing_{n}");
            let error = resolve_table_name(&name).expect_err("unknown table must be rejected");
            assert!(
                matches!(&error, wit::DbmsError::TableNotFound(table) if *table == name),
                "unexpected error {error:?}"
            );
        }
    }

    #[test]
    fn test_resolve_table_name_returns_the_static_schema_name() {
        for (requested, expected) in [
            (String::from("users"), schema::User::table_name()),
            (String::from("posts"), schema::Post::table_name()),
        ] {
            let resolved = resolve_table_name(&requested).expect("registered table");
            assert_eq!(resolved, expected);
            assert!(std::ptr::eq(resolved, expected));
            assert!(!std::ptr::eq(resolved, requested.as_str()));
        }
    }

    #[test]
    fn test_table_lists_match_the_schema() {
        let mut registered: Vec<String> = registered_table_names()
            .iter()
            .map(ToString::to_string)
            .collect();
        let mut compiled: Vec<String> =
            <ExampleDatabaseSchema as ::wasm_dbms::prelude::DatabaseSchema<FileMemoryProvider>>::compiled_snapshots()
                .into_iter()
                .map(|snapshot| snapshot.name)
                .collect();
        registered.sort();
        compiled.sort();
        assert_eq!(registered, compiled);

        for name in registered_table_names() {
            assert!(table_columns(name).is_ok(), "{name} has no column lookup");
        }
    }
}
