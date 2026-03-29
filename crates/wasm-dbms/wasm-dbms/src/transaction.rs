// Rust guideline compliant 2026-02-28

//! Transaction management for the DBMS engine.

pub(crate) mod journal;
mod overlay;
pub mod session;

use wasm_dbms_api::prelude::{
    ColumnDef, DbmsResult, DeleteBehavior, Filter, TableSchema, UpdateRecord as _, Value,
};

pub use self::overlay::{DatabaseOverlay, IndexOverlay};

/// A transaction represents a sequence of operations performed as a single
/// logical unit of work.
#[derive(Debug, Default)]
pub struct Transaction {
    /// Stack of operations performed in this transaction.
    pub(crate) operations: Vec<TransactionOp>,
    /// Overlay to track uncommitted changes.
    overlay: DatabaseOverlay,
}

impl Transaction {
    /// Inserts a new insert operation into the transaction.
    pub fn insert<T>(&mut self, values: Vec<(ColumnDef, Value)>) -> DbmsResult<()>
    where
        T: TableSchema,
    {
        self.overlay.insert::<T>(values.clone())?;
        self.operations.push(TransactionOp::Insert {
            table: T::table_name(),
            values,
        });
        Ok(())
    }

    /// Inserts a new update operation into the transaction.
    ///
    /// `rows` is a list of `(primary_key, current_row)` pairs for each affected record.
    /// The current row is needed to track old indexed values in the overlay.
    pub fn update<T>(
        &mut self,
        patch: T::Update,
        filter: Option<Filter>,
        rows: Vec<(Value, Vec<(ColumnDef, Value)>)>,
    ) -> DbmsResult<()>
    where
        T: TableSchema,
    {
        let patch_values = patch.update_values();
        let overlay_patch: Vec<_> = patch_values
            .iter()
            .map(|(col, val)| (col.name, val.clone()))
            .collect();

        for (pk, current_row) in rows {
            self.overlay
                .update::<T>(pk, overlay_patch.clone(), &current_row);
        }

        self.operations.push(TransactionOp::Update {
            table: T::table_name(),
            patch: patch_values,
            filter,
        });
        Ok(())
    }

    /// Inserts a new delete operation into the transaction.
    ///
    /// `rows` is a list of `(primary_key, current_row)` pairs for each affected record.
    /// The current row is needed to track removed indexed values in the overlay.
    pub fn delete<T>(
        &mut self,
        behaviour: DeleteBehavior,
        filter: Option<Filter>,
        rows: Vec<(Value, Vec<(ColumnDef, Value)>)>,
    ) -> DbmsResult<()>
    where
        T: TableSchema,
    {
        for (pk, current_row) in rows {
            self.overlay.delete::<T>(pk, &current_row);
        }

        self.operations.push(TransactionOp::Delete {
            table: T::table_name(),
            behaviour,
            filter,
        });
        Ok(())
    }

    /// Returns a reference to the overlay.
    pub fn overlay(&self) -> &DatabaseOverlay {
        &self.overlay
    }

    /// Returns a mutable reference to the overlay.
    pub fn overlay_mut(&mut self) -> &mut DatabaseOverlay {
        &mut self.overlay
    }
}

/// An operation within a transaction.
#[derive(Debug)]
pub enum TransactionOp {
    Insert {
        table: &'static str,
        values: Vec<(ColumnDef, Value)>,
    },
    Delete {
        table: &'static str,
        behaviour: DeleteBehavior,
        filter: Option<Filter>,
    },
    Update {
        table: &'static str,
        patch: Vec<(ColumnDef, Value)>,
        filter: Option<Filter>,
    },
}

#[cfg(test)]
mod tests {

    use wasm_dbms_api::prelude::{
        Database as _, InsertRecord as _, Query, TableSchema as _, Text, Uint32, UpdateRecord as _,
        Value,
    };
    use wasm_dbms_macros::{DatabaseSchema, Table};
    use wasm_dbms_memory::prelude::HeapMemoryProvider;

