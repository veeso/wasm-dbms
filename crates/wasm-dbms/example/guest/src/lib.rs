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
use std::collections::HashMap;

use ::wasm_dbms::prelude::{DatabaseSchema as _, DbmsContext, WasmDbmsDatabase};
use wasm_dbms_api::prelude::*;
use wasm_dbms_memory::prelude::NoAccessControl;

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
    static DBMS_CTX: RefCell<Option<DbmsContext<FileMemoryProvider, NoAccessControl>>> = const { RefCell::new(None) };
}

/// Runs `f` against the lazily initialised DBMS context.
fn with_dbms<F, R>(f: F) -> R
where
    F: FnOnce(&DbmsContext<FileMemoryProvider, NoAccessControl>) -> R,
{
    DBMS_CTX.with(|cell| {
        let mut ctx = cell.borrow_mut();
        if ctx.is_none() {
            let provider =
                FileMemoryProvider::new(DB_FILE_PATH).expect("Failed to open database file");
            let dbms_ctx = DbmsContext::with_acl(provider);
            ExampleDatabaseSchema::register_tables(&dbms_ctx).expect("Failed to register tables");
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
        wit::Value::TextVal(s) => Value::Text(t::Text(s)),
        wit::Value::BlobVal(b) => Value::Blob(t::Blob(b)),
        wit::Value::DecimalVal(s) => Value::Decimal(t::Decimal(
            rust_decimal::Decimal::from_str_exact(&s).unwrap_or_default(),
        )),
        wit::Value::DateVal(s) => parse_date(&s).map_or(Value::Null, Value::Date),
        wit::Value::DatetimeVal(_) => {
            // The DateTime grammar (with microseconds and timezone offset)
            // isn't worth re-parsing here. Pass DateTime values via the Text
            // variant if you need to round-trip them through the WIT boundary.
            Value::Null
        }
        wit::Value::JsonVal(s) => serde_json::from_str::<serde_json::Value>(&s)
            .map(|j| Value::Json(t::Json::from(j)))
            .unwrap_or(Value::Null),
        wit::Value::UuidVal(_) => {
            // Same rationale as DateTime — round-trip via Text instead.
            Value::Null
        }
        wit::Value::CustomVal(c) => Value::Custom(t::CustomValue {
            type_tag: c.type_tag,
            encoded: c.encoded,
            display: c.display,
        }),
        wit::Value::NullVal => Value::Null,
    }
}

/// Parses a `YYYY-MM-DD` date string into a [`wasm_dbms_api::prelude::Date`].
///
/// The parser is intentionally narrow — anything outside the canonical
/// `Display` form returns `None` and is mapped to `Value::Null` by the caller.
fn parse_date(s: &str) -> Option<wasm_dbms_api::prelude::Date> {
    let mut parts = s.splitn(3, '-');
    let year = parts.next()?.parse::<u16>().ok()?;
    let month = parts.next()?.parse::<u8>().ok()?;
    let day = parts.next()?.parse::<u8>().ok()?;
    Some(wasm_dbms_api::prelude::Date { year, month, day })
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

thread_local! {
    static INTERNED_STRINGS: RefCell<HashMap<String, &'static str>> = RefCell::new(HashMap::new());
}

/// Interns a string into `&'static str`, reusing previously leaked copies.
///
/// `DatabaseSchema` methods require `&'static str` table names, but the
/// WIT boundary delivers owned `String`s. This function ensures each
/// unique string value is leaked at most once.
fn intern_str(s: &str) -> &'static str {
    INTERNED_STRINGS.with_borrow_mut(|map| {
        if let Some(existing) = map.get(s) {
            return *existing;
        }
        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        map.insert(s.to_string(), leaked);
        leaked
    })
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
        })
    }

    fn insert(
        table: String,
        values: wit::Row,
        tx: Option<wit::TransactionId>,
    ) -> Result<(), wit::DbmsError> {
        let named_values = wit_row_to_named_values(values);
        with_dbms(|ctx| {
            let col_values = match_column_defs(&table, named_values).map_err(dbms_error_to_wit)?;
            let table_name = intern_str(&table);

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
        })
    }

    fn aggregate(
        table: String,
        query: wit::Query,
        aggregates: Vec<wit::AggregateFunction>,
    ) -> Result<Vec<wit::AggregatedRow>, wit::DbmsError> {
        let query = wit_query_to_dbms(query).map_err(wit::DbmsError::InvalidQuery)?;
        let aggs: Vec<AggregateFunction> =
            aggregates.into_iter().map(wit_aggregate_to_dbms).collect();
        with_dbms(|ctx| {
            let table_name = intern_str(&table);
            let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
            ExampleDatabaseSchema
                .aggregate(&db, table_name, query, &aggs)
                .map(|rows| rows.into_iter().map(aggregated_row_to_wit).collect())
                .map_err(dbms_error_to_wit)
        })
    }

    fn update(
        table: String,
        values: wit::Row,
        filter: Option<String>,
        tx: Option<wit::TransactionId>,
    ) -> Result<u64, wit::DbmsError> {
        let filter = parse_filter_json(filter)?;
        let named_values = wit_row_to_named_values(values);
        with_dbms(|ctx| {
            let col_values = match_column_defs(&table, named_values).map_err(dbms_error_to_wit)?;
            let table_name = intern_str(&table);

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
        })
    }

    fn delete(
        table: String,
        behavior: wit::DeleteBehavior,
        filter: Option<String>,
        tx: Option<wit::TransactionId>,
    ) -> Result<u64, wit::DbmsError> {
        let filter = parse_filter_json(filter)?;
        let behavior = wit_delete_behavior(behavior);
        with_dbms(|ctx| {
            let table_name = intern_str(&table);

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

    fn has_drift() -> Result<bool, wit::DbmsError> {
        with_dbms(|ctx| {
            let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
            db.has_drift().map_err(dbms_error_to_wit)
        })
    }

    fn pending_migrations() -> Result<Vec<wit::MigrationOp>, wit::DbmsError> {
        with_dbms(|ctx| {
            let db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
            db.pending_migrations()
                .map(|ops| ops.into_iter().map(migration_op_to_wit).collect())
                .map_err(dbms_error_to_wit)
        })
    }

    fn migrate(policy: wit::MigrationPolicy) -> Result<(), wit::DbmsError> {
        let policy = wit_migration_policy(policy);
        with_dbms(|ctx| {
            let mut db = WasmDbmsDatabase::oneshot(ctx, ExampleDatabaseSchema);
            db.migrate(policy).map_err(dbms_error_to_wit)
        })
    }
}
