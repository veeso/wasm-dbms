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

The implementation lives in `crates/ic-dbms-canister/src/dbms/join.rs`.

---

## Architecture

The engine is implemented as a generic struct:

```rust
pub struct JoinEngine<'a, Schema: ?Sized>
where
    Schema: DatabaseSchema,
{
    schema: &'a Schema,
}
```

Key design decisions:

- **`Schema: ?Sized`** — The `?Sized` bound allows the engine to work with `Box<dyn DatabaseSchema>`, which is how the API layer passes the schema at runtime.
- **Borrows `DatabaseSchema`** — The engine borrows the schema to read rows from tables via `schema.select(dbms, table, query)`.
- **Stateless** — The engine holds no mutable state; it takes a `Query` and returns results in a single call.

The `DatabaseSchema` trait provides the `select` method that the engine uses to read all rows from each table involved in the join.

---

## Processing Pipeline

The `join()` method processes a query through these steps:

```
                    ┌──────────────────────────┐
                    │ 1. Read FROM table rows  │
                    └────────────┬─────────────┘
                                 │
                    ┌────────────▼─────────────┐
                    │ 2. For each JOIN clause:  │◄──── left-to-right
                    │    Read right table rows  │
                    │    Nested-loop join       │
                    └────────────┬─────────────┘
                                 │
                    ┌────────────▼─────────────┐
                    │ 3. Apply filter           │
                    └────────────┬─────────────┘
                                 │
                    ┌────────────▼─────────────┐
                    │ 4. Apply ordering         │
                    └────────────┬─────────────┘
                                 │
                    ┌────────────▼─────────────┐
                    │ 5. Apply offset           │
                    └────────────┬─────────────┘
                                 │
                    ┌────────────▼─────────────┐
                    │ 6. Apply limit            │
                    └────────────┬─────────────┘
                                 │
                    ┌────────────▼─────────────┐
                    │ 7. Flatten to output      │
                    └──────────────────────────┘
```

1. **Read FROM table**: All rows from the primary table are loaded using an unfiltered `Query::builder().all().build()`.
2. **Process JOINs**: Each `Join` clause is processed left-to-right. For each clause, the right table is read in full, column references are resolved, and the nested-loop join is executed against the accumulated result.
3. **Filter**: The query's filter is applied to the combined rows using `filter.matches_joined_row()`, which supports qualified `table.column` references.
4. **Order**: Order-by clauses are applied in reverse (stable sort), so the primary sort key ends up correctly ordered.
5. **Offset**: Rows are skipped according to the offset value.
6. **Limit**: The result is truncated to the limit.
7. **Flatten**: Each joined row is converted from the internal `JoinedRow` representation to the output `Vec<(JoinColumnDef, Value)>` format, applying column selection.

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

When a row has no match on the opposite side (in LEFT, RIGHT, or FULL joins), the missing columns are filled with `Value::Null`. The engine determines which columns to pad by inspecting a sample row from the opposite table:

```rust
fn null_pad_columns(&self, sample_row: &[(ColumnDef, Value)]) -> Vec<(ColumnDef, Value)> {
    sample_row
        .iter()
        .map(|(col, _)| (*col, Value::Null))
        .collect()
}
```

This preserves the correct column definitions (name, type, nullability) while setting every value to NULL. If the opposite table is empty (no sample row available), the padded group has zero columns.

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

For filters and ordering on joined results, the same qualified/unqualified pattern applies. Unqualified names are searched across all table groups in the row, returning the first match.

---

## Output Format

Join results use `JoinColumnDef` instead of `ColumnDef`:

```rust
pub struct JoinColumnDef {
    pub table: Option<String>,  // Source table name
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

- **O(n*m) nested-loop join**: Each join performs a full nested-loop comparison. For two tables of size *n* and *m*, this is O(n*m) per join clause.
- **Full table scans for join matching**: The join ON condition itself does not use indexes — both sides are compared via linear scan. However, if the query has a filter, the individual table reads that feed the join may use indexes (via the standard select path).
- **All rows loaded into memory**: Every table involved in the join is fully materialized in memory before processing. This can be a concern for very large tables on the IC.
- **Equality joins only**: The ON condition only supports column equality (`left_col = right_col`). Range conditions, expressions, and multi-column ON clauses are not supported.
