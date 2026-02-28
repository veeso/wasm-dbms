// Rust guideline compliant 2026-02-28

//! Transaction session storage.
//!
//! Tracks active transactions and their ownership by identity.

use std::collections::HashMap;

use wasm_dbms_api::prelude::{DbmsError, DbmsResult, QueryError, TransactionId};

use super::Transaction;

/// Stores active transactions and their owner identities.
#[derive(Default, Debug)]
pub struct TransactionSession {
    /// Map between transaction IDs and transactions.
    transactions: HashMap<TransactionId, Transaction>,
    /// Map between transaction IDs and their owner identity bytes.
    owners: HashMap<TransactionId, Vec<u8>>,
    /// Next transaction ID to allocate.
    next_transaction_id: TransactionId,
}

impl TransactionSession {
    /// Begins a new transaction for the given owner identity and returns its ID.
    pub fn begin_transaction(&mut self, owner: Vec<u8>) -> TransactionId {
        let transaction_id = self.next_transaction_id;
        self.next_transaction_id += 1;

        self.transactions
            .insert(transaction_id, Transaction::default());
        self.owners.insert(transaction_id, owner);

        transaction_id
    }

    /// Checks whether a transaction exists and is owned by the given identity.
    pub fn has_transaction(&self, transaction_id: &TransactionId, caller: &[u8]) -> bool {
        self.owners
            .get(transaction_id)
            .is_some_and(|owner| owner.as_slice() == caller)
    }

    /// Retrieves a shared reference to the transaction.
    pub fn get_transaction(&self, transaction_id: &TransactionId) -> DbmsResult<&Transaction> {
        self.transactions
            .get(transaction_id)
            .ok_or(DbmsError::Query(QueryError::TransactionNotFound))
    }

    /// Removes and returns the transaction (used during commit).
    pub fn take_transaction(
        &mut self,
        transaction_id: &TransactionId,
    ) -> DbmsResult<Transaction> {
        let transaction = self
            .transactions
            .remove(transaction_id)
            .ok_or(DbmsError::Query(QueryError::TransactionNotFound))?;
        self.owners.remove(transaction_id);

        Ok(transaction)
    }

    /// Closes (discards) the transaction without returning it.
    pub fn close_transaction(&mut self, transaction_id: &TransactionId) {
        self.transactions.remove(transaction_id);
        self.owners.remove(transaction_id);
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
        let alice = vec![1, 2, 3];
        let bob = vec![4, 5, 6];
        let transaction_id = session.begin_transaction(alice.clone());

        assert!(session.has_transaction(&transaction_id, &alice));
        assert!(!session.has_transaction(&transaction_id, &bob));

        let transaction = session.get_transaction_mut(&transaction_id);
        assert!(transaction.is_ok());
    }

    #[test]
    fn test_should_close_transaction() {
        let mut session = TransactionSession::default();
        let alice = vec![1, 2, 3];
        let transaction_id = session.begin_transaction(alice.clone());

        assert!(session.has_transaction(&transaction_id, &alice));

        session.close_transaction(&transaction_id);

        assert!(!session.has_transaction(&transaction_id, &alice));
        let transaction = session.get_transaction_mut(&transaction_id);
        assert!(transaction.is_err());
        assert!(!session.owners.contains_key(&transaction_id));
        assert!(!session.transactions.contains_key(&transaction_id));
    }

    #[test]
    fn test_should_take_transaction() {
        let mut session = TransactionSession::default();
        let alice = vec![1, 2, 3];
        let transaction_id = session.begin_transaction(alice.clone());

        let _transaction = session
            .take_transaction(&transaction_id)
            .expect("failed to take tx");

        assert!(!session.has_transaction(&transaction_id, &alice));
        let transaction_after_take = session.get_transaction(&transaction_id);
        assert!(transaction_after_take.is_err());
        assert!(!session.owners.contains_key(&transaction_id));
        assert!(!session.transactions.contains_key(&transaction_id));
    }

    #[test]
    fn test_should_get_transaction() {
        let mut session = TransactionSession::default();
        let alice = vec![1, 2, 3];
        let transaction_id = session.begin_transaction(alice);

        let _tx = session
            .get_transaction(&transaction_id)
            .expect("failed to get tx");
    }
}
