//! Planning of aggregate queries: aggregate functions, `GROUP BY`, `HAVING`.

use wasm_dbms_api::prelude::{
    AggregateFunction, CandidDataTypeKind, ColumnDef, DataTypeKind, JoinColumnDef, Query, SqlError,
    Value,
};

use super::condition::{Clause, Target, filter};
use super::scope::{Resolved, Scope};
use super::select::{aliased_item, direction, pagination, where_filter};
use super::{AggregateOutput, AggregatePlan, AggregateSource};
use crate::ast::{Aggregate, AggregateKind, ColumnRef, Operand, Projection, Select};

/// The aggregates a query computes, without duplicates.
#[derive(Default)]
struct Aggregates {
    functions: Vec<AggregateFunction>,
    /// For each function, its name in the result and the type of its value.
    signatures: Vec<(String, DataTypeKind, bool)>,
}

impl Aggregates {
    /// Returns the index of `aggregate`, registering it on first use.
    fn index(&mut self, scope: &Scope, aggregate: &Aggregate) -> Result<usize, SqlError> {
        let column = aggregate
            .column
            .as_ref()
            .map(|column| scope.resolve(column).map(|resolved| resolved.def))
            .transpose()?;
        let name = column.map(|def| def.name.to_string());
        // COUNT yields a non-null Uint64, SUM and AVG a Decimal, MIN and MAX
        // the type of their column
        let (function, label, kind, nullable) = match (aggregate.function, name, column) {
            (AggregateKind::Count, name, _) => (
                AggregateFunction::Count(name),
                "COUNT",
                DataTypeKind::Uint64,
                false,
            ),
            (AggregateKind::Sum, Some(name), _) => (
                AggregateFunction::Sum(name),
                "SUM",
                DataTypeKind::Decimal,
                true,
            ),
            (AggregateKind::Avg, Some(name), _) => (
                AggregateFunction::Avg(name),
                "AVG",
                DataTypeKind::Decimal,
                true,
            ),
            (AggregateKind::Min, Some(name), Some(def)) => {
                (AggregateFunction::Min(name), "MIN", def.data_type, true)
            }
            (AggregateKind::Max, Some(name), Some(def)) => {
                (AggregateFunction::Max(name), "MAX", def.data_type, true)
            }
            _ => unreachable!("the parser requires a column for every aggregate but COUNT"),
        };
        if let Some(index) = self.functions.iter().position(|known| *known == function) {
            return Ok(index);
        }
        let argument = column.map_or("*", |def| def.name);
        self.functions.push(function);
        self.signatures
            .push((format!("{label}({argument})"), kind, nullable));
        Ok(self.functions.len() - 1)
    }

    /// Returns the filter target of the aggregate at `index`.
    fn target(&self, index: usize) -> Target {
        let (label, kind, _) = &self.signatures[index];
        Target {
            field: format!("agg{index}"),
            label: label.clone(),
            kind: *kind,
        }
    }
}

/// Plans a `SELECT` that uses aggregate functions, `GROUP BY`, or `HAVING`.
///
/// # Errors
///
/// - [`SqlError::Unsupported`] when the query has a `JOIN`, `DISTINCT`, or
///   `SELECT *`, uses `LIKE` in `HAVING`, or references a column that is
///   neither grouped nor aggregated.
/// - Name resolution and value conversion errors.
pub(super) fn plan(
    select: &Select,
    scope: &Scope,
    params: &[Value],
) -> Result<AggregatePlan, SqlError> {
    if !select.joins.is_empty() {
        return Err(SqlError::Unsupported(
            "aggregate functions, GROUP BY and HAVING are not supported together with JOIN"
                .to_string(),
        ));
    }
    if select.distinct {
        return Err(SqlError::Unsupported(
            "DISTINCT is not supported in an aggregate query".to_string(),
        ));
    }
    let Projection::Items(items) = &select.projection else {
        return Err(SqlError::Unsupported(
            "`SELECT *` is not allowed in an aggregate query".to_string(),
        ));
    };

    let group_by = select
        .group_by
        .iter()
        .map(|column| scope.resolve(column).map(|resolved| resolved.def))
        .collect::<Result<Vec<_>, _>>()?;
    let group_key = |column: &ColumnRef| -> Result<(usize, ColumnDef), SqlError> {
        let def = scope.resolve(column)?.def;
        group_by
            .iter()
            .position(|key| key.name == def.name)
            .map(|index| (index, def))
            .ok_or_else(|| {
                SqlError::Unsupported(format!(
                    "column `{column}` must appear in GROUP BY or be used in an aggregate \
                     function",
                    column = def.name
                ))
            })
    };

    let mut aggregates = Aggregates::default();
    let mut outputs = Vec::with_capacity(items.len());
    for item in items {
        let (source, mut def) = match &item.operand {
            Operand::Column(column) => {
                let (index, def) = group_key(column)?;
                (AggregateSource::GroupKey(index), JoinColumnDef::from(def))
            }
            Operand::Aggregate(aggregate) => {
                let index = aggregates.index(scope, aggregate)?;
                let (name, kind, nullable) = aggregates.signatures[index].clone();
                let def = JoinColumnDef {
                    table: None,
                    name,
                    data_type: CandidDataTypeKind::from(kind),
                    nullable,
                    primary_key: false,
                    foreign_key: None,
                };
                (AggregateSource::Aggregate(index), def)
            }
        };
        if let Some(alias) = &item.alias {
            def.name = alias.clone();
        }
        outputs.push(AggregateOutput { source, def });
    }

    let having = select
        .having
        .as_ref()
        .map(|condition| {
            filter(
                condition,
                params,
                Clause::Having,
                &mut |operand| match operand {
                    Operand::Column(column) => {
                        let (_, def) = group_key(column)?;
                        Ok(Target {
                            field: def.name.to_string(),
                            label: def.name.to_string(),
                            kind: def.data_type,
                        })
                    }
                    Operand::Aggregate(aggregate) => {
                        let index = aggregates.index(scope, aggregate)?;
                        Ok(aggregates.target(index))
                    }
                },
            )
        })
        .transpose()?;

    let mut order_by = Vec::with_capacity(select.order_by.len());
    for item in &select.order_by {
        let source = match (aliased_item(items, item), &item.operand) {
            (Some(index), _) => outputs[index].source,
            (None, Operand::Column(column)) => AggregateSource::GroupKey(group_key(column)?.0),
            (None, Operand::Aggregate(aggregate)) => {
                AggregateSource::Aggregate(aggregates.index(scope, aggregate)?)
            }
        };
        let field = match source {
            AggregateSource::GroupKey(index) => group_by[index].name.to_string(),
            AggregateSource::Aggregate(index) => format!("agg{index}"),
        };
        order_by.push((field, direction(item)));
    }

    let mut query = Query::default();
    query.filter = where_filter(
        scope,
        select.filter.as_ref(),
        params,
        &|resolved: &Resolved| resolved.def.name.to_string(),
    )?;
    query.group_by = group_by.iter().map(|def| def.name.to_string()).collect();
    query.having = having;
    query.order_by = order_by;
    (query.limit, query.offset) = pagination(select, params)?;

    Ok(AggregatePlan {
        table: scope.table_name(0).to_string(),
        query,
        aggregates: aggregates.functions,
        outputs,
    })
}
