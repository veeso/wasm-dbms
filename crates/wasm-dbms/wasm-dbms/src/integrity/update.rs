// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Integrity validator for update operations.

use wasm_dbms_api::prelude::{
    ColumnDef, Database as _, DbmsError, DbmsResult, Filter, Query, QueryError, TableRecord,
    TableSchema, Value,
};
use wasm_dbms_memory::prelude::{AccessControl, AccessControlList, MemoryProvider};

use super::common;
use crate::database::WasmDbmsDatabase;

/// Integrity validator for update operations.
///
/// Unlike [`super::InsertIntegrityValidator`], this validator allows the
/// primary key to remain unchanged during an update.
pub struct UpdateIntegrityValidator<'a, T, M, A = AccessControlList>
where
    T: TableSchema,
    M: MemoryProvider,
    A: AccessControl,
{
    database: &'a WasmDbmsDatabase<'a, M, A>,
    /// The current primary key value of the record being updated.
    old_pk: Value,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T, M, A> UpdateIntegrityValidator<'a, T, M, A>
where
    T: TableSchema,
    M: MemoryProvider,
    A: AccessControl,
{
    /// Creates a new update integrity validator.
    pub fn new(dbms: &'a WasmDbmsDatabase<'a, M, A>, old_pk: Value) -> Self {
        Self {
            database: dbms,
            old_pk,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T, M, A> UpdateIntegrityValidator<'_, T, M, A>
where
    T: TableSchema,
    M: MemoryProvider,
    A: AccessControl,
{
    /// Verifies whether the given updated record values are valid.
    pub fn validate(&self, record_values: &[(ColumnDef, Value)]) -> DbmsResult<()> {
        for (col, value) in record_values {
            common::check_column_validate::<T>(col, value)?;
        }
        self.check_primary_key_conflict(record_values)?;
        self.check_unique_constraints(record_values)?;
        common::check_foreign_keys::<T>(self.database, record_values)?;
        common::check_non_nullable_fields::<T>(record_values)?;

        Ok(())
    }

    /// Checks for primary key conflicts with *other* records.
    fn check_primary_key_conflict(&self, record_values: &[(ColumnDef, Value)]) -> DbmsResult<()> {
        let pk_name = T::primary_key();
        let new_pk = record_values
            .iter()
            .find(|(col_def, _)| col_def.name == pk_name)
            .map(|(_, value)| value.clone())
            .ok_or(DbmsError::Query(QueryError::MissingNonNullableField(
                pk_name.to_string(),
            )))?;

        let query = Query::builder()
            .field(pk_name)
            .and_where(Filter::Eq(pk_name.to_string(), new_pk.clone()))
            .build();

        let res = self.database.select::<T>(query)?;
        match res.len() {
            0 => Ok(()),
            1 => {
                if new_pk == self.old_pk {
                    Ok(())
                } else {
                    Err(DbmsError::Query(QueryError::PrimaryKeyConflict))
                }
            }
            _ => Err(DbmsError::Query(QueryError::PrimaryKeyConflict)),
        }
    }

    /// Checks for unique constraint violations, excluding the record being updated.
    ///
    /// For each unique field, queries for existing records with the same value.
    /// A match is only a conflict if it belongs to a different record (different primary key).
    fn check_unique_constraints(&self, record_values: &[(ColumnDef, Value)]) -> DbmsResult<()> {
        let pk_name = T::primary_key();

        for (col_def, value) in record_values.iter().filter(|(col_def, _)| col_def.unique) {
            let query = Query::builder()
                .field(pk_name)
                .and_where(Filter::Eq(col_def.name.to_string(), value.clone()))
                .build();

            let res = self.database.select::<T>(query)?;
            for record in &res {
                let record_pk = record
                    .to_values()
                    .into_iter()
                    .find(|(c, _)| c.name == pk_name)
                    .map(|(_, v)| v);

                if record_pk.as_ref() != Some(&self.old_pk) {
                    return Err(DbmsError::Query(QueryError::UniqueConstraintViolation {
                        field: col_def.name.to_string(),
                    }));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use wasm_dbms_api::prelude::{
        Database as _, Filter, InsertRecord as _, TableSchema as _, Text, Uint32,
        UpdateRecord as _, Value,
    };
    use wasm_dbms_macros::{DatabaseSchema, Table};
    use wasm_dbms_memory::prelude::HeapMemoryProvider;

    use crate::prelude::{DbmsContext, WasmDbmsDatabase};

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "users"]
    pub struct User {
        #[primary_key]
        pub id: Uint32,
        pub name: Text,
    }

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "contracts"]
    pub struct Contract {
        #[primary_key]
        pub id: Uint32,
        #[unique]
        pub code: Text,
        #[foreign_key(entity = "User", table = "users", column = "id")]
        pub user_id: Uint32,
    }

    #[derive(DatabaseSchema)]
    #[tables(User = "users", Contract = "contracts")]
    pub struct TestSchema;

    fn setup() -> DbmsContext<HeapMemoryProvider> {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        TestSchema::register_tables(&ctx).unwrap();
        ctx
    }

    fn insert_user(db: &WasmDbmsDatabase<'_, HeapMemoryProvider>, id: u32, name: &str) {
        let insert = UserInsertRequest::from_values(&[
            (User::columns()[0], Value::Uint32(Uint32(id))),
            (User::columns()[1], Value::Text(Text(name.to_string()))),
        ])
        .unwrap();
        db.insert::<User>(insert).unwrap();
    }

    fn insert_contract(
        db: &WasmDbmsDatabase<'_, HeapMemoryProvider>,
        id: u32,
        code: &str,
        user_id: u32,
    ) {
        let insert = ContractInsertRequest::from_values(&[
            (Contract::columns()[0], Value::Uint32(Uint32(id))),
            (Contract::columns()[1], Value::Text(Text(code.to_string()))),
            (Contract::columns()[2], Value::Uint32(Uint32(user_id))),
        ])
        .unwrap();
        db.insert::<Contract>(insert).unwrap();
    }

    #[test]
    fn test_update_unique_field_to_new_value_succeeds() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_user(&db, 1, "alice");
        insert_contract(&db, 1, "CONTRACT-001", 1);

        let patch = ContractUpdateRequest::from_values(
            &[(
                Contract::columns()[1],
                Value::Text(Text("CONTRACT-999".to_string())),
            )],
            Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
        );
        assert!(db.update::<Contract>(patch).is_ok());
    }

    #[test]
    fn test_update_keeping_same_unique_value_succeeds() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_user(&db, 1, "alice");
        insert_contract(&db, 1, "CONTRACT-001", 1);

        // Update the record but keep the same unique code
        let patch = ContractUpdateRequest::from_values(
            &[(
                Contract::columns()[1],
                Value::Text(Text("CONTRACT-001".to_string())),
            )],
            Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
        );
        assert!(db.update::<Contract>(patch).is_ok());
    }

    #[test]
    fn test_update_unique_field_to_existing_value_fails() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_user(&db, 1, "alice");
        insert_contract(&db, 1, "CONTRACT-001", 1);
        insert_contract(&db, 2, "CONTRACT-002", 1);

        // Try to update contract 2's code to match contract 1's code
        let patch = ContractUpdateRequest::from_values(
            &[(
                Contract::columns()[1],
                Value::Text(Text("CONTRACT-001".to_string())),
            )],
            Some(Filter::eq("id", Value::Uint32(Uint32(2)))),
        );
        let result = db.update::<Contract>(patch);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            wasm_dbms_api::prelude::DbmsError::Query(
                wasm_dbms_api::prelude::QueryError::UniqueConstraintViolation { ref field }
            ) if field == "code"
        ),);
    }
}
