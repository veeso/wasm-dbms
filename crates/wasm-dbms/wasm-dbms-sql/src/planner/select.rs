//! Planning of `SELECT` on one table or on a join.

use wasm_dbms_api::prelude::{Filter, Join, JoinType, OrderDirection, Query, SqlError, Value};

use super::coerce::row_count;
use super::condition::{Clause, Target, filter};
use super::scope::{Resolved, Scope};
use super::{Catalog, OutputColumn, SelectPlan, TablePlan, aggregate};
use crate::ast::{
    Condition, JoinClause, JoinKind, Operand, OrderItem, Projection, Select, SelectItem,
};

/// Plans a `SELECT`.
///
/// # Errors
///
/// - [`SqlError::UnknownTable`], [`SqlError::UnknownColumn`], and
///   [`SqlError::AmbiguousColumn`] for names that do not resolve.
/// - [`SqlError::TypeMismatch`] and [`SqlError::InvalidLiteral`] for values
///   that do not fit the column they are compared with, or incompatible join
///   column types.
/// - [`SqlError::Unsupported`] for combinations the DBMS cannot run, such as
///   `DISTINCT` with `JOIN`.
pub(crate) fn select(
    select: &Select,
    catalog: &Catalog<'_>,
    params: &[Value],
) -> Result<SelectPlan, SqlError> {
    let mut scope = Scope::default();
    scope.push(catalog, &select.from)?;
    let joins = select
        .joins
        .iter()
        .map(|clause| join(&mut scope, catalog, clause))
        .collect::<Result<Vec<_>, _>>()?;

    if is_aggregate(select) {
        return aggregate::plan(select, &scope, params).map(SelectPlan::Aggregate);
    }

    let joined = !joins.is_empty();
    // the join engine wants `table.column`; a single table wants bare names
    let field = |resolved: &Resolved| {
        if joined {
            scope.qualified_name(resolved)
        } else {
            resolved.def.name.to_string()
        }
    };

    let items: &[SelectItem] = match &select.projection {
        Projection::All => &[],
        Projection::Items(items) => items,
    };
    let resolved_items = items
        .iter()
        .map(|item| match &item.operand {
            Operand::Column(column) => scope.resolve(column),
            Operand::Aggregate(_) => unreachable!("aggregate queries are planned separately"),
        })
        .collect::<Result<Vec<_>, _>>()?;

    let order_by = select
        .order_by
        .iter()
        .map(|item| {
            let resolved = order_column(&scope, items, &resolved_items, item)?;
            Ok((field(&resolved), direction(item)))
        })
        .collect::<Result<Vec<_>, SqlError>>()?;

    let distinct_by = if select.distinct {
        distinct_columns(
            &scope,
            &select.projection,
            &resolved_items,
            &order_by,
            joined,
        )?
    } else {
        Vec::new()
    };

    let mut query = Query::default();
    query.joins = joins;
    query.filter = where_filter(&scope, select.filter.as_ref(), params, &field)?;
    query.distinct_by = distinct_by;
    query.order_by = order_by;
    (query.limit, query.offset) = pagination(select, params)?;

    let projection = match &select.projection {
        Projection::All => None,
        Projection::Items(items) => Some(
            items
                .iter()
                .zip(&resolved_items)
                .map(|(item, resolved)| OutputColumn {
                    table: joined.then(|| scope.table_name(resolved.table).to_string()),
                    column: resolved.def.name.to_string(),
                    name: item
                        .alias
                        .clone()
                        .unwrap_or_else(|| resolved.def.name.to_string()),
                })
                .collect(),
        ),
    };

    let plan = TablePlan {
        table: scope.table_name(0).to_string(),
        query,
        projection,
    };
    Ok(if joined {
        SelectPlan::Join(plan)
    } else {
        SelectPlan::Table(plan)
    })
}

/// Returns whether the statement must run as an aggregate query.
fn is_aggregate(select: &Select) -> bool {
    let is_aggregate = |operand: &Operand| matches!(operand, Operand::Aggregate(_));
    let in_projection = match &select.projection {
        Projection::All => false,
        Projection::Items(items) => items.iter().any(|item| is_aggregate(&item.operand)),
    };
    in_projection
        || !select.group_by.is_empty()
        || select.having.is_some()
        || select
            .order_by
            .iter()
            .any(|item| is_aggregate(&item.operand))
}

