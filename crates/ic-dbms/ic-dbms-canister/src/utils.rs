mod trap;

use candid::Principal;

pub use self::trap::trap;

/// Returns the caller's principal.
pub fn caller() -> Principal {
    #[cfg(target_family = "wasm")]
    {
        ic_cdk::api::msg_caller()
    }
    #[cfg(not(target_family = "wasm"))]
    {
        // dummy principal for non-wasm targets (e.g., during unit tests)
        Principal::from_text("ghsi2-tqaaa-aaaan-aaaca-cai").expect("it should be valid")
    }
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_should_return_caller_principal() {
        let principal = caller();
        // In non-wasm tests, the dummy principal is returned
        let expected = Principal::from_text("ghsi2-tqaaa-aaaan-aaaca-cai").unwrap();
        assert_eq!(principal, expected);
    }

    #[test]
    fn test_caller_returns_consistent_principal() {
        let principal1 = caller();
        let principal2 = caller();
        assert_eq!(principal1, principal2);
    }
}
