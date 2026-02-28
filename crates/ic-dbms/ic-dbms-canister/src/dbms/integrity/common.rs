//! Shared integrity-check functions used by both insert and update validators.

use ic_dbms_api::prelude::{
    ColumnDef, ForeignFetcher, ForeignKeyDef, IcDbmsError, IcDbmsResult, QueryError, TableSchema,
    Value,
};

use crate::dbms::IcDbmsDatabase;

/// Checks whether `value` passes the validator defined for `column`, if any.
pub fn check_column_validate<T: TableSchema>(
    column: &ColumnDef,
    value: &Value,
) -> IcDbmsResult<()> {
    let Some(validator) = T::validator(column.name) else {
        return Ok(());
    };

    validator.validate(value)
}

/// Checks whether all foreign keys in `record_values` reference existing records.
pub fn check_foreign_keys<T: TableSchema>(
    database: &IcDbmsDatabase,
    record_values: &[(ColumnDef, Value)],
) -> IcDbmsResult<()> {
    record_values
        .iter()
        .filter_map(|(col, value)| col.foreign_key.as_ref().map(|fk| (fk, value)))
        .try_for_each(|(fk, value)| check_foreign_key_existence::<T>(database, fk, value))
}

/// Checks whether a single foreign key references an existing record.
pub fn check_foreign_key_existence<T: TableSchema>(
    database: &IcDbmsDatabase,
    foreign_key: &ForeignKeyDef,
    value: &Value,
) -> IcDbmsResult<()> {
    let res = T::foreign_fetcher().fetch(
        database,
        foreign_key.foreign_table,
        foreign_key.local_column,
        value.clone(),
    )?;
    if res.is_empty() {
        Err(IcDbmsError::Query(
            QueryError::ForeignKeyConstraintViolation {
                field: foreign_key.local_column.to_string(),
                referencing_table: foreign_key.foreign_table.to_string(),
            },
        ))
    } else {
        Ok(())
    }
}

/// Checks whether all non-nullable columns are present in `record_values`.
pub fn check_non_nullable_fields<T: TableSchema>(
    record_values: &[(ColumnDef, Value)],
) -> IcDbmsResult<()> {
    for column in T::columns().iter().filter(|col| !col.nullable) {
        if !record_values
            .iter()
            .any(|(col_def, _)| col_def.name == column.name)
        {
            return Err(IcDbmsError::Query(QueryError::MissingNonNullableField(
                column.name.to_string(),
            )));
        }
    }

    Ok(())
}
