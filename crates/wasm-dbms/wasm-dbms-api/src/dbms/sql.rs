//! Shared SQL types: the result and error types returned by the SQL engine.
//!
//! The SQL front-end itself (parser, planner, executor) lives in the
//! `wasm-dbms-sql` crate. This module only defines the types that cross the
//! API boundary, so adapters can expose SQL without depending on the engine.
//!
//! Available with the `sql` feature.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dbms::table::{CandidDataTypeKind, JoinColumnDef};
use crate::dbms::transaction::TransactionId;
use crate::dbms::value::Value;
use crate::error::DbmsError;

/// One row returned by a SQL `SELECT`: a list of column definitions and values.
///
/// Columns appear in the order of the select list (table declaration order for
/// `SELECT *`). [`JoinColumnDef::table`] is `Some` for rows of a joined query
/// and `None` otherwise.
pub type SqlRow = Vec<(JoinColumnDef, Value)>;

/// Outcome of a successfully executed SQL statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum SqlResult {
    /// Rows produced by a `SELECT`.
    Rows(Vec<SqlRow>),
    /// Number of rows written by an `INSERT`, `UPDATE`, or `DELETE`.
    RowsAffected(u64),
    /// A transaction was started by `BEGIN`; later statements pass its ID.
    TxBegin(TransactionId),
    /// The active transaction was committed by `COMMIT`.
    TxCommit,
    /// The active transaction was discarded by `ROLLBACK`.
    TxRollback,
}

/// Errors returned while parsing, planning, or executing a SQL statement.
#[derive(Debug, Error, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum SqlError {
    /// The statement is not valid SQL for the supported grammar.
    ///
    /// `line` and `col` are 1-based and point at the offending token.
    #[error("SQL parse error at line {line}, column {col}: {msg}")]
    Parse {
        line: usize,
        col: usize,
        msg: String,
    },

    /// The statement references a table, or a table qualifier, that does not exist.
    #[error("Unknown table: {0}")]
    UnknownTable(String),

    /// The statement references a column that does not exist on `table`.
    #[error("Unknown column '{column}' on table '{table}'")]
    UnknownColumn { table: String, column: String },

    /// An unqualified column name exists on more than one table of a join.
    #[error("Ambiguous column: {0}")]
    AmbiguousColumn(String),

    /// A literal or parameter has a type that cannot be used for `column`.
    ///
    /// `got` is the kind of the offending literal (`Integer`, `Float`,
    /// `String`, `Boolean`) or the type name of the bound parameter.
    #[error("Type mismatch on column '{column}': expected {expected:?}, got {got}")]
    TypeMismatch {
        column: String,
        expected: CandidDataTypeKind,
        got: String,
    },

    /// A literal or parameter has the right kind but a value that `column`
    /// cannot hold, such as an out-of-range integer or a malformed date.
    #[error("Invalid value for column '{column}' of type {expected:?}: {reason}")]
    InvalidLiteral {
        column: String,
        expected: CandidDataTypeKind,
        reason: String,
    },

    /// An `UPDATE` or `DELETE` statement has no `WHERE` clause.
    #[error("UPDATE and DELETE require a WHERE clause")]
    MissingWhereClause,

    /// The number of bound parameters differs from the number of `?` placeholders.
    #[error("Statement expects {expected} parameters, got {got}")]
    ParameterCountMismatch { expected: usize, got: usize },

    /// `BEGIN` was issued together with a transaction ID.
    #[error("BEGIN was issued inside an active transaction")]
    TransactionAlreadyActive,

    /// `COMMIT` or `ROLLBACK` was issued without a transaction ID.
    #[error("COMMIT or ROLLBACK was issued without an active transaction")]
    NoActiveTransaction,

    /// The statement is valid SQL but uses a combination the engine cannot execute.
    #[error("Unsupported SQL: {0}")]
    Unsupported(String),

    /// The DBMS rejected the statement while executing it.
    #[error("Runtime error: {0}")]
    Runtime(#[from] DbmsError),
}

#[cfg(test)]
mod test {

    use super::*;
    use crate::dbms::query::QueryError;
    use crate::dbms::table::{CandidDataTypeKind, JoinColumnDef};
    use crate::dbms::transaction::TransactionError;
    use crate::dbms::value::Value;
    use crate::error::DbmsError;

