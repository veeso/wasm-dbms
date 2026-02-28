//! This module defines various integer types with common traits and implementations.
//!
//! All the types are auto-generated using a macro to reduce code duplication.

/// a macro to define integer types with common traits and implementations
macro_rules! int_type {
    ($name:ident, $candid_type:path, $candid_serialize:ident, $primitive:ty, $tests_name:ident) => {
        /// An integer type wrapper around a primitive integer type.
        #[derive(
            Clone,
            Copy,
            Debug,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Serialize,
            serde::Deserialize,
        )]
        pub struct $name(pub $primitive);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        #[cfg(feature = "candid")]
        impl candid::CandidType for $name {
            fn _ty() -> candid::types::Type {
                candid::types::Type(std::rc::Rc::new($candid_type))
            }

            fn idl_serialize<S>(&self, serializer: S) -> Result<(), S::Error>
            where
                S: candid::types::Serializer,
            {
                serializer.$candid_serialize(self.0)
            }
        }

        impl $name {
            const MEM_SIZE: usize = std::mem::size_of::<$primitive>();
        }

        impl $crate::prelude::Encode for $name {
            const SIZE: $crate::prelude::DataSize =
                $crate::prelude::DataSize::Fixed(Self::MEM_SIZE as $crate::prelude::MSize);

            const ALIGNMENT: $crate::prelude::PageOffset =
                std::mem::align_of::<$primitive>() as $crate::prelude::PageOffset;

            fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
                std::borrow::Cow::Owned(self.0.to_le_bytes().to_vec())
            }

            fn decode(data: std::borrow::Cow<[u8]>) -> crate::memory::MemoryResult<Self>
            where
                Self: Sized,
            {
                if data.len() < Self::MEM_SIZE {
                    return Err(crate::memory::MemoryError::DecodeError(
                        crate::memory::DecodeError::TooShort,
                    ));
                }

                let mut array = <$primitive>::to_le_bytes(0);
                array.copy_from_slice(&data[..Self::MEM_SIZE]);
                Ok(Self(<$primitive>::from_le_bytes(array)))
            }

            fn size(&self) -> crate::memory::MSize {
                Self::SIZE.get_fixed_size().expect("should be fixed")
            }
        }

        impl $crate::prelude::DataType for $name {}

        impl From<$primitive> for $name {
            fn from(value: $primitive) -> Self {
                $name(value)
            }
        }

        #[cfg(test)]
        mod $tests_name {

            use super::*;

            #[test]
            fn test_constants() {
                use $crate::prelude::Encode;

                let value = $name(0);
                assert_eq!(
                    value.size(),
                    $crate::prelude::DataSize::Fixed(
                        std::mem::size_of::<$primitive>() as $crate::prelude::MSize
                    )
                    .get_fixed_size()
                    .unwrap()
                );
                // alignment
                assert_eq!(
                    $name::ALIGNMENT,
                    std::mem::align_of::<$primitive>() as $crate::prelude::PageOffset
                );
            }

            #[test]
            fn test_encode_decode() {
                use $crate::prelude::Encode;

                let num: $primitive = 123;
                let value = $name(num);
                let encoded = value.encode();
                let decoded = $name::decode(encoded).unwrap();
                assert_eq!(value, decoded);
            }

            #[cfg(feature = "candid")]
            #[test]
            fn test_should_candid_encode_decode() {
                let num: $primitive = 123;
                let src = $name(num);
                let buf = candid::encode_one(src).expect("Candid encoding failed");
                let decoded: $name = candid::decode_one(&buf).expect("Candid decoding failed");
                assert_eq!(src, decoded);
            }
        }
    };
    () => {};
}

int_type!(
    Int8,
    candid::types::TypeInner::Int8,
    serialize_int8,
    i8,
    tests_int8
);
int_type!(
    Int16,
    candid::types::TypeInner::Int16,
    serialize_int16,
    i16,
    tests_int16
);
int_type!(
    Int32,
    candid::types::TypeInner::Int32,
    serialize_int32,
    i32,
    tests_int32
);
int_type!(
    Int64,
    candid::types::TypeInner::Int64,
    serialize_int64,
    i64,
    tests_int64
);
int_type!(
    Uint8,
    candid::types::TypeInner::Nat8,
    serialize_nat8,
    u8,
    tests_uint8
);
int_type!(
    Uint16,
    candid::types::TypeInner::Nat16,
    serialize_nat16,
    u16,
    tests_uint16
);
int_type!(
    Uint32,
    candid::types::TypeInner::Nat32,
    serialize_nat32,
    u32,
    tests_uint32
);
int_type!(
    Uint64,
    candid::types::TypeInner::Nat64,
    serialize_nat64,
    u64,
    tests_uint64
);