    use super::*;
    use crate::prelude::{DbmsContext, WasmDbmsDatabase};

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "items"]
    pub struct Item {
        #[primary_key]
        pub id: Uint32,
        pub name: Text,
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
    fn test_transaction_insert_records_operation() {
        let mut tx = Transaction::default();
        let values = vec![
            (Item::columns()[0], Value::Uint32(Uint32(1))),
            (Item::columns()[1], Value::Text(Text("foo".to_string()))),
        ];
        tx.insert::<Item>(values).unwrap();
        assert_eq!(tx.operations.len(), 1);
        assert!(matches!(
            &tx.operations[0],
            TransactionOp::Insert { table: "items", .. }
        ));
    }

    #[test]
    fn test_transaction_update_records_operation() {
        let mut tx = Transaction::default();
        let patch = ItemUpdateRequest::from_values(
            &[(Item::columns()[1], Value::Text(Text("bar".to_string())))],
            Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
        );
        let current_row = vec![
            (Item::columns()[0], Value::Uint32(Uint32(1))),
            (Item::columns()[1], Value::Text(Text("foo".to_string()))),
        ];
        tx.update::<Item>(
            patch,
            Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
            vec![(Value::Uint32(Uint32(1)), current_row)],
        )
        .unwrap();
        assert_eq!(tx.operations.len(), 1);
        assert!(matches!(
            &tx.operations[0],
            TransactionOp::Update { table: "items", .. }
        ));
    }

    #[test]
    fn test_transaction_delete_records_operation() {
        let mut tx = Transaction::default();
        let current_row = vec![
            (Item::columns()[0], Value::Uint32(Uint32(1))),
            (Item::columns()[1], Value::Text(Text("foo".to_string()))),
        ];
        tx.delete::<Item>(
            DeleteBehavior::Restrict,
            Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
            vec![(Value::Uint32(Uint32(1)), current_row)],
        )
        .unwrap();
        assert_eq!(tx.operations.len(), 1);
        assert!(matches!(
            &tx.operations[0],
            TransactionOp::Delete {
                table: "items",
                behaviour: DeleteBehavior::Restrict,
                ..
            }
        ));
    }

    #[test]
    fn test_transaction_overlay_accessors() {
        let mut tx = Transaction::default();
        // Overlay should start empty
        let overlay = tx.overlay();
        let overlay_str = format!("{overlay:?}");
        assert!(overlay_str.contains("DatabaseOverlay"));

        let _overlay_mut = tx.overlay_mut();
    }

    #[test]
    fn test_transaction_multiple_operations() {
        let mut tx = Transaction::default();
        let insert_values = vec![
            (Item::columns()[0], Value::Uint32(Uint32(1))),
            (Item::columns()[1], Value::Text(Text("a".to_string()))),
        ];
        tx.insert::<Item>(insert_values.clone()).unwrap();
        tx.delete::<Item>(
            DeleteBehavior::Cascade,
            None,
            vec![(Value::Uint32(Uint32(1)), insert_values)],
        )
        .unwrap();
        assert_eq!(tx.operations.len(), 2);
    }

    #[test]
    fn test_rollback_discards_transaction() {
        let ctx = setup();
        let owner = vec![1, 2, 3];
        let tx_id = ctx.begin_transaction(owner);
        let mut db = WasmDbmsDatabase::from_transaction(&ctx, TestSchema, tx_id);

        let insert = ItemInsertRequest::from_values(&[
            (Item::columns()[0], Value::Uint32(Uint32(42))),
            (
                Item::columns()[1],
                Value::Text(Text("rolled_back".to_string())),
            ),
        ])
        .unwrap();
        db.insert::<Item>(insert).unwrap();

        db.rollback().unwrap();

        // After rollback, the record should not exist
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        let rows = db.select::<Item>(Query::builder().build()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_rollback_without_transaction_returns_error() {
        let ctx = setup();
        let mut db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        let result = db.rollback();
        assert!(result.is_err());
    }
}
