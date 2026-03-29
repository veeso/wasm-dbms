//! Discriminant bytes for each [`Value`](super::Value) variant, used in the [`Encode`](crate::memory::Encode) implementation.

pub const BLOB: u8 = 0;
pub const BOOLEAN: u8 = 1;
pub const DATE: u8 = 2;
pub const DATE_TIME: u8 = 3;
pub const DECIMAL: u8 = 4;
pub const INT8: u8 = 5;
pub const INT16: u8 = 6;
pub const INT32: u8 = 7;
pub const INT64: u8 = 8;
pub const JSON: u8 = 9;
pub const NULL: u8 = 10;
pub const TEXT: u8 = 11;
pub const UINT8: u8 = 12;
pub const UINT16: u8 = 13;
pub const UINT32: u8 = 14;
pub const UINT64: u8 = 15;
pub const UUID: u8 = 16;
pub const CUSTOM: u8 = 17;
