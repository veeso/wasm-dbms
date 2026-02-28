use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

/// A type-erased representation of a custom data type value.
///
/// `CustomValue` holds the binary encoding and cached display string for
/// a user-defined data type, along with a type tag that identifies the
/// concrete type. This allows the DBMS engine to compare, order, hash,
/// and display custom values without knowing their concrete types.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct CustomValue {
    /// Type identifier from `CustomDataType::TYPE_TAG`.
    pub type_tag: String,
    /// Binary encoding via the concrete type's `Encode` impl.
    pub encoded: Vec<u8>,
    /// Cached `Display` output for human-readable representation.
    pub display: String,
}

impl CustomValue {
    /// Creates a new `CustomValue` from a concrete [`CustomDataType`](crate::dbms::types::CustomDataType).
    ///
    /// This constructor ensures consistency between the type tag, encoded bytes,
    /// and display string by deriving all three from the concrete value.
    pub fn new<T: crate::dbms::types::CustomDataType>(value: &T) -> Self {
        Self {
            type_tag: T::TYPE_TAG.to_string(),
            encoded: crate::memory::Encode::encode(value).into_owned(),
            display: value.to_string(),
        }
    }
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

    #[test]
    fn test_should_implement_custom_data_type() {
        use std::borrow::Cow;
        use std::fmt;

        use serde::{Deserialize, Serialize};

        use crate::dbms::types::{CustomDataType, DataType};
        use crate::dbms::value::Value;
        use crate::memory::{self, DataSize, MSize, MemoryResult, PageOffset};

        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum TestStatus {
            Active,
            Inactive,
        }

        // Manual Encode impl since #[derive(Encode)] only supports structs
        impl memory::Encode for TestStatus {
            const SIZE: DataSize = DataSize::Fixed(1);
            const ALIGNMENT: PageOffset = 1;

            fn size(&self) -> MSize {
                1
            }

            fn encode(&self) -> Cow<'_, [u8]> {
                match self {
                    TestStatus::Active => Cow::Borrowed(&[0]),
                    TestStatus::Inactive => Cow::Borrowed(&[1]),
                }
            }

            fn decode(data: Cow<[u8]>) -> MemoryResult<Self> {
                match data.first() {
                    Some(0) => Ok(TestStatus::Active),
                    Some(1) => Ok(TestStatus::Inactive),
                    _ => Err(crate::memory::MemoryError::DecodeError(
                        crate::memory::DecodeError::TooShort,
                    )),
                }
            }
        }

        impl fmt::Display for TestStatus {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{self:?}")
            }
        }

        impl Default for TestStatus {
            fn default() -> Self {
                Self::Active
            }
        }

        impl From<TestStatus> for Value {
            fn from(val: TestStatus) -> Value {
                Value::Custom(CustomValue {
                    type_tag: TestStatus::TYPE_TAG.to_string(),
                    encoded: crate::memory::Encode::encode(&val).into_owned(),
                    display: val.to_string(),
                })
            }
        }

        impl DataType for TestStatus {}

        impl CustomDataType for TestStatus {
            const TYPE_TAG: &'static str = "test_status";
        }

        // Test trait impl
        assert_eq!(TestStatus::TYPE_TAG, "test_status");

        // Test Into<Value> conversion
        let value: Value = TestStatus::Active.into();
        assert!(matches!(value, Value::Custom(_)));

        let cv = value.as_custom().unwrap();
        assert_eq!(cv.type_tag, "test_status");
        assert_eq!(cv.display, "Active");

        // Test round-trip via as_custom_type
        let decoded: TestStatus = value.as_custom_type::<TestStatus>().unwrap();
        assert_eq!(decoded, TestStatus::Active);
    }
}
