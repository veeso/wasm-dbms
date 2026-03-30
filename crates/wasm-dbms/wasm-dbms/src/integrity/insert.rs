// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Integrity validator for insert operations.

use wasm_dbms_api::prelude::{
    ColumnDef, Database as _, DbmsError, DbmsResult, Filter, Query, QueryError, TableSchema, Value,
};
use wasm_dbms_memory::prelude::{AccessControl, AccessControlList, MemoryProvider};

use super::common;
use crate::database::WasmDbmsDatabase;

/// Integrity validator for insert operations.
pub struct InsertIntegrityValidator<'a, T, M, A = AccessControlList>
where
    T: TableSchema,
    M: MemoryProvider,
    A: AccessControl,
{
    database: &'a WasmDbmsDatabase<'a, M, A>,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T, M, A> InsertIntegrityValidator<'a, T, M, A>
where
    T: TableSchema,
    M: MemoryProvider,
    A: AccessControl,
{
    /// Creates a new insert integrity validator.
    pub fn new(dbms: &'a WasmDbmsDatabase<'a, M, A>) -> Self {
        Self {
            database: dbms,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T, M, A> InsertIntegrityValidator<'_, T, M, A>
where
    T: TableSchema,
    M: MemoryProvider,
    A: AccessControl,
{
    /// Verifies whether the given insert record is valid.
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

    /// Checks for primary key conflicts.
    fn check_primary_key_conflict(&self, record_values: &[(ColumnDef, Value)]) -> DbmsResult<()> {
        let pk_name = T::primary_key();
        let pk = record_values
            .iter()
            .find(|(col_def, _)| col_def.name == pk_name)
            .map(|(_, value)| value.clone())
            .ok_or(DbmsError::Query(QueryError::MissingNonNullableField(
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
            Err(DbmsError::Query(QueryError::PrimaryKeyConflict))
        }
    }

    /// Checks for unique constraint violations.
    ///
    /// Queries each unique field individually, returning an error on the first conflict found.
    fn check_unique_constraints(&self, record_values: &[(ColumnDef, Value)]) -> DbmsResult<()> {
        for (col_def, value) in record_values.iter().filter(|(col_def, _)| col_def.unique) {
            let query = Query::builder()
                .field(T::primary_key())
                .and_where(Filter::Eq(col_def.name.to_string(), value.clone()))
                .build();

            if !self.database.select::<T>(query)?.is_empty() {
                return Err(DbmsError::Query(QueryError::UniqueConstraintViolation {
                    field: col_def.name.to_string(),
                }));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use wasm_dbms_api::prelude::{
        Database as _, InsertRecord as _, TableSchema as _, Text, Uint32, Value,
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
    fn test_insert_with_unique_field_succeeds() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_user(&db, 1, "alice");
        insert_contract(&db, 1, "CONTRACT-001", 1);
        insert_contract(&db, 2, "CONTRACT-002", 1);
    }

    #[test]
    fn test_insert_with_duplicate_unique_field_fails() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_user(&db, 1, "alice");
        insert_contract(&db, 1, "CONTRACT-001", 1);

        let insert = ContractInsertRequest::from_values(&[
            (Contract::columns()[0], Value::Uint32(Uint32(2))),
            (
                Contract::columns()[1],
                Value::Text(Text("CONTRACT-001".to_string())),
            ),
            (Contract::columns()[2], Value::Uint32(Uint32(1))),
        ])
        .unwrap();
        let result = db.insert::<Contract>(insert);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            wasm_dbms_api::prelude::DbmsError::Query(
                wasm_dbms_api::prelude::QueryError::UniqueConstraintViolation { ref field }
            ) if field == "code"
        ),);
    }

    #[test]
    fn test_insert_detects_conflict_on_each_unique_field_independently() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_user(&db, 1, "alice");
        insert_contract(&db, 1, "CONTRACT-001", 1);
        insert_contract(&db, 2, "CONTRACT-002", 1);

        // Insert with code matching the second contract (not the first)
        let insert = ContractInsertRequest::from_values(&[
            (Contract::columns()[0], Value::Uint32(Uint32(3))),
            (
                Contract::columns()[1],
                Value::Text(Text("CONTRACT-002".to_string())),
            ),
            (Contract::columns()[2], Value::Uint32(Uint32(1))),
        ])
        .unwrap();
        let result = db.insert::<Contract>(insert);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            wasm_dbms_api::prelude::DbmsError::Query(
                wasm_dbms_api::prelude::QueryError::UniqueConstraintViolation { ref field }
            ) if field == "code"
        ),);
    }
}
