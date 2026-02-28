use serde::{Deserialize, Serialize};

/// Defines the behavior for delete operations regarding foreign key constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum DeleteBehavior {
    /// Delete only the records matching the filter.
    /// If there are foreign key constraints that would be violated, the operation will fail.
    Restrict,
    /// Cascade delete to related records.
    /// Any records that reference the deleted records via foreign keys will also be deleted.
    Cascade,
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_should_create_delete_behavior_variants() {
        let restrict = DeleteBehavior::Restrict;
        let cascade = DeleteBehavior::Cascade;

        assert_eq!(restrict, DeleteBehavior::Restrict);
        assert_eq!(cascade, DeleteBehavior::Cascade);
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn test_should_clone_delete_behavior() {
        let behavior = DeleteBehavior::Cascade;
        let cloned = behavior.clone();
        assert_eq!(behavior, cloned);
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn test_should_copy_delete_behavior() {
        let behavior = DeleteBehavior::Restrict;
        let copied = behavior;
        assert_eq!(behavior, copied);
    }

    #[test]
    fn test_should_compare_delete_behaviors() {
        assert_eq!(DeleteBehavior::Restrict, DeleteBehavior::Restrict);
        assert_eq!(DeleteBehavior::Cascade, DeleteBehavior::Cascade);
        assert_ne!(DeleteBehavior::Restrict, DeleteBehavior::Cascade);
    }

    #[test]
    fn test_should_debug_delete_behavior() {
        assert_eq!(format!("{:?}", DeleteBehavior::Restrict), "Restrict");
        assert_eq!(format!("{:?}", DeleteBehavior::Cascade), "Cascade");
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_encode_decode_delete_behavior() {
        for behavior in [DeleteBehavior::Restrict, DeleteBehavior::Cascade] {
            let encoded = candid::encode_one(behavior).expect("failed to encode");
            let decoded: DeleteBehavior = candid::decode_one(&encoded).expect("failed to decode");
            assert_eq!(behavior, decoded);
        }
    }
}
