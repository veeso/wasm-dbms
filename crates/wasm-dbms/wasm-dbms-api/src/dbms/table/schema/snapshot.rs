//! [`TableSchema`](super::TableSchema) snapshot types.
//!
//! These types are used to represent a snapshot of a table schema, which can be used to compare different versions of a table schema and detect changes, in
//! order to trigger necessary migrations.

use serde::{Deserialize, Serialize};

use crate::memory::{DecodeError, MSize, MemoryError, MemoryResult, saturating_size};
use crate::prelude::{DataSize, Encode, PageOffset, Value};

/// Current binary version of the [`TableSchemaSnapshot`] format.
///
/// Bumped on any breaking change to the snapshot layout so that older snapshots can be detected and either migrated or rejected.
const SCHEMA_SNAPSHOT_VERSION: u8 = 0x01;

/// Frozen, comparable view of a [`TableSchema`](super::TableSchema) used for migration detection.
///
/// A snapshot captures the structural shape of a table at a point in time so that two versions can be diffed to derive the migration
/// steps required to bring the on-disk representation up to date with the current schema definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct TableSchemaSnapshot {
    /// Version tag of the snapshot binary layout, see
    /// `SCHEMA_SNAPSHOT_VERSION`.
    pub version: u8,
    /// Name of the table this snapshot was taken from.
    pub name: String,
    /// Name of the column declared as primary key.
    pub primary_key: String,
    /// Record alignment, in bytes, used for on-disk layout.
    pub alignment: u32,
    /// Snapshots of every column in declaration order.
    pub columns: Vec<ColumnSnapshot>,
    /// Snapshots of every secondary index defined on the table.
    pub indexes: Vec<IndexSnapshot>,
}

/// Snapshot of a single column definition.
///
/// Mirrors the subset of column metadata that is meaningful for migration detection; transient or derivable fields are omitted on purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct ColumnSnapshot {
    /// Column name.
    pub name: String,
    /// Stable encoding of the column data type.
    pub data_type: DataTypeSnapshot,
    /// Whether the column accepts `NULL`.
    pub nullable: bool,
    /// Whether the column is auto-incremented on insert.
    pub auto_increment: bool,
    /// Whether the column carries a `UNIQUE` constraint.
    pub unique: bool,
    /// Whether the column is part of the primary key.
    pub primary_key: bool,
    /// Foreign key reference, if the column is a foreign key.
    pub foreign_key: Option<ForeignKeySnapshot>,
    /// Default value applied when no value is supplied on insert.
    pub default: Option<Value>,
}

/// On-disk wire layout descriptor for a custom-typed column.
///
/// Tells the snapshot-driven record codec how many bytes a custom column
/// occupies in a stored record, without needing access to the user's
/// concrete `Encode` impl. Derived from `<T as Encode>::SIZE` at the time
/// the snapshot is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum WireSize {
    /// Column occupies exactly N bytes per record (`Encode::SIZE = Fixed(N)`).
    Fixed(u32),
    /// Column body is preceded by a 2-byte little-endian length prefix
    /// (the convention used by `Text`, `Blob`, `Json`, and any custom
    /// dynamic-size type — `Encode::SIZE = Dynamic`).
    LengthPrefixed,
}

/// User-defined custom-type metadata carried inside
/// [`DataTypeSnapshot::Custom`]. Boxed in the parent enum so the discriminant
/// stays compact (the migration error variants embed two `DataTypeSnapshot`s
/// each, and an inline `String` + `WireSize` would bloat
/// [`crate::error::DbmsError`] past clippy's `result_large_err` threshold).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct CustomDataTypeSnapshot {
    /// Stable type identifier (`CustomDataType::TYPE_TAG`).
    pub tag: String,
    /// On-disk wire layout used by the snapshot codec.
    pub wire_size: WireSize,
}

impl WireSize {
    /// Derive the on-disk wire layout from a custom type's [`DataSize`].
    ///
    /// `const fn` so generated code can use it inside `&[ColumnDef]`
    /// promotable array literals.
    pub const fn from_data_size(size: DataSize) -> Self {
        match size {
            DataSize::Fixed(n) => Self::Fixed(n as u32),
            DataSize::Dynamic => Self::LengthPrefixed,
        }
    }
}

/// Stable, tag-keyed encoding of a column data type.
///
/// The discriminants are part of the on-disk format and must not be reused or reordered; new variants must take a fresh tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
#[repr(u8)]
pub enum DataTypeSnapshot {
    /// Arbitrary binary blob.
    Blob = 0x50,
    /// Boolean value.
    Boolean = 0x30,
    /// User-defined custom data type, identified by name + on-disk wire layout.
    Custom(Box<CustomDataTypeSnapshot>) = 0xF0,
    /// Calendar date with no time component.
    Date = 0x40,
    /// Date and time.
    Datetime = 0x41,
    /// Arbitrary-precision decimal number.
    Decimal = 0x22,
    /// 32-bit IEEE-754 floating point.
    Float32 = 0x20,
    /// 64-bit IEEE-754 floating point.
    Float64 = 0x21,
    /// Signed 16-bit integer.
    Int16 = 0x02,
    /// Signed 32-bit integer.
    Int32 = 0x03,
    /// Signed 64-bit integer.
    Int64 = 0x04,
    /// Signed 8-bit integer.
    Int8 = 0x01,
    /// JSON document.
    Json = 0x60,
    /// UTF-8 text string.
    Text = 0x51,
    /// UUID value.
    Uuid = 0x52,
    /// Unsigned 16-bit integer.
    Uint16 = 0x11,
    /// Unsigned 32-bit integer.
    Uint32 = 0x12,
    /// Unsigned 64-bit integer.
    Uint64 = 0x13,
    /// Unsigned 8-bit integer.
    Uint8 = 0x10,
}

/// Snapshot of a secondary index defined on a table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct IndexSnapshot {
    /// Names of the columns covered by the index, in index order.
    pub columns: Vec<String>,
    /// Whether the index enforces uniqueness across the covered columns.
    pub unique: bool,
}

