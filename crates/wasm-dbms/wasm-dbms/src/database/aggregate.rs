// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Aggregate query execution: GROUP BY, aggregate functions, HAVING.

use std::collections::HashMap;

use rust_decimal::Decimal as RustDecimal;
use wasm_dbms_api::prelude::{
    AggregateFunction, AggregatedRow, AggregatedValue, ColumnDef, DataTypeKind, DbmsError,
    DbmsResult, Decimal, Filter, OrderDirection, Query, QueryError, TableSchema, Uint64, Value,
    ValuesSource,
};
use wasm_dbms_memory::prelude::{AccessControl, MemoryProvider};

use crate::database::{TableColumns, WasmDbmsDatabase, sort_values_with_direction};

/// Executes an aggregate query for table `T`.
///
/// Pipeline: `WHERE` -> `DISTINCT` -> group rows by `GROUP BY` keys -> compute
/// each [`AggregateFunction`] per group -> apply `HAVING` -> apply `ORDER BY`
/// -> apply `OFFSET`/`LIMIT`.
pub(super) fn run_aggregate<T, M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    query: Query,
    aggregates: &[AggregateFunction],
) -> DbmsResult<Vec<AggregatedRow>>
where
    T: TableSchema,
    M: MemoryProvider,
    A: AccessControl,
{
    if !query.joins.is_empty() {
        return Err(DbmsError::Query(QueryError::InvalidQuery(
            "joins are not supported in aggregate queries".to_string(),
        )));
    }
    if !query.eager_relations.is_empty() {
        return Err(DbmsError::Query(QueryError::InvalidQuery(
            "eager relations are not supported in aggregate queries".to_string(),
        )));
    }

    validate_group_by::<T>(&query.group_by)?;
    validate_aggregates::<T>(aggregates)?;
    if let Some(having) = &query.having {
        validate_having_filter(having, &query.group_by, aggregates)?;
    }
    validate_order_by(&query.order_by, &query.group_by, aggregates)?;

    // Strip post-aggregation clauses for the underlying scan; preserve WHERE
    // and DISTINCT.
    let base_query = Query::builder()
        .all()
        .filter(query.filter.clone())
        .distinct(&query.distinct_by)
        .build();

    let rows = db.select_columns::<T>(base_query)?;

    let groups = group_rows(&rows, &query.group_by);

    let mut output = Vec::with_capacity(groups.len());
    for (group_keys, group_rows) in groups {
        let values = compute_aggregates(&group_rows, aggregates)?;
        output.push(AggregatedRow { group_keys, values });
    }

    if let Some(having) = &query.having {
        output = filter_having(output, having, &query.group_by, aggregates)?;
    }

    apply_order_by(&mut output, &query.order_by, &query.group_by);

    let offset = query.offset.unwrap_or(0);
    if offset >= output.len() {
        output.clear();
    } else if offset > 0 {
        output.drain(..offset);
    }
    if let Some(limit) = query.limit {
        output.truncate(limit);
    }

    Ok(output)
}

/// Groups WHERE-filtered rows by the values of the `group_by` columns.
///
/// When `group_by` is empty, all rows form a single group with an empty key.
/// Group order follows first-seen order, which keeps results deterministic for
/// callers that do not specify an `ORDER BY`.
fn group_rows(rows: &[TableColumns], group_by: &[String]) -> Vec<(Vec<Value>, Vec<TableColumns>)> {
    if group_by.is_empty() {
        return vec![(Vec::new(), rows.to_vec())];
    }
    let mut order: Vec<Vec<Value>> = Vec::new();
    let mut buckets: HashMap<Vec<Value>, Vec<TableColumns>> = HashMap::new();
    for row in rows {
        let key = group_by
            .iter()
            .map(|col| this_value(row, col).cloned().unwrap_or(Value::Null))
            .collect::<Vec<_>>();
        if !buckets.contains_key(&key) {
            order.push(key.clone());
        }
        buckets.entry(key).or_default().push(row.clone());
    }
    order
        .into_iter()
        .map(|k| {
            let v = buckets.remove(&k).unwrap_or_default();
            (k, v)
        })
        .collect()
}

