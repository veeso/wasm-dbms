// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

use wasm_dbms_api::prelude::{
    AggregateFunction, AggregatedRow, ColumnDef, DbmsResult, DeleteBehavior, Filter, JoinColumnDef,
    Query, TableSchemaSnapshot, Value,
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

    /// Performs an aggregate query for the given table name
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

    /// Returns the default value of a column for a given table, if any.
    ///
    /// Looks up the table's per-row [`Migrate::default_value`](
    /// wasm_dbms_api::prelude::Migrate::default_value) hook, then falls back
    /// to the `#[default]` attribute compiled into [`ColumnDef::default`].
    /// Used by the migration planner to satisfy `AddColumn` ops on
    /// non-nullable columns.
    fn migrate_default(table: &str, column: &str) -> Option<Value>
    where
        Self: Sized;

    /// Object-safe sibling of [`Self::migrate_default`].
    ///
    /// The macro emits a one-line dispatch to the `Sized` variant so callers
    /// holding a `&dyn DatabaseSchema` can resolve `AddColumn` defaults
    /// without re-genericising on `S`.
    fn migrate_default_dyn(&self, table: &str, column: &str) -> Option<Value>;

    /// Transforms a stored value when migrating a column to an incompatible
    /// type by dispatching to the table's
    /// [`Migrate::transform_column`](wasm_dbms_api::prelude::Migrate::transform_column)
    /// hook.
    fn migrate_transform(table: &str, column: &str, old: Value) -> DbmsResult<Option<Value>>
    where
        Self: Sized;

    /// Object-safe sibling of [`Self::migrate_transform`].
    fn migrate_transform_dyn(
        &self,
        table: &str,
        column: &str,
        old: Value,
    ) -> DbmsResult<Option<Value>>;

    /// Returns the compile-time [`TableSchemaSnapshot`] for every table in the
    /// schema.
    ///
    /// Used by drift detection (boot path) and by the migration planner to
    /// diff against the snapshots stored on disk.
    fn compiled_snapshots() -> Vec<TableSchemaSnapshot>
    where
        Self: Sized;

    /// Object-safe sibling of [`Self::compiled_snapshots`].
    ///
    /// `WasmDbmsDatabase` holds a `Box<dyn DatabaseSchema>`, so it cannot call
    /// `compiled_snapshots()` directly (that method requires `Self: Sized`).
    fn compiled_snapshots_dyn(&self) -> Vec<TableSchemaSnapshot>;

    /// Returns the compile-time `renamed_from` chain for `column` on `table`.
    ///
    /// Used by the migration diff stage to detect rename operations: when a
    /// compiled column does not match any stored column by name, the diff
    /// walks this list looking for a stored column under a previous name.
    fn renamed_from_dyn(&self, table: &str, column: &str) -> Vec<&'static str>;
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{
        Database as _, DbmsResult, InsertRecord as _, Migrate, Query, TableSchema as _, Text,
        Uint32, Value,
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

    /// Table exercising the `#[default = ...]` field attribute. The literal
    /// `42_u32` flows through the macro, gets wrapped in
    /// `Value::from(...)`, and surfaces on `ColumnDef::default`.
    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "score_defaulted"]
    pub struct ScoreDefaulted {
        #[primary_key]
        pub id: Uint32,
        #[default = 42]
        pub score: Uint32,
    }

    /// Table exercising the `#[renamed_from(...)]` field attribute. The
    /// previous-name slice flows through to `ColumnDef::renamed_from` for
    /// the migration planner to consume on rename detection.
    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "renamed_table"]
    pub struct RenamedTable {
        #[primary_key]
        pub id: Uint32,
        #[renamed_from("old_name", "older_name")]
        pub name: Text,
    }

    /// Table opting into a manual `impl Migrate` via the `#[migrate]` struct
    /// attribute. The macro must NOT emit its own empty impl, otherwise the
    /// user impl below would be a duplicate.
    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "custom_migrate"]
    #[migrate]
    pub struct CustomMigrate {
        #[primary_key]
        pub id: Uint32,
        pub label: Text,
    }

    impl Migrate for CustomMigrate {
        fn default_value(column: &str) -> Option<Value> {
            if column == "label" {
                Some(Value::Text(Text("user-default".to_string())))
            } else {
                None
            }
        }

        fn transform_column(column: &str, _old: Value) -> DbmsResult<Option<Value>> {
            if column == "label" {
                Ok(Some(Value::Text(Text("transformed".to_string()))))
            } else {
                Ok(None)
            }
        }
    }

    #[derive(DatabaseSchema)]
    #[tables(Item = "items")]
    pub struct TestSchema;

    /// Multi-table schema covering every macro variant introduced for
    /// migrations, exercised through the new `DatabaseSchema` dispatch
    /// methods (`migrate_default`, `migrate_transform`, `compiled_snapshots`).
    #[derive(DatabaseSchema)]
    #[tables(
        Item = "items",
        ScoreDefaulted = "score_defaulted",
        RenamedTable = "renamed_table",
        CustomMigrate = "custom_migrate"
    )]
    pub struct MigrationSchema;

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

    // ---------- Macro tests for migration metadata --------------------------

    /// `#[default = LIT]` on a field must surface as a `fn() -> Value` on the
    /// column's `default` slot, returning a `Value` of the same variant as
    /// the column's data type.
    #[test]
    fn test_default_attribute_emits_constructor_on_column_def() {
        let columns = ScoreDefaulted::columns();
        let id = columns.iter().find(|c| c.name == "id").unwrap();
        let score = columns.iter().find(|c| c.name == "score").unwrap();

        assert!(id.default.is_none(), "id has no #[default]");
        let ctor = score.default.expect("score must have a default");
        assert_eq!(ctor(), Value::Uint32(Uint32(42)));
    }

    /// `#[renamed_from(...)]` populates the `renamed_from` slice in
    /// declaration order; absent on fields that did not use the attribute.
    #[test]
    fn test_renamed_from_attribute_populates_slice() {
        let columns = RenamedTable::columns();
        let id = columns.iter().find(|c| c.name == "id").unwrap();
        let name = columns.iter().find(|c| c.name == "name").unwrap();

        assert!(id.renamed_from.is_empty());
        assert_eq!(name.renamed_from, &["old_name", "older_name"]);
    }

    /// Default-impl path: the macro emits `impl Migrate for T {}`, so both
    /// `default_value` and `transform_column` produce trait defaults.
    #[test]
    fn test_table_macro_emits_default_migrate_impl() {
        assert_eq!(
            <Item as Migrate>::default_value("id"),
            None,
            "default Migrate returns None for default_value"
        );
        assert!(matches!(
            <Item as Migrate>::transform_column("id", Value::Uint32(Uint32(7))),
            Ok(None)
        ));
    }

    /// `#[migrate]` opts out of macro-emitted impl; the user-supplied impl is
    /// the only one in scope and its overrides take effect.
    #[test]
    fn test_migrate_struct_attribute_uses_user_impl() {
        assert_eq!(
            <CustomMigrate as Migrate>::default_value("label"),
            Some(Value::Text(Text("user-default".to_string())))
        );
        assert_eq!(<CustomMigrate as Migrate>::default_value("id"), None);

        let transformed =
            <CustomMigrate as Migrate>::transform_column("label", Value::Text(Text("x".into())))
                .expect("transform_column must succeed");
        assert_eq!(transformed, Some(Value::Text(Text("transformed".into()))));
    }

    /// `migrate_default` dispatch: the schema falls back to the column's
    /// static `#[default]` constructor when `Migrate::default_value` returns
    /// `None`.
    #[test]
    fn test_migrate_default_dispatch_falls_back_to_column_default() {
        let value = <MigrationSchema as super::DatabaseSchema<HeapMemoryProvider>>::migrate_default(
            "score_defaulted",
            "score",
        );
        assert_eq!(value, Some(Value::Uint32(Uint32(42))));
    }

    /// `migrate_default` dispatch: the user-provided `Migrate::default_value`
    /// wins over any static column default (and there is none here anyway).
    #[test]
    fn test_migrate_default_dispatch_uses_user_override() {
        let value = <MigrationSchema as super::DatabaseSchema<HeapMemoryProvider>>::migrate_default(
            "custom_migrate",
            "label",
        );
        assert_eq!(value, Some(Value::Text(Text("user-default".to_string()))));
    }

    /// `migrate_default` dispatch: unknown table → `None`, mirroring the
    /// `referenced_tables` no-match contract.
    #[test]
    fn test_migrate_default_dispatch_unknown_table_returns_none() {
        let value = <MigrationSchema as super::DatabaseSchema<HeapMemoryProvider>>::migrate_default(
            "nonexistent",
            "anything",
        );
        assert!(value.is_none());
    }

    /// `migrate_default` dispatch: known table but column without a default →
    /// `None`.
    #[test]
    fn test_migrate_default_dispatch_known_table_unknown_column_returns_none() {
        let value = <MigrationSchema as super::DatabaseSchema<HeapMemoryProvider>>::migrate_default(
            "items", "name",
        );
        assert!(value.is_none());
    }

    /// `migrate_transform` dispatch routes to the user-supplied impl on
    /// `CustomMigrate` and produces the override value.
    #[test]
    fn test_migrate_transform_dispatch_uses_user_override() {
        let value =
            <MigrationSchema as super::DatabaseSchema<HeapMemoryProvider>>::migrate_transform(
                "custom_migrate",
                "label",
                Value::Text(Text("x".into())),
            )
            .expect("transform must succeed");
        assert_eq!(value, Some(Value::Text(Text("transformed".into()))));
    }

    /// `migrate_transform` dispatch: tables with the macro-emitted default
    /// `Migrate` produce `Ok(None)` (no transform).
    #[test]
    fn test_migrate_transform_dispatch_default_impl_returns_none() {
        let value =
            <MigrationSchema as super::DatabaseSchema<HeapMemoryProvider>>::migrate_transform(
                "items",
                "id",
                Value::Uint32(Uint32(1)),
            )
            .expect("transform must succeed");
        assert!(value.is_none());
    }

    /// `migrate_transform` dispatch: unknown table → table-not-found error,
    /// matching the existing behaviour of CRUD dispatch methods.
    #[test]
    fn test_migrate_transform_dispatch_unknown_table_errors() {
        let result =
            <MigrationSchema as super::DatabaseSchema<HeapMemoryProvider>>::migrate_transform(
                "nonexistent",
                "anything",
                Value::Null,
            );
        assert!(result.is_err());
    }

    /// `compiled_snapshots` returns one snapshot per registered table, in
    /// declaration order, and each snapshot reflects the table's compile-time
    /// columns, primary key, and metadata such as `#[default]`/
    /// `#[renamed_from]`.
    #[test]
    fn test_compiled_snapshots_one_per_table_in_order() {
        let snapshots =
            <MigrationSchema as super::DatabaseSchema<HeapMemoryProvider>>::compiled_snapshots();
        assert_eq!(snapshots.len(), 4);
        assert_eq!(
            snapshots
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "items",
                "score_defaulted",
                "renamed_table",
                "custom_migrate"
            ],
        );

        let score_snapshot = snapshots
            .iter()
            .find(|s| s.name == "score_defaulted")
            .unwrap();
        let score_col = score_snapshot
            .columns
            .iter()
            .find(|c| c.name == "score")
            .unwrap();
        assert_eq!(score_col.default, Some(Value::Uint32(Uint32(42))));
        assert_eq!(score_snapshot.primary_key, "id");
    }
}