/// Snapshot of a foreign key reference attached to a column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct ForeignKeySnapshot {
    /// Name of the referenced table.
    pub table: String,
    /// Name of the referenced column on the target table.
    pub column: String,
    /// Action performed on referenced row deletion.
    pub on_delete: OnDeleteSnapshot,
}

/// Stable, tag-keyed encoding of the `ON DELETE` referential action.
///
/// Mirrors [`DeleteBehavior`](crate::dbms::query::DeleteBehavior). Discriminants are part of the on-disk format and must not be reused
/// or reordered; new variants must take a fresh tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
#[repr(u8)]
pub enum OnDeleteSnapshot {
    /// Reject deletion of referenced row while dependent rows exist.
    Restrict = 0x01,
    /// Delete dependent rows together with referenced row.
    Cascade = 0x02,
}

/// Largest value of a one-byte length or count field in the snapshot format.
const U8_FIELD_MAX: usize = u8::MAX as usize;

/// Largest value of a two-byte length or count field in the snapshot format.
const U16_FIELD_MAX: usize = u16::MAX as usize;

/// Checks that `len` fits a snapshot field whose largest value is `max`.
///
/// `field` is only evaluated on error, to describe the offending metadata.
///
/// # Errors
///
/// Returns [`MemoryError::ConstraintViolation`] when `len` exceeds `max`.
fn check_field_len<F>(field: F, len: usize, max: usize) -> MemoryResult<usize>
where
    F: FnOnce() -> String,
{
    if len > max {
        return Err(MemoryError::ConstraintViolation(format!(
            "{} is {len}, but the schema snapshot format allows at most {max}",
            field()
        )));
    }
    Ok(len)
}

/// Narrows a length already checked by [`TableSchemaSnapshot::validate_encoding`]
/// to its one-byte field, saturating instead of wrapping.
fn u8_len(len: usize) -> u8 {
    u8::try_from(len).unwrap_or(u8::MAX)
}

/// Narrows a length already checked by [`TableSchemaSnapshot::validate_encoding`]
/// to its two-byte little-endian field, saturating instead of wrapping.
fn u16_len(len: usize) -> [u8; 2] {
    u16::try_from(len).unwrap_or(u16::MAX).to_le_bytes()
}

impl TableSchemaSnapshot {
    /// Returns the latest version of the snapshot format.
    pub fn latest_version() -> u8 {
        SCHEMA_SNAPSHOT_VERSION
    }

    /// Checks that every name, count and nested payload fits its on-disk
    /// field, so the snapshot round-trips exactly through [`Encode`].
    ///
    /// Table, primary key, column, foreign key, custom type and index column
    /// names, and the number of columns of each index, are limited to 255
    /// bytes or entries. Column and index counts, each encoded column, index
    /// and default value, and the whole snapshot are limited to 65 535 bytes
    /// or entries.
    ///
    /// Must be called before the snapshot is persisted: [`Encode::encode`]
    /// cannot report an error and its output is unspecified for a snapshot
    /// that fails this check.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::ConstraintViolation`] naming the first field
    /// that exceeds its limit.
    pub fn validate_encoding(&self) -> MemoryResult<()> {
        // version(1) + name_len(1) + name + pk_len(1) + pk + alignment(4)
        let mut total = 1
            + 1
            + check_field_len(
                || "table name length".to_string(),
                self.name.len(),
                U8_FIELD_MAX,
            )?
            + 1
            + check_field_len(
                || format!("table `{}` primary key name length", self.name),
                self.primary_key.len(),
                U8_FIELD_MAX,
            )?
            + 4;

        // columns_len(2) + sum(col_size_prefix(2) + col bytes)
        check_field_len(
            || format!("table `{}` column count", self.name),
            self.columns.len(),
            U16_FIELD_MAX,
        )?;
        total += 2;
        for column in &self.columns {
            total += 2 + check_field_len(
                || format!("column `{}.{}` encoded length", self.name, column.name),
                column.checked_encoded_len(&self.name)?,
                U16_FIELD_MAX,
            )?;
        }

        // indexes_len(2) + sum(idx_size_prefix(2) + idx bytes)
        check_field_len(
            || format!("table `{}` index count", self.name),
            self.indexes.len(),
            U16_FIELD_MAX,
        )?;
        total += 2;
        for index in &self.indexes {
            total += 2 + check_field_len(
                || format!("table `{}` index encoded length", self.name),
                index.checked_encoded_len(&self.name)?,
                U16_FIELD_MAX,
            )?;
        }

        check_field_len(
            || format!("table `{}` schema snapshot length", self.name),
            total,
            MSize::MAX as usize,
        )
        .map(|_| ())
    }
}

impl IndexSnapshot {
    /// Returns the exact encoded length after checking every one-byte field.
    fn checked_encoded_len(&self, table: &str) -> MemoryResult<usize> {
        check_field_len(
            || format!("table `{table}` index column count"),
            self.columns.len(),
            U8_FIELD_MAX,
        )?;
        // columns_len(1) + sum(col_len(1) + col bytes) + unique(1)
        let mut len = 2;
        for column in &self.columns {
            len += 1 + check_field_len(
                || format!("table `{table}` index column `{column}` name length"),
                column.len(),
                U8_FIELD_MAX,
            )?;
        }
        Ok(len)
    }
}

impl ForeignKeySnapshot {
    /// Returns the exact encoded length after checking every one-byte field.
    fn checked_encoded_len(&self, column: &str) -> MemoryResult<usize> {
        // table_len(1) + table + column_len(1) + column + on_delete(1)
        Ok(1 + check_field_len(
            || format!("column `{column}` foreign key table name length"),
            self.table.len(),
            U8_FIELD_MAX,
        )? + 1
            + check_field_len(
                || format!("column `{column}` foreign key column name length"),
                self.column.len(),
                U8_FIELD_MAX,
            )?
            + 1)
    }
}