/// Computes each aggregate function over the rows in a single group.
fn compute_aggregates(
    rows: &[TableColumns],
    aggregates: &[AggregateFunction],
) -> DbmsResult<Vec<AggregatedValue>> {
    let mut out = Vec::with_capacity(aggregates.len());
    for agg in aggregates {
        let value = match agg {
            AggregateFunction::Count(None) => AggregatedValue::Count(rows.len() as u64),
            AggregateFunction::Count(Some(col)) => {
                let mut n = 0u64;
                for row in rows {
                    if let Some(v) = this_value(row, col)
                        && !v.is_null()
                    {
                        n += 1;
                    }
                }
                AggregatedValue::Count(n)
            }
            AggregateFunction::Sum(col) => AggregatedValue::Sum(sum_column(rows, col)?),
            AggregateFunction::Avg(col) => AggregatedValue::Avg(avg_column(rows, col)?),
            AggregateFunction::Min(col) => AggregatedValue::Min(min_max_column(rows, col, true)),
            AggregateFunction::Max(col) => AggregatedValue::Max(min_max_column(rows, col, false)),
        };
        out.push(value);
    }
    Ok(out)
}

/// Sums non-null numeric values of `col` across `rows`, returning a [`Decimal`]
/// regardless of input integer width.
///
/// An all-null group returns `Value::Null` (matching SQL `SUM` semantics).
fn sum_column(rows: &[TableColumns], col: &str) -> DbmsResult<Value> {
    let mut acc = RustDecimal::ZERO;
    let mut any = false;
    for row in rows {
        if let Some(v) = this_value(row, col)
            && !v.is_null()
        {
            acc += value_to_decimal(v).map_err(DbmsError::Query)?;
            any = true;
        }
    }
    if any {
        Ok(Value::Decimal(Decimal(acc)))
    } else {
        Ok(Value::Null)
    }
}

/// Computes the mean of `col` across non-null values of `rows`.
///
/// Returns [`Decimal`] regardless of the input's integer type. An all-null
/// group returns `Value::Null` (matching SQL `AVG` semantics).
fn avg_column(rows: &[TableColumns], col: &str) -> DbmsResult<Value> {
    let mut acc = RustDecimal::ZERO;
    let mut count: u64 = 0;
    for row in rows {
        if let Some(v) = this_value(row, col)
            && !v.is_null()
        {
            acc += value_to_decimal(v).map_err(DbmsError::Query)?;
            count += 1;
        }
    }
    if count == 0 {
        return Ok(Value::Null);
    }
    let mean = acc / RustDecimal::from(count);
    Ok(Value::Decimal(Decimal(mean)))
}

/// Returns the minimum (when `is_min`) or maximum value of `col` across `rows`,
/// using the [`Value`] `Ord` implementation. Nulls are skipped; if every value
/// is null the result is [`Value::Null`].
fn min_max_column(rows: &[TableColumns], col: &str, is_min: bool) -> Value {
    let mut best: Option<Value> = None;
    for row in rows {
        if let Some(v) = this_value(row, col)
            && !v.is_null()
        {
            best = Some(match best {
                None => v.clone(),
                Some(cur) => {
                    let take_new = if is_min { v < &cur } else { v > &cur };
                    if take_new { v.clone() } else { cur }
                }
            });
        }
    }
    best.unwrap_or(Value::Null)
}

