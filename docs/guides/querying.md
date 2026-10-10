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
    - [Composite Indexes](#composite-indexes)
    - [OR and AND Across Indexes](#or-and-and-across-indexes)
    - [Null Checks on Indexed Columns](#null-checks-on-indexed-columns)
    - [Covering Reads](#covering-reads)
    - [When the Engine Scans Instead](#when-the-engine-scans-instead)
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
| `\%`    | Literal `%` character      |
| `\_`    | Literal `_` character      |
| `\\`    | Literal `\` character      |

A backslash escapes the character that follows it. A pattern that ends with an
unescaped backslash, such as `abc\`, is rejected with
`QueryError::InvalidQuery`. Write `abc\\` to match text ending with a literal
backslash.

```rust
// Find users whose email ends with @company.com
let filter = Filter::like("email", "%@company.com");

// Find products starting with "Pro"
let filter = Filter::like("name", "Pro%");

// Find codes with pattern XX-###
let filter = Filter::like("code", "__-___");

// Find text containing literal %
let filter = Filter::like("description", r"%25\% off%");
```

### Null Checks

Check for null or non-null values:

```rust
// Find users without a phone number
let filter = Filter::is_null("phone");

// Find users with a profile picture
let filter = Filter::not_null("avatar_url");
```

A `NULL` value in a nullable `Text` or `Json` column never matches a `like`
filter or a JSON filter, so the row is left out of the result instead of
failing the query. The pattern or JSON path is still checked, and an invalid
one returns an error. `not()` inverts the result, so a negated `like` or JSON
filter matches `NULL` values, in the same way `ne` matches them. Combine the
filter with `not_null` to leave `NULL` values out:

```rust
// Users whose nickname does not start with "x", without users lacking a nickname
let filter = Filter::like("nickname", "x%")
.not()
.and(Filter::not_null("nickname"));
```

The same rules apply to plain queries and to join filters, where the columns
of an unmatched side of a `LEFT`, `RIGHT`, or `FULL` join are `NULL`.

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
[`JoinColumnDef`]: ../reference/query.md#join-results
[`JoinResultSet`]: ../reference/query.md#join-results
[`Value`]: ../reference/data-types.md

---

## Joins

Joins combine rows from two or more tables based on a related column, producing a single result set with columns from all joined tables. Use joins when you need to correlate data across tables in a single flat result -- for example, listing posts alongside their author names.

> **Note:** Joins require the `select_join` method, which returns a
> [`JoinResultSet`]: the [`JoinColumnDef`] of every selected column once, each
> with its source table name, followed by the rows as plain `Vec<Value>`. Typed
> `select::<T>` rejects queries that contain joins with a
> `JoinInsideTypedSelect` error.

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

// select_join returns the column descriptions once, then the rows
let result = database.select_join("users", query)?;

// Each JoinRow pairs every value with its column description
for row in &result {
    for (col_def, value) in row.iter() {
        // col_def.table tells you which table the column came from
        let table = col_def.table.as_deref().unwrap_or("?");
        println!("{table}.{column} = {value:?}", column = col_def.name.as_str());
    }
}

// Resolve column positions once when reading many rows
let name_index = result
    .column_index("users.name")
    .expect("users.name is selected");
let title_index = result
    .column_index("posts.title")
    .expect("posts.title is selected");
for row in &result.rows {
    println!(
        "{user_name:?} wrote {post_title:?}",
        user_name = &row[name_index],
        post_title = &row[title_index]
    );
}

// Or look up a value directly from a borrowed row
if let Some(row) = result.row(0) {
    let first_title = row.get("posts.title");
    let any_id = row.get("id"); // unqualified: first column named "id"
}
```

A bare column name resolves to the first column with that name in row order,
so qualify names that exist on both sides (`users.id` and `posts.id`).
`result.columns` is the complete header even when `result.rows` is empty.

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

Join keys match only when both values are non-null and equal. A `NULL` join key
does not match another `NULL`. For LEFT, RIGHT, and FULL joins, the row remains
unmatched and columns from the missing side are filled with `Value::Null`.

### Chaining Multiple Joins

Chain multiple joins to combine more than two tables:

```rust
// Users -> Posts -> Comments
let query = Query::builder()
    .all()
    .inner_join("posts", "id", "user_id")
    .left_join("comments", "posts.id", "post_id")
    .build();

let result = database.select_join("users", query)?;
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

|                           | Eager Loading             | Joins                                           |
| ------------------------- | ------------------------- | ----------------------------------------------- |
| **Result type**           | Typed (`Vec<T>`)          | `JoinResultSet` (columns once, rows of `Value`) |
| **Result format**         | Separate related records  | Flat combined rows                              |
| **API method**            | `select::<T>`             | `select_join`                                   |
| **Column disambiguation** | Not needed                | Use `table.column` syntax                       |
| **Use case**              | Load parent with children | Correlate columns across tables                 |

Use **eager loading** when you want typed results with related records attached. Use **joins** when you need a flat, cross-table result set -- for example, for reporting, search, or when you need columns from multiple tables in a single row.

---

## Index-Accelerated Queries

When a table has indexes defined (via `#[index]` or the automatic primary key index), the query
engine can use them to avoid full table scans. This happens transparently — you write the same
filters as before, and the engine picks the best available index.

### How Indexes Improve Queries

Without indexes, every SELECT, UPDATE, and DELETE scans all records in the table. With an index
on the filtered column, the engine navigates the B-tree to locate matching records directly,
then loads only those records from memory. Covered projections can return values from index keys
without loading record pages.

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

The planner reads the conditions combined with AND, in any order and nesting,
and matches them against every index of the table:

| Filter                                   | Index access                               |
| ---------------------------------------- | ------------------------------------------ |
| `Filter::eq("col", val)`                 | Exact key lookup                           |
| `Filter::in_list("col", vals)`           | One exact lookup per distinct value        |
| `ge`, `gt`, `le`, `lt` on one column     | Range scan with inclusive/exclusive bounds |
| Several bounds on the same column        | The strictest bounds                       |
| `Filter::is_null("col")`                 | Exact lookup of the NULL key               |
| `Filter::not_null("col")`                | Range of the non-NULL keys                 |
| `a OR b` where every branch is indexable | Union of the branch lookups                |

Each result is checked against the query's filter, so an index narrows the rows
to consider without changing which rows qualify. Contradictory conditions such
as `price >= 5 AND price < 5` or an empty `in_list` return no rows without
reading the table.

### Composite Indexes

A composite index is used from its first column onward, in declaration order.
For an index on `(category, brand, price)`:

- Equality on all three columns is one exact lookup.
- Equality on `category` reads only that category.
- Equality on `category` and a range on `brand` reads only that range.
- A condition on `brand` or `price` alone cannot use the index.
- If an index column has no condition, conditions on later columns are checked
  on the candidate rows instead.

A leading `in_list` expands into at most 64 index ranges. Above that limit, the
engine falls back or uses a narrower index path available from other conditions.

### OR and AND Across Indexes

`a OR b` uses an index when every branch can; the results are merged and each
row is returned once. If one branch has no usable index, the whole OR falls
back, although another AND condition outside the OR may still use an index. At
most 64 index ranges are read for one OR.

For `a AND b` on separately indexed columns, the engine prefers a primary-key,
unique, or complete composite equality. Otherwise it intersects useful index
paths and keeps only rows present in each chosen path before loading records.

### Null Checks on Indexed Columns

`is_null` is an exact lookup. `not_null` reads the non-NULL part of the index:
signed integers, dates, date-times, decimals, booleans, blobs, and JSON sort
below NULL; text and unsigned integers sort above it. Nullable `Uuid` and
custom columns have no proven range and use a table scan.

### Covering Reads

Outside a transaction, one index can build rows from keys alone when it
contains every selected, filtered, ordered, and distinct column, the query
loads no relations, and either the filter plans to one range or no filter is
given. Duplicate keys still produce one result per record. A secondary index
contains the primary key only when the index definition lists it.

Without ordering or distinct processing, covering reads stop after enough
matching rows have been read to satisfy the offset and limit. A zero limit
does not read index entries.

### When the Engine Scans Instead

The engine uses a full table scan with the same results when no index path can
provide a complete candidate set. This includes queries where no condition
matches an index and the query is not eligible for an unfiltered covering read.
For record-fetching plans, a range that would materialize more than 4,096
candidates or a plan that would materialize more than 16,384 also triggers a
scan unless another complete index path narrows the candidates within the
limit. Covering reads stream index keys instead of materializing addresses.
Results are never truncated.

Filters containing `like` or JSON keep the earlier single-column index plan
and residual evaluation so their error and short-circuit behavior stays
unchanged.

### Transaction-Aware Lookups

Inside a transaction, the engine reads committed index entries, applies the
transaction's inserts, updates, and deletes to the candidate rows, adds rows
the transaction moved into the filter, and checks every row against the whole
filter. Updates to columns outside the chosen index conditions do not add
extra record fetches. Filters containing `like` or JSON first check the visible
index key and then evaluate the earlier residual filter, preserving their
error behavior. A row is returned once even when it moved between OR branches or
changed its primary key. On commit the indexes are updated; on rollback they
are unchanged.
