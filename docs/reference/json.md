# JSON Reference

- [Overview](#overview)
- [Defining JSON Columns](#defining-json-columns)
- [Creating JSON Values](#creating-json-values)
- [JSON Filtering](#json-filtering)
  - [Path Syntax](#path-syntax)
- [Filter Operations](#filter-operations)
  - [Contains (Structural Containment)](#contains-structural-containment)
  - [Extract (Path Extraction + Comparison)](#extract-path-extraction--comparison)
  - [HasKey (Path Existence)](#haskey-path-existence)
- [Combining JSON Filters](#combining-json-filters)
- [Type Conversion](#type-conversion)
- [Complete Example](#complete-example)
- [Error Handling](#error-handling)

---

## Overview

The `Json` data type allows you to store and query semi-structured JSON data within your database tables. This is useful for:

- Flexible schemas where structure varies between records
- Metadata storage
- User preferences and settings
- Any scenario where data structure may evolve

---

## Defining JSON Columns

To use JSON in your schema, use the `Json` type:

```rust
use wasm_dbms_api::prelude::*;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    pub metadata: Json,          // Required JSON field
    pub settings: Nullable<Json>, // Optional JSON field
}
```

---

## Creating JSON Values

**From string:**

```rust
use std::str::FromStr;
use wasm_dbms_api::prelude::Json;

let json = Json::from_str(r#"{"name": "Alice", "age": 30}"#).unwrap();
```

**From serde_json::Value:**

```rust
use serde_json::json;
use wasm_dbms_api::prelude::Json;

let json: Json = json!({
    "name": "Alice",
    "age": 30,
    "tags": ["developer", "rust"],
    "address": {
        "city": "New York",
        "country": "US"
    }
}).into();
```

**In insert requests:**

```rust
let user = UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    metadata: json!({
        "role": "admin",
        "permissions": ["read", "write", "delete"]
    }).into(),
    settings: Nullable::Value(json!({
        "theme": "dark",
        "notifications": true
    }).into()),
};
```

---

## JSON Filtering

wasm-dbms provides powerful JSON filtering through the `JsonFilter` enum:

- **Contains**: Check if JSON contains a pattern (structural containment)
- **Extract**: Extract value at path and compare
- **HasKey**: Check if a path exists

### Path Syntax

Paths use dot notation with bracket array indices:

| Path | Meaning |
|------|---------|
| `"name"` | Root-level field `name` |
| `"user.name"` | Nested field at `user.name` |
| `"items[0]"` | First element of `items` array |
| `"users[0].name"` | `name` field of first user |
| `"data[0][1]"` | Nested array access |
| `"[0]"` | First element of root array |

**Path examples:**

```json
{
  "name": "Alice",           // Path: "name"
  "user": {
    "email": "a@b.com"       // Path: "user.email"
  },
  "tags": ["a", "b", "c"],   // Path: "tags[0]" = "a"
  "matrix": [[1,2], [3,4]]   // Path: "matrix[1][0]" = 3
}
```

---

## Filter Operations

### Contains (Structural Containment)

Checks if the JSON column contains a specified pattern. Implements PostgreSQL `@>` style containment:

- **Objects**: All key-value pairs in pattern must exist in target (recursive)
- **Arrays**: All elements in pattern must exist in target (order-independent)
- **Primitives**: Must be equal

```rust
use wasm_dbms_api::prelude::*;
use std::str::FromStr;

// Filter where metadata contains {"active": true}
let pattern = Json::from_str(r#"{"active": true}"#).unwrap();
let filter = Filter::json("metadata", JsonFilter::contains(pattern));
```

**Containment behavior:**

| Target | Pattern | Result |
|--------|---------|--------|
| `{"a": 1, "b": 2}` | `{"a": 1}` | Match |
| `{"a": 1}` | `{"a": 1, "b": 2}` | No match |
| `{"user": {"name": "Alice", "age": 30}}` | `{"user": {"name": "Alice"}}` | Match |
| `[1, 2, 3]` | `[3, 1]` | Match (order-independent) |
| `[1, 2]` | `[1, 2, 3]` | No match |
| `{"tags": ["a", "b", "c"]}` | `{"tags": ["b"]}` | Match |

**Use cases:**
- Check if user has specific role: `contains({"role": "admin"})`
- Check if array contains value: `contains({"tags": ["important"]})`
- Check nested properties: `contains({"settings": {"theme": "dark"}})`

### Extract (Path Extraction + Comparison)

Extract a value at path and apply comparison:

```rust
// Equal
let filter = Filter::json("metadata",
    JsonFilter::extract_eq("user.name", Value::Text("Alice".into()))
);

// Greater than
let filter = Filter::json("metadata",
    JsonFilter::extract_gt("user.age", Value::Int64(18.into()))
);

// In list
let filter = Filter::json("metadata",
    JsonFilter::extract_in("status", vec![
        Value::Text("active".into()),
        Value::Text("pending".into()),
    ])
);

// Is null (path doesn't exist or value is null)
let filter = Filter::json("metadata",
    JsonFilter::extract_is_null("deleted_at")
);

// Not null (path exists and value is not null)
let filter = Filter::json("metadata",
    JsonFilter::extract_not_null("email")
);
```

**Available comparison methods:**

| Method | Description |
|--------|-------------|
| `extract_eq(path, value)` | Equal |
| `extract_ne(path, value)` | Not equal |
| `extract_gt(path, value)` | Greater than |
| `extract_lt(path, value)` | Less than |
| `extract_ge(path, value)` | Greater than or equal |
| `extract_le(path, value)` | Less than or equal |
| `extract_in(path, values)` | Value in list |
| `extract_is_null(path)` | Path doesn't exist or is null |
| `extract_not_null(path)` | Path exists and is not null |

### HasKey (Path Existence)

Check if a path exists in the JSON:

```rust
// Check for root-level key
let filter = Filter::json("metadata", JsonFilter::has_key("email"));

// Check for nested path
let filter = Filter::json("metadata", JsonFilter::has_key("user.address.city"));

// Check for array element
let filter = Filter::json("metadata", JsonFilter::has_key("items[0]"));
```

> **Note:** `HasKey` returns `true` even if the value at path is `null`. It only checks for path existence.

---

## Combining JSON Filters

JSON filters combine with other filters using `and()`, `or()`, `not()`:

```rust
// has email AND age > 18
let filter = Filter::json("metadata", JsonFilter::has_key("email"))
    .and(Filter::json("metadata", JsonFilter::extract_gt("age", Value::Int64(18.into()))));

// role = "admin" OR role = "moderator"
let filter = Filter::json("metadata", JsonFilter::extract_eq("role", Value::Text("admin".into())))
    .or(Filter::json("metadata", JsonFilter::extract_eq("role", Value::Text("moderator".into()))));

// Combine with regular filters
let pattern = Json::from_str(r#"{"active": true}"#).unwrap();
let filter = Filter::eq("id", Value::Int32(1.into()))
    .and(Filter::json("metadata", JsonFilter::contains(pattern)));

// NOT has deleted_at
let filter = Filter::json("metadata", JsonFilter::has_key("deleted_at")).not();
```

---

## Type Conversion

When extracting JSON values, they're converted to DBMS types:

| JSON Type | DBMS Value |
|-----------|------------|
| `null` | `Value::Null` |
| `true`/`false` | `Value::Boolean` |
| Integer number | `Value::Int64` |
| Float number | `Value::Decimal` |
| String | `Value::Text` |
| Array | `Value::Json` |
| Object | `Value::Json` |

**Comparison examples:**

```rust
// JSON: {"count": 42}
// Extracted as Int64, compare with Int64
JsonFilter::extract_eq("count", Value::Int64(42.into()))

// JSON: {"price": 19.99}
// Extracted as Decimal, compare with Decimal
JsonFilter::extract_gt("price", Value::Decimal(10.0.into()))

// JSON: {"active": true}
// Extracted as Boolean
JsonFilter::extract_eq("active", Value::Boolean(true))

// JSON: {"name": "Alice"}
// Extracted as Text
JsonFilter::extract_eq("name", Value::Text("Alice".into()))
```

---

## Complete Example

```rust
use wasm_dbms_api::prelude::*;
use std::str::FromStr;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "products"]
pub struct Product {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    pub attributes: Json,  // {"color": "red", "size": "M", "tags": ["sale", "new"], "price": 29.99}
}

fn example_queries(database: &impl Database) -> Result<(), Box<dyn std::error::Error>> {
    // Find all red products
    let filter = Filter::json("attributes",
        JsonFilter::extract_eq("color", Value::Text("red".into()))
    );
    let query = Query::builder().filter(filter).build();
    let red_products = database.select::<Product>(query)?;

    // Find products with "sale" tag
    let pattern = Json::from_str(r#"{"tags": ["sale"]}"#)?;
    let filter = Filter::json("attributes", JsonFilter::contains(pattern));
    let query = Query::builder().filter(filter).build();
    let sale_products = database.select::<Product>(query)?;

    // Find products with size attribute
    let filter = Filter::json("attributes", JsonFilter::has_key("size"));
    let query = Query::builder().filter(filter).build();
    let sized_products = database.select::<Product>(query)?;

    // Find red products with price > 20
    let filter = Filter::json("attributes", JsonFilter::extract_eq("color", Value::Text("red".into())))
        .and(Filter::json("attributes", JsonFilter::extract_gt("price", Value::Decimal(20.0.into()))));
    let query = Query::builder().filter(filter).build();
    let expensive_red = database.select::<Product>(query)?;

    // Find products in specific sizes
    let filter = Filter::json("attributes",
        JsonFilter::extract_in("size", vec![
            Value::Text("S".into()),
            Value::Text("M".into()),
            Value::Text("L".into()),
        ])
    );
    let query = Query::builder().filter(filter).build();
    let standard_sizes = database.select::<Product>(query)?;

    Ok(())
}
```

---

## Error Handling

JSON filter operations return errors for:

**Invalid path syntax:**
- Empty paths
- Trailing dots (`"user."`)
- Unclosed brackets (`"items[0"`)
- Negative indices (`"items[-1]"`)
- Non-numeric array indices (`"items[abc]"`)

**Non-JSON column:**
- Applying JSON filter to a non-JSON column

```rust
// Invalid path - will error
let filter = Filter::json("metadata", JsonFilter::has_key("user."));  // Trailing dot

// Non-JSON column - will error
let filter = Filter::json("name", JsonFilter::has_key("field"));  // "name" is Text, not Json

let result = database.select::<User>(
    Query::builder().filter(filter).build(),
);

match result {
    Err(DbmsError::Query(QueryError::InvalidQuery)) => {
        println!("Invalid JSON filter");
    }
    _ => {}
}
```