/// Adds the joined table to the scope and converts its `ON` condition.
fn join(scope: &mut Scope, catalog: &Catalog<'_>, clause: &JoinClause) -> Result<Join, SqlError> {
    let joined = scope.push(catalog, &clause.table)?;
    let left = scope.resolve(&clause.left)?;
    let right = scope.resolve(&clause.right)?;
    // the condition may be written either way round
    let (outer, inner) = match (left.table == joined, right.table == joined) {
        (false, true) => (left, right),
        (true, false) => (right, left),
        _ => {
            return Err(SqlError::Unsupported(format!(
                "the JOIN condition of `{table}` must compare one of its columns with a column \
                 of a table that precedes it",
                table = clause.table.name
            )));
        }
    };
    if outer.def.data_type != inner.def.data_type {
        return Err(SqlError::TypeMismatch {
            column: scope.qualified_name(&inner),
            expected: outer.def.data_type.into(),
            got: format!("{data_type:?}", data_type = inner.def.data_type),
        });
    }
    Ok(Join {
        join_type: match clause.kind {
            JoinKind::Inner => JoinType::Inner,
            JoinKind::Left => JoinType::Left,
            JoinKind::Right => JoinType::Right,
            JoinKind::Full => JoinType::Full,
        },
        table: clause.table.name.clone(),
        left_column: scope.qualified_name(&outer),
        right_column: inner.def.name.to_string(),
    })
}

/// Converts an optional `WHERE` condition; `field` names a column for the DBMS.
pub(super) fn where_filter(
    scope: &Scope,
    condition: Option<&Condition>,
    params: &[Value],
    field: &dyn Fn(&Resolved) -> String,
) -> Result<Option<Filter>, SqlError> {
    condition
        .map(|condition| {
            filter(condition, params, Clause::Where, &mut |operand| {
                let Operand::Column(column) = operand else {
                    return Err(SqlError::Unsupported(
                        "aggregate functions are not allowed in WHERE".to_string(),
                    ));
                };
                let resolved = scope.resolve(column)?;
                Ok(Target {
                    field: field(&resolved),
                    label: resolved.def.name.to_string(),
                    kind: resolved.def.data_type,
                })
            })
        })
        .transpose()
}

/// Resolves the `LIMIT` and `OFFSET` of a statement.
pub(super) fn pagination(
    select: &Select,
    params: &[Value],
) -> Result<(Option<usize>, Option<usize>), SqlError> {
    let limit = select
        .limit
        .as_ref()
        .map(|count| row_count(count, params, "LIMIT"))
        .transpose()?;
    let offset = select
        .offset
        .as_ref()
        .map(|count| row_count(count, params, "OFFSET"))
        .transpose()?;
    Ok((limit, offset))
}

/// Returns the sort direction of an `ORDER BY` item.
pub(super) fn direction(item: &OrderItem) -> OrderDirection {
    if item.descending {
        OrderDirection::Descending
    } else {
        OrderDirection::Ascending
    }
}

/// Returns the index of the select item whose alias `item` refers to.
///
/// Only an unqualified name can be an alias. An alias takes precedence over a
/// column of the same name.
pub(super) fn aliased_item(items: &[SelectItem], item: &OrderItem) -> Option<usize> {
    let Operand::Column(column) = &item.operand else {
        return None;
    };
    if column.table.is_some() {
        return None;
    }
    items
        .iter()
        .position(|candidate| candidate.alias.as_deref() == Some(column.column.as_str()))
}

/// Resolves the column an `ORDER BY` item sorts by.
fn order_column(
    scope: &Scope,
    items: &[SelectItem],
    resolved_items: &[Resolved],
    item: &OrderItem,
) -> Result<Resolved, SqlError> {
    if let Some(index) = aliased_item(items, item) {
        return Ok(resolved_items[index]);
    }
    match &item.operand {
        Operand::Column(column) => scope.resolve(column),
        Operand::Aggregate(_) => unreachable!("aggregate queries are planned separately"),
    }
}

/// Returns the columns a `SELECT DISTINCT` deduplicates on.
fn distinct_columns(
    scope: &Scope,
    projection: &Projection,
    resolved_items: &[Resolved],
    order_by: &[(String, OrderDirection)],
    joined: bool,
) -> Result<Vec<String>, SqlError> {
    if joined {
        return Err(SqlError::Unsupported(
            "DISTINCT is not supported together with JOIN".to_string(),
        ));
    }
    let columns: Vec<String> = match projection {
        Projection::All => scope
            .columns(0)
            .iter()
            .map(|def| def.name.to_string())
            .collect(),
        Projection::Items(_) => resolved_items
            .iter()
            .map(|resolved| resolved.def.name.to_string())
            .collect(),
    };
    // rows are deduplicated before they are sorted, so a sort key outside the
    // select list would have an arbitrary value for each distinct row
    if let Some((column, _)) = order_by
        .iter()
        .find(|(column, _)| !columns.contains(column))
    {
        return Err(SqlError::Unsupported(format!(
            "ORDER BY column `{column}` must appear in the select list of a SELECT DISTINCT"
        )));
    }
    Ok(columns)
}