/// Coerces a numeric [`Value`] into a `RustDecimal` for sum/avg accumulation.
///
/// Errors with [`QueryError::InvalidQuery`] when called on a non-numeric value;
/// the planner's [`validate_aggregates`] should make this unreachable in
/// practice.
fn value_to_decimal(value: &Value) -> Result<RustDecimal, QueryError> {
    Ok(match value {
        Value::Int8(v) => RustDecimal::from(v.0),
        Value::Int16(v) => RustDecimal::from(v.0),
        Value::Int32(v) => RustDecimal::from(v.0),
        Value::Int64(v) => RustDecimal::from(v.0),
        Value::Uint8(v) => RustDecimal::from(v.0),
        Value::Uint16(v) => RustDecimal::from(v.0),
        Value::Uint32(v) => RustDecimal::from(v.0),
        Value::Uint64(v) => RustDecimal::from(v.0),
        Value::Decimal(v) => v.0,
        other => {
            return Err(QueryError::InvalidQuery(format!(
                "cannot aggregate non-numeric value: {other:?}"
            )));
        }
    })
}

/// Returns true when the column kind is numeric and therefore valid for
/// `SUM` and `AVG`.
fn is_numeric_kind(kind: DataTypeKind) -> bool {
    matches!(
        kind,
        DataTypeKind::Int8
            | DataTypeKind::Int16
            | DataTypeKind::Int32
            | DataTypeKind::Int64
            | DataTypeKind::Uint8
            | DataTypeKind::Uint16
            | DataTypeKind::Uint32
            | DataTypeKind::Uint64
            | DataTypeKind::Decimal
    )
}

/// Looks up a column on the `ValuesSource::This` slice of a row.
fn this_value<'a>(row: &'a TableColumns, column: &str) -> Option<&'a Value> {
    row.iter()
        .find(|(src, _)| *src == ValuesSource::This)
        .and_then(|(_, cols)| {
            cols.iter()
                .find(|(cd, _)| cd.name == column)
                .map(|(_, v)| v)
        })
}

/// Validates that every column listed in `group_by` exists on table `T`.
fn validate_group_by<T>(group_by: &[String]) -> DbmsResult<()>
where
    T: TableSchema,
{
    for col in group_by {
        if !T::columns().iter().any(|c| c.name == col.as_str()) {
            return Err(DbmsError::Query(QueryError::UnknownColumn(col.clone())));
        }
    }
    Ok(())
}

/// Validates aggregate planning rules: referenced columns exist, and `SUM` /
/// `AVG` operate on numeric columns.
fn validate_aggregates<T>(aggregates: &[AggregateFunction]) -> DbmsResult<()>
where
    T: TableSchema,
{
    for agg in aggregates {
        match agg {
            AggregateFunction::Count(None) => {}
            AggregateFunction::Count(Some(col))
            | AggregateFunction::Min(col)
            | AggregateFunction::Max(col) => {
                lookup_column::<T>(col)?;
            }
            AggregateFunction::Sum(col) | AggregateFunction::Avg(col) => {
                let cd = lookup_column::<T>(col)?;
                if !is_numeric_kind(cd.data_type) {
                    return Err(DbmsError::Query(QueryError::InvalidQuery(format!(
                        "aggregate requires numeric column: '{col}'"
                    ))));
                }
            }
        }
    }
    Ok(())
}

/// Returns the column definition for `name` on table `T`, or
/// [`QueryError::UnknownColumn`] if absent.
fn lookup_column<T>(name: &str) -> DbmsResult<ColumnDef>
where
    T: TableSchema,
{
    T::columns()
        .iter()
        .find(|c| c.name == name)
        .copied()
        .ok_or_else(|| DbmsError::Query(QueryError::UnknownColumn(name.to_string())))
}

