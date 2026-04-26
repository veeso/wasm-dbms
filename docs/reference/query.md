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
    - [Ordering](#ordering)
    - [Pagination](#pagination)
  - [Execution Order](#execution-order)
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
    pub eager_relations: Vec<String>,
    pub joins: Vec<Join>,
    pub filter: Option<Filter>,
    pub distinct_by: Vec<String>,
    pub order_by: Vec<(String, OrderDirection)>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}
```

| Field             | Type                            | Description                                       |
|-------------------|---------------------------------|---------------------------------------------------|
| `columns`         | `Select`                        | `Select::All` or `Select::Columns(Vec<String>)`   |
| `eager_relations` | `Vec<String>`                   | Foreign-key relations to load eagerly             |
| `joins`           | `Vec<Join>`                     | Join clauses (only valid via `select_join`)       |
| `filter`          | `Option<Filter>`                | WHERE-clause expression                           |
| `distinct_by`     | `Vec<String>`                   | Columns used to deduplicate results               |
| `order_by`        | `Vec<(String, OrderDirection)>` | Multi-column ordering                             |
| `limit`           | `Option<usize>`                 | Maximum number of records to return               |
| `offset`          | `Option<usize>`                 | Number of records to skip                         |

Use `Query::builder()` to obtain a `QueryBuilder`.

---

## QueryBuilder

### Field Selection

| Method          | Effect                                 |
|-----------------|----------------------------------------|
| `.all()`        | Selects all columns (`Select::All`)    |
| `.field(name)`  | Adds a single column to the selection  |
| `.fields(iter)` | Adds multiple columns                  |

The primary key is always included by `Database::select::<T>` even when not explicitly listed.

### Filters

| Method                    | Effect                                     |
|---------------------------|--------------------------------------------|
| `.filter(Option<Filter>)` | Replaces the current filter                |
| `.and_where(Filter)`      | Combines with existing filter using `AND`  |
| `.or_where(Filter)`       | Combines with existing filter using `OR`   |

See [Filters in the Querying Guide](../guides/querying.md#filters) and
[JSON Filters](./json.md) for the full filter API.

### Joins

| Method                                    | Join type |
|-------------------------------------------|-----------|
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

### Ordering

| Method                   | Effect                              |
|--------------------------|-------------------------------------|
| `.order_by_asc(column)`  | Appends ascending sort by `column`  |
| `.order_by_desc(column)` | Appends descending sort by `column` |

Multiple `order_by_*` calls produce stable multi-key sorts; later keys break
ties from earlier keys.

### Pagination

| Method           | Effect                              |
|------------------|-------------------------------------|
| `.limit(usize)`  | Caps the number of records returned |
| `.offset(usize)` | Skips the first N records           |

---

## Execution Order

The select pipeline applies the query elements in this order — matching
standard SQL semantics:

1. **WHERE** — `filter` is applied while scanning records (or via an index plan).
2. **DISTINCT** — `distinct_by` deduplicates the surviving rows.
3. **Eager loading** — relations declared by `with(...)` are batch-fetched.
4. **Column selection** — non-selected columns are dropped from each row.
5. **ORDER BY** — `order_by` keys are applied in declared order.
6. **OFFSET / LIMIT** — applied last when `order_by` or `distinct_by` is set;
   otherwise applied during the scan for early termination.

> When neither `order_by` nor `distinct_by` is present, the engine applies
> `offset`/`limit` during iteration to avoid materialising the entire result
> set.

---

## Related Types

- [`Filter`](../guides/querying.md#filters) — predicates for `WHERE`
- [`JsonFilter`](./json.md) — JSON-specific predicates
- [`Join`, `JoinType`](../guides/querying.md#joins) — join clauses
- [`OrderDirection`](../guides/querying.md#ordering) — ascending/descending
- [`Select`](../guides/querying.md#field-selection) — `All` or `Columns(...)`
- [`QueryError`](./errors.md) — query-time error variants
