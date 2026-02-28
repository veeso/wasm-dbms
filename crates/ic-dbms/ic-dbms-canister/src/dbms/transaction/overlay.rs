mod reader;
mod table;

use std::collections::HashMap;

use ic_dbms_api::prelude::{ColumnDef, IcDbmsError, IcDbmsResult, QueryError, TableSchema, Value};

pub use self::reader::DatabaseOverlayReader;
use self::table::TableOverlay;
use crate::memory::{MemoryProvider, TableReader};

/// The database overlay is used to manage uncommitted changes during a transaction.
///
/// Basically it provides an overlay over the existing database state to track uncommitted changes.
#[derive(Debug, Default, Clone)]
pub struct DatabaseOverlay {
    tables: HashMap<String, TableOverlay>,
}

impl DatabaseOverlay {
    /// Get a [`DatabaseOverlayReader`] for the specified table.
    pub fn reader<'a, T, P>(
        &'a mut self,
        table_reader: TableReader<'a, T, P>,
    ) -> DatabaseOverlayReader<'a, T, P>
    where
        T: TableSchema,
        P: MemoryProvider,
    {
        let table_name = T::table_name();
        let table_overlay = self.tables.entry(table_name.to_string()).or_default();
        DatabaseOverlayReader::new(table_overlay, table_reader)
    }

    /// Insert a record into the overlay for the specified table.
    pub fn insert<T>(&mut self, values: Vec<(ColumnDef, Value)>) -> IcDbmsResult<()>
    where
        T: TableSchema,
    {
        let table_name = T::table_name();
        let pk = T::primary_key();
        let pk = Self::primary_key(pk, &values)?;
        let overlay = self.tables.entry(table_name.to_string()).or_default();
        overlay.insert(pk, values);

        Ok(())
    }

    /// Update a record in the overlay for the specified table.
    pub fn update<T>(&mut self, pk: Value, updates: Vec<(&'static str, Value)>)
    where
        T: TableSchema,
    {
        let table_name = T::table_name();
        let overlay = self.tables.entry(table_name.to_string()).or_default();
        overlay.update(pk, updates);
    }

    /// Delete a record in the overlay for the specified table.
    pub fn delete<T>(&mut self, pk: Value)
    where
        T: TableSchema,
    {
        let table_name = T::table_name();
        let overlay = self.tables.entry(table_name.to_string()).or_default();
        overlay.delete(pk);
    }

    fn primary_key(pk: &'static str, values: &[(ColumnDef, Value)]) -> IcDbmsResult<Value> {
        for (col_def, value) in values {
            if col_def.name == pk {
                return Ok(value.clone());
            }
        }
        Err(IcDbmsError::Query(QueryError::MissingNonNullableField(
            pk.to_string(),
        )))
    }
}

#[cfg(test)]
mod tests {

    use ic_dbms_api::prelude::DataTypeKind;

    use super::*;
    use crate::tests::User;

    #[test]
    fn test_should_insert() {
        let mut overlay = DatabaseOverlay::default();
        let pk = Value::Uint32(1.into());
        let values = vec![
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
        ];
        overlay.insert::<User>(values).expect("failed to insert");

        // get entries for user table
        let table_overlay = overlay
            .tables
            .get(User::table_name())
            .expect("table not found");
        let record = table_overlay
            .operations
            .first()
            .expect("no operations found");
        assert!(matches!(
            record,
            table::Operation::Insert(pk_value, _values) if pk_value == &pk
        ));
    }

    #[test]
    fn test_should_fail_insert_missing_pk() {
        let mut overlay = DatabaseOverlay::default();
        let values = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Text("Alice".to_string().into()),
        )];
        let result = overlay.insert::<User>(values);
        assert!(result.is_err());
    }

    #[test]
    fn test_should_update() {
        let mut overlay = DatabaseOverlay::default();
        let pk = Value::Uint32(1.into());
        let updates = vec![("name", Value::Text("Bob".to_string().into()))];
        overlay.update::<User>(pk.clone(), updates.clone());

        let table_overlay = overlay
            .tables
            .get(User::table_name())
            .expect("table not found");
        let record = table_overlay
            .operations
            .first()
            .expect("no operations found");
        assert!(matches!(
            record,
            table::Operation::Update(pk_value, update_values)
                if pk_value == &pk && update_values == &updates
        ));
    }

    #[test]
    fn test_should_delete() {
        let mut overlay = DatabaseOverlay::default();
        let pk = Value::Uint32(1.into());
        overlay.delete::<User>(pk.clone());

        let table_overlay = overlay
            .tables
            .get(User::table_name())
            .expect("table not found");

        let record = table_overlay
            .operations
            .first()
            .expect("no operations found");
        assert!(matches!(
            record,
            table::Operation::Delete(pk_value) if pk_value == &pk
        ));
    }
}