/// Validates that every column reference inside the `HAVING` filter resolves
/// to either a `GROUP BY` column or a synthetic aggregate output name
/// (`agg{N}`). Also rejects `LIKE` and `JSON` filter kinds since they are not
/// supported over aggregated rows.
fn validate_having_filter(
    filter: &Filter,
    group_by: &[String],
    aggregates: &[AggregateFunction],
) -> DbmsResult<()> {
    walk_filter(filter, &mut |f| match f {
        Filter::Like(_, _) => Err(DbmsError::Query(QueryError::InvalidQuery(
            "LIKE is not supported in HAVING".to_string(),
        ))),
        Filter::Json(_, _) => Err(DbmsError::Query(QueryError::InvalidQuery(
            "JSON filters are not supported in HAVING".to_string(),
        ))),
        _ => {
            if let Some(col) = filter_column(f)
                && !is_known_having_column(col, group_by, aggregates)
            {
                return Err(DbmsError::Query(QueryError::InvalidQuery(format!(
                    "HAVING references unknown column or aggregate: '{col}'"
                ))));
            }
            Ok(())
        }
    })
}

/// Validates that each `ORDER BY` key is either a `GROUP BY` column or an
/// `agg{N}` reference within range.
fn validate_order_by(
    order_by: &[(String, OrderDirection)],
    group_by: &[String],
    aggregates: &[AggregateFunction],
) -> DbmsResult<()> {
    for (col, _) in order_by {
        if !is_known_having_column(col, group_by, aggregates) {
            return Err(DbmsError::Query(QueryError::InvalidQuery(format!(
                "ORDER BY references unknown aggregate output: '{col}'"
            ))));
        }
    }
    Ok(())
}

/// Returns true when `col` is either a `GROUP BY` column name or an `agg{N}`
/// reference whose index is within the supplied aggregates.
fn is_known_having_column(
    col: &str,
    group_by: &[String],
    aggregates: &[AggregateFunction],
) -> bool {
    if group_by.iter().any(|g| g == col) {
        return true;
    }
    matches!(parse_agg_index(col), Some(idx) if idx < aggregates.len())
}

/// Parses `agg{N}` returning the numeric index, or `None` if the input does
/// not match.
fn parse_agg_index(name: &str) -> Option<usize> {
    name.strip_prefix("agg")
        .and_then(|s| s.parse::<usize>().ok())
}

/// Returns the column name a leaf [`Filter`] references, or `None` for
/// composite (`And`/`Or`/`Not`) variants.
fn filter_column(filter: &Filter) -> Option<&str> {
    match filter {
        Filter::Eq(c, _)
        | Filter::Ne(c, _)
        | Filter::Gt(c, _)
        | Filter::Lt(c, _)
        | Filter::Ge(c, _)
        | Filter::Le(c, _)
        | Filter::In(c, _)
        | Filter::Json(c, _)
        | Filter::Like(c, _)
        | Filter::NotNull(c)
        | Filter::IsNull(c) => Some(c),
        Filter::And(_, _) | Filter::Or(_, _) | Filter::Not(_) => None,
    }
}

/// Walks every node in a [`Filter`] tree, invoking `visit` for each.
/// Short-circuits on the first error.
fn walk_filter(
    filter: &Filter,
    visit: &mut dyn FnMut(&Filter) -> DbmsResult<()>,
) -> DbmsResult<()> {
    visit(filter)?;
    match filter {
        Filter::And(a, b) | Filter::Or(a, b) => {
            walk_filter(a, visit)?;
            walk_filter(b, visit)
        }
        Filter::Not(inner) => walk_filter(inner, visit),
        _ => Ok(()),
    }
}

/// Filters aggregated rows by `HAVING`, evaluating the filter against a row's
/// `GROUP BY` keys plus its `agg{N}` outputs.
fn filter_having(
    rows: Vec<AggregatedRow>,
    filter: &Filter,
    group_by: &[String],
    aggregates: &[AggregateFunction],
) -> DbmsResult<Vec<AggregatedRow>> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let lookup = build_lookup(&row, group_by, aggregates);
        if eval_filter(filter, &lookup)? {
            out.push(row);
        }
    }
    Ok(out)
}

