# Join Engine

- [Overview](#overview)
- [Architecture](#architecture)
- [Processing Pipeline](#processing-pipeline)
- [Hash Join Algorithm](#hash-join-algorithm)
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
│    Hash join              │
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
│ 8. Materialize result set │
└──────────────────────────┘
```

1. **Load schemas and validate**: Compile-time column definitions are loaded for every table. The filter is validated against those definitions so ambiguous references, out-of-scope tables, and invalid typed operands fail even when the join produces no rows.
2. **Read FROM table**: All rows from the primary table are loaded using an unfiltered `Query::builder().all().build()`.
3. **Process JOINs**: Each `Join` clause is processed left-to-right. For each
   clause, the right table rows are loaded (see [Hash Join Algorithm](#hash-join-algorithm)
   for when the left keys are pushed down) and the hash join is executed against
   the accumulated result.
4. **Filter**: The validated filter is applied to the combined rows using
   `filter.matches_joined_row_ref()`, which borrows the loaded rows and supports
   qualified `table.column` references.
5. **Order**: Order-by clauses are applied in reverse (stable sort), so the primary sort key ends up correctly ordered.
6. **Offset**: Rows are skipped according to the offset value.
7. **Limit**: The result is truncated to the limit.
8. **Materialize**: Each joined row, held until now as one row index per table,
   is turned into a `Vec<Value>` of the selected columns. The `JoinColumnDef`
   list is built once per query and returned next to the rows as a
   `JoinResultSet`.

---

## Hash Join Algorithm

All four join types are handled by a single `hash_join` method using two boolean flags:

| Join Type | `keep_unmatched_left` | `keep_unmatched_right` |
| --------- | --------------------- | ---------------------- |
| INNER     | `false`               | `false`                |
| LEFT      | `true`                | `false`                |
| RIGHT     | `false`               | `true`                 |
| FULL      | `true`                | `true`                 |

Every table taking part in the join is loaded once into a `JoinSide` (its
rows, its compile-time columns, and a NULL row of the same shape). An
intermediate joined row is a `Vec<Option<usize>>` with one entry per side: the
index of that side's row, or `None` when the side is NULL-padded.

The algorithm, per join clause:

1. Build a `HashMap<&Value, Vec<usize>>` over the right rows, keyed by the join
   column. `NULL` keys are skipped.
2. For each accumulated left row, look up its join key in the map. Every hit
   emits a combined row (left indices plus the right index) and marks the right
   row as matched. Hits are emitted in right-row order, so output order equals
   the former nested loop.
3. If `keep_unmatched_left` is true and the key had no hit, emit the left row
   with `None` for the right side.
4. After all left rows: if `keep_unmatched_right` is true, emit each unmatched
   right row with `None` for every left side.

Cost is O(n + m + k) per join clause for _n_ left rows, _m_ right rows and _k_
output rows, instead of O(n*m).

### Loading the right side

Right and full joins need every right row. For inner and left joins the
distinct left keys are pushed down as an `IN` filter only when the right join
column leads an index of the right table (`DatabaseSchema::table_indexes`).
The planner answers that filter with index lookups. On an unindexed column an
`IN` filter costs a list scan per row, so the table is scanned in full and the
hash join does the matching.

---

## NULL Padding

When a row has no match on the opposite side (in LEFT, RIGHT, or FULL joins),
the missing columns are filled with `Value::Null`. Each `JoinSide` builds one
NULL row from the compile-time column definitions supplied by
`DatabaseSchema::table_columns`:

```rust
null_row: columns.iter().map(|column| (*column, Value::Null)).collect()
```

This preserves the complete row shape and column definitions even when the
opposite table contains no rows. The derive-generated `DatabaseSchema`
implementation supplies this metadata. Handwritten implementations remain
source-compatible through the trait's default method, but must override
`table_columns` to execute joins.

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

Join results are returned as a `JoinResultSet`:

```rust
pub struct JoinResultSet {
    pub columns: Vec<JoinColumnDef>, // one per selected column, `table` set
    pub rows: Vec<Vec<Value>>,       // rows[r][c] belongs to columns[c]
}
```

`JoinColumnDef` mirrors `ColumnDef` with owned strings and a `table` field
naming the source table. Storing it once per query instead of once per cell is
what keeps materialization cheap: a 10,000-row join with eight columns would
otherwise allocate the strings of 80,000 column definitions.

`JoinResultSet::column_index("table.column")` resolves a name to a position;
`JoinRow` (from `row(i)` or `iter()`) offers `get("table.column")` and `iter()`
over `(column, value)` pairs. A bare name resolves to the first column with
that name in row order.

The SQL engine converts a `JoinResultSet` into `SqlResult::Rows`, whose rows
keep the `(JoinColumnDef, Value)` pair shape.

---

## Limitations

- **Hash join on equality only**: The build side is always the right table of
  the clause; there is no join reordering or build-side selection by table
  size.
- **Index use limited to the push-down**: The join itself matches rows in
  memory. When the right join column leads an index, the left keys are pushed
  down so the right-side read uses it; otherwise the right table is scanned in
  full.
- **All rows loaded into memory**: Every table involved in the join is fully materialized in memory before processing. This can be a concern for very large tables.
- **Equality joins only**: The ON condition only supports column equality (`left_col = right_col`). Range conditions, expressions, and multi-column ON clauses are not supported.
