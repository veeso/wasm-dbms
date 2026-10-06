//! Planning of `INSERT`, `UPDATE`, and `DELETE`.

use wasm_dbms_api::prelude::{ColumnDef, DeleteBehavior, Filter, SqlError, Value};

use super::coerce::coerce;
use super::condition::{Clause, Target, filter};
use super::scope::Scope;
use super::{Catalog, DeletePlan, InsertPlan, UpdatePlan};
use crate::ast::{Condition, Delete, Insert, Operand, Update, ValueExpr};

/// Plans an `INSERT`.
///
/// # Errors
///
/// Returns [`SqlError::UnknownTable`], [`SqlError::UnknownColumn`], or a value
/// conversion error.
pub(crate) fn insert(
    insert: &Insert,
    catalog: &Catalog<'_>,
    params: &[Value],
) -> Result<InsertPlan, SqlError> {
    let scope = Scope::single(catalog, &insert.table)?;
    let values = insert
        .columns
        .iter()
        .zip(&insert.values)
        .map(|(column, value)| column_value(&scope, &insert.table, column, value, params))
        .collect::<Result<_, _>>()?;
    Ok(InsertPlan {
        table: insert.table.clone(),
        values,
    })
}

/// Plans an `UPDATE`.
///
/// # Errors
///
/// Returns [`SqlError::UnknownTable`], [`SqlError::UnknownColumn`], or a value
/// conversion error.
pub(crate) fn update(
    update: &Update,
    catalog: &Catalog<'_>,
    params: &[Value],
) -> Result<UpdatePlan, SqlError> {
    let scope = Scope::single(catalog, &update.table)?;
    let values = update
        .assignments
        .iter()
        .map(|assignment| {
            column_value(
                &scope,
                &update.table,
                &assignment.column,
                &assignment.value,
                params,
            )
        })
        .collect::<Result<_, _>>()?;
    Ok(UpdatePlan {
        table: update.table.clone(),
        values,
        filter: table_filter(&scope, &update.filter, params)?,
    })
}

/// Plans a `DELETE`.
///
/// # Errors
///
/// Returns [`SqlError::UnknownTable`], [`SqlError::UnknownColumn`], or a value
/// conversion error.
pub(crate) fn delete(
    delete: &Delete,
    catalog: &Catalog<'_>,
    params: &[Value],
) -> Result<DeletePlan, SqlError> {
    let scope = Scope::single(catalog, &delete.table)?;
    Ok(DeletePlan {
        table: delete.table.clone(),
        filter: table_filter(&scope, &delete.filter, params)?,
        behavior: if delete.cascade {
            DeleteBehavior::Cascade
        } else {
            DeleteBehavior::Restrict
        },
    })
}

/// Pairs the definition of `column` with `value` converted to its type.
fn column_value(
    scope: &Scope,
    table: &str,
    column: &str,
    value: &ValueExpr,
    params: &[Value],
) -> Result<(ColumnDef, Value), SqlError> {
    let def = scope
        .columns(0)
        .iter()
        .find(|def| def.name == column)
        .copied()
        .ok_or_else(|| SqlError::UnknownColumn {
            table: table.to_string(),
            column: column.to_string(),
        })?;
    let value = coerce(value, params, column, def.data_type)?;
    Ok((def, value))
}

/// Converts the `WHERE` condition of a statement that works on one table.
pub(super) fn table_filter(
    scope: &Scope,
    condition: &Condition,
    params: &[Value],
) -> Result<Filter, SqlError> {
    filter(condition, params, Clause::Where, &mut |operand| {
        let Operand::Column(column) = operand else {
            return Err(SqlError::Unsupported(
                "aggregate functions are not allowed in WHERE".to_string(),
            ));
        };
        let def = scope.resolve(column)?.def;
        Ok(Target {
            field: def.name.to_string(),
            label: def.name.to_string(),
            kind: def.data_type,
        })
    })
}
