use ic_dbms_api::prelude::{ColumnDef, IcDbmsResult, TableSchema, Value};

use crate::dbms::transaction::overlay::table::TableOverlay;
use crate::memory::{MemoryProvider, TableReader};

/// A reader for the database with overlay applied.
pub struct DatabaseOverlayReader<'a, T, P>
where
    T: TableSchema,
    P: MemoryProvider,
{
    /// Track the position in the new rows.
    new_rows_cursor: usize,
    /// Reference to the table overlay.
    table_overlay: &'a TableOverlay,
    /// The underlying table reader.
    table_reader: TableReader<'a, T, P>,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T, P> DatabaseOverlayReader<'a, T, P>
where
    T: TableSchema,
    P: MemoryProvider,
{
    /// Creates a new [`DatabaseOverlayReader`].
    ///
    /// # Arguments
    ///
    /// * `table_overlay`: Reference to the table overlay.
    /// * `table_reader`: The underlying table reader.
    pub fn new(table_overlay: &'a TableOverlay, table_reader: TableReader<'a, T, P>) -> Self {
        Self {
            new_rows_cursor: 0,
            table_overlay,
            table_reader,
            _marker: std::marker::PhantomData,
        }
    }

    /// Attempts to get the next row, applying overlay changes.
    pub fn try_next(&mut self) -> IcDbmsResult<Option<Vec<(ColumnDef, Value)>>> {
        loop {
            // get next from table reader
            let next_base_row = self
                .table_reader
                .try_next()?
                .map(|row| row.record.to_values());

            // if is none, get next from inserted records
            let Some(next_row) = next_base_row.or_else(|| self.next_overlay_row()) else {
                // so there are no more rows in both base and overlay
                return Ok(None);
            };

            // patch row;
            // NOTE: here if it gets None, it means it was deleted not that we finished reading, so we need to continue!
            if let Some(patched) = self.table_overlay.patch_row(next_row) {
                return Ok(Some(patched));
            }
            // keep reading
        }
    }

    /// Get the next row from the overlay's inserted records.
    fn next_overlay_row(&mut self) -> Option<Vec<(ColumnDef, Value)>> {
        let row_to_get = self.new_rows_cursor;
        self.new_rows_cursor += 1;
        self.table_overlay.iter_inserted().nth(row_to_get)
    }
}

#[cfg(test)]
mod tests {

    use ic_dbms_api::prelude::DataTypeKind;

    use super::*;
    use crate::memory::{MEMORY_MANAGER, SCHEMA_REGISTRY, TableRegistry};
    use crate::tests::{USERS_FIXTURES, User, load_fixtures};

    #[test]
    fn test_should_read_with_no_overlay() {
        load_fixtures();
        let table_overlay = TableOverlay::default();
        MEMORY_MANAGER.with_borrow(|mm| {
            let registry = registry(mm);
            let table_reader = registry.read::<User, _>(mm);

            let mut overlay_reader = DatabaseOverlayReader::new(&table_overlay, table_reader);
            // collect all
            let mut all = vec![];
            while let Some(row) = overlay_reader.try_next().expect("failed to read row") {
                all.push(row);
            }
            assert_eq!(all.len(), USERS_FIXTURES.len());
            let names = all
                .iter()
                .map(|row| {
                    row.iter()
                        .find(|(col, _)| col.name == "name")
                        .expect("name column not found")
                        .1
                        .clone()
                })
                .map(|v| match v {
                    Value::Text(s) => s.to_string(),
                    _ => panic!("expected text value"),
                })
                .collect::<Vec<_>>();

            for (i, name) in USERS_FIXTURES.iter().enumerate() {
                assert_eq!(*name, names[i].as_str());
            }
        });
    }

    #[test]
    fn test_should_not_return_deleted_records() {
        load_fixtures();
        let mut table_overlay = TableOverlay::default();
        table_overlay.delete(Value::Uint32(1.into()));
        table_overlay.delete(Value::Uint32(9.into()));

        MEMORY_MANAGER.with_borrow(|mm| {
            let registry = registry(mm);
            let table_reader = registry.read::<User, _>(mm);
            let mut overlay_reader = DatabaseOverlayReader::new(&table_overlay, table_reader);
            // collect all
            let mut all = vec![];
            while let Some(row) = overlay_reader.try_next().expect("failed to read row") {
                all.push(row);
            }
            assert_eq!(all.len(), USERS_FIXTURES.len() - 2);
        });
    }

    #[test]
    fn test_should_return_inserted_records() {
        load_fixtures();
        let mut table_overlay = TableOverlay::default();

        let first_pk = Value::Uint32(100.into());
        let new_user_1 = vec![
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
                Value::Text("NewUser1".to_string().into()),
            ),
        ];
        let second_pk: Value = Value::Uint32(101.into());
        let new_user_2 = vec![
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
                Value::Text("NewUser2".to_string().into()),
            ),
        ];
        table_overlay.insert(first_pk.clone(), new_user_1.clone());
        table_overlay.insert(second_pk.clone(), new_user_2.clone());

        MEMORY_MANAGER.with_borrow(|mm| {
            let registry = registry(mm);
            let table_reader = registry.read::<User, _>(mm);
            let mut overlay_reader = DatabaseOverlayReader::new(&table_overlay, table_reader);
            // collect all
            let mut all = vec![];
            while let Some(row) = overlay_reader.try_next().expect("failed to read row") {
                all.push(row);
            }
            assert_eq!(all.len(), USERS_FIXTURES.len() + 2);

            // check that new users are present
            let names = all
                .iter()
                .map(|row| {
                    row.iter()
                        .find(|(col, _)| col.name == "name")
                        .expect("name column not found")
                        .1
                        .clone()
                })
                .map(|v| match v {
                    Value::Text(s) => s.to_string(),
                    _ => panic!("expected text value"),
                })
                .collect::<Vec<_>>();

            assert!(names.contains(&"NewUser1".to_string()));
            assert!(names.contains(&"NewUser2".to_string()));
        });
    }

    #[test]
    fn test_should_not_return_deleted_inserted_record() {
        load_fixtures();
        let mut table_overlay = TableOverlay::default();

        let first_pk = Value::Uint32(100.into());
        let new_user_1 = vec![
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
                Value::Text("NewUser1".to_string().into()),
            ),
        ];
        table_overlay.insert(first_pk.clone(), new_user_1.clone());
        table_overlay.delete(first_pk.clone());

        MEMORY_MANAGER.with_borrow(|mm| {
            let registry = registry(mm);
            let table_reader = registry.read::<User, _>(mm);
            let mut overlay_reader = DatabaseOverlayReader::new(&table_overlay, table_reader);
            // collect all
            let mut all = vec![];
            while let Some(row) = overlay_reader.try_next().expect("failed to read row") {
                all.push(row);
            }
            assert_eq!(all.len(), USERS_FIXTURES.len());
        });
    }

    #[test]
    fn test_should_return_updated_record() {
        load_fixtures();
        let mut table_overlay = TableOverlay::default();

        let pk_to_update = Value::Uint32(1.into());
        let updated_name = "UpdatedName".to_string();
        let updates = vec![("name", Value::Text(updated_name.clone().into()))];
        table_overlay.update(pk_to_update.clone(), updates);

        MEMORY_MANAGER.with_borrow(|mm| {
            let registry = registry(mm);
            let table_reader = registry.read::<User, _>(mm);
            let mut overlay_reader = DatabaseOverlayReader::new(&table_overlay, table_reader);
            // collect all
            let mut all = vec![];
            while let Some(row) = overlay_reader.try_next().expect("failed to read row") {
                all.push(row);
            }
            assert_eq!(all.len(), USERS_FIXTURES.len());

            // check that updated user is present
            let names = all
                .iter()
                .map(|row| {
                    row.iter()
                        .find(|(col, _)| col.name == "name")
                        .expect("name column not found")
                        .1
                        .clone()
                })
                .map(|v| match v {
                    Value::Text(s) => s.to_string(),
                    _ => panic!("expected text value"),
                })
                .collect::<Vec<_>>();

            assert!(names.contains(&updated_name));
        });
    }

    fn registry(
        mm: &crate::memory::MemoryManager<impl crate::memory::MemoryProvider>,
    ) -> TableRegistry {
        let user_pages = SCHEMA_REGISTRY
            .with_borrow(|sr| sr.table_registry_page::<User>())
            .expect("failed to register `User` table");

        TableRegistry::load(user_pages, mm).expect("failed to load `User` table registry")
    }
}
