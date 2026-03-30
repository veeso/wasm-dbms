// Rust guideline compliant 2026-02-28

mod index;

use wasm_dbms_api::prelude::{ColumnDef, IndexDef, Value};

pub use self::index::IndexOverlay;

/// The table overlay tracks uncommitted changes for a specific table.
#[derive(Debug, Clone)]
pub struct TableOverlay {
    /// The stack of operations applied to the table.
    pub(super) operations: Vec<Operation>,
    /// The index overlay for tracking uncommitted changes to indexes.
    pub(super) index_overlay: IndexOverlay,
    /// Table index definitions, used for knowing when to update indexes on insert/update/delete operations.
    indexes: &'static [IndexDef],
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
    /// Creates a new [`TableOverlay`] with the given index definitions.
    pub fn new(indexes: &'static [IndexDef]) -> Self {
        Self {
            operations: Vec::default(),
            index_overlay: IndexOverlay::default(),
            indexes,
        }
    }

    /// Inserts a new record into the overlay.
    ///
    /// Indexed column values are extracted from `record` and added to the index overlay.
    pub fn insert(&mut self, pk: Value, record: Vec<(ColumnDef, Value)>) {
        for index_def in self.indexes {
            let indexed_values = Self::extract_indexed_values(index_def.columns(), &record);
            self.index_overlay
                .insert(index_def.columns(), indexed_values, pk.clone());
        }
        self.operations.push(Operation::Insert(pk, record));
    }

