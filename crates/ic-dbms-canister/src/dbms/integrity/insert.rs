use ic_dbms_api::prelude::{
    ColumnDef, Database as _, Filter, IcDbmsError, IcDbmsResult, Query, QueryError, TableSchema,
    Value,
};

use super::common;
use crate::dbms::IcDbmsDatabase;

/// Integrity validator for insert operations.
pub struct InsertIntegrityValidator<'a, T>
where
    T: TableSchema,
{
    database: &'a IcDbmsDatabase,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T> InsertIntegrityValidator<'a, T>
where
    T: TableSchema,
{
    /// Creates a new insert integrity validator.
    pub fn new(dbms: &'a IcDbmsDatabase) -> Self {
        Self {
            database: dbms,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> InsertIntegrityValidator<'_, T>
where
    T: TableSchema,
{
    /// Verify whether the given insert record is valid.
    ///
    /// An insert is valid when:
    ///
    /// - All column values pass their respective validators.
    /// - No primary key conflicts with existing records.
    /// - All foreign keys reference existing records.
    /// - All non-nullable columns are provided.
    pub fn validate(&self, record_values: &[(ColumnDef, Value)]) -> IcDbmsResult<()> {
        for (col, value) in record_values {
            common::check_column_validate::<T>(col, value)?;
        }
        self.check_primary_key_conflict(record_values)?;
        common::check_foreign_keys::<T>(self.database, record_values)?;
        common::check_non_nullable_fields::<T>(record_values)?;

        Ok(())
    }

    /// Checks for primary key conflicts.
    ///
    /// For inserts, *any* existing record with the same PK is a conflict.
    fn check_primary_key_conflict(&self, record_values: &[(ColumnDef, Value)]) -> IcDbmsResult<()>
    where
        T: TableSchema,
    {
        let pk_name = T::primary_key();
        let pk = record_values
            .iter()
            .find(|(col_def, _)| col_def.name == pk_name)
            .map(|(_, value)| value.clone())
            .ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
                pk_name.to_string(),
            )))?;

        let query: Query = Query::builder()
            .field(pk_name)
            .and_where(Filter::Eq(pk_name.to_string(), pk))
            .build();

        let res = self.database.select::<T>(query)?;
        if res.is_empty() {
            Ok(())
        } else {
            Err(IcDbmsError::Query(QueryError::PrimaryKeyConflict))
        }
    }
}

#[cfg(test)]
mod tests {

    use ic_dbms_api::prelude::DateTime;

    use super::*;
    use crate::tests::{Message, Post, TestDatabaseSchema, User, load_fixtures};

    #[test]
    fn test_should_not_pass_email_validation() {
        load_fixtures();
        let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

        let values = User::columns()
            .iter()
            .cloned()
            .zip(vec![
                Value::Uint32(10.into()),
                Value::Text("Bob".to_string().into()),
                Value::Text("invalid-email".to_string().into()),
                Value::Uint32(25.into()), // age field
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let validator = InsertIntegrityValidator::<User>::new(&dbms);
        let result = validator.validate(&values);
        assert!(matches!(result, Err(IcDbmsError::Validation(_))));
    }

    #[test]
    fn test_should_not_pass_check_for_pk_conflict() {
        load_fixtures();
        let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

        let values = User::columns()
            .iter()
            .cloned()
            .zip(vec![
                Value::Uint32(1.into()),
                Value::Text("Alice".to_string().into()),
                Value::Text("alice@example.com".into()),
                Value::Uint32(30.into()), // age field
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let validator = InsertIntegrityValidator::<User>::new(&dbms);
        let result = validator.validate(&values);
        assert!(matches!(
            result,
            Err(IcDbmsError::Query(QueryError::PrimaryKeyConflict))
        ));
    }
    #[test]
    fn test_should_pass_check_for_pk_conflict() {
        load_fixtures();
        let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

        // no conflict case
        let values = User::columns()
            .iter()
            .cloned()
            .zip(vec![
                Value::Uint32(1000.into()),
                Value::Text("Alice".to_string().into()),
                Value::Text("alice@example.com".into()),
                Value::Uint32(30.into()), // age field
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let validator = InsertIntegrityValidator::<User>::new(&dbms);
        let result = validator.validate(&values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_should_not_pass_check_for_fk_conflict() {
        load_fixtures();
        let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

        let values = Post::columns()
            .iter()
            .cloned()
            .zip(vec![
                Value::Uint32(1.into()),
                Value::Text("Title".to_string().into()),
                Value::Text("Content".to_string().into()),
                Value::Uint32(9999.into()), // non-existing user_id
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let result = common::check_foreign_keys::<Post>(&dbms, &values);
        assert!(matches!(
            result,
            Err(IcDbmsError::Query(QueryError::BrokenForeignKeyReference {
                table,
                ..
            })) if table == User::table_name()
        ));
    }

    #[test]
    fn test_should_pass_check_for_fk_conflict() {
        load_fixtures();
        let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

        let values = Post::columns()
            .iter()
            .cloned()
            .zip(vec![
                Value::Uint32(1.into()),
                Value::Text("Title".to_string().into()),
                Value::Text("Content".to_string().into()),
                Value::Uint32(1.into()), // existing user_id
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let result = common::check_foreign_keys::<Post>(&dbms, &values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_should_not_pass_non_nullable_field_check() {
        load_fixtures();

        let values = Post::columns()
            .iter()
            .cloned()
            .filter(|col| col.name != "title") // omit non-nullable field
            .zip(vec![
                Value::Uint32(1.into()),
                // Missing title
                Value::Text("Content".to_string().into()),
                Value::Uint32(1.into()),
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let result = common::check_non_nullable_fields::<Post>(&values);
        assert!(matches!(
            result,
            Err(IcDbmsError::Query(QueryError::MissingNonNullableField(
                field_name
            ))) if field_name == "title"
        ));
    }

    #[test]
    fn test_should_pass_non_nullable_field_check() {
        load_fixtures();

        let values = Message::columns()
            .iter()
            .filter(|col| !col.nullable)
            .cloned()
            .zip(vec![
                Value::Uint32(100.into()),
                Value::Text("Hello".to_string().into()),
                Value::Uint32(1.into()),
                Value::Uint32(2.into()),
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let result = common::check_non_nullable_fields::<Message>(&values);
        assert!(result.is_ok());

        // should pass with nullable set

        let values = Message::columns()
            .iter()
            .cloned()
            .zip(vec![
                Value::Uint32(100.into()),
                Value::Text("Hello".to_string().into()),
                Value::Uint32(1.into()),
                Value::Uint32(2.into()),
                Value::DateTime(DateTime {
                    year: 2024,
                    month: 6,
                    day: 1,
                    hour: 12,
                    minute: 0,
                    second: 0,
                    microsecond: 0,
                    timezone_offset_minutes: 0,
                }),
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let result = common::check_non_nullable_fields::<Message>(&values);
        assert!(result.is_ok());
    }
}
