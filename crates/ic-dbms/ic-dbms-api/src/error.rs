/// IC DBMS error type (backward-compatible alias for [`wasm_dbms_api::error::DbmsError`]).
pub type IcDbmsError = wasm_dbms_api::error::DbmsError;

/// IC DBMS result type (backward-compatible alias for [`wasm_dbms_api::error::DbmsResult`]).
pub type IcDbmsResult<T> = wasm_dbms_api::error::DbmsResult<T>;

#[cfg(test)]
mod test {

    use wasm_dbms_api::prelude::{MemoryError, QueryError, TableError, TransactionError};

    use super::*;

    #[test]
    fn test_should_display_memory_error() {
        let error = IcDbmsError::Memory(MemoryError::OutOfBounds);
        assert_eq!(
            error.to_string(),
            "Memory error: Stable memory access out of bounds"
        );
    }

    #[test]
    fn test_should_display_query_error() {
        let error = IcDbmsError::Query(QueryError::UnknownColumn("foo".to_string()));
        assert_eq!(error.to_string(), "Query error: Unknown column: foo");
    }

    #[test]
    fn test_should_display_sanitize_error() {
        let error = IcDbmsError::Sanitize("invalid input".to_string());
        assert_eq!(error.to_string(), "Sanitize error: invalid input");
    }

    #[test]
    fn test_should_display_table_error() {
        let error = IcDbmsError::Table(TableError::TableNotFound);
        assert_eq!(error.to_string(), "Table error: Table not found");
    }

    #[test]
    fn test_should_display_transaction_error() {
        let error = IcDbmsError::Transaction(TransactionError::NoActiveTransaction);
        assert_eq!(
            error.to_string(),
            "Transaction error: No active transaction"
        );
    }

    #[test]
    fn test_should_display_validation_error() {
        let error = IcDbmsError::Validation("invalid email".to_string());
        assert_eq!(error.to_string(), "Validation error: invalid email");
    }

    #[test]
    fn test_should_convert_from_memory_error() {
        let error: IcDbmsError = MemoryError::OutOfBounds.into();
        assert!(matches!(
            error,
            IcDbmsError::Memory(MemoryError::OutOfBounds)
        ));
    }

    #[test]
    fn test_should_convert_from_query_error() {
        let error: IcDbmsError = QueryError::UnknownColumn("col".to_string()).into();
        assert!(matches!(error, IcDbmsError::Query(_)));
    }

    #[test]
    fn test_should_convert_from_table_error() {
        let error: IcDbmsError = TableError::TableNotFound.into();
        assert!(matches!(error, IcDbmsError::Table(_)));
    }

    #[test]
    fn test_should_convert_from_transaction_error() {
        let error: IcDbmsError = TransactionError::NoActiveTransaction.into();
        assert!(matches!(error, IcDbmsError::Transaction(_)));
    }
}