impl DataTypeSnapshot {
    /// Returns the exact encoded length after checking every one-byte field.
    fn checked_encoded_len(&self, column: &str) -> MemoryResult<usize> {
        match self {
            DataTypeSnapshot::Custom(meta) => {
                let ws_bytes = match meta.wire_size {
                    WireSize::Fixed(_) => 1 + 4,
                    WireSize::LengthPrefixed => 1,
                };
                // tag(1) + wire_size header + name_len(1) + name bytes
                Ok(1 + ws_bytes
                    + 1
                    + check_field_len(
                        || format!("column `{column}` custom type tag length"),
                        meta.tag.len(),
                        U8_FIELD_MAX,
                    )?)
            }
            _ => Ok(1),
        }
    }
}

impl ColumnSnapshot {
    /// Returns the exact encoded length after checking every nested field.
    fn checked_encoded_len(&self, table: &str) -> MemoryResult<usize> {
        // name_len(1) + name + data_type + flags(1) + fk_flag(1) + default_flag(1)
        let mut len =
            1 + check_field_len(
                || format!("table `{table}` column name length"),
                self.name.len(),
                U8_FIELD_MAX,
            )? + self.data_type.checked_encoded_len(&self.name)?
                + 3;
        if let Some(fk) = &self.foreign_key {
            // fk_size_prefix(2) + fk bytes
            len += 2 + check_field_len(
                || format!("column `{}` foreign key encoded length", self.name),
                fk.checked_encoded_len(&self.name)?,
                U16_FIELD_MAX,
            )?;
        }
        if let Some(value) = &self.default {
            // default_size_prefix(2) + value bytes
            len += 2 + check_field_len(
                || format!("column `{}` default value encoded length", self.name),
                Encode::encode(value).len(),
                U16_FIELD_MAX,
            )?;
        }
        Ok(len)
    }
}

impl Encode for IndexSnapshot {
    const ALIGNMENT: PageOffset = 32;

    const SIZE: DataSize = DataSize::Dynamic;

    fn size(&self) -> crate::prelude::MSize {
        // 1 byte for columns_len + (1 + column bytes) * columns_len + 1 byte for the unique tag
        let columns: usize = self.columns.iter().map(|col| 1 + col.len()).sum();
        saturating_size(2, columns)
    }

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(self.size() as usize);
        bytes.push(u8_len(self.columns.len()));
        for col in &self.columns {
            bytes.push(u8_len(col.len()));
            bytes.extend_from_slice(col.as_bytes());
        }
        bytes.push(self.unique as u8);

        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::prelude::MemoryResult<Self>
    where
        Self: Sized,
    {
        let data = data.into_owned();
        let mut offset = 0;
        if data.len() < 2 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }

        let columns_len = data[offset] as usize;
        offset += 1;
        let mut columns = Vec::with_capacity(columns_len);
        for _ in 0..columns_len {
            if data.len() < offset + 1 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let col_len = data[offset] as usize;
            offset += 1;
            if data.len() < offset + col_len + 1 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let col = String::from_utf8(data[offset..offset + col_len].to_vec())?;
            offset += col_len;
            columns.push(col);
        }

        let unique = data[offset] != 0;

        Ok(Self { columns, unique })
    }
}

impl Encode for ForeignKeySnapshot {
    const ALIGNMENT: PageOffset = 32;

    const SIZE: DataSize = DataSize::Dynamic;

    fn size(&self) -> crate::prelude::MSize {
        // 1 byte for the table_len + table bytes + 1 byte for the column_len + column bytes + 1 byte for the on_delete tag
        saturating_size(3, self.table.len().saturating_add(self.column.len()))
    }

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(self.size() as usize);
        bytes.push(u8_len(self.table.len()));
        bytes.extend_from_slice(self.table.as_bytes());
        bytes.push(u8_len(self.column.len()));
        bytes.extend_from_slice(self.column.as_bytes());
        bytes.push(self.on_delete as u8);

        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::prelude::MemoryResult<Self>
    where
        Self: Sized,
    {
        let data = data.into_owned();
        let mut offset = 0;
        if data.len() < 3 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }

        let table_len = data[offset] as usize;
        offset += 1;
        if data.len() < offset + table_len + 1 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let table = String::from_utf8(data[offset..offset + table_len].to_vec())?;
        offset += table_len;

        let column_len = data[offset] as usize;
        offset += 1;
        if data.len() < offset + column_len + 1 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let column = String::from_utf8(data[offset..offset + column_len].to_vec())?;
        offset += column_len;

        let on_delete = match data[offset] {
            0x01 => OnDeleteSnapshot::Restrict,
            0x02 => OnDeleteSnapshot::Cascade,
            value => {
                return Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
                    format!("Unknown `OnDeleteSnapshot`: {value:#x}"),
                )));
            }
        };

        Ok(Self {
            table,
            column,
            on_delete,
        })
    }
}

impl Encode for DataTypeSnapshot {
    const ALIGNMENT: PageOffset = 32;

    const SIZE: DataSize = DataSize::Dynamic;

    fn size(&self) -> crate::prelude::MSize {
        match self {
            // 1 tag + wire_size header + 1 name_len + name bytes
            DataTypeSnapshot::Custom(meta) => {
                let ws_bytes: crate::prelude::MSize = match meta.wire_size {
                    // 1 ws_tag + 4 (u32 LE)
                    WireSize::Fixed(_) => 1 + 4,
                    // 1 ws_tag
                    WireSize::LengthPrefixed => 1,
                };
                saturating_size(1 + ws_bytes + 1, meta.tag.len())
            }
            // single tag byte
            _ => 1,
        }
    }

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let tag = match self {
            DataTypeSnapshot::Blob => 0x50u8,
            DataTypeSnapshot::Boolean => 0x30,
            DataTypeSnapshot::Custom(_) => 0xF0,
            DataTypeSnapshot::Date => 0x40,
            DataTypeSnapshot::Datetime => 0x41,
            DataTypeSnapshot::Decimal => 0x22,
            DataTypeSnapshot::Float32 => 0x20,
            DataTypeSnapshot::Float64 => 0x21,
            DataTypeSnapshot::Int16 => 0x02,
            DataTypeSnapshot::Int32 => 0x03,
            DataTypeSnapshot::Int64 => 0x04,
            DataTypeSnapshot::Int8 => 0x01,
            DataTypeSnapshot::Json => 0x60,
            DataTypeSnapshot::Text => 0x51,
            DataTypeSnapshot::Uuid => 0x52,
            DataTypeSnapshot::Uint16 => 0x11,
            DataTypeSnapshot::Uint32 => 0x12,
            DataTypeSnapshot::Uint64 => 0x13,
            DataTypeSnapshot::Uint8 => 0x10,
        };

