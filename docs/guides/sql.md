# SQL

- [SQL](#sql)
  - [Overview](#overview)
  - [Setup](#setup)
    - [Dependencies](#dependencies)
    - [Create the Engine](#create-the-engine)
    - [Keep the Engine Alive](#keep-the-engine-alive)
  - [Running Statements](#running-statements)
    - [Parameters](#parameters)
    - [Reading Results](#reading-results)
  - [Queries](#queries)
    - [Filtering and Sorting](#filtering-and-sorting)
    - [Joins](#joins)
    - [Aggregates](#aggregates)
  - [Writes](#writes)
  - [Transactions](#transactions)
  - [Dates, UUIDs, and Other Types](#dates-uuids-and-other-types)
  - [Error Handling](#error-handling)
  - [SQL and the Typed API](#sql-and-the-typed-api)

---

## Overview

The `wasm-dbms-sql` crate lets you read and write a wasm-dbms database with
SQL text instead of the query builder:

```rust
let result = engine.execute(
    &ctx,
    caller,
    "SELECT name, email FROM users WHERE age >= ? ORDER BY name",
    &[Value::from(18u8)],
)?;
```

It is useful when:

- you already know SQL and want to explore or debug the data;
- the query is easier to read as text than as a chain of builder calls;
- the statement comes from outside your program, such as an admin tool.

SQL covers `SELECT` (with joins, `DISTINCT`, aggregates, and `GROUP BY`),
`INSERT`, `UPDATE`, `DELETE`, and transactions. It does not change the schema:
tables are still defined with `#[derive(Table)]`. Every detail of the dialect
is in the [SQL Reference](../reference/sql.md).

SQL support is a separate crate. Programs that do not depend on it do not pay
for it in binary size.

---

## Setup

### Dependencies

```toml
[dependencies]
wasm-dbms = "0.9"
wasm-dbms-api = "0.9"
wasm-dbms-memory = "0.9"
wasm-dbms-sql = "0.9"
```

`wasm-dbms-sql` turns on the `sql` feature of `wasm-dbms` and `wasm-dbms-api`
by itself. That feature adds the `SqlResult` and `SqlError` types to
`wasm_dbms_api::prelude`.

### Create the Engine

The engine needs your database schema, and the schema must be `Clone`. A
schema is normally a unit struct, so deriving `Clone` is enough:

```rust
use wasm_dbms::prelude::*;
use wasm_dbms_api::prelude::*;
use wasm_dbms_memory::prelude::HeapMemoryProvider;
use wasm_dbms_sql::SqlEngine;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    pub email: Nullable<Text>,
    pub age: Uint8,
}

#[derive(Clone, DatabaseSchema)]
#[tables(User = "users")]
pub struct MySchema;

let ctx = DbmsContext::new(HeapMemoryProvider::default());
MySchema::register_tables(&ctx)?;

let engine = SqlEngine::new(MySchema);
```

### Keep the Engine Alive

The engine remembers which caller has a transaction open. Create it once and
keep it for as long as the database context lives, so that a `BEGIN` in one
call is still known in the next one. The engine does not borrow the context,
so both can be stored side by side:

```rust
thread_local! {
    static DBMS: DbmsContext<MyMemoryProvider> = DbmsContext::new(MyMemoryProvider::default());
    static SQL: SqlEngine<MySchema> = SqlEngine::new(MySchema);
}

fn run_sql(caller: &[u8], sql: &str, params: &[Value]) -> Result<SqlResult, SqlError> {
    DBMS.with(|ctx| SQL.with(|engine| engine.execute(ctx, caller, sql, params)))
}
```

An engine that is created for a single call works too, as long as that code
never uses `BEGIN`.

---

## Running Statements

```rust
pub fn execute(
    &self,
    ctx: &DbmsContext<M>,
    caller: &[u8],
    sql: &str,
    params: &[Value],
) -> Result<SqlResult, SqlError>
```

| Argument | Meaning                                                             |
| -------- | ------------------------------------------------------------------- |
| `ctx`    | The database to run against                                         |
| `caller` | Who is running the statement; selects the transaction it belongs to |
| `sql`    | One SQL statement, with an optional trailing `;`                    |
| `params` | One value per `?` placeholder, in order                             |

`caller` is any byte string that identifies the user or session, the same kind
of identity that `DbmsContext::begin_transaction` takes. Programs with a
single user can pass a constant.

### Parameters

Write `?` where a value goes and pass the values separately:

```rust
engine.execute(
    &ctx,
    caller,
    "INSERT INTO users (id, name, email, age) VALUES (?, ?, ?, ?)",
    &[
        Value::from(1u32),
        Value::from("Alice"),
        Value::Null,
        Value::from(30u8),
    ],
)?;
```

Always pass values that come from users as parameters. A parameter is data: it
is never read as SQL, so it cannot change what the statement does. Building
the SQL text with `format!` does not have that guarantee.

Parameters do not need the exact type of the column. An integer of any width
fits any integer column if the number is in range, and a text value is
converted the same way as a string literal.

### Reading Results

`execute` returns a `SqlResult`:

```rust
match engine.execute(&ctx, caller, sql, &[])? {
    SqlResult::Rows(rows) => {
        for row in rows {
            for (column, value) in row {
                println!("{name} = {value:?}", name = column.name);
            }
        }
    }
    SqlResult::RowsAffected(count) => println!("{count} rows written"),
    SqlResult::TxBegin | SqlResult::TxCommit | SqlResult::TxRollback => {}
}
```

Each row has one `(JoinColumnDef, Value)` pair per selected column, in the
order of the select list. `column.name` is the column name, or the alias given
with `AS`. For a join, `column.table` holds the table the column comes from.

---

## Queries

### Filtering and Sorting

```sql
SELECT id, name
FROM users
WHERE (age < 18 OR age > 65)
  AND email IS NOT NULL
  AND name LIKE 'A%'
ORDER BY age DESC, name
LIMIT 10 OFFSET 20;
```

`LIMIT` and `OFFSET` accept values through `4294967295` on every target.
Larger values are rejected consistently by native and WASM builds.

`WHERE` supports `=`, `!=`, `<`, `<=`, `>`, `>=`, `IN`, `LIKE`, `IS NULL`, and
their negations, combined with `AND`, `OR`, `NOT`, and parentheses. The left
side of a test is always a column, and the right side a value.

`SELECT DISTINCT` removes duplicate rows:

```sql
SELECT DISTINCT city FROM users ORDER BY city;
```

### Joins

```sql
SELECT u.name, p.title AS headline
FROM users AS u
JOIN posts AS p ON u.id = p.user_id
WHERE p.published = TRUE
ORDER BY p.title;
```

`JOIN`, `LEFT JOIN`, `RIGHT JOIN`, and `FULL JOIN` are supported. The `ON`
condition is one equality between a column of the joined table and a column of
a table before it. When two tables have a column with the same name, qualify
it with the table name or alias.

The two `ON` columns must have the same underlying type, even if their
nullability differs. Incompatible types produce `SqlError::TypeMismatch`.
Null join keys never match, including two `NULL` values.

### Aggregates

```sql
SELECT category, COUNT(*) AS items, SUM(price) AS revenue
FROM sales
WHERE price > 0
GROUP BY category
HAVING COUNT(*) >= 5
ORDER BY revenue DESC
LIMIT 3;
```

`COUNT`, `SUM`, `AVG`, `MIN`, and `MAX` are available. `SUM` and `AVG` return
a `Decimal`, and `COUNT` a `Uint64`. Aggregate queries read a single table:
they cannot be combined with a join.

---

## Writes

```sql
INSERT INTO users (id, name, age) VALUES (2, 'Bob', 25);

UPDATE users SET name = 'Robert', email = 'bob@example.com' WHERE id = 2;

DELETE FROM users WHERE id = 2;
```

Each write returns `SqlResult::RowsAffected` with the number of rows written.

`UPDATE` and `DELETE` must have a `WHERE` clause. Without one the statement is
rejected with `SqlError::MissingWhereClause` and nothing changes. This protects
against wiping a table by accident.

`DELETE` fails when another table still references the row. Add `CASCADE` to
delete the referencing rows as well:

```sql
DELETE FROM users WHERE id = 1 CASCADE;
```

Writes go through the same sanitizers, validators, and integrity checks as the
typed API.

---

## Transactions

`BEGIN`, `COMMIT`, and `ROLLBACK` group several statements into one unit:

```rust
engine.execute(&ctx, caller, "BEGIN", &[])?;

let transfer = (|| {
    engine.execute(&ctx, caller, "UPDATE accounts SET balance = ? WHERE id = ?", &[
        Value::from(50u64),
        Value::from(1u32),
    ])?;
    engine.execute(&ctx, caller, "UPDATE accounts SET balance = ? WHERE id = ?", &[
        Value::from(150u64),
        Value::from(2u32),
    ])
})();

match transfer {
    Ok(_) => engine.execute(&ctx, caller, "COMMIT", &[])?,
    Err(error) => {
        engine.execute(&ctx, caller, "ROLLBACK", &[])?;
        return Err(error);
    }
};
```

What to know:

- The transaction belongs to the `caller` that sent `BEGIN` in the supplied
  context. Each caller has at most one transaction per context, and callers do
  not see each other's uncommitted changes.
- Statements of a caller without a transaction are applied immediately.
- A statement that fails does not close the transaction. Send `ROLLBACK` to
  discard it, as in the example above.
- `COMMIT` applies all changes together. If it fails, for example because
  another caller took the same primary key in the meantime, nothing is applied
  and the transaction is closed.
- `engine.in_transaction(ctx, caller)` tells whether a caller has a transaction
  open in that context.

SQL transactions and the transactions of the typed API
(`ctx.begin_transaction`) do not know about each other. Use one kind for a
given unit of work.

---

## Dates, UUIDs, and Other Types

SQL has literals for numbers, strings, booleans, and `NULL`. Other values are
written as strings and converted by the type of the column:

```sql
INSERT INTO events (day, at, token, payload, meta)
VALUES ('2026-04-24', '2026-04-24T10:30:00Z',
        '550e8400-e29b-41d4-a716-446655440000', 'deadbeef',
        '{"kind": "signup"}');

SELECT * FROM events WHERE day >= '2026-01-01';
```

| Column type | Written as                               |
| ----------- | ---------------------------------------- |
| `Date`      | `'2026-04-24'`                           |
| `DateTime`  | `'2026-04-24T10:30:00Z'`                 |
| `Uuid`      | `'550e8400-e29b-41d4-a716-446655440000'` |
| `Blob`      | Hexadecimal: `'deadbeef'`                |
| `Json`      | JSON text: `'{"kind": "signup"}'`        |

The same values can be passed as typed parameters (`Value::Date`,
`Value::Uuid`, and so on), which is the only way to pass a
[custom data type](./custom-data-types.md).

A value that does not fit its column is reported before anything is written,
for example `300` for a `Uint8` column or `'2026-02-30'` for a `Date` column.

---

## Error Handling

`execute` returns a `SqlError`:

```rust
match engine.execute(&ctx, caller, sql, params) {
    Ok(result) => { /* ... */ }
    Err(SqlError::Parse { line, col, msg }) => {
        println!("syntax error at {line}:{col}: {msg}");
    }
    Err(SqlError::UnknownTable(table)) => println!("no table named {table}"),
    Err(SqlError::UnknownColumn { table, column }) => {
        println!("table {table} has no column {column}");
    }
    Err(SqlError::Runtime(DbmsError::Query(QueryError::PrimaryKeyConflict))) => {
        println!("that key already exists");
    }
    Err(other) => println!("{other}"),
}
```

Problems in the statement itself (syntax, unknown names, values of the wrong
type) are found before the database is touched. Errors raised by the database
while running the statement, such as constraint violations, are wrapped in
`SqlError::Runtime`. All variants are listed in the
[Errors Reference](../reference/errors.md#sql-errors).

---

## SQL and the Typed API

SQL and the typed API work on the same data and can be mixed freely: a row
inserted with SQL is returned by `database.select::<User>(...)`, and the other
way round.

Some features are only available through the typed API:

- [JSON filters](../reference/json.md) on the content of `Json` columns;
- [eager loading](./relationships.md) of related records;
- typed records (`UserRecord`) instead of lists of column values;
- [schema migrations](./migrations.md).