    /// Updates a record in the overlay.
    ///
    /// `current_row` is the full row before the update, used to compute old indexed values
    /// for any indexes whose columns are affected by the update.
    pub fn update(
        &mut self,
        pk: Value,
        updates: Vec<(&'static str, Value)>,
        current_row: &[(ColumnDef, Value)],
    ) {
        for index_def in self.indexes {
            let columns = index_def.columns();
            let affects_index = columns
                .iter()
                .any(|col| updates.iter().any(|(name, _)| name == col));
            if affects_index {
                let old_values = Self::extract_indexed_values(columns, current_row);
                let new_values =
                    Self::compute_updated_indexed_values(columns, current_row, &updates);
                self.index_overlay
                    .update(columns, old_values, new_values, pk.clone());
            }
        }
        self.operations.push(Operation::Update(pk, updates));
    }

    /// Marks a record as deleted in the overlay.
    ///
    /// `current_row` is the full row being deleted, used to remove its indexed values
    /// from the index overlay.
    pub fn delete(&mut self, pk: Value, current_row: &[(ColumnDef, Value)]) {
        for index_def in self.indexes {
            let indexed_values = Self::extract_indexed_values(index_def.columns(), current_row);
            self.index_overlay
                .delete(index_def.columns(), indexed_values, pk.clone());
        }
        self.operations.push(Operation::Delete(pk));
    }

    /// Extracts the values for the given indexed columns from a row.
    fn extract_indexed_values(columns: &[&'static str], row: &[(ColumnDef, Value)]) -> Vec<Value> {
        columns
            .iter()
            .map(|col_name| {
                row.iter()
                    .find(|(col_def, _)| col_def.name == *col_name)
                    .map(|(_, value)| value.clone())
                    .unwrap_or(Value::Null)
            })
            .collect()
    }

    /// Computes the new indexed values after applying updates to a row.
    ///
    /// For each indexed column, uses the updated value if present in `updates`,
    /// otherwise falls back to the current row value.
    fn compute_updated_indexed_values(
        columns: &[&'static str],
        current_row: &[(ColumnDef, Value)],
        updates: &[(&'static str, Value)],
    ) -> Vec<Value> {
        columns
            .iter()
            .map(|col_name| {
                updates
                    .iter()
                    .find(|(name, _)| name == col_name)
                    .map(|(_, value)| value.clone())
                    .or_else(|| {
                        current_row
                            .iter()
                            .find(|(col_def, _)| col_def.name == *col_name)
                            .map(|(_, value)| value.clone())
                    })
                    .unwrap_or(Value::Null)
            })
            .collect()
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
    /// The current PK is tracked across operations so that a PK update
    /// (e.g. `id: 1 → 2`) correctly chains subsequent operations keyed by
    /// the new PK value. See <https://github.com/veeso/wasm-dbms/issues/65>.
    ///
    /// NOTE: `clippy::manual_try_fold`
    /// this lint is TOTALLY WRONG HERE. We may have a row which first becomes None (deleted), then an insert again returns Some.
    #[allow(clippy::manual_try_fold)]
    pub fn patch_row(&self, row: Vec<(ColumnDef, Value)>) -> Option<Vec<(ColumnDef, Value)>> {
        // get primary key value
        let mut current_pk = row
            .iter()
            .find(|(col_def, _)| col_def.primary_key)
            .map(|(_, value)| value)
            .cloned()?;

        // apply all operations for this primary key to the row, tracking PK changes
        let mut current_row = Some(row);
        for op in &self.operations {
            if op.primary_key_value() != &current_pk {
                continue;
            }
            current_row = self.apply_operation(current_row, op);
            // If an update changed the PK column, track the new value
            if let (Some(patched), Operation::Update(_, updates)) = (&current_row, op)
                && let Some((_, new_pk)) = updates.iter().find(|(name, _)| {
                    patched
                        .iter()
                        .any(|(col_def, _)| col_def.primary_key && col_def.name == *name)
                })
            {
                current_pk = new_pk.clone();
            }
        }

        current_row
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

    use wasm_dbms_api::prelude::DataTypeKind;

    use super::*;

    fn index_defs() -> &'static [IndexDef] {
        &[IndexDef(&["id"])]
    }

    fn name_index_defs() -> &'static [IndexDef] {
        &[IndexDef(&["name"])]
    }

    fn multi_index_defs() -> &'static [IndexDef] {
        &[IndexDef(&["id"]), IndexDef(&["name"])]
    }

    fn composite_index_defs() -> &'static [IndexDef] {
        &[IndexDef(&["name", "age"])]
    }

    fn col_def(name: &'static str, data_type: DataTypeKind, primary_key: bool) -> ColumnDef {
        ColumnDef {
            name,
            data_type,
            nullable: false,
            unique: false,
            primary_key,
            foreign_key: None,
        }
    }

    fn make_row(id: u32, name: &str, age: u32) -> Vec<(ColumnDef, Value)> {
        vec![
            (
                col_def("id", DataTypeKind::Uint32, true),
                Value::Uint32(id.into()),
            ),
            (
                col_def("name", DataTypeKind::Text, false),
                Value::Text(name.to_string().into()),
            ),
            (
                col_def("age", DataTypeKind::Uint32, false),
                Value::Uint32(age.into()),
            ),
        ]
    }

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
        let mut overlay = TableOverlay::new(index_defs());
        let pk = Value::Uint32(1.into());
        let row = vec![
            (
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Uint32,
                    nullable: false,
                    primary_key: true,
                    unique: false,
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
                    unique: false,
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
                    unique: false,
                    foreign_key: None,
                },
                Value::Uint32(24.into()),
            ),
        ];
        // update name
        overlay.update(
            pk.clone(),
            vec![("name", Value::Text("Bob".to_string().into()))],
            &row,
        );
        // update age
        let row_after_name_update = vec![
            row[0].clone(),
            (row[1].0, Value::Text("Bob".to_string().into())),
            row[2].clone(),
        ];
        overlay.update(
            pk.clone(),
            vec![("age", Value::Uint32(30.into()))],
            &row_after_name_update,
        );

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
                        unique: false,
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
                        unique: false,
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
                        unique: false,
                        foreign_key: None,
                    },
                    Value::Uint32(30.into()),
                ),
            ]
        );
    }

    #[test]
    fn test_should_iter_inserted_row_with_patch() {
        let mut overlay = TableOverlay::new(index_defs());
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
                        unique: false,
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
                        unique: false,
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
                        unique: false,
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
                        unique: false,
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
                        unique: false,
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
                        unique: false,
                        foreign_key: None,
                    },
                    Value::Uint32(32.into()),
                ),
            ],
        );

        // update second row
        let second_row = vec![
            (
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Uint32,
                    nullable: false,
                    primary_key: true,
                    unique: false,
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
                    unique: false,
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
                    unique: false,
                    foreign_key: None,
                },
                Value::Uint32(32.into()),
            ),
        ];
        overlay.update(
            second_pk.clone(),
            vec![("age", Value::Uint32(33.into()))],
            &second_row,
        );

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
                        unique: false,
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
                        unique: false,
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
                        unique: false,
                        foreign_key: None,
                    },
                    Value::Uint32(28.into()),
                ),
            ],
        );

        // delete third
        let third_row = vec![
            (
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Uint32,
                    nullable: false,
                    primary_key: true,
                    unique: false,
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
                    unique: false,
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
                    unique: false,
                    foreign_key: None,
                },
                Value::Uint32(28.into()),
            ),
        ];
        overlay.delete(third_pk.clone(), &third_row);

        // update second row (again) — age was updated to 33 above
        let second_row_after_age_update = vec![
            second_row[0].clone(),
            second_row[1].clone(),
            (second_row[2].0, Value::Uint32(33.into())),
        ];
        overlay.update(
            second_pk.clone(),
            vec![("name", Value::Text("Robert".to_string().into()))],
            &second_row_after_age_update,
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
                        unique: false,
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
                        unique: false,
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
                        unique: false,
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
                        unique: false,
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
                        unique: false,
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
                        unique: false,
                        foreign_key: None,
                    },
                    Value::Uint32(33.into()), // patched age
                ),
            ]
        );
    }

    // -- extract_indexed_values tests --

    #[test]
    fn test_extract_indexed_values_single_column() {
        let row = make_row(1, "Alice", 24);
        let values = TableOverlay::extract_indexed_values(&["name"], &row);
        assert_eq!(values, vec![Value::Text("Alice".to_string().into())]);
    }

    #[test]
    fn test_extract_indexed_values_composite() {
        let row = make_row(1, "Alice", 24);
        let values = TableOverlay::extract_indexed_values(&["name", "age"], &row);
        assert_eq!(
            values,
            vec![
                Value::Text("Alice".to_string().into()),
                Value::Uint32(24.into()),
            ]
        );
    }

    #[test]
    fn test_extract_indexed_values_missing_column_returns_null() {
        let row = make_row(1, "Alice", 24);
        let values = TableOverlay::extract_indexed_values(&["nonexistent"], &row);
        assert_eq!(values, vec![Value::Null]);
    }

    // -- compute_updated_indexed_values tests --

    #[test]
    fn test_compute_updated_indexed_values_with_update() {
        let row = make_row(1, "Alice", 24);
        let updates = vec![("name", Value::Text("Bob".to_string().into()))];
        let values = TableOverlay::compute_updated_indexed_values(&["name"], &row, &updates);
        assert_eq!(values, vec![Value::Text("Bob".to_string().into())]);
    }

    #[test]
    fn test_compute_updated_indexed_values_falls_back_to_current_row() {
        let row = make_row(1, "Alice", 24);
        let updates = vec![("age", Value::Uint32(30.into()))];
        // "name" not in updates, should fall back to row value
        let values = TableOverlay::compute_updated_indexed_values(&["name"], &row, &updates);
        assert_eq!(values, vec![Value::Text("Alice".to_string().into())]);
    }

    #[test]
    fn test_compute_updated_indexed_values_composite_partial_update() {
        let row = make_row(1, "Alice", 24);
        let updates = vec![("age", Value::Uint32(30.into()))];
        let values = TableOverlay::compute_updated_indexed_values(&["name", "age"], &row, &updates);
        assert_eq!(
            values,
            vec![
                Value::Text("Alice".to_string().into()), // unchanged
                Value::Uint32(30.into()),                // updated
            ]
        );
    }

    // -- insert index overlay integration tests --

    #[test]
    fn test_insert_populates_index_overlay() {
        let mut overlay = TableOverlay::new(name_index_defs());
        let row = make_row(1, "Alice", 24);
        overlay.insert(Value::Uint32(1.into()), row);

        let added = overlay
            .index_overlay
            .added_pks(&["name"], &[Value::Text("Alice".to_string().into())]);
        assert!(added.contains(&Value::Uint32(1.into())));
    }

    #[test]
    fn test_insert_populates_multiple_indexes() {
        let mut overlay = TableOverlay::new(multi_index_defs());
        let row = make_row(1, "Alice", 24);
        overlay.insert(Value::Uint32(1.into()), row);

        let id_added = overlay
            .index_overlay
            .added_pks(&["id"], &[Value::Uint32(1.into())]);
        assert!(id_added.contains(&Value::Uint32(1.into())));

        let name_added = overlay
            .index_overlay
            .added_pks(&["name"], &[Value::Text("Alice".to_string().into())]);
        assert!(name_added.contains(&Value::Uint32(1.into())));
    }

    #[test]
    fn test_insert_populates_composite_index() {
        let mut overlay = TableOverlay::new(composite_index_defs());
        let row = make_row(1, "Alice", 24);
        overlay.insert(Value::Uint32(1.into()), row);

        let added = overlay.index_overlay.added_pks(
            &["name", "age"],
            &[
                Value::Text("Alice".to_string().into()),
                Value::Uint32(24.into()),
            ],
        );
        assert!(added.contains(&Value::Uint32(1.into())));
    }

    // -- delete index overlay integration tests --

    #[test]
    fn test_delete_populates_index_overlay_removed() {
        let mut overlay = TableOverlay::new(name_index_defs());
        let row = make_row(1, "Alice", 24);
        overlay.delete(Value::Uint32(1.into()), &row);

        let removed = overlay
            .index_overlay
            .removed_pks(&["name"], &[Value::Text("Alice".to_string().into())]);
        assert!(removed.contains(&Value::Uint32(1.into())));
    }

    #[test]
    fn test_delete_populates_multiple_indexes_removed() {
        let mut overlay = TableOverlay::new(multi_index_defs());
        let row = make_row(1, "Alice", 24);
        overlay.delete(Value::Uint32(1.into()), &row);

        let id_removed = overlay
            .index_overlay
            .removed_pks(&["id"], &[Value::Uint32(1.into())]);
        assert!(id_removed.contains(&Value::Uint32(1.into())));

        let name_removed = overlay
            .index_overlay
            .removed_pks(&["name"], &[Value::Text("Alice".to_string().into())]);
        assert!(name_removed.contains(&Value::Uint32(1.into())));
    }

    // -- update index overlay integration tests --

    #[test]
    fn test_update_indexed_column_updates_overlay() {
        let mut overlay = TableOverlay::new(name_index_defs());
        let row = make_row(1, "Alice", 24);
        overlay.update(
            Value::Uint32(1.into()),
            vec![("name", Value::Text("Bob".to_string().into()))],
            &row,
        );

        // old value should be in removed
        let removed = overlay
            .index_overlay
            .removed_pks(&["name"], &[Value::Text("Alice".to_string().into())]);
        assert!(removed.contains(&Value::Uint32(1.into())));

        // new value should be in added
        let added = overlay
            .index_overlay
            .added_pks(&["name"], &[Value::Text("Bob".to_string().into())]);
        assert!(added.contains(&Value::Uint32(1.into())));
    }

    #[test]
    fn test_update_non_indexed_column_does_not_affect_index_overlay() {
        let mut overlay = TableOverlay::new(name_index_defs());
        let row = make_row(1, "Alice", 24);
        // update "age" which is not in the name index
        overlay.update(
            Value::Uint32(1.into()),
            vec![("age", Value::Uint32(30.into()))],
            &row,
        );

        // index overlay should be empty — "age" is not indexed
        let added = overlay
            .index_overlay
            .added_pks(&["name"], &[Value::Text("Alice".to_string().into())]);
        assert!(added.is_empty());

        let removed = overlay
            .index_overlay
            .removed_pks(&["name"], &[Value::Text("Alice".to_string().into())]);
        assert!(removed.is_empty());
    }

    #[test]
    fn test_update_composite_index_partial_column_change() {
        let mut overlay = TableOverlay::new(composite_index_defs());
        let row = make_row(1, "Alice", 24);
        // update only "age" — composite index ["name", "age"] is affected
        overlay.update(
            Value::Uint32(1.into()),
            vec![("age", Value::Uint32(30.into()))],
            &row,
        );

        // old composite key removed
        let removed = overlay.index_overlay.removed_pks(
            &["name", "age"],
            &[
                Value::Text("Alice".to_string().into()),
                Value::Uint32(24.into()),
            ],
        );
        assert!(removed.contains(&Value::Uint32(1.into())));

        // new composite key added (name unchanged, age updated)
        let added = overlay.index_overlay.added_pks(
            &["name", "age"],
            &[
                Value::Text("Alice".to_string().into()),
                Value::Uint32(30.into()),
            ],
        );
        assert!(added.contains(&Value::Uint32(1.into())));
    }

    // -- insert then delete consistency --

    #[test]
    fn test_patch_row_after_pk_update_applies_subsequent_operations() {
        // Reproduce #65: update PK (id: 1 → 2), then update another column keyed by new PK.
        // patch_row must apply both operations to the base row.
        let mut overlay = TableOverlay::new(index_defs());
        let original_pk = Value::Uint32(1.into());
        let row = make_row(1, "Alice", 24);

        // First op: update PK from 1 to 2 (keyed by original PK = 1)
        overlay.update(
            original_pk.clone(),
            vec![("id", Value::Uint32(2.into()))],
            &row,
        );

        // After the PK update, existing_rows_for_filter would return the row with PK = 2.
        // So the second op is keyed by the new PK = 2.
        let new_pk = Value::Uint32(2.into());
        let row_after_pk_update = make_row(2, "Alice", 24);
        overlay.update(
            new_pk.clone(),
            vec![("name", Value::Text("Bob".to_string().into()))],
            &row_after_pk_update,
        );

        // patch_row receives the original base row (PK = 1)
        let patched = overlay.patch_row(row).expect("row should not be deleted");
        assert_eq!(
            patched[0].1,
            Value::Uint32(2.into()),
            "PK should be updated to 2"
        );
        assert_eq!(
            patched[1].1,
            Value::Text("Bob".to_string().into()),
            "name should be updated to Bob"
        );
        assert_eq!(
            patched[2].1,
            Value::Uint32(24.into()),
            "age should remain 24"
        );
    }

    #[test]
    fn test_patch_row_after_pk_update_then_delete() {
        // Reproduce #65 variant: update PK then delete by new PK.
        let mut overlay = TableOverlay::new(index_defs());
        let original_pk = Value::Uint32(1.into());
        let row = make_row(1, "Alice", 24);

        // Update PK: 1 → 2
        overlay.update(
            original_pk.clone(),
            vec![("id", Value::Uint32(2.into()))],
            &row,
        );

        // Delete by new PK = 2
        let row_after_pk_update = make_row(2, "Alice", 24);
        overlay.delete(Value::Uint32(2.into()), &row_after_pk_update);

        // patch_row with original row (PK = 1) should return None (deleted)
        let patched = overlay.patch_row(row);
        assert!(
            patched.is_none(),
            "row should be deleted after PK update + delete"
        );
    }

    #[test]
    fn test_insert_then_delete_leaves_clean_index_overlay() {
        let mut overlay = TableOverlay::new(name_index_defs());
        let row = make_row(1, "Alice", 24);
        overlay.insert(Value::Uint32(1.into()), row.clone());
        overlay.delete(Value::Uint32(1.into()), &row);

        // Insert added the pk, delete should remove from added.
        // Since the entry was overlay-only (never in base index), it should NOT be in removed.
        let added = overlay
            .index_overlay
            .added_pks(&["name"], &[Value::Text("Alice".to_string().into())]);
        assert!(!added.contains(&Value::Uint32(1.into())));

        let removed = overlay
            .index_overlay
            .removed_pks(&["name"], &[Value::Text("Alice".to_string().into())]);
        assert!(!removed.contains(&Value::Uint32(1.into())));
    }
}
