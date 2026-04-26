# Querying

- [Querying](#querying)
  - [Overview](#overview)
  - [Query Builder](#query-builder)
    - [Basic Queries](#basic-queries)
    - [Query Structure](#query-structure)
  - [Filters](#filters)
    - [Comparison Filters](#comparison-filters)
    - [List Membership](#list-membership)
    - [Pattern Matching](#pattern-matching)
    - [Null Checks](#null-checks)
    - [Combining Filters](#combining-filters)
  - [JSON Filters](#json-filters)
  - [Ordering](#ordering)
    - [Single Column Ordering](#single-column-ordering)
    - [Multiple Column Ordering](#multiple-column-ordering)
  - [Pagination](#pagination)
    - [Limit](#limit)
    - [Offset](#offset)
    - [Pagination Pattern](#pagination-pattern)
  - [Field Selection](#field-selection)
    - [Select All Fields](#select-all-fields)
    - [Select Specific Fields](#select-specific-fields)
  - [Eager Loading](#eager-loading)
  - [Distinct](#distinct)
    - [Basic Distinct](#basic-distinct)
    - [Distinct by Multiple Columns](#distinct-by-multiple-columns)
    - [Distinct with Ordering and Pagination](#distinct-with-ordering-and-pagination)
    - [Distinct Semantics](#distinct-semantics)
  - [Aggregations](#aggregations)
    - [Defining Aggregates](#defining-aggregates)
    - [Group By and Having](#group-by-and-having)
    - [Aggregate Result Types](#aggregate-result-types)
  - [Joins](#joins)
    - [Join Types](#join-types)
    - [Basic Join](#basic-join)
    - [Left, Right, and Full Joins](#left-right-and-full-joins)
    - [Chaining Multiple Joins](#chaining-multiple-joins)
    - [Qualified Column Names](#qualified-column-names)
    - [Joins vs Eager Loading](#joins-vs-eager-loading)
  - [Index-Accelerated Queries](#index-accelerated-queries)
    - [How Indexes Improve Queries](#how-indexes-improve-queries)
    - [Which Filters Use Indexes](#which-filters-use-indexes)
    - [Residual Filters](#residual-filters)
    - [Transaction-Aware Lookups](#transaction-aware-lookups)

---

## Overview

wasm-dbms provides a powerful query API for retrieving data from your tables. Queries are built using the `QueryBuilder`
and can include:

- **Filters** - Narrow down which records to return
- **Ordering** - Sort results by one or more columns
- **Pagination** - Limit results and implement pagination
- **Field Selection** - Choose which columns to return
- **Eager Loading** - Load related records in a single query
- **Joins** - Combine rows from multiple tables

---

## Query Builder

### Basic Queries

Use `Query::builder()` to construct queries:

```rust
use wasm_dbms_api::prelude::*;

// Select all records
let query = Query::builder().all().build();

// Select with filter
let query = Query::builder()
.filter(Filter::eq("status", Value::Text("active".into())))
.build();

// Complex query with multiple options
let query = Query::builder()
.filter(Filter::gt("age", Value::Int32(18.into())))
.order_by("created_at", OrderDirection::Descending)
.limit(10)
.offset(20)
.build();
```

### Query Structure

A query consists of these optional components:

| Component     | Method                   | Description               |
| ------------- | ------------------------ | ------------------------- |
| Filter        | `.filter()`              | Which records to return   |
| Select        | `.all()` or `.columns()` | Which columns to return   |
| Order         | `.order_by()`            | Sort order                |
| Limit         | `.limit()`               | Maximum records to return |
| Offset        | `.offset()`              | Records to skip           |
| Eager Loading | `.with()`                | Related tables to load    |
| Join          | `.inner_join()`, etc.    | Cross-table join          |

---

## Filters

Filters determine which records match your query. All filters are created using the `Filter` struct.

### Comparison Filters

| Filter         | Description           | Example                                               |
| -------------- | --------------------- | ----------------------------------------------------- |
| `Filter::eq()` | Equal to              | `Filter::eq("status", Value::Text("active".into()))`  |
| `Filter::ne()` | Not equal to          | `Filter::ne("status", Value::Text("deleted".into()))` |
| `Filter::gt()` | Greater than          | `Filter::gt("age", Value::Int32(18.into()))`          |
| `Filter::ge()` | Greater than or equal | `Filter::ge("score", Value::Decimal(90.0.into()))`    |
| `Filter::lt()` | Less than             | `Filter::lt("price", Value::Decimal(100.0.into()))`   |
| `Filter::le()` | Less than or equal    | `Filter::le("quantity", Value::Int32(10.into()))`     |

**Examples:**

```rust
// Find users older than 21
let filter = Filter::gt("age", Value::Int32(21.into()));

// Find products under $50
let filter = Filter::lt("price", Value::Decimal(50.0.into()));

// Find orders from a specific date
let filter = Filter::ge("created_at", Value::DateTime(some_datetime));
```

### List Membership

Check if a value is in a list of values:

```rust
// Find users with specific roles
let filter = Filter::in_list("role", vec![
    Value::Text("admin".into()),
    Value::Text("moderator".into()),
    Value::Text("editor".into()),
]);

// Find products in certain categories
let filter = Filter::in_list("category_id", vec![
    Value::Uint32(1.into()),
    Value::Uint32(2.into()),
    Value::Uint32(5.into()),
]);
```

### Pattern Matching

Use `like` for pattern matching with wildcards:

| Pattern | Matches                    |
| ------- | -------------------------- |
| `%`     | Any sequence of characters |
| `_`     | Any single character       |
| `%%`    | Literal `%` character      |

```rust
// Find users whose email ends with @company.com
let filter = Filter::like("email", "%@company.com");

// Find products starting with "Pro"
let filter = Filter::like("name", "Pro%");

// Find codes with pattern XX-###
let filter = Filter::like("code", "__-___");

// Find text containing literal %
let filter = Filter::like("description", "%%25%% off");
```

### Null Checks

Check for null or non-null values:

```rust
// Find users without a phone number
let filter = Filter::is_null("phone");

// Find users with a profile picture
let filter = Filter::not_null("avatar_url");
```

### Combining Filters

Filters can be combined using logical operators:

**AND - Both conditions must match:**

```rust
// Active users over 18
let filter = Filter::eq("status", Value::Text("active".into()))
.and(Filter::gt("age", Value::Int32(18.into())));
```

**OR - Either condition matches:**

```rust
// Admins or moderators
let filter = Filter::eq("role", Value::Text("admin".into()))
.or(Filter::eq("role", Value::Text("moderator".into())));
```

**NOT - Negate a condition:**

```rust
// Users who are not banned
let filter = Filter::eq("status", Value::Text("banned".into())).not();
```

**Complex combinations:**

```rust
// (active AND age > 18) OR role = "admin"
let filter = Filter::eq("status", Value::Text("active".into()))
.and(Filter::gt("age", Value::Int32(18.into())))
.or(Filter::eq("role", Value::Text("admin".into())));

// NOT (deleted OR archived)
let filter = Filter::eq("status", Value::Text("deleted".into()))
.or(Filter::eq("status", Value::Text("archived".into())))
.not();
```

---

## JSON Filters

For columns with `Json` type, use specialized JSON filters. See the [JSON Reference](../reference/json.md) for
comprehensive documentation.

**Quick examples:**

```rust
// Check if JSON contains a pattern
let pattern = Json::from_str(r#"{"active": true}"#).unwrap();
let filter = Filter::json("metadata", JsonFilter::contains(pattern));

// Extract and compare a value
let filter = Filter::json(
"settings",
JsonFilter::extract_eq("theme", Value::Text("dark".into()))
);

// Check if a path exists
let filter = Filter::json("data", JsonFilter::has_key("user.email"));
```

---

## Ordering

### Single Column Ordering

Sort results by a single column:

```rust
// Sort by name ascending (A-Z)
let query = Query::builder()
.all()
.order_by("name", OrderDirection::Ascending)
.build();

// Sort by created_at descending (newest first)
let query = Query::builder()
.all()
.order_by("created_at", OrderDirection::Descending)
.build();
```

### Multiple Column Ordering

Chain multiple `order_by` calls for secondary sorting:

```rust
// Sort by category, then by price within each category
let query = Query::builder()
.all()
.order_by("category", OrderDirection::Ascending)
.order_by("price", OrderDirection::Descending)
.build();

// Sort by status, then by priority, then by created_at
let query = Query::builder()
.all()
.order_by("status", OrderDirection::Ascending)
.order_by("priority", OrderDirection::Descending)
.order_by("created_at", OrderDirection::Ascending)
.build();
```

---

## Pagination

### Limit

Restrict the number of records returned:

```rust
// Get only the first 10 records
let query = Query::builder()
.all()
.limit(10)
.build();
```

### Offset

Skip a number of records before returning results:

```rust
// Skip the first 20 records
let query = Query::builder()
.all()
.offset(20)
.build();
```

### Pagination Pattern

Combine `limit` and `offset` for pagination:

```rust
const PAGE_SIZE: u64 = 20;

fn get_page_query(page: u64) -> Query {
    Query::builder()
        .all()
        .order_by("id", OrderDirection::Ascending)  // Consistent ordering is important
        .limit(PAGE_SIZE)
        .offset(page * PAGE_SIZE)
        .build()
}

// Page 0: records 0-19
let page_0 = get_page_query(0);

// Page 1: records 20-39
let page_1 = get_page_query(1);

// Page 2: records 40-59
let page_2 = get_page_query(2);
```

> **Tip:** Always use `order_by` with pagination to ensure consistent ordering across pages.

---

## Field Selection

### Select All Fields

Use `.all()` to select all columns:

```rust
let query = Query::builder()
.all()
.build();

let users = database.select::<User>(query)?;
// All fields are populated
```

### Select Specific Fields

Use `.columns()` to select only specific columns:

```rust
let query = Query::builder()
.columns(vec!["id".to_string(), "name".to_string(), "email".to_string()])
.build();

let users = database.select::<User>(query)?;
// Only id, name, and email are populated
// Other fields will have default values
```

> **Note:** The primary key is always included, even if not specified.

---

## Eager Loading

Load related records in a single query using `.with()`:

```rust
// Define tables with foreign key
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub author_id: Uint32,
}

// Query posts with authors eagerly loaded
let query = Query::builder()
.all()
.with("users")
.build();

let posts = database.select::<Post>(query)?;
```

See the [Relationships Guide](./relationships.md) for more on eager loading.

---

## Distinct

Use `.distinct(&[...])` to remove duplicate rows from the result set based on
one or more columns. Rows are deduplicated by the tuple of values across the
listed columns; the first row encountered for each distinct tuple is kept.

### Basic Distinct

```rust
// Get the unique set of names from the users table
let query = Query::builder()
    .all()
    .distinct(&["name"])
    .build();

let users = database.select::<User>(query)?;
```

### Distinct by Multiple Columns

```rust
// Unique (category, vendor) pairs from products
let query = Query::builder()
    .all()
    .distinct(&["category", "vendor"])
    .build();

let products = database.select::<Product>(query)?;
```

### Distinct with Ordering and Pagination

`DISTINCT` runs before `ORDER BY`, `OFFSET`, and `LIMIT`, so paging through
distinct values works as expected:

```rust
// Page 2 (size 10) of unique names, alphabetical
let query = Query::builder()
    .all()
    .distinct(&["name"])
    .order_by_asc("name")
    .offset(10)
    .limit(10)
    .build();
```

Without `DISTINCT`, `LIMIT 10` could yield ten copies of the same name. With
`DISTINCT`, the limit applies to the deduplicated stream.

### Distinct Semantics

- Lookup is performed against the source row's columns. The columns named in
  `.distinct(...)` do **not** need to be in the field selection.
- A column not present on the row is treated as `Value::Null`. Listing an
  unknown column collapses every row into a single result.
- Calling `.distinct(&[])` (or omitting it) is a no-op.
- Pipeline order: `WHERE` -> `DISTINCT` -> eager loading -> column selection
  -> `ORDER BY` -> `OFFSET` / `LIMIT`. See the
  [Query API Reference](../reference/query.md#execution-order) for the full
  pipeline.

> **Tip:** `distinct(&[pk_column])` returns at most one row per primary key,
> which can be useful when joining sources that fan out the parent rows.

---

## Aggregations

Aggregations summarise groups of rows using `COUNT`, `SUM`, `AVG`, `MIN`, and
`MAX`. Group rows with `.group_by(...)`, filter the resulting groups with
`.having(...)`, and describe the aggregates to compute via the
`AggregateFunction` enum.

### Defining Aggregates

Each aggregate is one variant of [`AggregateFunction`]:

```rust
use wasm_dbms_api::prelude::AggregateFunction;

let aggregates = vec![
    AggregateFunction::Count(None),               // COUNT(*)
    AggregateFunction::Count(Some("email".into())), // COUNT(email)
    AggregateFunction::Sum("amount".into()),
    AggregateFunction::Avg("amount".into()),
    AggregateFunction::Min("created_at".into()),
    AggregateFunction::Max("created_at".into()),
];
```

`Count(None)` counts every row in the group; `Count(Some(col))` counts only
rows where `col` is non-null. The other variants take a column name and operate
over its values.

### Group By and Having

Use `.group_by(&[...])` to define grouping keys and `.having(filter)` to filter
the aggregated groups:

```rust
let query = Query::builder()
    .all()
    .group_by(&["category"])
    .having(Filter::gt("count", Value::Uint64(10u64.into())))
    .order_by_desc("category")
    .build();
```

`HAVING` is evaluated after aggregation, against grouping keys and aggregate
results. `WHERE` (set with `.and_where()` / `.or_where()`) still applies first
to the raw rows.

### Aggregate Result Types

Aggregated queries return [`AggregatedRow`] values:

```rust
pub struct AggregatedRow {
    pub group_keys: Vec<Value>,
    pub values: Vec<AggregatedValue>,
}
```

`group_keys` carries the grouping tuple (one [`Value`] per `group_by` column).
`values` holds one [`AggregatedValue`] per requested aggregate, in the same
order as the `AggregateFunction` list.

```rust
pub enum AggregatedValue {
    Count(u64),
    Sum(Value),
    Avg(Value),
    Min(Value),
    Max(Value),
}
```

`Count` is always `u64`; the other variants wrap a [`Value`] whose concrete
variant matches the source column's data type.

See the [Query API Reference](../reference/query.md#aggregate-types) for the
full type definitions and pipeline ordering.

[`AggregateFunction`]: ../reference/query.md#aggregatefunction
[`AggregatedRow`]: ../reference/query.md#aggregatedrow
[`AggregatedValue`]: ../reference/query.md#aggregatedvalue
[`Value`]: ../reference/data-types.md

---

## Joins

Joins combine rows from two or more tables based on a related column, producing a single result set with columns from all joined tables. Use joins when you need to correlate data across tables in a single flat result -- for example, listing posts alongside their author names.

> **Note:** Joins require the `select_join` method, which returns rows with [`JoinColumnDef`] that include the source table name. Typed `select::<T>` rejects queries that contain joins with a `JoinInsideTypedSelect` error.

### Join Types

| Type  | Builder Method  | Description                                                  |
| ----- | --------------- | ------------------------------------------------------------ |
| INNER | `.inner_join()` | Returns only rows where both sides match                     |
| LEFT  | `.left_join()`  | Returns all left rows; unmatched right columns are NULL      |
| RIGHT | `.right_join()` | Returns all right rows; unmatched left columns are NULL      |
| FULL  | `.full_join()`  | Returns all rows from both sides; unmatched columns are NULL |

### Basic Join

Use `.inner_join(table, left_column, right_column)` to join two tables:

```rust
use wasm_dbms_api::prelude::*;

// Join users with their posts (INNER JOIN)
let query = Query::builder()
    .all()
    .inner_join("posts", "id", "user_id")
    .build();

// Use select_join since joins return rows with table provenance
let rows = database.select_join("users", query)?;

// Each row contains columns from both "users" and "posts"
for row in &rows {
    for (col_def, value) in row {
        // col_def.table tells you which table the column came from
        println!("{}.{} = {:?}", col_def.table.as_deref().unwrap_or("?"), col_def.name, value);
    }
}
```

### Left, Right, and Full Joins

```rust
// LEFT JOIN: all users, even those without posts
let query = Query::builder()
    .all()
    .left_join("posts", "id", "user_id")
    .build();

// RIGHT JOIN: all posts, even those with missing/deleted authors
let query = Query::builder()
    .all()
    .right_join("posts", "id", "user_id")
    .build();

// FULL JOIN: all users and all posts, matched where possible
let query = Query::builder()
    .all()
    .full_join("posts", "id", "user_id")
    .build();
```

For LEFT, RIGHT, and FULL joins, columns from the unmatched side are filled with `Value::Null`.

### Chaining Multiple Joins

Chain multiple joins to combine more than two tables:

```rust
// Users -> Posts -> Comments
let query = Query::builder()
    .all()
    .inner_join("posts", "id", "user_id")
    .left_join("comments", "posts.id", "post_id")
    .build();

let rows = database.select_join("users", query)?;
```

Joins are processed left-to-right. The second join operates on the result of the first.

### Qualified Column Names

When joining tables that share column names, use `table.column` syntax to disambiguate:

```rust
// Both "users" and "posts" have an "id" column
let query = Query::builder()
    .field("users.id")
    .field("users.name")
    .field("posts.title")
    .inner_join("posts", "users.id", "user_id")
    .and_where(Filter::eq("users.name", Value::Text("Alice".into())))
    .order_by_asc("posts.title")
    .build();
```

Qualified names (`table.column`) work in:

- Field selection (`.field()`)
- Filters (`.and_where()`, `.or_where()`)
- Ordering (`.order_by_asc()`, `.order_by_desc()`)
- Join ON conditions

Unqualified names default to the FROM table (the table passed to `select_join`).

### Joins vs Eager Loading

|                           | Eager Loading             | Joins                                        |
| ------------------------- | ------------------------- | -------------------------------------------- |
| **Result type**           | Typed (`Vec<T>`)          | Untyped (`Vec<Vec<(JoinColumnDef, Value)>>`) |
| **Result format**         | Separate related records  | Flat combined rows                           |
| **API method**            | `select::<T>`             | `select_join`                                |
| **Column disambiguation** | Not needed                | Use `table.column` syntax                    |
| **Use case**              | Load parent with children | Correlate columns across tables              |

Use **eager loading** when you want typed results with related records attached. Use **joins** when you need a flat, cross-table result set -- for example, for reporting, search, or when you need columns from multiple tables in a single row.

---

## Index-Accelerated Queries

When a table has indexes defined (via `#[index]` or the automatic primary key index), the query
engine can use them to avoid full table scans. This happens transparently — you write the same
filters as before, and the engine picks the best available index.

### How Indexes Improve Queries

Without indexes, every SELECT, UPDATE, and DELETE scans all records in the table. With an index
on the filtered column, the engine navigates the B-tree to locate matching records directly,
then loads only those records from memory.

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    #[index]
    pub email: Text,
    pub name: Text,
}

// This uses the index on `email` — no full table scan
let query = Query::builder()
    .filter(Filter::eq("email", Value::Text("alice@example.com".into())))
    .build();

let users = database.select::<User>(query)?;
```

### Which Filters Use Indexes

The filter analyzer extracts an index plan from the leftmost AND-chain of conditions on
indexed columns:

| Filter                              | Index plan               | Notes                                  |
| ----------------------------------- | ------------------------ | -------------------------------------- |
| `Filter::eq("col", val)`            | Exact match              | Best case — direct B-tree lookup       |
| `Filter::ge("col", val)`            | Range scan (start bound) | Uses linked-leaf traversal             |
| `Filter::le("col", val)`            | Range scan (end bound)   | Uses linked-leaf traversal             |
| `Filter::gt("col", val)`            | Range scan + residual    | Range is inclusive, so GT is rechecked |
| `Filter::lt("col", val)`            | Range scan + residual    | Range is inclusive, so LT is rechecked |
| `Filter::in_list("col", vals)`      | Multi-lookup             | One exact match per value              |
| AND of range filters on same column | Merged range             | e.g., `age >= 18 AND age <= 65`        |

**Filters that fall back to full scan:**

- OR at the top level
- NOT wrapping an indexable condition
- Filters on non-indexed columns
- Complex nested expressions

### Residual Filters

When the index narrows down the candidate set but doesn't fully satisfy the filter, the
remaining conditions are applied as a residual check on each loaded record:

```rust
// Index on `email` handles the equality check.
// `name LIKE 'A%'` is applied as a residual filter on the results.
let filter = Filter::eq("email", Value::Text("alice@example.com".into()))
    .and(Filter::like("name", "A%"));
```

### Transaction-Aware Lookups

Inside a transaction, index lookups are merged with the transaction overlay. Records
added in the current transaction appear in index results, and deleted records are
excluded — even though the on-disk B-tree has not been modified yet. On commit, overlay
changes are flushed to the persistent B-tree. On rollback, the overlay is discarded and
the B-tree remains unchanged.
