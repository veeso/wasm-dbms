use candid::CandidType;
use serde::{Deserialize, Serialize};

use crate::dbms::table::{ColumnDef, TableColumns, TableRecord, TableSchema, ValuesSource};
use crate::dbms::types::{DataTypeKind, Text, Uint32};
use crate::dbms::value::Value;
use crate::memory::{DEFAULT_ALIGNMENT, Encode, PageOffset};
use crate::prelude::{
    Filter, IcDbmsError, InsertRecord, NoForeignFetcher, QueryError, UpdateRecord, Validate,
};

/// A simple user struct for testing purposes.
#[derive(Debug, Clone, PartialEq, Eq, CandidType)]
pub struct User {
    pub id: Uint32,
    pub name: Text,
}

impl Encode for User {
    const SIZE: crate::prelude::DataSize = crate::prelude::DataSize::Dynamic;

    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(self.size() as usize);
        bytes.extend_from_slice(&self.id.encode());
        bytes.extend_from_slice(&self.name.encode());
        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::prelude::MemoryResult<Self>
    where
        Self: Sized,
    {
        let mut offset = 0;
        let id = Uint32::decode(std::borrow::Cow::Borrowed(&data[offset..]))?;
        offset += id.size() as usize;
        let name = Text::decode(std::borrow::Cow::Borrowed(&data[offset..]))?;
        Ok(User { id, name })
    }

    fn size(&self) -> crate::prelude::MSize {
        self.id.size() + self.name.size()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, CandidType, Deserialize)]
pub struct UserRecord {
    pub id: Option<Uint32>,
    pub name: Option<Text>,
}

#[derive(Clone, CandidType, Serialize)]
pub struct UserInsertRequest {
    pub id: Uint32,
    pub name: Text,
}

#[derive(CandidType, Serialize)]
pub struct UserUpdateRequest {
    pub id: Option<Uint32>,
    pub name: Option<Text>,
    pub where_clause: Option<Filter>,
}

impl InsertRecord for UserInsertRequest {
    type Record = UserRecord;
    type Schema = User;

    fn from_values(values: &[(ColumnDef, Value)]) -> crate::prelude::IcDbmsResult<Self> {
        let mut id = None;
        let mut name = None;

        for (col_def, value) in values {
            match col_def.name {
                "id" => {
                    if let Value::Uint32(v) = value {
                        id = Some(*v);
                    }
                }
                "name" => {
                    if let Value::Text(v) = value {
                        name = Some(v.clone());
                    }
                }
                _ => {}
            }
        }

        Ok(UserInsertRequest {
            id: id.ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
                "id".to_string(),
            )))?,
            name: name.ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
                "name".to_string(),
            )))?,
        })
    }

    fn into_values(self) -> Vec<(ColumnDef, crate::dbms::value::Value)> {
        vec![
            (Self::Schema::columns()[0], Value::Uint32(self.id)),
            (Self::Schema::columns()[1], Value::Text(self.name)),
        ]
    }

    fn into_record(self) -> Self::Schema {
        User {
            id: self.id,
            name: self.name,
        }
    }
}

impl UpdateRecord for UserUpdateRequest {
    type Record = UserRecord;
    type Schema = User;

    fn from_values(values: &[(ColumnDef, Value)], where_clause: Option<Filter>) -> Self {
        let mut id = None;
        let mut name = None;

        for (col_def, value) in values {
            match col_def.name {
                "id" => {
                    if let Value::Uint32(v) = value {
                        id = Some(*v);
                    }
                }
                "name" => {
                    if let Value::Text(v) = value {
                        name = Some(v.clone());
                    }
                }
                _ => {}
            }
        }

        UserUpdateRequest {
            id,
            name,
            where_clause,
        }
    }

    fn update_values(&self) -> Vec<(ColumnDef, crate::dbms::value::Value)> {
        let mut values = vec![];
        if let Some(id) = self.id {
            values.push((
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Uint32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                crate::dbms::value::Value::Uint32(id),
            ));
        }
        if let Some(name) = &self.name {
            values.push((
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                crate::dbms::value::Value::Text(name.clone()),
            ));
        }
        values
    }

    fn where_clause(&self) -> Option<Filter> {
        self.where_clause.clone()
    }
}

impl TableRecord for UserRecord {
    type Schema = User;

    fn from_values(values: TableColumns) -> Self {
        let mut id = None;
        let mut name = None;

        let user_values = values
            .iter()
            .find(|(table_name, _)| *table_name == ValuesSource::This)
            .map(|(_, cols)| cols);

        for (col_def, value) in user_values.unwrap_or(&vec![]) {
            match col_def.name {
                "id" => {
                    if let crate::dbms::value::Value::Uint32(v) = value {
                        id = Some(*v);
                    }
                }
                "name" => {
                    if let crate::dbms::value::Value::Text(v) = value {
                        name = Some(v.clone());
                    }
                }
                _ => {}
            }
        }

        UserRecord { id, name }
    }

