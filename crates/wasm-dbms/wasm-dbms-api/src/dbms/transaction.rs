use serde::{Deserialize, Serialize};

/// Type alias for Transaction ID
pub type TransactionId = u64;

/// An enum representing possible errors that can occur during transaction operations.
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum TransactionError {
    #[error("No active transaction")]
    NoActiveTransaction,
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_should_display_transaction_error() {
        let error = TransactionError::NoActiveTransaction;
        assert_eq!(error.to_string(), "No active transaction");
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_encode_decode_transaction_error() {
        let error = TransactionError::NoActiveTransaction;
        let encoded = candid::encode_one(&error).expect("failed to encode");
        let decoded: TransactionError = candid::decode_one(&encoded).expect("failed to decode");
        assert!(matches!(decoded, TransactionError::NoActiveTransaction));
    }
}