    fn sample_rows() -> SqlResult {
        SqlResult::Rows(vec![vec![(
            JoinColumnDef {
                table: None,
                name: "id".to_string(),
                data_type: CandidDataTypeKind::Uint32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::from(1u32),
        )]])
    }

    #[test]
    fn test_should_display_parse_error_with_position() {
        let error = SqlError::Parse {
            line: 2,
            col: 7,
            msg: "expected `FROM`, found end of input".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "SQL parse error at line 2, column 7: expected `FROM`, found end of input"
        );
    }

    #[test]
    fn test_should_display_name_resolution_errors() {
        assert_eq!(
            SqlError::UnknownTable("ghosts".to_string()).to_string(),
            "Unknown table: ghosts"
        );
        assert_eq!(
            SqlError::UnknownColumn {
                table: "users".to_string(),
                column: "age".to_string(),
            }
            .to_string(),
            "Unknown column 'age' on table 'users'"
        );
        assert_eq!(
            SqlError::AmbiguousColumn("id".to_string()).to_string(),
            "Ambiguous column: id"
        );
    }

    #[test]
    fn test_should_display_value_errors() {
        assert_eq!(
            SqlError::TypeMismatch {
                column: "age".to_string(),
                expected: CandidDataTypeKind::Uint32,
                got: "String".to_string(),
            }
            .to_string(),
            "Type mismatch on column 'age': expected Uint32, got String"
        );
        assert_eq!(
            SqlError::InvalidLiteral {
                column: "born".to_string(),
                expected: CandidDataTypeKind::Date,
                reason: "`2026-02-30` is not a valid date".to_string(),
            }
            .to_string(),
            "Invalid value for column 'born' of type Date: `2026-02-30` is not a valid date"
        );
        assert_eq!(
            SqlError::ParameterCountMismatch {
                expected: 2,
                got: 1,
            }
            .to_string(),
            "Statement expects 2 parameters, got 1"
        );
    }

    #[test]
    fn test_should_display_statement_errors() {
        assert_eq!(
            SqlError::MissingWhereClause.to_string(),
            "UPDATE and DELETE require a WHERE clause"
        );
        assert_eq!(
            SqlError::TransactionAlreadyActive.to_string(),
            "BEGIN was issued inside an active transaction"
        );
        assert_eq!(
            SqlError::NoActiveTransaction.to_string(),
            "COMMIT or ROLLBACK was issued without an active transaction"
        );
        assert_eq!(
            SqlError::Unsupported("DISTINCT is not supported with JOIN".to_string()).to_string(),
            "Unsupported SQL: DISTINCT is not supported with JOIN"
        );
    }

    #[test]
    fn test_should_wrap_dbms_error_as_runtime() {
        let error: SqlError = DbmsError::Query(QueryError::PrimaryKeyConflict).into();
        assert!(matches!(
            error,
            SqlError::Runtime(DbmsError::Query(QueryError::PrimaryKeyConflict))
        ));
        assert_eq!(
            error.to_string(),
            "Runtime error: Query error: Primary key conflict: record with the same primary key \
             already exists"
        );

        let error: SqlError = DbmsError::Transaction(TransactionError::NoActiveTransaction).into();
        assert_eq!(
            error.to_string(),
            "Runtime error: Transaction error: No active transaction"
        );
    }

    #[test]
    fn test_should_round_trip_sql_result_through_serde() {
        for result in [
            sample_rows(),
            SqlResult::RowsAffected(3),
            SqlResult::TxBegin(7),
            SqlResult::TxCommit,
            SqlResult::TxRollback,
        ] {
            let encoded = serde_json::to_string(&result).expect("failed to encode");
            let decoded: SqlResult = serde_json::from_str(&encoded).expect("failed to decode");
            assert_eq!(result, decoded);
        }
    }

    #[test]
    fn test_should_round_trip_sql_error_through_serde() {
        let error = SqlError::Parse {
            line: 1,
            col: 8,
            msg: "expected `FROM`".to_string(),
        };
        let encoded = serde_json::to_string(&error).expect("failed to encode");
        let decoded: SqlError = serde_json::from_str(&encoded).expect("failed to decode");
        assert!(matches!(
            decoded,
            SqlError::Parse {
                line: 1,
                col: 8,
                ..
            }
        ));
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_encode_decode_sql_types() {
        let result = sample_rows();
        let encoded = candid::encode_one(&result).expect("failed to encode");
        let decoded: SqlResult = candid::decode_one(&encoded).expect("failed to decode");
        assert_eq!(result, decoded);

        let error = SqlError::TypeMismatch {
            column: "age".to_string(),
            expected: CandidDataTypeKind::Uint32,
            got: "String".to_string(),
        };
        let encoded = candid::encode_one(&error).expect("failed to encode");
        let decoded: SqlError = candid::decode_one(&encoded).expect("failed to decode");
        assert!(matches!(decoded, SqlError::TypeMismatch { .. }));
    }
}
