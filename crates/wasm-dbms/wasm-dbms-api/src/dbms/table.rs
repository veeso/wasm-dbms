//! This module contains types related to database tables.

mod column_def;
mod record;
mod schema;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use self::column_def::{
    CandidDataTypeKind, CandidForeignKeyDef, ColumnDef, ForeignKeyDef, IndexDef, JoinColumnDef,
};
pub use self::record::{
    InsertRecord, TableColumns, TableRecord, UpdateRecord, ValuesSource, flatten_table_columns,
};
pub use self::schema::{
    ColumnSnapshot, CustomDataTypeSnapshot, DataTypeSnapshot, ForeignKeySnapshot, IndexSnapshot,
    OnDeleteSnapshot, TableFingerprint, TableSchema, TableSchemaSnapshot, WireSize,
    fingerprint_for_name,
};

/// Table related errors
#[derive(Debug, Error, Deserialize, Serialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum TableError {
    #[error("Table not found")]
    TableNotFound,
    #[error("Schema mismatch")]
    SchemaMismatch,
}