        match self {
            DataTypeSnapshot::Custom(meta) => {
                let mut bytes = Vec::with_capacity(self.size() as usize);
                bytes.push(tag);
                match meta.wire_size {
                    WireSize::Fixed(n) => {
                        bytes.push(0x01u8);
                        bytes.extend_from_slice(&n.to_le_bytes());
                    }
                    WireSize::LengthPrefixed => {
                        bytes.push(0x02u8);
                    }
                }
                bytes.push(u8_len(meta.tag.len()));
                bytes.extend_from_slice(meta.tag.as_bytes());
                std::borrow::Cow::Owned(bytes)
            }
            _ => std::borrow::Cow::Owned(vec![tag]),
        }
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::prelude::MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.is_empty() {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }

        let tag = data[0];
        match tag {
            0x01 => Ok(DataTypeSnapshot::Int8),
            0x02 => Ok(DataTypeSnapshot::Int16),
            0x03 => Ok(DataTypeSnapshot::Int32),
            0x04 => Ok(DataTypeSnapshot::Int64),
            0x10 => Ok(DataTypeSnapshot::Uint8),
            0x11 => Ok(DataTypeSnapshot::Uint16),
            0x12 => Ok(DataTypeSnapshot::Uint32),
            0x13 => Ok(DataTypeSnapshot::Uint64),
            0x20 => Ok(DataTypeSnapshot::Float32),
            0x21 => Ok(DataTypeSnapshot::Float64),
            0x22 => Ok(DataTypeSnapshot::Decimal),
            0x30 => Ok(DataTypeSnapshot::Boolean),
            0x40 => Ok(DataTypeSnapshot::Date),
            0x41 => Ok(DataTypeSnapshot::Datetime),
            0x50 => Ok(DataTypeSnapshot::Blob),
            0x51 => Ok(DataTypeSnapshot::Text),
            0x52 => Ok(DataTypeSnapshot::Uuid),
            0x60 => Ok(DataTypeSnapshot::Json),
            0xF0 => {
                if data.len() < 2 {
                    return Err(MemoryError::DecodeError(DecodeError::TooShort));
                }
                let (wire_size, header_len) = match data[1] {
                    0x01 => {
                        if data.len() < 6 {
                            return Err(MemoryError::DecodeError(DecodeError::TooShort));
                        }
                        let n = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
                        (WireSize::Fixed(n), 6)
                    }
                    0x02 => (WireSize::LengthPrefixed, 2),
                    v => {
                        return Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
                            format!("Unknown WireSize tag: {v:#x}"),
                        )));
                    }
                };
                if data.len() < header_len + 1 {
                    return Err(MemoryError::DecodeError(DecodeError::TooShort));
                }
                let name_len = data[header_len] as usize;
                let name_off = header_len + 1;
                if data.len() < name_off + name_len {
                    return Err(MemoryError::DecodeError(DecodeError::TooShort));
                }
                let tag = String::from_utf8(data[name_off..name_off + name_len].to_vec())?;
                Ok(DataTypeSnapshot::Custom(Box::new(CustomDataTypeSnapshot {
                    tag,
                    wire_size,
                })))
            }
            value => Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
                format!("Unknown `DataTypeSnapshot` tag: {value:#x}"),
            ))),
        }
    }
}

/// Flag bits packed into the [`ColumnSnapshot`] flags byte.
const COL_FLAG_NULLABLE: u8 = 0b0000_0001;
const COL_FLAG_AUTO_INCREMENT: u8 = 0b0000_0010;
const COL_FLAG_UNIQUE: u8 = 0b0000_0100;
const COL_FLAG_PRIMARY_KEY: u8 = 0b0000_1000;

impl Encode for ColumnSnapshot {
    const ALIGNMENT: PageOffset = 32;

    const SIZE: DataSize = DataSize::Dynamic;

    fn size(&self) -> crate::prelude::MSize {
        // name_len(1) + name + data_type + flags(1)
        // + fk_flag(1) + (fk_size_prefix(2) + fk bytes)?
        // + default_flag(1) + (default_size_prefix(2) + value bytes)?
        let mut total = 1 + self.name.len() + self.data_type.size() as usize + 1;
        total += 1;
        if let Some(fk) = &self.foreign_key {
            total += 2 + fk.size() as usize;
        }
        total += 1;
        if let Some(value) = &self.default {
            total += 2 + Encode::size(value) as usize;
        }
        saturating_size(0, total)
    }

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(self.size() as usize);
        bytes.push(u8_len(self.name.len()));
        bytes.extend_from_slice(self.name.as_bytes());

        bytes.extend_from_slice(&self.data_type.encode());

        let mut flags: u8 = 0;
        if self.nullable {
            flags |= COL_FLAG_NULLABLE;
        }
        if self.auto_increment {
            flags |= COL_FLAG_AUTO_INCREMENT;
        }
        if self.unique {
            flags |= COL_FLAG_UNIQUE;
        }
        if self.primary_key {
            flags |= COL_FLAG_PRIMARY_KEY;
        }
        bytes.push(flags);

        match &self.foreign_key {
            Some(fk) => {
                bytes.push(1);
                let encoded = fk.encode();
                bytes.extend_from_slice(&u16_len(encoded.len()));
                bytes.extend_from_slice(&encoded);
            }
            None => bytes.push(0),
        }

