use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use candid::CandidType;
use serde::{Deserialize, Serialize};

/// A type-erased representation of a custom data type value.
///
/// `CustomValue` holds the binary encoding and cached display string for
/// a user-defined data type, along with a type tag that identifies the
/// concrete type. This allows the DBMS engine to compare, order, hash,
/// and display custom values without knowing their concrete types.
#[derive(Clone, Debug, CandidType, Serialize, Deserialize)]
pub struct CustomValue {
    /// Type identifier from `CustomDataType::TYPE_TAG`.
    pub type_tag: String,
    /// Binary encoding via the concrete type's `Encode` impl.
    pub encoded: Vec<u8>,
    /// Cached `Display` output for human-readable representation.
    pub display: String,
}

impl PartialEq for CustomValue {
    fn eq(&self, other: &Self) -> bool {
        self.type_tag == other.type_tag && self.encoded == other.encoded
    }
}

impl Eq for CustomValue {}

impl PartialOrd for CustomValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CustomValue {
    fn cmp(&self, other: &Self) -> Ordering {
        self.type_tag
            .cmp(&other.type_tag)
            .then_with(|| self.encoded.cmp(&other.encoded))
    }
}

impl Hash for CustomValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.type_tag.hash(state);
        self.encoded.hash(state);
    }
}

impl fmt::Display for CustomValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display)
    }
}

#[cfg(test)]
mod test {

    use std::collections::HashSet;

    use super::*;

    fn make_custom_value(type_tag: &str, encoded: &[u8], display: &str) -> CustomValue {
        CustomValue {
            type_tag: type_tag.to_string(),
            encoded: encoded.to_vec(),
            display: display.to_string(),
        }
    }

    #[test]
    fn test_should_compare_equal_custom_values() {
        let a = make_custom_value("color", &[0x01, 0x02], "red");
        let b = make_custom_value("color", &[0x01, 0x02], "red");
        assert_eq!(a, b);
    }

    #[test]
    fn test_should_not_equal_different_encoded() {
        let a = make_custom_value("color", &[0x01], "red");
        let b = make_custom_value("color", &[0x02], "blue");
        assert_ne!(a, b);
    }

    #[test]
    fn test_should_not_equal_different_type_tag() {
        let a = make_custom_value("color", &[0x01], "red");
        let b = make_custom_value("size", &[0x01], "red");
        assert_ne!(a, b);
    }

    #[test]
    fn test_should_ignore_display_in_equality() {
        let a = make_custom_value("color", &[0x01, 0x02], "red");
        let b = make_custom_value("color", &[0x01, 0x02], "rouge");
        assert_eq!(a, b);
    }

    #[test]
    fn test_should_order_by_type_tag_first() {
        let alpha = make_custom_value("alpha", &[0xFF], "big");
        let beta = make_custom_value("beta", &[0x00], "small");
        assert!(alpha < beta);
    }

    #[test]
    fn test_should_order_by_encoded_within_same_tag() {
        let a = make_custom_value("color", &[0x01], "red");
        let b = make_custom_value("color", &[0x02], "blue");
        assert!(a < b);
    }

    #[test]
    fn test_should_hash_consistently() {
        let a = make_custom_value("color", &[0x01, 0x02], "red");
        let b = make_custom_value("color", &[0x01, 0x02], "rouge");

        let mut set = HashSet::new();
        set.insert(a.clone());

        // b has a different display but same tag+encoded, so it should be found in the set
        assert!(set.contains(&b));

        // inserting b should not increase the set size
        set.insert(b);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_should_display_cached_string() {
        let cv = make_custom_value("color", &[0x01], "red");
        assert_eq!(format!("{cv}"), "red");
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn test_should_clone() {
        let original = make_custom_value("color", &[0x01, 0x02, 0x03], "red");
        let cloned = original.clone();
        assert_eq!(original, cloned);
        assert_eq!(original.display, cloned.display);
    }

    #[test]
    fn test_should_debug() {
        let cv = make_custom_value("color", &[0x01], "red");
        let debug_output = format!("{cv:?}");
        assert!(debug_output.contains("color"));
        assert!(debug_output.contains("red"));
    }
}
