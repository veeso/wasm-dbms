// Rust guideline compliant 2026-02-28

//! Shared integrity-check functions used by both insert and update validators.

use wasm_dbms_api::prelude::{
    ColumnDef, Database, DbmsError, DbmsResult, ForeignFetcher, ForeignKeyDef, QueryError,
    TableSchema, Value,
};

/// Checks whether `value` passes the validator defined for `column`, if any.
pub fn check_column_validate<T: TableSchema>(column: &ColumnDef, value: &Value) -> DbmsResult<()> {
    let Some(validator) = T::validator(column.name) else {
        return Ok(());
    };

    validator.validate(value)
}

/// Checks whether all foreign keys in `record_values` reference existing records.
pub fn check_foreign_keys<T: TableSchema>(
    database: &impl Database,
    record_values: &[(ColumnDef, Value)],
) -> DbmsResult<()> {
    record_values
        .iter()
        .filter_map(|(col, value)| col.foreign_key.as_ref().map(|fk| (fk, value)))
        .try_for_each(|(fk, value)| check_foreign_key_existence::<T>(database, fk, value))
}

/// Checks whether a single foreign key references an existing record.
pub fn check_foreign_key_existence<T: TableSchema>(
    database: &impl Database,
    foreign_key: &ForeignKeyDef,
    value: &Value,
) -> DbmsResult<()> {
    let res = T::foreign_fetcher().fetch(
        database,
        foreign_key.foreign_table,
        foreign_key.local_column,
        value.clone(),
    )?;
    if res.is_empty() {
        Err(DbmsError::Query(
            QueryError::ForeignKeyConstraintViolation {
                field: foreign_key.local_column.to_string(),
                referencing_table: foreign_key.foreign_table.to_string(),
            },
        ))
    } else {
        Ok(())
    }
}

/// Checks whether all non-nullable columns are present in `record_values`.
pub fn check_non_nullable_fields<T: TableSchema>(
    record_values: &[(ColumnDef, Value)],
) -> DbmsResult<()> {
    for column in T::columns().iter().filter(|col| !col.nullable) {
        if !record_values
            .iter()
            .any(|(col_def, _)| col_def.name == column.name)
        {
            return Err(DbmsError::Query(QueryError::MissingNonNullableField(
                column.name.to_string(),
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    use wasm_dbms_api::prelude::{
        Database as _, InsertRecord as _, MaxStrlenValidator, TableSchema as _, Text, Uint32, Value,
    };
    use wasm_dbms_macros::{DatabaseSchema, Table};
    use wasm_dbms_memory::prelude::HeapMemoryProvider;

    use super::*;
    use crate::prelude::{DbmsContext, WasmDbmsDatabase};

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "users"]
    pub struct User {
        #[primary_key]
        pub id: Uint32,
        #[validate(MaxStrlenValidator(10))]
        pub name: Text,
    }

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "posts"]
    pub struct Post {
        #[primary_key]
        pub id: Uint32,
        pub title: Text,
        #[foreign_key(entity = "User", table = "users", column = "id")]
        pub user_id: Uint32,
    }

    #[derive(DatabaseSchema)]
    #[tables(User = "users", Post = "posts")]
    pub struct TestSchema;

    fn setup() -> DbmsContext<HeapMemoryProvider> {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        TestSchema::register_tables(&ctx).unwrap();
        ctx
    }

    #[test]
    fn test_check_column_validate_with_no_validator() {
        // Primary key field has no validator
        let column = User::columns()[0]; // id
        let value = Value::Uint32(Uint32(1));
        let result = check_column_validate::<User>(&column, &value);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_column_validate_passes_with_valid_value() {
        // name field has MaxStrlenValidator(10)
        let column = User::columns()[1]; // name
        let value = Value::Text(Text("short".to_string()));
        let result = check_column_validate::<User>(&column, &value);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_column_validate_fails_with_invalid_value() {
        let column = User::columns()[1]; // name
        let value = Value::Text(Text("this string is way too long".to_string()));
        let result = check_column_validate::<User>(&column, &value);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_non_nullable_fields_all_present() {
        let values = vec![
            (User::columns()[0], Value::Uint32(Uint32(1))),
            (User::columns()[1], Value::Text(Text("foo".to_string()))),
        ];
        let result = check_non_nullable_fields::<User>(&values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_non_nullable_fields_missing_field() {
        // Only provide id, missing name
        let values = vec![(User::columns()[0], Value::Uint32(Uint32(1)))];
        let result = check_non_nullable_fields::<User>(&values);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DbmsError::Query(QueryError::MissingNonNullableField(_))
        ));
    }

    #[test]
    fn test_check_foreign_keys_no_fk_columns() {
        // User table has no foreign keys
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        let values = vec![
            (User::columns()[0], Value::Uint32(Uint32(1))),
            (User::columns()[1], Value::Text(Text("foo".to_string()))),
        ];
        let result = check_foreign_keys::<User>(&db, &values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_foreign_keys_with_existing_reference() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);

        // Insert a user first
        let user_insert = UserInsertRequest::from_values(&[
            (User::columns()[0], Value::Uint32(Uint32(1))),
            (User::columns()[1], Value::Text(Text("alice".to_string()))),
        ])
        .unwrap();
        db.insert::<User>(user_insert).unwrap();

        // Now check FK for a post referencing user_id=1
        let post_values = vec![
            (Post::columns()[0], Value::Uint32(Uint32(10))),
            (Post::columns()[1], Value::Text(Text("title".to_string()))),
            (Post::columns()[2], Value::Uint32(Uint32(1))),
        ];
        let result = check_foreign_keys::<Post>(&db, &post_values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_foreign_keys_with_missing_reference() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);

        // No users inserted; FK reference to user_id=999 should fail.
        // The generated ForeignFetcher returns BrokenForeignKeyReference
        // when the referenced record does not exist.
        let post_values = vec![
            (Post::columns()[0], Value::Uint32(Uint32(10))),
            (Post::columns()[1], Value::Text(Text("title".to_string()))),
            (Post::columns()[2], Value::Uint32(Uint32(999))),
        ];
        let result = check_foreign_keys::<Post>(&db, &post_values);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DbmsError::Query(QueryError::BrokenForeignKeyReference { .. })
        ));
    }

    #[test]
    fn test_check_foreign_key_existence_found() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);

        // Insert a user
        let user_insert = UserInsertRequest::from_values(&[
            (User::columns()[0], Value::Uint32(Uint32(1))),
            (User::columns()[1], Value::Text(Text("bob".to_string()))),
        ])
        .unwrap();
        db.insert::<User>(user_insert).unwrap();

        let fk = ForeignKeyDef {
            foreign_table: "users",
            foreign_column: "id",
            local_column: "user_id",
        };
        let result = check_foreign_key_existence::<Post>(&db, &fk, &Value::Uint32(Uint32(1)));
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_foreign_key_existence_missing() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);

        let fk = ForeignKeyDef {
            foreign_table: "users",
            foreign_column: "id",
            local_column: "user_id",
        };
        let result = check_foreign_key_existence::<Post>(&db, &fk, &Value::Uint32(Uint32(999)));
        assert!(result.is_err());
        // The generated ForeignFetcher returns BrokenForeignKeyReference
        assert!(matches!(
            result.unwrap_err(),
            DbmsError::Query(QueryError::BrokenForeignKeyReference { .. })
        ));
    }
}
