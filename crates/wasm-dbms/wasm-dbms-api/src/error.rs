use serde::{Deserialize, Serialize};
use thiserror::Error;

/// DBMS error type.
#[derive(Debug, Error, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum DbmsError {
    #[error("Memory error: {0}")]
    Memory(#[from] crate::memory::MemoryError),
    #[error("Query error: {0}")]
    Query(#[from] crate::dbms::query::QueryError),
    #[error("Sanitize error: {0}")]
    Sanitize(String),
    #[error("Table error: {0}")]
    Table(#[from] crate::dbms::table::TableError),
    #[error("Transaction error: {0}")]
    Transaction(#[from] crate::dbms::transaction::TransactionError),
    #[error("Validation error: {0}")]
    Validation(String),
}

/// DBMS result type.
pub type DbmsResult<T> = Result<T, DbmsError>;

#[cfg(test)]
mod test {

    use super::*;
    use crate::dbms::query::QueryError;
    use crate::dbms::table::TableError;
    use crate::dbms::transaction::TransactionError;
    use crate::memory::MemoryError;

    #[test]
    fn test_should_display_memory_error() {
        let error = DbmsError::Memory(MemoryError::OutOfBounds);
        assert_eq!(
            error.to_string(),
            "Memory error: Stable memory access out of bounds"
        );
    }

    #[test]
    fn test_should_display_query_error() {
        let error = DbmsError::Query(QueryError::UnknownColumn("foo".to_string()));
        assert_eq!(error.to_string(), "Query error: Unknown column: foo");
    }

    #[test]
    fn test_should_display_sanitize_error() {
        let error = DbmsError::Sanitize("invalid input".to_string());
        assert_eq!(error.to_string(), "Sanitize error: invalid input");
    }

    #[test]
    fn test_should_display_table_error() {
        let error = DbmsError::Table(TableError::TableNotFound);
        assert_eq!(error.to_string(), "Table error: Table not found");
    }

    #[test]
    fn test_should_display_transaction_error() {
        let error = DbmsError::Transaction(TransactionError::NoActiveTransaction);
        assert_eq!(
            error.to_string(),
            "Transaction error: No active transaction"
        );
    }

    #[test]
    fn test_should_display_validation_error() {
        let error = DbmsError::Validation("invalid email".to_string());
        assert_eq!(error.to_string(), "Validation error: invalid email");
    }

    #[test]
    fn test_should_convert_from_memory_error() {
        let error: DbmsError = MemoryError::OutOfBounds.into();
        assert!(matches!(error, DbmsError::Memory(MemoryError::OutOfBounds)));
    }

    #[test]
    fn test_should_convert_from_query_error() {
        let error: DbmsError = QueryError::UnknownColumn("col".to_string()).into();
        assert!(matches!(error, DbmsError::Query(_)));
    }

    #[test]
    fn test_should_convert_from_table_error() {
        let error: DbmsError = TableError::TableNotFound.into();
        assert!(matches!(error, DbmsError::Table(_)));
    }

    #[test]
    fn test_should_convert_from_transaction_error() {
        let error: DbmsError = TransactionError::NoActiveTransaction.into();
        assert!(matches!(error, DbmsError::Transaction(_)));
    }
}
