use ic_dbms_api::prelude::{ColumnDef, Value};

/// The table overlay tracks uncommitted changes for a specific table.
#[derive(Debug, Default, Clone)]
pub struct TableOverlay {
    /// The stack of operations applied to the table.
    pub(super) operations: Vec<Operation>,
}

/// An operation within a [`TableOverlay`].
///
/// All operations are indexed by a primary key value.
#[derive(Debug, Clone)]
pub(super) enum Operation {
    Insert(Value, Vec<(ColumnDef, Value)>),
    Update(Value, Vec<(&'static str, Value)>),
    Delete(Value),
}

impl Operation {
    /// Get the primary key value associated with the operation.
    fn primary_key_value(&self) -> &Value {
        match self {
            Operation::Insert(pk, _) => pk,
            Operation::Update(pk, _) => pk,
            Operation::Delete(pk) => pk,
        }
    }
}

impl TableOverlay {
    /// Inserts a new record into the overlay.
    pub fn insert(&mut self, pk: Value, record: Vec<(ColumnDef, Value)>) {
        self.operations.push(Operation::Insert(pk, record));
    }

    /// Updates a record in the overlay.
    pub fn update(&mut self, pk: Value, updates: Vec<(&'static str, Value)>) {
        self.operations.push(Operation::Update(pk, updates));
    }

    /// Marks a record as deleted in the overlay.
    pub fn delete(&mut self, pk: Value) {
        self.operations.push(Operation::Delete(pk));
    }

    /// Returns an iterator over the inserted records which are still valid after the operation stack.
    pub fn iter_inserted(&self) -> impl Iterator<Item = Vec<(ColumnDef, Value)>> {
        self.operations.iter().filter_map(|op| {
            if let Operation::Insert(_, record) = op {
                self.patch_row(record.clone())
            } else {
                None
            }
        })
    }

    /// Patches a row with the overlay changes.
    ///
    /// The return may be [`None`] if the row has been deleted in the overlay.
    ///
    /// NOTE: `clippy::manual_try_fold`
    /// this lint is TOTALLY WRONG HERE. We may have a row which first becomes None (deleted), then an insert again returns Some.
    #[allow(clippy::manual_try_fold)]
    pub fn patch_row(&self, row: Vec<(ColumnDef, Value)>) -> Option<Vec<(ColumnDef, Value)>> {
        // get primary key value
        let pk = row
            .iter()
            .find(|(col_def, _)| col_def.primary_key)
            .map(|(_, value)| value)
            .cloned()?;

        // apply all operations for this primary key to the row
        self.operations
            .iter()
            .filter(|op| op.primary_key_value() == &pk)
            .fold(Some(row), |acc, op| self.apply_operation(acc, op))
    }

    /// Applies a single [`Operation`] to a row.
    fn apply_operation(
        &self,
        row: Option<Vec<(ColumnDef, Value)>>,
        op: &Operation,
    ) -> Option<Vec<(ColumnDef, Value)>> {
        match (row, op) {
            (_, Operation::Insert(_, record)) => Some(record.clone()), // it's definetely weird if we have `Some` row here, but just return the inserted record
            (_, Operation::Delete(_)) => None, // row is deleted; it would be weird to have `None` row here; just return None
            (None, Operation::Update(_, _)) => None, // trying to update a non-existing row; just return None
            (Some(mut existing_row), Operation::Update(_, updates)) => {
                for (col_name, new_value) in updates {
                    if let Some((_, value)) = existing_row
                        .iter_mut()
                        .find(|(col_def, _)| col_def.name == *col_name)
                    {
                        *value = new_value.clone();
                    }
                }
                Some(existing_row)
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use ic_dbms_api::prelude::DataTypeKind;

    use super::*;

    #[test]
    fn test_should_get_op_pk() {
        let op = Operation::Insert(Value::Int32(1.into()), vec![]);
        assert_eq!(op.primary_key_value(), &Value::Int32(1.into()));
        let op = Operation::Update(Value::Text("key".to_string().into()), vec![]);
        assert_eq!(
            op.primary_key_value(),
            &Value::Text("key".to_string().into())
        );
        let op = Operation::Delete(Value::Null);
        assert_eq!(op.primary_key_value(), &Value::Null);
    }

    #[test]
    fn test_should_patch_row() {
        // let's make some ops
        let mut overlay = TableOverlay::default();
        let pk = Value::Uint32(1.into());
        let row = vec![
            (
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Uint32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                pk.clone(),
            ),
            (
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Text("Alice".to_string().into()),
            ),
            (
                ColumnDef {
                    name: "age",
                    data_type: DataTypeKind::Uint32,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Uint32(24.into()),
            ),
        ];
        // update name
        overlay.update(
            pk.clone(),
            vec![("name", Value::Text("Bob".to_string().into()))],
        );
        // update age
        overlay.update(pk.clone(), vec![("age", Value::Uint32(30.into()))]);

        // get patched row
        let row = overlay.patch_row(row).expect("should be Some");
        assert_eq!(
            row,
            vec![
                (
                    ColumnDef {
                        name: "id",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: true,
                        foreign_key: None,
                    },
                    pk.clone(),
                ),
                (
                    ColumnDef {
                        name: "name",
                        data_type: DataTypeKind::Text,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Text("Bob".to_string().into()),
                ),
                (
                    ColumnDef {
                        name: "age",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Uint32(30.into()),
                ),
            ]
        );
    }

    #[test]
    fn test_should_iter_inserted_row_with_patch() {
        let mut overlay = TableOverlay::default();
        let first_pk = Value::Uint32(1.into());
        overlay.insert(
            first_pk.clone(),
            vec![
                (
                    ColumnDef {
                        name: "id",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: true,
                        foreign_key: None,
                    },
                    first_pk.clone(),
                ),
                (
                    ColumnDef {
                        name: "name",
                        data_type: DataTypeKind::Text,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Text("Alice".to_string().into()),
                ),
                (
                    ColumnDef {
                        name: "age",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Uint32(24.into()),
                ),
            ],
        );
        let second_pk = Value::Uint32(2.into());
        overlay.insert(
            second_pk.clone(),
            vec![
                (
                    ColumnDef {
                        name: "id",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: true,
                        foreign_key: None,
                    },
                    second_pk.clone(),
                ),
                (
                    ColumnDef {
                        name: "name",
                        data_type: DataTypeKind::Text,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Text("Bob".to_string().into()),
                ),
                (
                    ColumnDef {
                        name: "age",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Uint32(32.into()),
                ),
            ],
        );

        // update second row
        overlay.update(second_pk.clone(), vec![("age", Value::Uint32(33.into()))]);

        // insert a third
        let third_pk = Value::Uint32(3.into());
        overlay.insert(
            third_pk.clone(),
            vec![
                (
                    ColumnDef {
                        name: "id",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: true,
                        foreign_key: None,
                    },
                    third_pk.clone(),
                ),
                (
                    ColumnDef {
                        name: "name",
                        data_type: DataTypeKind::Text,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Text("Charlie".to_string().into()),
                ),
                (
                    ColumnDef {
                        name: "age",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Uint32(28.into()),
                ),
            ],
        );

        // delete third
        overlay.delete(third_pk.clone());

        // update second row (again)
        overlay.update(
            second_pk.clone(),
            vec![("name", Value::Text("Robert".to_string().into()))],
        );

        let inserted_rows: Vec<_> = overlay.iter_inserted().collect();
        assert_eq!(inserted_rows.len(), 2); // third should be deleted
        assert_eq!(
            inserted_rows[0],
            vec![
                (
                    ColumnDef {
                        name: "id",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: true,
                        foreign_key: None,
                    },
                    first_pk.clone(),
                ),
                (
                    ColumnDef {
                        name: "name",
                        data_type: DataTypeKind::Text,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Text("Alice".to_string().into()),
                ),
                (
                    ColumnDef {
                        name: "age",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Uint32(24.into()),
                ),
            ]
        );
        assert_eq!(
            inserted_rows[1],
            vec![
                (
                    ColumnDef {
                        name: "id",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: true,
                        foreign_key: None,
                    },
                    second_pk.clone(),
                ),
                (
                    ColumnDef {
                        name: "name",
                        data_type: DataTypeKind::Text,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Text("Robert".to_string().into()), // patched name
                ),
                (
                    ColumnDef {
                        name: "age",
                        data_type: DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Uint32(33.into()), // patched age
                ),
            ]
        );
    }
}
