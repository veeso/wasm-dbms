//! This module contains types related to database tables.

mod column_def;
mod record;
mod schema;

use candid::CandidType;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use self::column_def::{CandidColumnDef, CandidForeignKeyDef, ColumnDef, ForeignKeyDef};
pub use self::record::{
    InsertRecord, TableColumns, TableRecord, UpdateRecord, ValuesSource, flatten_table_columns,
};
pub use self::schema::{TableFingerprint, TableSchema};

/// Table related errors
#[derive(Debug, Error, CandidType, Deserialize, Serialize)]
pub enum TableError {
    #[error("Table not found")]
    TableNotFound,
    #[error("Schema mismatch")]
    SchemaMismatch,
}
