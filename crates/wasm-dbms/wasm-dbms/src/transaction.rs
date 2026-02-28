// Rust guideline compliant 2026-02-28

//! Transaction management for the DBMS engine.

mod overlay;
pub mod session;

use wasm_dbms_api::prelude::{
    ColumnDef, DeleteBehavior, DbmsResult, Filter, TableSchema, UpdateRecord as _, Value,
};

pub use self::overlay::DatabaseOverlay;

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
    pub fn update<T>(
        &mut self,
        patch: T::Update,
        filter: Option<Filter>,
        primary_keys: Vec<Value>,
    ) -> DbmsResult<()>
    where
        T: TableSchema,
    {
        let patch_values = patch.update_values();
        let overlay_patch: Vec<_> = patch_values
            .iter()
            .map(|(col, val)| (col.name, val.clone()))
            .collect();

        for pk in primary_keys {
            self.overlay.update::<T>(pk, overlay_patch.clone());
        }

        self.operations.push(TransactionOp::Update {
            table: T::table_name(),
            patch: patch_values,
            filter,
        });
        Ok(())
    }

    /// Inserts a new delete operation into the transaction.
    pub fn delete<T>(
        &mut self,
        behaviour: DeleteBehavior,
        filter: Option<Filter>,
        primary_keys: Vec<Value>,
    ) -> DbmsResult<()>
    where
        T: TableSchema,
    {
        for pk in primary_keys {
            self.overlay.delete::<T>(pk);
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