        match &self.default {
            Some(value) => {
                bytes.push(1);
                let encoded = Encode::encode(value);
                bytes.extend_from_slice(&u16_len(encoded.len()));
                bytes.extend_from_slice(&encoded);
            }
            None => bytes.push(0),
        }

        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::prelude::MemoryResult<Self>
    where
        Self: Sized,
    {
        let data = data.into_owned();
        let mut offset = 0;

        if data.is_empty() {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let name_len = data[offset] as usize;
        offset += 1;
        if data.len() < offset + name_len {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let name = String::from_utf8(data[offset..offset + name_len].to_vec())?;
        offset += name_len;

        // data_type: peek tag, derive consumed length
        if data.len() < offset + 1 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let dt_consumed = if data[offset] == 0xF0 {
            if data.len() < offset + 2 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let header = match data[offset + 1] {
                0x01 => 6,
                0x02 => 2,
                v => {
                    return Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
                        format!("Unknown WireSize tag: {v:#x}"),
                    )));
                }
            };
            if data.len() < offset + header + 1 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            header + 1 + data[offset + header] as usize
        } else {
            1
        };
        if data.len() < offset + dt_consumed {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let data_type = DataTypeSnapshot::decode(std::borrow::Cow::Owned(
            data[offset..offset + dt_consumed].to_vec(),
        ))?;
        offset += dt_consumed;

        if data.len() < offset + 1 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let flags = data[offset];
        offset += 1;
        let nullable = flags & COL_FLAG_NULLABLE != 0;
        let auto_increment = flags & COL_FLAG_AUTO_INCREMENT != 0;
        let unique = flags & COL_FLAG_UNIQUE != 0;
        let primary_key = flags & COL_FLAG_PRIMARY_KEY != 0;

        if data.len() < offset + 1 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let fk_flag = data[offset];
        offset += 1;
        let foreign_key = if fk_flag != 0 {
            if data.len() < offset + 2 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let fk_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
            offset += 2;
            if data.len() < offset + fk_len {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let fk = ForeignKeySnapshot::decode(std::borrow::Cow::Owned(
                data[offset..offset + fk_len].to_vec(),
            ))?;
            offset += fk_len;
            Some(fk)
        } else {
            None
        };

        if data.len() < offset + 1 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let default_flag = data[offset];
        offset += 1;
        let default = if default_flag != 0 {
            if data.len() < offset + 2 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let v_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
            offset += 2;
            if data.len() < offset + v_len {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let value = Value::decode(std::borrow::Cow::Owned(
                data[offset..offset + v_len].to_vec(),
            ))?;
            Some(value)
        } else {
            None
        };

        Ok(Self {
            name,
            data_type,
            nullable,
            auto_increment,
            unique,
            primary_key,
            foreign_key,
            default,
        })
    }
}

impl Encode for TableSchemaSnapshot {
    const ALIGNMENT: PageOffset = 32;

    const SIZE: DataSize = DataSize::Dynamic;

    fn size(&self) -> crate::prelude::MSize {
        // version(1)
        // + name_len(1) + name
        // + pk_len(1) + pk
        // + alignment(4)
        // + columns_len(2) + sum(col_size_prefix(2) + col bytes)
        // + indexes_len(2) + sum(idx_size_prefix(2) + idx bytes)
        let mut total = 1 + 1 + self.name.len() + 1 + self.primary_key.len() + 4 + 2;
        for c in &self.columns {
            total += 2 + c.size() as usize;
        }
        total += 2;
        for i in &self.indexes {
            total += 2 + i.size() as usize;
        }
        saturating_size(0, total)
    }

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(self.size() as usize);
        bytes.push(self.version);

        bytes.push(u8_len(self.name.len()));
        bytes.extend_from_slice(self.name.as_bytes());

        bytes.push(u8_len(self.primary_key.len()));
        bytes.extend_from_slice(self.primary_key.as_bytes());

        bytes.extend_from_slice(&self.alignment.to_le_bytes());

        bytes.extend_from_slice(&u16_len(self.columns.len()));
        for c in &self.columns {
            let encoded = c.encode();
            bytes.extend_from_slice(&u16_len(encoded.len()));
            bytes.extend_from_slice(&encoded);
        }

        bytes.extend_from_slice(&u16_len(self.indexes.len()));
        for i in &self.indexes {
            let encoded = i.encode();
            bytes.extend_from_slice(&u16_len(encoded.len()));
            bytes.extend_from_slice(&encoded);
        }

        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::prelude::MemoryResult<Self>
    where
        Self: Sized,
    {
        let data = data.into_owned();
        let mut offset = 0;

        if data.is_empty() {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let version = data[offset];
        offset += 1;
        if version != SCHEMA_SNAPSHOT_VERSION {
            return Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
                format!("Unsupported `TableSchemaSnapshot` version: {version:#x}"),
            )));
        }

        if data.len() < offset + 1 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let name_len = data[offset] as usize;
        offset += 1;
        if data.len() < offset + name_len {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let name = String::from_utf8(data[offset..offset + name_len].to_vec())?;
        offset += name_len;

        if data.len() < offset + 1 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let pk_len = data[offset] as usize;
        offset += 1;
        if data.len() < offset + pk_len {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let primary_key = String::from_utf8(data[offset..offset + pk_len].to_vec())?;
        offset += pk_len;

        if data.len() < offset + 4 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let alignment = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;

        if data.len() < offset + 2 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let columns_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;
        let mut columns = Vec::with_capacity(columns_len);
        for _ in 0..columns_len {
            if data.len() < offset + 2 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let c_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
            offset += 2;
            if data.len() < offset + c_len {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let c = ColumnSnapshot::decode(std::borrow::Cow::Owned(
                data[offset..offset + c_len].to_vec(),
            ))?;
            offset += c_len;
            columns.push(c);
        }

        if data.len() < offset + 2 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let indexes_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;
        let mut indexes = Vec::with_capacity(indexes_len);
        for _ in 0..indexes_len {
            if data.len() < offset + 2 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let i_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
            offset += 2;
            if data.len() < offset + i_len {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let i = IndexSnapshot::decode(std::borrow::Cow::Owned(
                data[offset..offset + i_len].to_vec(),
            ))?;
            offset += i_len;
            indexes.push(i);
        }

        Ok(Self {
            version,
            name,
            primary_key,
            alignment,
            columns,
            indexes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T>(value: T) -> T
    where
        T: Encode + PartialEq + std::fmt::Debug,
    {
        let encoded = value.encode();
        assert_eq!(
            encoded.len() as crate::prelude::MSize,
            value.size(),
            "size() must match encoded length",
        );
        T::decode(std::borrow::Cow::Owned(encoded.into_owned())).expect("decode failed")
    }

    #[test]
    fn test_index_snapshot_roundtrip() {
        let idx = IndexSnapshot {
            columns: vec!["a".to_string(), "long_column_name".to_string()],
            unique: true,
        };
        assert_eq!(roundtrip(idx.clone()), idx);

        let empty = IndexSnapshot {
            columns: vec![],
            unique: false,
        };
        assert_eq!(roundtrip(empty.clone()), empty);
    }

    #[test]
    fn test_index_snapshot_decode_too_short() {
        let err = IndexSnapshot::decode(std::borrow::Cow::Owned(vec![0u8])).unwrap_err();
        assert!(matches!(
            err,
            MemoryError::DecodeError(DecodeError::TooShort)
        ));
    }

    #[test]
    fn test_foreign_key_snapshot_roundtrip() {
        for on_delete in [OnDeleteSnapshot::Restrict, OnDeleteSnapshot::Cascade] {
            let fk = ForeignKeySnapshot {
                table: "users".to_string(),
                column: "id".to_string(),
                on_delete,
            };
            assert_eq!(roundtrip(fk.clone()), fk);
        }
    }

    #[test]
    fn test_foreign_key_snapshot_decode_unknown_on_delete() {
        let bytes = vec![1u8, b'a', 1, b'b', 0xFE];
        let err = ForeignKeySnapshot::decode(std::borrow::Cow::Owned(bytes)).unwrap_err();
        assert!(matches!(
            err,
            MemoryError::DecodeError(DecodeError::IdentityDecodeError(_))
        ));
    }

    #[test]
    fn test_data_type_snapshot_roundtrip_all_variants() {
        let cases = [
            DataTypeSnapshot::Blob,
            DataTypeSnapshot::Boolean,
            DataTypeSnapshot::Date,
            DataTypeSnapshot::Datetime,
            DataTypeSnapshot::Decimal,
            DataTypeSnapshot::Float32,
            DataTypeSnapshot::Float64,
            DataTypeSnapshot::Int8,
            DataTypeSnapshot::Int16,
            DataTypeSnapshot::Int32,
            DataTypeSnapshot::Int64,
            DataTypeSnapshot::Json,
            DataTypeSnapshot::Text,
            DataTypeSnapshot::Uint8,
            DataTypeSnapshot::Uint16,
            DataTypeSnapshot::Uint32,
            DataTypeSnapshot::Uint64,
            DataTypeSnapshot::Uuid,
            DataTypeSnapshot::Custom(Box::new(CustomDataTypeSnapshot {
                tag: "Money".to_string(),
                wire_size: WireSize::Fixed(16),
            })),
            DataTypeSnapshot::Custom(Box::new(CustomDataTypeSnapshot {
                tag: String::new(),
                wire_size: WireSize::LengthPrefixed,
            })),
        ];
        for dt in cases {
            assert_eq!(roundtrip(dt.clone()), dt);
        }
    }

    #[test]
    fn test_custom_wire_size_fixed_roundtrip() {
        let dt = DataTypeSnapshot::Custom(Box::new(CustomDataTypeSnapshot {
            tag: "Money".to_string(),
            wire_size: WireSize::Fixed(8),
        }));
        assert_eq!(roundtrip(dt.clone()), dt);
    }

    #[test]
    fn test_custom_wire_size_length_prefixed_roundtrip() {
        let dt = DataTypeSnapshot::Custom(Box::new(CustomDataTypeSnapshot {
            tag: "Json".to_string(),
            wire_size: WireSize::LengthPrefixed,
        }));
        assert_eq!(roundtrip(dt.clone()), dt);
    }

    #[test]
    fn test_data_type_snapshot_decode_unknown_tag() {
        let err = DataTypeSnapshot::decode(std::borrow::Cow::Owned(vec![0xAA])).unwrap_err();
        assert!(matches!(
            err,
            MemoryError::DecodeError(DecodeError::IdentityDecodeError(_))
        ));
    }

    #[test]
    fn test_data_type_snapshot_decode_empty() {
        let err = DataTypeSnapshot::decode(std::borrow::Cow::Owned(vec![])).unwrap_err();
        assert!(matches!(
            err,
            MemoryError::DecodeError(DecodeError::TooShort)
        ));
    }

    #[test]
    fn test_data_type_snapshot_tags_are_stable() {
        // Discriminants are part of the on-disk format; this test fails loudly if any reordered or reused.
        assert_eq!(DataTypeSnapshot::Int8.encode()[0], 0x01);
        assert_eq!(DataTypeSnapshot::Int16.encode()[0], 0x02);
        assert_eq!(DataTypeSnapshot::Int32.encode()[0], 0x03);
        assert_eq!(DataTypeSnapshot::Int64.encode()[0], 0x04);
        assert_eq!(DataTypeSnapshot::Uint8.encode()[0], 0x10);
        assert_eq!(DataTypeSnapshot::Uint16.encode()[0], 0x11);
        assert_eq!(DataTypeSnapshot::Uint32.encode()[0], 0x12);
        assert_eq!(DataTypeSnapshot::Uint64.encode()[0], 0x13);
        assert_eq!(DataTypeSnapshot::Float32.encode()[0], 0x20);
        assert_eq!(DataTypeSnapshot::Float64.encode()[0], 0x21);
        assert_eq!(DataTypeSnapshot::Decimal.encode()[0], 0x22);
        assert_eq!(DataTypeSnapshot::Boolean.encode()[0], 0x30);
        assert_eq!(DataTypeSnapshot::Date.encode()[0], 0x40);
        assert_eq!(DataTypeSnapshot::Datetime.encode()[0], 0x41);
        assert_eq!(DataTypeSnapshot::Blob.encode()[0], 0x50);
        assert_eq!(DataTypeSnapshot::Text.encode()[0], 0x51);
        assert_eq!(DataTypeSnapshot::Uuid.encode()[0], 0x52);
        assert_eq!(DataTypeSnapshot::Json.encode()[0], 0x60);
        assert_eq!(
            DataTypeSnapshot::Custom(Box::new(CustomDataTypeSnapshot {
                tag: "x".into(),
                wire_size: WireSize::Fixed(0),
            }))
            .encode()[0],
            0xF0
        );
    }

    fn sample_column(name: &str) -> ColumnSnapshot {
        ColumnSnapshot {
            name: name.to_string(),
            data_type: DataTypeSnapshot::Int32,
            nullable: false,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        }
    }

    #[test]
    fn test_column_snapshot_minimal_roundtrip() {
        let col = sample_column("id");
        assert_eq!(roundtrip(col.clone()), col);
    }

    #[test]
    fn test_column_snapshot_all_flags_roundtrip() {
        let col = ColumnSnapshot {
            name: "user_id".to_string(),
            data_type: DataTypeSnapshot::Uint64,
            nullable: true,
            auto_increment: true,
            unique: true,
            primary_key: true,
            foreign_key: None,
            default: None,
        };
        assert_eq!(roundtrip(col.clone()), col);
    }

    #[test]
    fn test_column_snapshot_with_fk_roundtrip() {
        let col = ColumnSnapshot {
            name: "owner".to_string(),
            data_type: DataTypeSnapshot::Uint32,
            nullable: false,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: Some(ForeignKeySnapshot {
                table: "users".to_string(),
                column: "id".to_string(),
                on_delete: OnDeleteSnapshot::Cascade,
            }),
            default: None,
        };
        assert_eq!(roundtrip(col.clone()), col);
    }

    #[test]
    fn test_column_snapshot_with_default_roundtrip() {
        use crate::prelude::Uint32;
        let col = ColumnSnapshot {
            name: "score".to_string(),
            data_type: DataTypeSnapshot::Uint32,
            nullable: true,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: Some(Value::Uint32(Uint32(42))),
        };
        assert_eq!(roundtrip(col.clone()), col);
    }

    #[test]
    fn test_column_snapshot_with_custom_data_type_roundtrip() {
        let col = ColumnSnapshot {
            name: "amount".to_string(),
            data_type: DataTypeSnapshot::Custom(Box::new(CustomDataTypeSnapshot {
                tag: "Money".to_string(),
                wire_size: WireSize::Fixed(16),
            })),
            nullable: false,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        };
        assert_eq!(roundtrip(col.clone()), col);
    }

    #[test]
    fn test_column_snapshot_full_roundtrip() {
        use crate::prelude::Text;
        let col = ColumnSnapshot {
            name: "email".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: true,
            auto_increment: false,
            unique: true,
            primary_key: false,
            foreign_key: Some(ForeignKeySnapshot {
                table: "accounts".to_string(),
                column: "email".to_string(),
                on_delete: OnDeleteSnapshot::Restrict,
            }),
            default: Some(Value::Text(Text("none@example.com".to_string()))),
        };
        assert_eq!(roundtrip(col.clone()), col);
    }

    #[test]
    fn test_column_snapshot_decode_too_short() {
        let err = ColumnSnapshot::decode(std::borrow::Cow::Owned(vec![])).unwrap_err();
        assert!(matches!(
            err,
            MemoryError::DecodeError(DecodeError::TooShort)
        ));
    }

    #[test]
    fn test_table_schema_snapshot_empty_roundtrip() {
        let snap = TableSchemaSnapshot {
            version: TableSchemaSnapshot::latest_version(),
            name: "empty".to_string(),
            primary_key: "id".to_string(),
            alignment: 32,
            columns: vec![],
            indexes: vec![],
        };
        assert_eq!(roundtrip(snap.clone()), snap);
    }

    #[test]
    fn test_table_schema_snapshot_full_roundtrip() {
        use crate::prelude::Uint32;
        let snap = TableSchemaSnapshot {
            version: TableSchemaSnapshot::latest_version(),
            name: "users".to_string(),
            primary_key: "id".to_string(),
            alignment: 64,
            columns: vec![
                ColumnSnapshot {
                    name: "id".to_string(),
                    data_type: DataTypeSnapshot::Uint32,
                    nullable: false,
                    auto_increment: true,
                    unique: true,
                    primary_key: true,
                    foreign_key: None,
                    default: None,
                },
                ColumnSnapshot {
                    name: "owner".to_string(),
                    data_type: DataTypeSnapshot::Uint32,
                    nullable: true,
                    auto_increment: false,
                    unique: false,
                    primary_key: false,
                    foreign_key: Some(ForeignKeySnapshot {
                        table: "accounts".to_string(),
                        column: "id".to_string(),
                        on_delete: OnDeleteSnapshot::Cascade,
                    }),
                    default: Some(Value::Uint32(Uint32(0))),
                },
            ],
            indexes: vec![
                IndexSnapshot {
                    columns: vec!["owner".to_string()],
                    unique: false,
                },
                IndexSnapshot {
                    columns: vec!["owner".to_string(), "id".to_string()],
                    unique: true,
                },
            ],
        };
        assert_eq!(roundtrip(snap.clone()), snap);
    }

    #[test]
    fn test_table_schema_snapshot_unsupported_version() {
        let mut snap_bytes = TableSchemaSnapshot {
            version: TableSchemaSnapshot::latest_version(),
            name: "t".to_string(),
            primary_key: "id".to_string(),
            alignment: 32,
            columns: vec![],
            indexes: vec![],
        }
        .encode()
        .into_owned();
        snap_bytes[0] = 0xEE;
        let err = TableSchemaSnapshot::decode(std::borrow::Cow::Owned(snap_bytes)).unwrap_err();
        assert!(matches!(
            err,
            MemoryError::DecodeError(DecodeError::IdentityDecodeError(_))
        ));
    }

    #[test]
    fn test_table_schema_snapshot_decode_too_short() {
        let err = TableSchemaSnapshot::decode(std::borrow::Cow::Owned(vec![])).unwrap_err();
        assert!(matches!(
            err,
            MemoryError::DecodeError(DecodeError::TooShort)
        ));
    }

    #[test]
    fn test_latest_version_matches_constant() {
        assert_eq!(
            TableSchemaSnapshot::latest_version(),
            SCHEMA_SNAPSHOT_VERSION
        );
    }

    // -- Format limits --

    /// Snapshot with one fully populated column and one index, whose
    /// one-byte fields are mutated by the boundary tests below.
    fn limits_snapshot() -> TableSchemaSnapshot {
        TableSchemaSnapshot {
            version: TableSchemaSnapshot::latest_version(),
            name: "t".to_string(),
            primary_key: "id".to_string(),
            alignment: 32,
            columns: vec![ColumnSnapshot {
                name: "id".to_string(),
                data_type: DataTypeSnapshot::Custom(Box::new(CustomDataTypeSnapshot {
                    tag: "tag".to_string(),
                    wire_size: WireSize::Fixed(4),
                })),
                nullable: false,
                auto_increment: false,
                unique: true,
                primary_key: true,
                foreign_key: Some(ForeignKeySnapshot {
                    table: "other".to_string(),
                    column: "id".to_string(),
                    on_delete: OnDeleteSnapshot::Restrict,
                }),
                default: None,
            }],
            indexes: vec![IndexSnapshot {
                columns: vec!["id".to_string()],
                unique: true,
            }],
        }
    }

    /// Every one-byte length or count field, set to `len`.
    fn one_byte_field_cases(len: usize) -> Vec<(&'static str, TableSchemaSnapshot)> {
        let name = "x".repeat(len);
        let mut cases = Vec::new();

        let mut s = limits_snapshot();
        s.name = name.clone();
        cases.push(("table name", s));

        let mut s = limits_snapshot();
        s.primary_key = name.clone();
        cases.push(("primary key", s));

        let mut s = limits_snapshot();
        s.columns[0].name = name.clone();
        cases.push(("column name", s));

        let mut s = limits_snapshot();
        s.columns[0].data_type = DataTypeSnapshot::Custom(Box::new(CustomDataTypeSnapshot {
            tag: name.clone(),
            wire_size: WireSize::LengthPrefixed,
        }));
        cases.push(("custom type tag", s));

        let mut s = limits_snapshot();
        s.columns[0].foreign_key.as_mut().unwrap().table = name.clone();
        cases.push(("foreign key table", s));

        let mut s = limits_snapshot();
        s.columns[0].foreign_key.as_mut().unwrap().column = name.clone();
        cases.push(("foreign key column", s));

        let mut s = limits_snapshot();
        s.indexes[0].columns = vec![name];
        cases.push(("index column name", s));

        let mut s = limits_snapshot();
        s.indexes[0].columns = (0..len).map(|i| format!("c{i}")).collect();
        cases.push(("index column count", s));

        cases
    }

    #[test]
    fn test_table_schema_snapshot_round_trips_one_byte_fields_at_255() {
        for (field, snap) in one_byte_field_cases(255) {
            snap.validate_encoding()
                .unwrap_or_else(|err| panic!("{field} of 255 must be accepted: {err}"));
            assert_eq!(
                roundtrip(snap.clone()),
                snap,
                "{field} of 255 must round-trip"
            );
        }
    }

    #[test]
    fn test_table_schema_snapshot_rejects_one_byte_fields_at_256() {
        for (field, snap) in one_byte_field_cases(256) {
            let err = snap.validate_encoding().unwrap_err();
            assert!(
                matches!(err, MemoryError::ConstraintViolation(ref msg) if msg.contains("256")),
                "{field} of 256 must be rejected, got {err:?}"
            );
        }
    }

    #[test]
    fn test_table_schema_snapshot_rejects_oversized_default_value() {
        use crate::prelude::Text;

        // a large default that still fits every field round-trips
        let mut fits = limits_snapshot();
        fits.columns[0].default = Some(Value::Text(Text("x".repeat(60_000))));
        fits.validate_encoding().expect("60_000-byte default fits");
        assert_eq!(roundtrip(fits.clone()), fits);

        // discriminant(1) + prefix(2) + 65_532 = 65_535 fits the default's
        // u16 prefix, but not the enclosing column's
        let mut column_too_large = limits_snapshot();
        column_too_large.columns[0].default = Some(Value::Text(Text("x".repeat(65_532))));
        let err = column_too_large.validate_encoding().unwrap_err();
        assert!(
            matches!(err, MemoryError::ConstraintViolation(ref msg) if msg.contains("column `t.id` encoded length")),
            "got {err:?}"
        );

        let mut too_large = limits_snapshot();
        too_large.columns[0].default = Some(Value::Text(Text("x".repeat(65_533))));
        let err = too_large.validate_encoding().unwrap_err();
        assert!(
            matches!(err, MemoryError::ConstraintViolation(ref msg) if msg.contains("default value")),
            "got {err:?}"
        );
    }

    #[test]
    fn test_table_schema_snapshot_rejects_snapshot_larger_than_msize() {
        let mut snap = limits_snapshot();
        snap.columns = (0..300)
            .map(|i| ColumnSnapshot {
                name: format!("{i:0>250}"),
                ..snap.columns[0].clone()
            })
            .collect();

        // size() saturates instead of overflowing
        assert_eq!(snap.size(), crate::prelude::MSize::MAX);
        let err = snap.validate_encoding().unwrap_err();
        assert!(
            matches!(err, MemoryError::ConstraintViolation(ref msg) if msg.contains("snapshot length")),
            "got {err:?}"
        );
    }

    #[test]
    fn test_validate_encoding_agrees_with_encoded_length() {
        let snap = limits_snapshot();
        snap.validate_encoding().expect("valid snapshot");
        assert_eq!(snap.encode().len(), snap.size() as usize);
    }
}
