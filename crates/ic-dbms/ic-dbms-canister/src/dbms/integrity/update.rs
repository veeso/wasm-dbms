use ic_dbms_api::prelude::{
    ColumnDef, Database as _, Filter, IcDbmsError, IcDbmsResult, Query, QueryError, TableSchema,
    Value,
};

use super::common;
use crate::dbms::IcDbmsDatabase;

/// Integrity validator for update operations.
///
/// Unlike [`super::InsertIntegrityValidator`], this validator allows the primary key to remain
/// unchanged during an update. It verifies that:
///
/// - All column values pass their respective validators.
/// - If the primary key changed, no other record already holds the new value.
/// - All foreign keys reference existing records.
/// - All non-nullable columns are provided.
pub struct UpdateIntegrityValidator<'a, T>
where
    T: TableSchema,
{
    database: &'a IcDbmsDatabase,
    /// The primary key value of the record being updated.
    old_pk: Value,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T> UpdateIntegrityValidator<'a, T>
where
    T: TableSchema,
{
    /// Creates a new update integrity validator.
    ///
    /// The `old_pk` is the current primary key value of the record being updated, used to
    /// distinguish a PK conflict from the record simply keeping its own PK.
    pub fn new(dbms: &'a IcDbmsDatabase, old_pk: Value) -> Self {
        Self {
            database: dbms,
            old_pk,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> UpdateIntegrityValidator<'_, T>
where
    T: TableSchema,
{
    /// Verifies whether the given updated record values are valid.
    ///
    /// An update is valid when:
    ///
    /// - All column values pass their respective validators.
    /// - No primary key conflict with a *different* existing record.
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

    /// Checks for primary key conflicts with *other* records.
    ///
    /// The update is allowed if:
    /// - No record exists with the new PK, or
    /// - Exactly one record exists and it is the record being updated (i.e. the PK did not change).
    fn check_primary_key_conflict(&self, record_values: &[(ColumnDef, Value)]) -> IcDbmsResult<()> {
        let pk_name = T::primary_key();
        let new_pk = record_values
            .iter()
            .find(|(col_def, _)| col_def.name == pk_name)
            .map(|(_, value)| value.clone())
            .ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
                pk_name.to_string(),
            )))?;

        // query for records with the new PK value
        let query = Query::builder()
            .field(pk_name)
            .and_where(Filter::Eq(pk_name.to_string(), new_pk.clone()))
            .build();

        let res = self.database.select::<T>(query)?;
        match res.len() {
            0 => Ok(()),
            1 => {
                // there is one record; it's fine only if its PK matches our old PK
                // (meaning the record being updated is the same one we found)
                if new_pk == self.old_pk {
                    Ok(())
                } else {
                    Err(IcDbmsError::Query(QueryError::PrimaryKeyConflict))
                }
            }
            _ => Err(IcDbmsError::Query(QueryError::PrimaryKeyConflict)),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::tests::{Post, TestDatabaseSchema, User, load_fixtures};

    #[test]
    fn test_should_pass_update_with_unchanged_pk() {
        load_fixtures();
        let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

        // user with id=1 already exists; updating it with the same PK should pass
        let values = User::columns()
            .iter()
            .cloned()
            .zip(vec![
                Value::Uint32(1.into()),
                Value::Text("UpdatedAlice".to_string().into()),
                Value::Text("alice@example.com".into()),
                Value::Uint32(30.into()),
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let validator = UpdateIntegrityValidator::<User>::new(&dbms, Value::Uint32(1.into()));
        let result = validator.validate(&values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_should_pass_update_with_new_unique_pk() {
        load_fixtures();
        let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

        // user with id=1 changes PK to 9999 (unused)
        let values = User::columns()
            .iter()
            .cloned()
            .zip(vec![
                Value::Uint32(9999.into()),
                Value::Text("Alice".to_string().into()),
                Value::Text("alice@example.com".into()),
                Value::Uint32(30.into()),
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let validator = UpdateIntegrityValidator::<User>::new(&dbms, Value::Uint32(1.into()));
        let result = validator.validate(&values);
        assert!(result.is_ok());
    }

    #[test]
    fn test_should_fail_update_with_conflicting_pk() {
        load_fixtures();
        let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

        // user with id=1 tries to change PK to id=2 (which already belongs to another user)
        let values = User::columns()
            .iter()
            .cloned()
            .zip(vec![
                Value::Uint32(2.into()),
                Value::Text("Alice".to_string().into()),
                Value::Text("alice@example.com".into()),
                Value::Uint32(30.into()),
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let validator = UpdateIntegrityValidator::<User>::new(&dbms, Value::Uint32(1.into()));
        let result = validator.validate(&values);
        assert!(matches!(
            result,
            Err(IcDbmsError::Query(QueryError::PrimaryKeyConflict))
        ));
    }

    #[test]
    fn test_should_fail_update_with_invalid_column_value() {
        load_fixtures();
        let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

        let values = User::columns()
            .iter()
            .cloned()
            .zip(vec![
                Value::Uint32(1.into()),
                Value::Text("Alice".to_string().into()),
                Value::Text("invalid-email".to_string().into()),
                Value::Uint32(30.into()),
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let validator = UpdateIntegrityValidator::<User>::new(&dbms, Value::Uint32(1.into()));
        let result = validator.validate(&values);
        assert!(matches!(result, Err(IcDbmsError::Validation(_))));
    }

    #[test]
    fn test_should_fail_update_with_invalid_fk() {
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
    fn test_should_pass_update_with_valid_fk() {
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
    fn test_should_fail_update_with_missing_non_nullable_field() {
        load_fixtures();
        let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

        // omit the "title" non-nullable field from Post
        let values = Post::columns()
            .iter()
            .cloned()
            .filter(|col| col.name != "title")
            .zip(vec![
                Value::Uint32(1.into()),
                // title is omitted
                Value::Text("Content".to_string().into()),
                Value::Uint32(1.into()),
            ])
            .collect::<Vec<(ColumnDef, Value)>>();

        let validator = UpdateIntegrityValidator::<Post>::new(&dbms, Value::Uint32(1.into()));
        let result = validator.validate(&values);
        assert!(matches!(
            result,
            Err(IcDbmsError::Query(QueryError::MissingNonNullableField(field_name)))
                if field_name == "title"
        ));
    }
}