/// Builds the `name -> Value` map used by [`HAVING`](filter_having) and
/// [`ORDER BY`](apply_order_by) to resolve column references against an
/// aggregated row.
fn build_lookup(
    row: &AggregatedRow,
    group_by: &[String],
    aggregates: &[AggregateFunction],
) -> HashMap<String, Value> {
    let mut map = HashMap::with_capacity(group_by.len() + aggregates.len());
    for (i, col) in group_by.iter().enumerate() {
        let value = row.group_keys.get(i).cloned().unwrap_or(Value::Null);
        map.insert(col.clone(), value);
    }
    for (i, value) in row.values.iter().enumerate() {
        map.insert(format!("agg{i}"), aggregated_value_to_value(value));
    }
    map
}

/// Lifts an [`AggregatedValue`] into a comparable [`Value`].
///
/// `Count` is exposed as [`Value::Uint64`] so HAVING / ORDER BY filters can
/// compare against integer literals; the other variants pass through the
/// inner [`Value`].
fn aggregated_value_to_value(v: &AggregatedValue) -> Value {
    match v {
        AggregatedValue::Count(n) => Value::Uint64(Uint64(*n)),
        AggregatedValue::Sum(v)
        | AggregatedValue::Avg(v)
        | AggregatedValue::Min(v)
        | AggregatedValue::Max(v) => v.clone(),
    }
}

/// Recursive [`Filter`] evaluator over a `name -> Value` map. `LIKE` and
/// `JSON` are rejected at validation time, so this evaluator only covers the
/// comparison, set, null, and boolean variants.
fn eval_filter(filter: &Filter, lookup: &HashMap<String, Value>) -> DbmsResult<bool> {
    let res = match filter {
        Filter::Eq(c, v) => lookup.get(c).is_some_and(|x| x == v),
        Filter::Ne(c, v) => lookup.get(c).is_some_and(|x| x != v),
        Filter::Gt(c, v) => lookup.get(c).is_some_and(|x| x > v),
        Filter::Lt(c, v) => lookup.get(c).is_some_and(|x| x < v),
        Filter::Ge(c, v) => lookup.get(c).is_some_and(|x| x >= v),
        Filter::Le(c, v) => lookup.get(c).is_some_and(|x| x <= v),
        Filter::In(c, list) => lookup.get(c).is_some_and(|x| list.iter().any(|v| v == x)),
        Filter::IsNull(c) => lookup.get(c).is_some_and(Value::is_null),
        Filter::NotNull(c) => lookup.get(c).is_some_and(|x| !x.is_null()),
        Filter::And(a, b) => eval_filter(a, lookup)? && eval_filter(b, lookup)?,
        Filter::Or(a, b) => eval_filter(a, lookup)? || eval_filter(b, lookup)?,
        Filter::Not(inner) => !eval_filter(inner, lookup)?,
        Filter::Like(_, _) | Filter::Json(_, _) => {
            return Err(DbmsError::Query(QueryError::InvalidQuery(
                "LIKE/JSON not supported in HAVING".to_string(),
            )));
        }
    };
    Ok(res)
}

/// Applies multi-key ORDER BY to aggregated rows. Keys are processed last to
/// first so that earlier keys dominate later ones in the final order.
fn apply_order_by(
    rows: &mut [AggregatedRow],
    order_by: &[(String, OrderDirection)],
    group_by: &[String],
) {
    if order_by.is_empty() {
        return;
    }
    let agg_count = rows.first().map(|r| r.values.len()).unwrap_or(0);
    let aggregates: Vec<AggregateFunction> = (0..agg_count)
        .map(|_| AggregateFunction::Count(None))
        .collect();

    for (col, direction) in order_by.iter().rev() {
        rows.sort_by(|a, b| {
            let la = build_lookup(a, group_by, &aggregates);
            let lb = build_lookup(b, group_by, &aggregates);
            sort_values_with_direction(la.get(col), lb.get(col), *direction)
        });
    }
}
