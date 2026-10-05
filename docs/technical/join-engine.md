# Join Engine

- [Overview](#overview)
- [Architecture](#architecture)
- [Processing Pipeline](#processing-pipeline)
- [Nested-Loop Join Algorithm](#nested-loop-join-algorithm)
- [NULL Padding](#null-padding)
- [Column Resolution](#column-resolution)
- [Output Format](#output-format)
- [Limitations](#limitations)

---

## Overview

The join engine executes cross-table join queries, combining rows from two or more tables based on column equality conditions. It supports four join types — INNER, LEFT, RIGHT, and FULL — and integrates with the existing query pipeline for filtering, ordering, pagination, and column selection.

The implementation lives in `crates/wasm-dbms/wasm-dbms/src/join.rs`.

---

## Architecture

The engine is implemented as a generic struct:

```rust
pub struct JoinEngine<'a, Schema: ?Sized, M>
where
    Schema: DatabaseSchema<M>,
    M: MemoryProvider,
{
    schema: &'a Schema,
}
```

Key design decisions:

- **`Schema: ?Sized`** — The `?Sized` bound allows the engine to work with `Box<dyn DatabaseSchema>`, which is how the API layer passes the schema at runtime.
- **Borrows `DatabaseSchema`** — The engine borrows the schema to read rows via `schema.select(dbms, table, query)` and compile-time column definitions via `schema.table_columns(table)`.
- **Stateless** — The engine holds no mutable state; it takes a `Query` and returns results in a single call.

The `DatabaseSchema` trait provides the `select` method that the engine uses to read all rows from each table involved in the join.

---

## Processing Pipeline

The `join()` method processes a query through these steps:

```
┌──────────────────────────┐
│ 1. Load table schemas    │
│    Validate filter       │
└────────────┬─────────────┘
             │
┌────────────▼─────────────┐
│ 2. Read FROM table rows  │
└────────────┬─────────────┘
             │
┌────────────▼─────────────┐
│ 3. For each JOIN clause:  │◄──── left-to-right
│    Read right table rows  │
│    Nested-loop join       │
└────────────┬─────────────┘
             │
┌────────────▼─────────────┐
│ 4. Apply filter           │
└────────────┬─────────────┘
             │
┌────────────▼─────────────┐
│ 5. Apply ordering         │
└────────────┬─────────────┘
             │
┌────────────▼─────────────┐
│ 6. Apply offset           │
└────────────┬─────────────┘
             │
┌────────────▼─────────────┐
│ 7. Apply limit            │
└────────────┬─────────────┘
             │
┌────────────▼─────────────┐
│ 8. Flatten to output      │
└──────────────────────────┘
```

1. **Load schemas and validate**: Compile-time column definitions are loaded for every table. The filter is validated against those definitions so ambiguous references, out-of-scope tables, and invalid typed operands fail even when the join produces no rows.
2. **Read FROM table**: All rows from the primary table are loaded using an unfiltered `Query::builder().all().build()`.
3. **Process JOINs**: Each `Join` clause is processed left-to-right. For each clause, matching rows from the right table are loaded and the nested-loop join is executed against the accumulated result.
4. **Filter**: The validated filter is applied to the combined rows using `filter.matches_joined_row()`, which supports qualified `table.column` references.
5. **Order**: Order-by clauses are applied in reverse (stable sort), so the primary sort key ends up correctly ordered.
6. **Offset**: Rows are skipped according to the offset value.
7. **Limit**: The result is truncated to the limit.
8. **Flatten**: Each joined row is converted from the internal `JoinedRow` representation to the output `Vec<(JoinColumnDef, Value)>` format, applying column selection.

---

## Nested-Loop Join Algorithm

All four join types are handled by a single `nested_loop_join` method using two boolean flags:

| Join Type | `keep_unmatched_left` | `keep_unmatched_right` |
| --------- | --------------------- | ---------------------- |
| INNER     | `false`               | `false`                |
| LEFT      | `true`                | `false`                |
| RIGHT     | `false`               | `true`                 |
| FULL      | `true`                | `true`                 |

The algorithm:

1. For each left row, iterate over all right rows.
2. If the left column value equals the right column value (and is not `None`), emit a combined row and mark the right row as matched.
3. After scanning all right rows for a given left row: if `keep_unmatched_left` is true and no match was found, emit the left row with NULL-padded right columns.
4. After all left rows are processed: if `keep_unmatched_right` is true, emit each unmatched right row with NULL-padded left columns.

This unified approach avoids code duplication across join types while keeping the logic straightforward.

---

## NULL Padding

When a row has no match on the opposite side (in LEFT, RIGHT, or FULL joins), the missing columns are filled with `Value::Null`. The engine uses compile-time column definitions supplied by `DatabaseSchema::table_columns`:

```rust
fn null_pad_columns(&self, columns: &[ColumnDef]) -> Vec<(ColumnDef, Value)> {
    columns
        .iter()
        .map(|column| (*column, Value::Null))
        .collect()
}
```

This preserves the complete row shape and column definitions even when the opposite table contains no rows. The derive-generated `DatabaseSchema` implementation supplies this metadata. Handwritten implementations remain source-compatible through the trait's default method, but must override `table_columns` to execute joins.

---

## Column Resolution

Column references in join ON conditions, filters, and ordering can be either qualified or unqualified:

- **Qualified**: `"users.id"` — explicitly specifies the table.
- **Unqualified**: `"id"` — defaults to the FROM table (for ON left-column) or the joined table (for ON right-column).

Resolution is handled by `resolve_column_ref`:

```rust
fn resolve_column_ref(&self, field: &str, default_table: &str) -> (String, &str) {
    if let Some((table, column)) = field.split_once('.') {
        (table.to_string(), column)
    } else {
        (default_table.to_string(), field)
    }
}
```

For filters and ordering on joined results, the same qualified/unqualified pattern applies. Filter validation rejects an unqualified name that exists in multiple joined tables and asks the caller to qualify it with a table name.

---

## Output Format

Join results use `JoinColumnDef` instead of `ColumnDef`:

```rust
pub struct JoinColumnDef {
    pub table: Option<String>, // Source table name
    pub name: String,
    pub data_type: DataTypeKind,
    pub nullable: bool,
    pub primary_key: bool,
}
```

The `table` field is `Some(table_name)` for join results, allowing consumers to distinguish columns that share the same name across different tables.

At the API layer, the generated `select` endpoint checks `query.has_joins()`:

- **With joins**: Routes to `select_join`, which uses `JoinEngine`.
- **Without joins**: Routes to `select_raw`, the standard single-table path.

Both paths return `Vec<Vec<(JoinColumnDef, Value)>>`, but for non-join queries the `table` field is `None`.

---

## Limitations

- **O(n*m) nested-loop join**: Each join performs a full nested-loop comparison. For two tables of size _n_ and _m_, this is O(n*m) per join clause.
- **Full table scans for join matching**: The join ON condition itself does not use indexes — both sides are compared via linear scan. However, if the query has a filter, the individual table reads that feed the join may use indexes (via the standard select path).
- **All rows loaded into memory**: Every table involved in the join is fully materialized in memory before processing. This can be a concern for very large tables.
- **Equality joins only**: The ON condition only supports column equality (`left_col = right_col`). Range conditions, expressions, and multi-column ON clauses are not supported.
