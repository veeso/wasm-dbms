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
  - [Joins](#joins)
    - [Join Types](#join-types)
    - [Basic Join](#basic-join)
    - [Left, Right, and Full Joins](#left-right-and-full-joins)
    - [Chaining Multiple Joins](#chaining-multiple-joins)
    - [Qualified Column Names](#qualified-column-names)
    - [Joins vs Eager Loading](#joins-vs-eager-loading)

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
|---------------|--------------------------|---------------------------|
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
|----------------|-----------------------|-------------------------------------------------------|
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
|---------|----------------------------|
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

fn get_page_query(page: u64) -> Query<User> {
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

## Joins

Joins combine rows from two or more tables based on a related column, producing a single result set with columns from all joined tables. Use joins when you need to correlate data across tables in a single flat result -- for example, listing posts alongside their author names.

> **Note:** Joins only work through the untyped `select_raw` path. Typed `select::<T>` rejects queries that contain joins with a `JoinInsideTypedSelect` error.

### Join Types

| Type | Builder Method | Description |
|------|---------------|-------------|
| INNER | `.inner_join()` | Returns only rows where both sides match |
| LEFT | `.left_join()` | Returns all left rows; unmatched right columns are NULL |
| RIGHT | `.right_join()` | Returns all right rows; unmatched left columns are NULL |
| FULL | `.full_join()` | Returns all rows from both sides; unmatched columns are NULL |

### Basic Join

Use `.inner_join(table, left_column, right_column)` to join two tables:

```rust
use wasm_dbms_api::prelude::*;

// Join users with their posts (INNER JOIN)
let query = Query::builder()
    .all()
    .inner_join("posts", "id", "user_id")
    .build();

// Use select_raw since joins return untyped rows
let rows = database.select_raw("users", query)?;

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

let rows = database.select_raw("users", query)?;
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

Unqualified names default to the FROM table (the table passed to `select_raw`).

### Joins vs Eager Loading

| | Eager Loading | Joins |
|--|--------------|-------|
| **Result type** | Typed (`Vec<T>`) | Untyped (`Vec<Vec<(ColumnDef, Value)>>`) |
| **Result format** | Separate related records | Flat combined rows |
| **API method** | `select::<T>` | `select_raw` |
| **Column disambiguation** | Not needed | Use `table.column` syntax |
| **Use case** | Load parent with children | Correlate columns across tables |

Use **eager loading** when you want typed results with related records attached. Use **joins** when you need a flat, cross-table result set -- for example, for reporting, search, or when you need columns from multiple tables in a single row.
