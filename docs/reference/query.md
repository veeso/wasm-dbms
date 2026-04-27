# Query API Reference

- [Query API Reference](#query-api-reference)
  - [Overview](#overview)
  - [Query Struct](#query-struct)
  - [QueryBuilder](#querybuilder)
    - [Field Selection](#field-selection)
    - [Filters](#filters)
    - [Joins](#joins)
    - [Eager Loading](#eager-loading)
    - [Distinct](#distinct)
    - [Aggregations](#aggregations)
    - [Ordering](#ordering)
    - [Pagination](#pagination)
  - [Aggregate Types](#aggregate-types)
    - [`AggregateFunction`](#aggregatefunction)
    - [`AggregatedRow`](#aggregatedrow)
    - [`AggregatedValue`](#aggregatedvalue)
  - [Execution Order](#execution-order)
  - [Errors](#errors)
  - [Related Types](#related-types)

---

## Overview

A `Query` describes what to retrieve from the database: which rows match,
which columns to return, how to order and paginate them, and how to combine
data across tables. Queries are constructed with `QueryBuilder` and consumed
by `Database::select`, `Database::select_raw`, and `Database::select_join`.

For an introductory walkthrough, see the [Querying Guide](../guides/querying.md).

---

## Query Struct

```rust
pub struct Query {
    columns: Select,
    pub distinct_by: Vec<String>,
    pub eager_relations: Vec<String>,
    pub filter: Option<Filter>,
    pub group_by: Vec<String>,
    pub having: Option<Filter>,
    pub joins: Vec<Join>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub order_by: Vec<(String, OrderDirection)>,
}
```

| Field             | Type                            | Description                                     |
| ----------------- | ------------------------------- | ----------------------------------------------- |
| `columns`         | `Select`                        | `Select::All` or `Select::Columns(Vec<String>)` |
| `distinct_by`     | `Vec<String>`                   | Columns used to deduplicate results             |
| `eager_relations` | `Vec<String>`                   | Foreign-key relations to load eagerly           |
| `filter`          | `Option<Filter>`                | WHERE-clause expression                         |
| `group_by`        | `Vec<String>`                   | GROUP BY columns for aggregate queries          |
| `having`          | `Option<Filter>`                | HAVING filter applied to aggregated groups      |
| `joins`           | `Vec<Join>`                     | Join clauses (only valid via `select_join`)     |
| `limit`           | `Option<usize>`                 | Maximum number of records to return             |
| `offset`          | `Option<usize>`                 | Number of records to skip                       |
| `order_by`        | `Vec<(String, OrderDirection)>` | Multi-column ordering                           |

Use `Query::builder()` to obtain a `QueryBuilder`.

---

## QueryBuilder

### Field Selection

| Method          | Effect                                |
| --------------- | ------------------------------------- |
| `.all()`        | Selects all columns (`Select::All`)   |
| `.field(name)`  | Adds a single column to the selection |
| `.fields(iter)` | Adds multiple columns                 |

The primary key is always included by `Database::select::<T>` even when not explicitly listed.

### Filters

| Method                    | Effect                                    |
| ------------------------- | ----------------------------------------- |
| `.filter(Option<Filter>)` | Replaces the current filter               |
| `.and_where(Filter)`      | Combines with existing filter using `AND` |
| `.or_where(Filter)`       | Combines with existing filter using `OR`  |

See [Filters in the Querying Guide](../guides/querying.md#filters) and
[JSON Filters](./json.md) for the full filter API.

### Joins

| Method                                    | Join type |
| ----------------------------------------- | --------- |
| `.inner_join(table, left_col, right_col)` | INNER     |
| `.left_join(table, left_col, right_col)`  | LEFT      |
| `.right_join(table, left_col, right_col)` | RIGHT     |
| `.full_join(table, left_col, right_col)`  | FULL      |

Queries containing joins must be executed via `Database::select_join`. Calling
`Database::select::<T>` with a joined query returns
`QueryError::JoinInsideTypedSelect`.

### Eager Loading

```rust
.with("posts")
```

Adds a foreign-key relation to load eagerly. Each relation is loaded once via a
batch fetch keyed by the foreign-key column.

### Distinct

```rust
.distinct(&["name"])
.distinct(&["category", "vendor"])
```

Sets `distinct_by` to the supplied list of column names. Rows are deduplicated
by the tuple of values across those columns; the first row encountered for each
distinct tuple is retained. Passing an empty slice is a no-op.

Semantics:

- Columns are looked up on the source record (`ValuesSource::This`).
- Missing columns are treated as `Value::Null`, so listing an unknown column
  collapses every row into a single result.
- Deduplication runs **before** ordering, offset, and limit.
- The selected fields (`Select::Columns`) do not need to include the
  `distinct_by` columns.

### Aggregations

```rust
.group_by(&["category"])
.having(Filter::gt("count", Value::Uint64(10u64.into())))
```

| Method                 | Effect                                              |
| ---------------------- | --------------------------------------------------- |
| `.group_by(&[col...])` | Sets `group_by` to the supplied list of columns     |
| `.having(Filter)`      | Sets the HAVING filter applied to aggregated groups |

Aggregations operate over the rows that survive `WHERE` and `DISTINCT`. Each
group of rows sharing the same `group_by` tuple produces one
[`AggregatedRow`](#aggregatedrow). The aggregate functions to compute are
described by [`AggregateFunction`](#aggregatefunction); their results are
returned as [`AggregatedValue`](#aggregatedvalue) entries inside the row.

The `HAVING` filter is evaluated after aggregation, against the grouping keys
and aggregate results.

### Ordering

| Method                   | Effect                              |
| ------------------------ | ----------------------------------- |
| `.order_by_asc(column)`  | Appends ascending sort by `column`  |
| `.order_by_desc(column)` | Appends descending sort by `column` |

Multiple `order_by_*` calls produce stable multi-key sorts; later keys break
ties from earlier keys.

### Pagination

| Method           | Effect                              |
| ---------------- | ----------------------------------- |
| `.limit(usize)`  | Caps the number of records returned |
| `.offset(usize)` | Skips the first N records           |

---

## Aggregate Types

Types used to describe and return aggregated query results. All three are
re-exported from the `wasm-dbms-api`, `ic-dbms-api`, and `ic-dbms-client`
preludes.

### `AggregateFunction`

```rust
pub enum AggregateFunction {
    Count(Option<String>),
    Sum(String),
    Avg(String),
    Min(String),
    Max(String),
}
```

Describes one aggregate function to compute over a group of rows.

| Variant          | SQL equivalent | Notes                                |
| ---------------- | -------------- | ------------------------------------ |
| `Count(None)`    | `COUNT(*)`     | Counts every row in the group        |
| `Count(Some(c))` | `COUNT(c)`     | Counts non-null values of column `c` |
| `Sum(c)`         | `SUM(c)`       | Sum of `c` across the group          |
| `Avg(c)`         | `AVG(c)`       | Arithmetic mean of `c`               |
| `Min(c)`         | `MIN(c)`       | Minimum value of `c`                 |
| `Max(c)`         | `MAX(c)`       | Maximum value of `c`                 |

### `AggregatedRow`

```rust
pub struct AggregatedRow {
    pub group_keys: Vec<Value>,
    pub values: Vec<AggregatedValue>,
}
```

A single row of aggregated output. `group_keys` holds the values of the
`group_by` columns that identify the group; `values` holds the aggregate
results in the same order as the `AggregateFunction` list supplied with the
query.

### `AggregatedValue`

```rust
pub enum AggregatedValue {
    Count(u64),
    Sum(Value),
    Avg(Value),
    Min(Value),
    Max(Value),
}
```

Carries the result of one aggregate function. `Count` is always a `u64`; the
remaining variants wrap a [`Value`](../reference/data-types.md) whose concrete
variant depends on the source column's data type.

---

## Execution Order

The select pipeline applies the query elements in this order — matching
standard SQL semantics:

1. **WHERE** — `filter` is applied while scanning records (or via an index plan).
2. **DISTINCT** — `distinct_by` deduplicates the surviving rows.
3. **GROUP BY / aggregates** — when `group_by` is set, surviving rows are
   bucketed by the grouping tuple and the requested
   [`AggregateFunction`](#aggregatefunction)s are computed per bucket,
   producing [`AggregatedRow`](#aggregatedrow)s.
4. **HAVING** — `having` filters the aggregated groups.
5. **Eager loading** — relations declared by `with(...)` are batch-fetched
   (non-aggregate selects only).
6. **Column selection** — non-selected columns are dropped from each row.
7. **ORDER BY** — `order_by` keys are applied in declared order.
8. **OFFSET / LIMIT** — applied last when `order_by` or `distinct_by` is set;
   otherwise applied during the scan for early termination.

> When neither `order_by` nor `distinct_by` is present, the engine applies
> `offset`/`limit` during iteration to avoid materialising the entire result
> set.

---

## Errors

All variants come from [`QueryError`](./errors.md). Most are surfaced at
planning time (before any rows are scanned) so callers fail fast.

### Aggregate-specific (`Database::aggregate`)

| Condition                                       | Variant                                                                  |
| ----------------------------------------------- | ------------------------------------------------------------------------ |
| `SUM` or `AVG` references a non-numeric column  | `InvalidQuery("aggregate requires numeric column: '<col>'")`             |
| Aggregate references a column not on the table  | `UnknownColumn(<col>)`                                                   |
| `GROUP BY` references a column not on the table | `UnknownColumn(<col>)`                                                   |
| `HAVING` references unknown column or `agg{N}`  | `InvalidQuery("HAVING references unknown column or aggregate: '<col>'")` |
| `ORDER BY` references unknown `agg{N}`          | `InvalidQuery("ORDER BY references unknown aggregate output: '<col>'")`  |
| `LIKE` used inside a `HAVING` clause            | `InvalidQuery("LIKE is not supported in HAVING")`                        |
| `JSON` filter used inside a `HAVING` clause     | `InvalidQuery("JSON filters are not supported in HAVING")`               |
| Query carries `joins` on an aggregate call      | `InvalidQuery("joins are not supported in aggregate queries")`           |
| Query carries `eager_relations` on an aggregate | `InvalidQuery("eager relations are not supported in aggregate queries")` |

### Non-aggregate select paths

| Condition                                                             | Variant                                               |
| --------------------------------------------------------------------- | ----------------------------------------------------- |
| `group_by` or `having` set on `select` / `select_raw` / `select_join` | `AggregateClauseInSelect` (use `Database::aggregate`) |
| Query carries `joins` on a typed `select::<T>` call                   | `JoinInsideTypedSelect`                               |

---

## Related Types

- [`AggregateFunction`](#aggregatefunction) — aggregate to compute per group
- [`AggregatedRow`](#aggregatedrow) — single row of aggregated output
- [`AggregatedValue`](#aggregatedvalue) — single aggregate result value
- [`Filter`](../guides/querying.md#filters) — predicates for `WHERE`
- [`JsonFilter`](./json.md) — JSON-specific predicates
- [`Join`, `JoinType`](../guides/querying.md#joins) — join clauses
- [`OrderDirection`](../guides/querying.md#ordering) — ascending/descending
- [`Select`](../guides/querying.md#field-selection) — `All` or `Columns(...)`
- [`QueryError`](./errors.md) — query-time error variants
