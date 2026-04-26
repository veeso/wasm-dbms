// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

use wasm_dbms_api::prelude::{
    AggregateFunction, AggregatedRow, ColumnDef, DbmsResult, DeleteBehavior, Filter, JoinColumnDef,
    Query, Value,
};
use wasm_dbms_memory::prelude::{AccessControl, AccessControlList, MemoryProvider};

use crate::database::WasmDbmsDatabase;

/// Provides schema-driven dynamic dispatch for database operations.
///
/// Implementations of this trait know which concrete table types exist
/// and forward generic operations (identified by table name) to the
/// appropriate typed methods on [`WasmDbmsDatabase`].
///
/// This trait is typically implemented by generated code from the
/// `#[derive(DatabaseSchema)]` macro.
pub trait DatabaseSchema<M, A = AccessControlList>
where
    M: MemoryProvider,
    A: AccessControl,
{
    /// Performs a generic select for the given table name and query.
    fn select(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &str,
        query: Query,
    ) -> DbmsResult<Vec<Vec<(ColumnDef, Value)>>>;

    /// Performs a join query, returning results with column definitions
    /// that include source table names.
    fn select_join(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        from_table: &str,
        query: Query,
    ) -> DbmsResult<Vec<Vec<(JoinColumnDef, Value)>>> {
        crate::join::JoinEngine::new(self).join(dbms, from_table, query)
    }

    /// Performs an aggregate query for the given table name, dispatching to
    /// the typed [`crate::WasmDbmsDatabase::aggregate`] for the matching
    /// table.
    fn aggregate(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &str,
        query: Query,
        aggregates: &[AggregateFunction],
    ) -> DbmsResult<Vec<AggregatedRow>>;

    /// Returns tables and columns that reference the given table via foreign keys.
    fn referenced_tables(&self, table: &'static str) -> Vec<(&'static str, Vec<&'static str>)>;

    /// Performs an insert for the given table name.
    fn insert(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> DbmsResult<()>;

    /// Performs a delete for the given table name.
    fn delete(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &'static str,
        delete_behavior: DeleteBehavior,
        filter: Option<Filter>,
    ) -> DbmsResult<u64>;

    /// Performs an update for the given table name.
    fn update(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &'static str,
        patch_values: &[(ColumnDef, Value)],
        filter: Option<Filter>,
    ) -> DbmsResult<u64>;

    /// Validates an insert operation.
    fn validate_insert(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> DbmsResult<()>;

    /// Validates an update operation.
    fn validate_update(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
        old_pk: Value,
    ) -> DbmsResult<()>;
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{
        Database as _, InsertRecord as _, Query, TableSchema as _, Text, Uint32, Value,
    };
    use wasm_dbms_macros::{DatabaseSchema, Table};
    use wasm_dbms_memory::prelude::HeapMemoryProvider;

    use super::DatabaseSchema as _;
    use crate::prelude::{DbmsContext, WasmDbmsDatabase};

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "items"]
    pub struct Item {
        #[primary_key]
        pub id: Uint32,
        pub name: Text,
    }

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "products"]
    pub struct Product {
        #[primary_key]
        pub id: Uint32,
        #[index]
        pub sku: Text,
        #[index(group = "category_brand")]
        pub category: Text,
        #[index(group = "category_brand")]
        pub brand: Text,
    }

    #[derive(DatabaseSchema)]
    #[tables(Item = "items")]
    pub struct TestSchema;

    fn setup() -> DbmsContext<HeapMemoryProvider> {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        TestSchema::register_tables(&ctx).unwrap();
        ctx
    }

    #[test]
    fn test_should_register_tables_via_macro() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        TestSchema::register_tables(&ctx).unwrap();
    }

    #[test]
    fn test_should_insert_and_select_via_schema() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);

        let insert = ItemInsertRequest::from_values(&[
            (Item::columns()[0], Value::Uint32(Uint32(1))),
            (Item::columns()[1], Value::Text(Text("foo".to_string()))),
        ])
        .unwrap();
        db.insert::<Item>(insert).unwrap();

        let rows = TestSchema
            .select(&db, "items", Query::builder().build())
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][1].1, Value::Text(Text("foo".to_string())));
    }

    #[test]
    fn test_should_delete_via_schema() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);

        let insert = ItemInsertRequest::from_values(&[
            (Item::columns()[0], Value::Uint32(Uint32(1))),
            (Item::columns()[1], Value::Text(Text("foo".to_string()))),
        ])
        .unwrap();
        db.insert::<Item>(insert).unwrap();

        let deleted = TestSchema
            .delete(
                &db,
                "items",
                wasm_dbms_api::prelude::DeleteBehavior::Restrict,
                None,
            )
            .unwrap();
        assert_eq!(deleted, 1);
    }

    #[test]
    fn test_should_return_error_for_unknown_table() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);

        let result = TestSchema.select(&db, "nonexistent", Query::builder().build());
        assert!(result.is_err());
    }

    #[test]
    fn test_should_return_referenced_tables() {
        let refs = <TestSchema as super::DatabaseSchema<HeapMemoryProvider>>::referenced_tables(
            &TestSchema,
            "items",
        );
        assert!(refs.is_empty());
    }

    #[test]
    fn test_commit_rolls_back_all_operations_on_failure() {
        let ctx = setup();
        let owner = vec![1, 2, 3];

        // Begin a transaction and queue two inserts.
        let tx_id = ctx.begin_transaction(owner);
        let mut db = WasmDbmsDatabase::from_transaction(&ctx, TestSchema, tx_id);

        let first = ItemInsertRequest::from_values(&[
            (Item::columns()[0], Value::Uint32(Uint32(1))),
            (Item::columns()[1], Value::Text(Text("first".to_string()))),
        ])
        .unwrap();
        db.insert::<Item>(first).unwrap();

        let second = ItemInsertRequest::from_values(&[
            (Item::columns()[0], Value::Uint32(Uint32(2))),
            (Item::columns()[1], Value::Text(Text("second".to_string()))),
        ])
        .unwrap();
        db.insert::<Item>(second).unwrap();

        // Before committing, insert PK=2 outside the transaction so the
        // second operation will conflict at commit time.
        let oneshot = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        let conflicting = ItemInsertRequest::from_values(&[
            (Item::columns()[0], Value::Uint32(Uint32(2))),
            (
                Item::columns()[1],
                Value::Text(Text("conflict".to_string())),
            ),
        ])
        .unwrap();
        oneshot.insert::<Item>(conflicting).unwrap();

        // Commit should fail: the first insert (PK=1) succeeds, but the
        // second (PK=2) hits a primary key conflict.
        let result = db.commit();
        assert!(result.is_err());

        // Verify that the first insert was also rolled back: only the
        // conflicting row should remain.
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        let rows = db.select::<Item>(Query::builder().build()).unwrap();
        assert_eq!(rows.len(), 1, "expected only the conflicting row");
        assert_eq!(rows[0].id, Some(Uint32(2)));
        assert_eq!(rows[0].name, Some(Text("conflict".to_string())));
    }

    #[test]
    fn test_indexes_contains_pk_by_default() {
        let indexes = Item::indexes();
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].columns(), &["id"]);
    }

    #[test]
    fn test_indexes_single_and_composite() {
        let indexes = Product::indexes();
        // [pk("id"), standalone("sku"), composite("category", "brand")]
        assert_eq!(indexes.len(), 3);
        assert_eq!(indexes[0].columns(), &["id"]);
        assert_eq!(indexes[1].columns(), &["sku"]);
        assert_eq!(indexes[2].columns(), &["category", "brand"]);
    }
}
