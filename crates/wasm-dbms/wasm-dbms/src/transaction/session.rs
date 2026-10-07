// Rust guideline compliant 2026-02-28

//! Transaction session storage.
//!
//! Tracks the open transactions of a context by their ID.

use std::collections::HashMap;

use wasm_dbms_api::prelude::{DbmsError, DbmsResult, QueryError, TransactionId};

use super::Transaction;

/// Stores the open transactions of a context, keyed by transaction ID.
#[derive(Default, Debug)]
pub struct TransactionSession {
    /// Map between transaction IDs and transactions.
    transactions: HashMap<TransactionId, Transaction>,
    /// Next transaction ID to allocate.
    next_transaction_id: TransactionId,
}

impl TransactionSession {
    /// Begins a new transaction and returns its ID.
    pub fn begin_transaction(&mut self) -> TransactionId {
        let transaction_id = self.next_transaction_id;
        self.next_transaction_id += 1;

        self.transactions
            .insert(transaction_id, Transaction::default());

        transaction_id
    }

    /// Returns whether the transaction exists and is still open.
    pub fn has_transaction(&self, transaction_id: &TransactionId) -> bool {
        self.transactions.contains_key(transaction_id)
    }

    /// Retrieves a shared reference to the transaction.
    pub fn get_transaction(&self, transaction_id: &TransactionId) -> DbmsResult<&Transaction> {
        self.transactions
            .get(transaction_id)
            .ok_or(DbmsError::Query(QueryError::TransactionNotFound))
    }

    /// Removes and returns the transaction (used during commit).
    pub fn take_transaction(&mut self, transaction_id: &TransactionId) -> DbmsResult<Transaction> {
        self.transactions
            .remove(transaction_id)
            .ok_or(DbmsError::Query(QueryError::TransactionNotFound))
    }

    /// Closes (discards) the transaction without returning it.
    pub fn close_transaction(&mut self, transaction_id: &TransactionId) {
        self.transactions.remove(transaction_id);
    }

    /// Retrieves a mutable reference to the transaction.
    pub fn get_transaction_mut(
        &mut self,
        transaction_id: &TransactionId,
    ) -> DbmsResult<&mut Transaction> {
        self.transactions
            .get_mut(transaction_id)
            .ok_or(DbmsError::Query(QueryError::TransactionNotFound))
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_should_begin_transaction() {
        let mut session = TransactionSession::default();
        let first = session.begin_transaction();
        let second = session.begin_transaction();

        assert_ne!(first, second);
        assert!(session.has_transaction(&first));
        assert!(session.has_transaction(&second));
        assert!(!session.has_transaction(&(second + 1)));
        assert!(session.get_transaction_mut(&first).is_ok());
    }

    #[test]
    fn test_should_close_transaction() {
        let mut session = TransactionSession::default();
        let transaction_id = session.begin_transaction();

        assert!(session.has_transaction(&transaction_id));

        session.close_transaction(&transaction_id);

        assert!(!session.has_transaction(&transaction_id));
        assert!(session.get_transaction_mut(&transaction_id).is_err());
        assert!(!session.transactions.contains_key(&transaction_id));
    }

    #[test]
    fn test_should_take_transaction() {
        let mut session = TransactionSession::default();
        let transaction_id = session.begin_transaction();

        let _transaction = session
            .take_transaction(&transaction_id)
            .expect("failed to take tx");

        assert!(!session.has_transaction(&transaction_id));
        assert!(session.get_transaction(&transaction_id).is_err());
        assert!(!session.transactions.contains_key(&transaction_id));
    }

    #[test]
    fn test_should_get_transaction() {
        let mut session = TransactionSession::default();
        let transaction_id = session.begin_transaction();

        let _tx = session
            .get_transaction(&transaction_id)
            .expect("failed to get tx");
    }
}
