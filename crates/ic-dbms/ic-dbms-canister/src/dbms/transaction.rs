//! This module contains the implementation of transactions within the DBMS engine.

mod overlay;
mod session;

use ic_dbms_api::prelude::{
    ColumnDef, DeleteBehavior, Filter, IcDbmsResult, TableSchema, UpdateRecord as _, Value,
};

pub use self::overlay::DatabaseOverlay;
pub use self::session::TRANSACTION_SESSION;

/// A transaction represents a sequence of operations performed as a single logical unit of work.
#[derive(Debug, Default)]
pub struct Transaction {
    /// Stack of operations performed in this transaction.
    pub(super) operations: Vec<TransactionOp>,
    /// Overlay to track uncommitted changes.
    overlay: DatabaseOverlay,
}

impl Transaction {
    /// Insert a new `insert` operation into the transaction.
    pub fn insert<T>(&mut self, values: Vec<(ColumnDef, Value)>) -> IcDbmsResult<()>
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

    /// Insert a new `update` operation into the transaction.
    pub fn update<T>(
        &mut self,
        patch: T::Update,
        filter: Option<Filter>,
        primary_keys: Vec<Value>,
    ) -> IcDbmsResult<()>
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

    /// Insert a new `delete` operation into the transaction.
    pub fn delete<T>(
        &mut self,
        behaviour: DeleteBehavior,
        filter: Option<Filter>,
        primary_keys: Vec<Value>,
    ) -> IcDbmsResult<()>
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

    /// Get a reference to the [`DatabaseOverlay`] associated with this transaction.
    pub fn overlay(&self) -> &DatabaseOverlay {
        &self.overlay
    }

    /// Get a mutable reference to the [`DatabaseOverlay`] associated with this transaction.
    pub fn overlay_mut(&mut self) -> &mut DatabaseOverlay {
        &mut self.overlay
    }
}

/// An enum representing the different types of operations that can be performed within a transaction.
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
mod test {

    use ic_dbms_api::prelude::{DataTypeKind, Text, Uint32};

    use super::*;
    use crate::tests::User;

    #[test]
    fn test_should_create_default_transaction() {
        let tx = Transaction::default();
        assert!(tx.operations.is_empty());
    }

    #[test]
    fn test_should_insert_into_transaction() {
        let mut tx = Transaction::default();

        let values: Vec<(ColumnDef, Value)> = vec![
            (
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Uint32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Uint32(Uint32(1)),
            ),
            (
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Text(Text("Alice".to_string())),
            ),
        ];

        tx.insert::<User>(values.clone())
            .expect("failed to insert into transaction");

        assert_eq!(tx.operations.len(), 1);
        match &tx.operations[0] {
            TransactionOp::Insert { table, values: v } => {
                assert_eq!(*table, "users");
                assert_eq!(v.len(), 2);
            }
            _ => panic!("expected Insert operation"),
        }
    }

    #[test]
    fn test_should_delete_from_transaction() {
        let mut tx = Transaction::default();

        let pk = Value::Uint32(Uint32(1));
        tx.delete::<User>(DeleteBehavior::Restrict, None, vec![pk])
            .expect("failed to delete from transaction");

        assert_eq!(tx.operations.len(), 1);
        match &tx.operations[0] {
            TransactionOp::Delete {
                table,
                behaviour,
                filter,
            } => {
                assert_eq!(*table, "users");
                assert_eq!(*behaviour, DeleteBehavior::Restrict);
                assert!(filter.is_none());
            }
            _ => panic!("expected Delete operation"),
        }
    }

    #[test]
    fn test_should_get_overlay_reference() {
        let tx = Transaction::default();
        let _overlay = tx.overlay();
        // Just verify we can get a reference to the overlay
    }

    #[test]
    fn test_should_get_mutable_overlay_reference() {
        let mut tx = Transaction::default();
        let _overlay = tx.overlay_mut();
        // Just verify we can get a mutable reference to the overlay
    }

    #[test]
    fn test_should_debug_transaction_op_insert() {
        let values: Vec<(ColumnDef, Value)> = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Uint32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Uint32(Uint32(1)),
        )];

        let op = TransactionOp::Insert {
            table: "users",
            values,
        };

        let debug_str = format!("{:?}", op);
        assert!(debug_str.contains("Insert"));
        assert!(debug_str.contains("users"));
    }

    #[test]
    fn test_should_debug_transaction_op_delete() {
        let op = TransactionOp::Delete {
            table: "users",
            behaviour: DeleteBehavior::Cascade,
            filter: None,
        };

        let debug_str = format!("{:?}", op);
        assert!(debug_str.contains("Delete"));
        assert!(debug_str.contains("users"));
        assert!(debug_str.contains("Cascade"));
    }

    #[test]
    fn test_should_debug_transaction_op_update() {
        let patch: Vec<(ColumnDef, Value)> = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Text(Text("Bob".to_string())),
        )];

        let op = TransactionOp::Update {
            table: "users",
            patch,
            filter: None,
        };

        let debug_str = format!("{:?}", op);
        assert!(debug_str.contains("Update"));
        assert!(debug_str.contains("users"));
    }

    #[test]
    fn test_should_debug_transaction() {
        let tx = Transaction::default();
        let debug_str = format!("{:?}", tx);
        assert!(debug_str.contains("Transaction"));
    }
}