    fn to_values(&self) -> Vec<(ColumnDef, crate::dbms::value::Value)> {
        Self::Schema::columns()
            .iter()
            .zip(vec![
                match self.id {
                    Some(v) => Value::Uint32(v),
                    None => Value::Null,
                },
                match &self.name {
                    Some(v) => Value::Text(v.clone()),
                    None => Value::Null,
                },
            ])
            .map(|(col_def, value)| (*col_def, value))
            .collect()
    }
}

impl TableSchema for User {
    type Record = UserRecord;
    type Insert = UserInsertRequest;
    type Update = UserUpdateRequest;
    type ForeignFetcher = NoForeignFetcher;

    fn table_name() -> &'static str {
        "users"
    }

    fn columns() -> &'static [ColumnDef] {
        &[
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Uint32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
        ]
    }

    fn primary_key() -> &'static str {
        "id"
    }

    fn sanitizer(_column_name: &'static str) -> Option<Box<dyn crate::prelude::Sanitize>> {
        None
    }

    fn validator(_column_name: &'static str) -> Option<Box<dyn Validate>> {
        None
    }

    fn to_values(self) -> Vec<(ColumnDef, Value)> {
        vec![
            (Self::columns()[0], Value::Uint32(self.id)),
            (Self::columns()[1], Value::Text(self.name)),
        ]
    }
}

#[allow(clippy::module_inception)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_encode_decode() {
        let user = User {
            id: 42u32.into(),
            name: "Alice".to_string().into(),
        };
        let encoded = user.encode();
        let decoded = User::decode(encoded).unwrap();
        assert_eq!(user, decoded);
    }

    #[test]
    fn test_should_have_fingerprint() {
        let fingerprint = User::fingerprint();
        assert_ne!(fingerprint, 0);
    }
}

#[cfg(test)]
mod custom_type_tests {
    use std::borrow::Cow;
    use std::fmt;

    use crate::dbms::custom_value::CustomValue;
    use crate::dbms::table::{ColumnDef, TableColumns, TableRecord, TableSchema, ValuesSource};
    use crate::dbms::types::{CustomDataType, DataTypeKind, Nullable, Text, Uint32};
    use crate::dbms::value::Value;
    use crate::memory::{DEFAULT_ALIGNMENT, DataSize, Encode, MSize, MemoryResult, PageOffset};
    use crate::prelude::{DecodeError, InsertRecord, MemoryError, UpdateRecord};

    /// A simple custom data type for testing: Priority
    #[derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        Default,
        candid::CandidType,
        serde::Serialize,
        serde::Deserialize,
    )]
    pub enum Priority {
        #[default]
        Low,
        Medium,
        High,
    }

    impl fmt::Display for Priority {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Priority::Low => write!(f, "low"),
                Priority::Medium => write!(f, "medium"),
                Priority::High => write!(f, "high"),
            }
        }
    }

    impl Encode for Priority {
        const SIZE: DataSize = DataSize::Fixed(1);
        const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

        fn encode(&self) -> Cow<'_, [u8]> {
            Cow::Owned(vec![match self {
                Priority::Low => 0,
                Priority::Medium => 1,
                Priority::High => 2,
            }])
        }

        fn decode(data: Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            match data[0] {
                0 => Ok(Priority::Low),
                1 => Ok(Priority::Medium),
                2 => Ok(Priority::High),
                other => Err(MemoryError::DecodeError(DecodeError::TryFromSliceError(
                    format!("invalid Priority byte: {other}"),
                ))),
            }
        }

        fn size(&self) -> MSize {
            1
        }
    }

    impl From<Priority> for Value {
        fn from(val: Priority) -> Value {
            Value::Custom(CustomValue {
                type_tag: <Priority as CustomDataType>::TYPE_TAG.to_string(),
                encoded: Encode::encode(&val).into_owned(),
                display: val.to_string(),
            })
        }
    }

    impl crate::dbms::types::DataType for Priority {}

    impl CustomDataType for Priority {
        const TYPE_TAG: &'static str = "priority";
    }

    /// A table with a custom type field, using the Table derive macro
    #[derive(
        Debug, Clone, PartialEq, Eq, candid::CandidType, serde::Deserialize, crate::prelude::Table,
    )]
    #[table = "tasks"]
    pub struct Task {
        #[primary_key]
        pub id: Uint32,
        #[custom_type]
        pub priority: Priority,
    }

    #[test]
    fn test_columns_has_custom_data_type_kind() {
        let columns = Task::columns();
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].name, "id");
        assert_eq!(columns[0].data_type, DataTypeKind::Uint32);
        assert_eq!(columns[1].name, "priority");
        assert_eq!(columns[1].data_type, DataTypeKind::Custom("priority"));
    }

    #[test]
    fn test_to_values_produces_custom_value() {
        let task = Task {
            id: 1u32.into(),
            priority: Priority::High,
        };
        let values = task.to_values();
        assert_eq!(values.len(), 2);

        // First is Uint32
        assert!(matches!(values[0].1, Value::Uint32(_)));

        // Second is Custom
        match &values[1].1 {
            Value::Custom(cv) => {
                assert_eq!(cv.type_tag, "priority");
                assert_eq!(cv.display, "high");
                // Verify the encoded bytes decode correctly
                let decoded =
                    Priority::decode(Cow::Borrowed(&cv.encoded)).expect("decode should succeed");
                assert_eq!(decoded, Priority::High);
            }
            other => panic!("expected Value::Custom, got {other:?}"),
        }
    }

    #[test]
    fn test_table_schema_round_trip() {
        let task = Task {
            id: 42u32.into(),
            priority: Priority::Medium,
        };

        // Convert to values
        let values = task.clone().to_values();

        // Build TableColumns for Record::from_values
        let table_columns: TableColumns = vec![(ValuesSource::This, values.clone())];

        // Create record from values
        let record = TaskRecord::from_values(table_columns);
        assert_eq!(record.id, Some(42u32.into()));
        assert_eq!(record.priority, Some(Priority::Medium));

        // Round-trip record: to_values → from_values
        let record_values = record.to_values();
        let table_columns2: TableColumns = vec![(ValuesSource::This, record_values)];
        let record2 = TaskRecord::from_values(table_columns2);
        assert_eq!(record2.id, Some(42u32.into()));
        assert_eq!(record2.priority, Some(Priority::Medium));
    }

    #[test]
    fn test_insert_request_from_values() {
        let values: Vec<(ColumnDef, Value)> = vec![
            (Task::columns()[0], Value::Uint32(10u32.into())),
            (
                Task::columns()[1],
                Value::Custom(CustomValue {
                    type_tag: "priority".to_string(),
                    encoded: Encode::encode(&Priority::Low).into_owned(),
                    display: "low".to_string(),
                }),
            ),
        ];

        let insert = TaskInsertRequest::from_values(&values).expect("from_values should succeed");
        assert_eq!(insert.id, 10u32.into());
        assert_eq!(insert.priority, Priority::Low);

        // into_record
        let task = insert.into_record();
        assert_eq!(task.id, 10u32.into());
        assert_eq!(task.priority, Priority::Low);
    }

    #[test]
    fn test_update_request_from_values() {
        let values: Vec<(ColumnDef, Value)> = vec![(
            Task::columns()[1],
            Value::Custom(CustomValue {
                type_tag: "priority".to_string(),
                encoded: Encode::encode(&Priority::High).into_owned(),
                display: "high".to_string(),
            }),
        )];

        let update = TaskUpdateRequest::from_values(&values, None);
        assert_eq!(update.priority, Some(Priority::High));
    }

    /// A table with a nullable custom type field, using the Table derive macro
    #[derive(
        Debug, Clone, PartialEq, Eq, candid::CandidType, serde::Deserialize, crate::prelude::Table,
    )]
    #[table = "tasks_with_nullable"]
    pub struct TaskWithNullable {
        #[primary_key]
        pub id: Uint32,
        pub title: Text,
        #[custom_type]
        pub priority: Nullable<Priority>,
    }

    #[test]
    fn test_nullable_custom_type_round_trip() {
        // Test with a value
        let task = TaskWithNullable {
            id: Uint32(1),
            title: Text::from("Test"),
            priority: Nullable::Value(Priority::High),
        };
        let values = task.to_values();
        // Verify priority column has Value::Custom
        let priority_val = &values[2].1;
        assert!(matches!(priority_val, Value::Custom(_)));

        // Build TableColumns for Record::from_values
        let table_columns: TableColumns = vec![(ValuesSource::This, values)];

        // Test round-trip
        let record = TaskWithNullableRecord::from_values(table_columns);
        assert_eq!(record.priority, Some(Nullable::Value(Priority::High)));

        // Test with null
        let task_null = TaskWithNullable {
            id: Uint32(2),
            title: Text::from("Null test"),
            priority: Nullable::Null,
        };
        let values_null = task_null.to_values();
        assert!(matches!(values_null[2].1, Value::Null));

        let table_columns_null: TableColumns = vec![(ValuesSource::This, values_null)];
        let record_null = TaskWithNullableRecord::from_values(table_columns_null);
        assert_eq!(record_null.priority, Some(Nullable::Null));
    }
}
