# SQL Reference

- [SQL Reference](#sql-reference)
  - [Overview](#overview)
  - [Lexical Structure](#lexical-structure)
    - [Whitespace and Comments](#whitespace-and-comments)
    - [Keywords](#keywords)
    - [Identifiers](#identifiers)
    - [Literals](#literals)
    - [Placeholders](#placeholders)
    - [Statement Terminator](#statement-terminator)
  - [Grammar](#grammar)
  - [SELECT](#select)
    - [Select List](#select-list)
    - [FROM and Table Aliases](#from-and-table-aliases)
    - [JOIN](#join)
    - [WHERE](#where)
    - [DISTINCT](#distinct)
    - [GROUP BY and HAVING](#group-by-and-having)
    - [ORDER BY](#order-by)
    - [LIMIT and OFFSET](#limit-and-offset)
    - [Execution Order](#execution-order)
  - [INSERT](#insert)
  - [UPDATE](#update)
  - [DELETE](#delete)
  - [Transactions](#transactions)
  - [Conditions](#conditions)
    - [Comparison Operators](#comparison-operators)
    - [IN](#in)
    - [LIKE](#like)
    - [IS NULL](#is-null)
    - [AND, OR, NOT](#and-or-not)
    - [NULL Handling](#null-handling)
  - [Aggregate Functions](#aggregate-functions)
  - [Type Conversion](#type-conversion)
    - [Literals by Column Type](#literals-by-column-type)
    - [String Formats](#string-formats)
  - [Parameters](#parameters)
  - [Results](#results)
  - [Errors](#errors)
  - [Limits](#limits)
  - [Reserved Words](#reserved-words)
  - [Not Supported](#not-supported)

---

## Overview

The `wasm-dbms-sql` crate runs SQL statements against a wasm-dbms database. It
covers reading and writing data:

| Statement                   | Purpose                                |
| --------------------------- | -------------------------------------- |
| [`SELECT`](#select)         | Read rows, join tables, aggregate      |
| [`INSERT`](#insert)         | Add one row                            |
| [`UPDATE`](#update)         | Change the rows that match a condition |
| [`DELETE`](#delete)         | Remove the rows that match a condition |
| [`BEGIN`](#transactions)    | Open a transaction                     |
| [`COMMIT`](#transactions)   | Apply the transaction                  |
| [`ROLLBACK`](#transactions) | Discard the transaction                |

Tables, columns, indexes, and foreign keys are defined in Rust with
`#[derive(Table)]`. There is no SQL to create or alter them: schema changes go
through [migrations](./migrations.md).

SQL is executed by `SqlEngine::execute`, one statement per call. For setup and
worked examples, see the [SQL Guide](../guides/sql.md).

---

## Lexical Structure

### Whitespace and Comments

Spaces, tabs, and line breaks separate tokens and are otherwise ignored. A
statement can span any number of lines.

Two comment forms are skipped like whitespace:

```sql
-- a line comment runs to the end of the line
SELECT name /* a block comment can sit between tokens
               and span lines */ FROM users;
```

Block comments do not nest: the first `*/` ends the comment. A block comment
that is never closed is an error. `#` does not start a comment.

### Keywords

Keywords are case-insensitive: `SELECT`, `select`, and `SeLeCt` are the same.
Every keyword is reserved, which means it cannot be used as a table, column,
or alias name unless it is [quoted](#identifiers). The full list is in
[Reserved Words](#reserved-words).

The aggregate function names `COUNT`, `SUM`, `AVG`, `MIN`, and `MAX` are not
reserved. They are case-insensitive and are read as functions only when an
opening parenthesis follows, so a column can be called `count` or `max`.

### Identifiers

Identifiers name tables, columns, and aliases. They are case-sensitive and
must match the names declared in the Rust schema exactly: `users` and `Users`
are different tables.

An unquoted identifier starts with an ASCII letter or `_`, followed by ASCII
letters, digits, or `_`.

A quoted identifier is written between double quotes. It can contain any
character, including spaces and non-ASCII letters. Write `""` for a double
quote inside it. Quoting is required for a name that is a reserved word, and
never changes the meaning of a name that does not need it.

```sql
SELECT "order", "first name" FROM "group" WHERE "select" = 1;
```

Backticks (`` `name` ``) and square brackets (`[name]`) are not accepted. Double
quotes always mean an identifier, never a string.

A column can be qualified with its table: `users.name`. When the table has an
[alias](#from-and-table-aliases), the alias is the qualifier.

### Literals

| Kind    | Form                       | Examples                   |
| ------- | -------------------------- | -------------------------- |
| Integer | Digits                     | `0`, `42`, `-7`            |
| Decimal | Digits, a dot, digits      | `1.5`, `0.25`, `-10.00`    |
| String  | Text between single quotes | `'Alice'`, `'it''s'`, `''` |
| Boolean | `TRUE` or `FALSE`          | `TRUE`, `false`            |
| Null    | `NULL`                     | `NULL`                     |

- A number is made negative with a leading `-`. Unsigned integer literals go up
  to `18446744073709551615`.
- A decimal needs digits on both sides of the dot. `.5`, `5.`, and exponent
  notation such as `1e3` are not accepted.
- A string can contain line breaks. Write `''` for a single quote inside it.
  A backslash has no special meaning in a string.
- There are no date, UUID, or binary literals. Those values are written as
  strings and converted by the type of the column they are used with. See
  [Type Conversion](#type-conversion).

### Placeholders

A `?` stands for a value supplied separately when the statement is executed.
Placeholders are positional: the first `?` takes the first parameter, the
second `?` the second, and so on.

```sql
SELECT name FROM users WHERE age > ? AND city = ? LIMIT ?;
```

A placeholder can be used wherever a literal can, and as the argument of
`LIMIT` and `OFFSET`. Named (`:name`, `@name`) and numbered (`$1`)
placeholders are not accepted. See [Parameters](#parameters).

### Statement Terminator

A statement can end with one `;`. It is optional. Only one statement is
accepted per call: anything after the `;` other than whitespace and comments
is an error.

---

## Grammar

The grammar below uses these conventions: `[ x ]` is optional, `{ x }` repeats
zero or more times, `a | b` is a choice, and quoted words are keywords or
symbols. Keywords are shown in upper case and are case-insensitive.

```text
statement       = ( select | insert | update | delete
                  | begin | commit | rollback ) [ ";" ]

begin           = "BEGIN" [ "TRANSACTION" ]
commit          = "COMMIT" [ "TRANSACTION" ]
rollback        = "ROLLBACK" [ "TRANSACTION" ]

select          = "SELECT" [ "DISTINCT" ] select_list
                  "FROM" table_ref { join }
                  [ "WHERE" condition ]
                  [ "GROUP" "BY" column_ref { "," column_ref } ]
                  [ "HAVING" condition ]
                  [ "ORDER" "BY" order_item { "," order_item } ]
                  [ limit_offset ]
select_list     = "*" | select_item { "," select_item }
select_item     = operand [ "AS" identifier ]
table_ref       = identifier [ [ "AS" ] identifier ]
join            = join_type table_ref "ON" column_ref "=" column_ref
join_type       = [ "INNER" ] "JOIN"
                | ( "LEFT" | "RIGHT" | "FULL" ) [ "OUTER" ] "JOIN"
order_item      = operand [ "ASC" | "DESC" ]
limit_offset    = "LIMIT" row_count [ "OFFSET" row_count ]
                | "OFFSET" row_count [ "LIMIT" row_count ]
row_count       = integer | "?"

insert          = "INSERT" "INTO" identifier
                  "(" identifier { "," identifier } ")"
                  "VALUES" "(" value { "," value } ")"

update          = "UPDATE" identifier
                  "SET" assignment { "," assignment }
                  "WHERE" condition
assignment      = identifier "=" value

delete          = "DELETE" "FROM" identifier
                  "WHERE" condition
                  [ "CASCADE" | "RESTRICT" ]

condition       = and_condition { "OR" and_condition }
and_condition   = not_condition { "AND" not_condition }
not_condition   = "NOT" not_condition
                | "(" condition ")"
                | predicate
predicate       = operand comparison value
                | operand [ "NOT" ] "IN" "(" value { "," value } ")"
                | operand [ "NOT" ] "LIKE" ( string | "?" )
                | operand "IS" [ "NOT" ] "NULL"
comparison      = "=" | "!=" | "<>" | "<" | "<=" | ">" | ">="

operand         = column_ref | aggregate
column_ref      = identifier [ "." identifier ]
aggregate       = "COUNT" "(" ( "*" | column_ref ) ")"
                | ( "SUM" | "AVG" | "MIN" | "MAX" ) "(" column_ref ")"

value           = literal | "?"
literal         = [ "-" ] integer | [ "-" ] decimal | string
                | "TRUE" | "FALSE" | "NULL"
```

Two rules are not visible in the grammar:

- An `aggregate` is allowed in the select list, in `HAVING`, and in
  `ORDER BY`, but not in `WHERE`.
- `NULL` is allowed as a value in `INSERT` and in `UPDATE ... SET`, but not on
  the right of a comparison or inside an `IN` list. Use
  [`IS NULL`](#is-null) instead.

---

## SELECT

```sql
SELECT name, age
FROM users
WHERE age >= 18
ORDER BY age DESC, name
LIMIT 10 OFFSET 20;
```

### Select List

`*` returns every column, in the order the columns are declared in the table.
For a join, it returns the columns of every table, in the order of the tables
in the statement. `*` cannot be mixed with other items, and `table.*` is not
supported.

A list of columns returns exactly those columns, in the order written. The
same column can appear more than once.

`AS` gives a column a different name in the result. The `AS` keyword is
required.

```sql
SELECT id AS user_id, name AS "full name" FROM users;
```

The select list can also contain [aggregate functions](#aggregate-functions).
Expressions, arithmetic, literals, and scalar functions are not supported.

### FROM and Table Aliases

`FROM` names the table to read. An alias can follow the table name, with or
without `AS`:

```sql
SELECT u.name FROM users AS u WHERE u.id = 1;
SELECT u.name FROM users u WHERE u.id = 1;
```

Once a table has an alias, its columns are qualified with the alias and no
longer with the table name: `users.id` is an error in the queries above.

### JOIN

A join combines the rows of two tables whose columns are equal.

```sql
SELECT users.name, posts.title
FROM users
JOIN posts ON users.id = posts.user_id
LEFT JOIN comments ON comments.post_id = posts.id;
```

| Syntax                           | Rows returned                                 |
| -------------------------------- | --------------------------------------------- |
| `JOIN`, `INNER JOIN`             | Only pairs of rows that match                 |
| `LEFT JOIN`, `LEFT OUTER JOIN`   | Also rows of the left side without a match    |
| `RIGHT JOIN`, `RIGHT OUTER JOIN` | Also rows of the joined table without a match |
| `FULL JOIN`, `FULL OUTER JOIN`   | Also unmatched rows of both sides             |

The columns of the missing side of an outer join are `NULL`.
Null join keys never match, including two `NULL` values.

Rules for the `ON` condition:

- It is exactly one equality between two columns.
- Both columns must have the same underlying type; their nullability may
  differ. Incompatible types produce `SqlError::TypeMismatch` during planning.
- One column belongs to the table being joined, and the other to a table that
  comes before it in the statement. The two sides can be written in either
  order.
- `AND`, `OR`, other operators, and literals are not accepted. Put any other
  condition in `WHERE`.

In a join, a column name that exists in more than one table must be qualified,
in every clause. An unqualified name that is ambiguous is an error.

Other rules:

- A table can appear only once in a statement, so a table cannot be joined
  with itself.
- `CROSS JOIN`, `NATURAL JOIN`, `USING (...)`, and comma-separated tables in
  `FROM` are not supported.
- `DISTINCT`, aggregate functions, `GROUP BY`, and `HAVING` cannot be used
  together with a join.

### WHERE

`WHERE` keeps the rows for which the [condition](#conditions) is true.

```sql
SELECT * FROM users
WHERE (age < 18 OR age > 65) AND email IS NOT NULL;
```

### DISTINCT

`SELECT DISTINCT` returns each combination of the selected columns once.

```sql
SELECT DISTINCT city, country FROM users ORDER BY country, city;
```

- With a column list, rows are compared on the listed columns. With `*`, rows
  are compared on every column.
- Every `ORDER BY` column must be in the select list.
- `DISTINCT` cannot be combined with a join or with aggregate functions.

### GROUP BY and HAVING

`GROUP BY` puts the rows that have the same values in the listed columns into
one group. The query then returns one row per group.

```sql
SELECT category, COUNT(*) AS items, SUM(price) AS revenue
FROM sales
WHERE price > 0
GROUP BY category
HAVING COUNT(*) >= 5
ORDER BY revenue DESC;
```

- Every column in the select list must be listed in `GROUP BY`, or be used
  inside an aggregate function. `SELECT *` is not allowed.
- Without `GROUP BY`, a query that uses an aggregate function treats the whole
  table as one group and returns one row, even when the table is empty.
- `HAVING` filters the groups. Its [condition](#conditions) can compare
  aggregate functions and grouped columns. An aggregate used in `HAVING` does
  not need to be in the select list. `LIKE` is not supported in `HAVING`.
- `WHERE` is applied to the rows before they are grouped, and `HAVING` to the
  groups afterwards.

### ORDER BY

`ORDER BY` sorts the result. Each item is a column, optionally followed by
`ASC` (the default) or `DESC`. Earlier items take precedence; later items
break ties.

```sql
SELECT name AS n, age FROM users ORDER BY age DESC, n;
```

- An item can be an alias defined in the select list. When a name is both an
  alias and a column, the alias wins.
- A column does not have to be in the select list, except with `DISTINCT`.
- In a query with aggregate functions or `GROUP BY`, an item must be a grouped
  column, an aggregate function, or an alias of either.
- Sorting by position (`ORDER BY 1`) and `NULLS FIRST` / `NULLS LAST` are not
  supported.

Without `ORDER BY`, the order of the rows is not defined.

### LIMIT and OFFSET

`LIMIT n` returns at most `n` rows. `OFFSET n` skips the first `n` rows. Each
takes a non-negative integer or a `?` placeholder, and they can be written in
either order. The maximum value is `4294967295` on every target; larger values
are rejected so native and WASM builds accept the same statements.

```sql
SELECT * FROM users ORDER BY id LIMIT 20 OFFSET 40;
SELECT * FROM users ORDER BY id LIMIT ? OFFSET ?;
```

### Execution Order

A `SELECT` is evaluated in this order:

1. `FROM` and `JOIN` build the rows.
2. `WHERE` filters them.
3. `DISTINCT` removes duplicates, or `GROUP BY` forms groups and the
   aggregate functions are computed.
4. `HAVING` filters the groups.
5. `ORDER BY` sorts.
6. `OFFSET` and then `LIMIT` are applied.
7. The select list picks and names the columns.

---

## INSERT

```sql
INSERT INTO users (id, name, email) VALUES (1, 'Alice', NULL);
INSERT INTO users (id, name, email) VALUES (?, ?, ?);
```

`INSERT` adds one row and reports one affected row.

- The column list is required, and each column can be listed once. There must
  be exactly one value per column.
- A column that is not listed is left to the DBMS: a nullable column becomes
  `NULL`, and an `#[autoincrement]` column gets the next number. Leaving out
  any other column is an error.
- Each value is a literal or a placeholder, converted to the type of its
  column. See [Type Conversion](#type-conversion).
- Sanitizers, validators, primary key, unique, and foreign key checks run as
  they do for the typed API.

Inserting several rows in one statement, `INSERT ... SELECT`, and
`DEFAULT VALUES` are not supported.

---

## UPDATE

```sql
UPDATE users SET name = 'Bob', email = NULL WHERE id = 2;
```

`UPDATE` changes the listed columns of every row that matches the `WHERE`
condition, and reports the number of rows written. Zero matching rows is not
an error.

- **`WHERE` is required.** A statement without it is rejected before anything
  is written. To change every row on purpose, write a condition that is true
  for all of them, such as `WHERE id >= 0`.
- Each column can be assigned once. The new value is a literal or a
  placeholder: a value cannot be computed from a column (`SET n = n + 1` is not
  supported).
- Updating a primary key also updates the foreign keys that point at it.

Table aliases, `ORDER BY`, and `LIMIT` are not supported in `UPDATE`.

---

## DELETE

```sql
DELETE FROM posts WHERE user_id = 3;
DELETE FROM users WHERE id = 3 CASCADE;
```

`DELETE` removes every row that matches the `WHERE` condition and reports the
number of rows removed.

- **`WHERE` is required.** A statement without it is rejected before anything
  is deleted.
- An optional keyword after the condition says what happens when other rows
  reference a deleted row through a foreign key:

| Keyword              | Behavior                                    |
| -------------------- | ------------------------------------------- |
| `RESTRICT` (default) | The statement fails and nothing is deleted. |
| `CASCADE`            | The referencing rows are deleted too.       |

Outside a transaction, the reported count includes the rows removed by
`CASCADE`. Inside a transaction, it only counts the rows that matched the
condition.

`CASCADE` and `RESTRICT` are an extension to standard SQL, which has no way to
choose the behavior in the statement.

Table aliases, `ORDER BY`, and `LIMIT` are not supported in `DELETE`.

---

## Transactions

```sql
BEGIN;
UPDATE accounts SET balance = 50 WHERE id = 1;
UPDATE accounts SET balance = 150 WHERE id = 2;
COMMIT;
```

Each statement above is a separate `execute` call.

| Statement                | Effect                                          |
| ------------------------ | ----------------------------------------------- |
| `BEGIN [TRANSACTION]`    | Opens a transaction for the caller              |
| `COMMIT [TRANSACTION]`   | Applies every change of the transaction at once |
| `ROLLBACK [TRANSACTION]` | Discards every change of the transaction        |

- A transaction belongs to the caller that opened it. Each caller has at most
  one, and `BEGIN` inside a transaction is an error.
- Inside a transaction, the caller's statements see its own uncommitted
  changes. Other callers do not see them until `COMMIT`.
- A statement that fails does not end the transaction. The caller decides
  whether to continue, `COMMIT`, or `ROLLBACK`.
- `COMMIT` checks the constraints again while applying the changes. If it
  fails, nothing is applied and the transaction is over.
- `COMMIT` or `ROLLBACK` without a transaction is an error.
- Without a transaction, each statement is applied immediately and atomically.

`START TRANSACTION`, `END`, savepoints, and isolation level settings are not
supported.

---

## Conditions

A condition is used in `WHERE` and `HAVING`. It is built from predicates
combined with `AND`, `OR`, `NOT`, and parentheses.

Every predicate has a column on its left side and literals or placeholders on
its right side. In `HAVING`, the left side can also be an aggregate function.
Comparing two columns with each other, a literal on the left side, arithmetic,
`BETWEEN`, `EXISTS`, and subqueries are not supported.

### Comparison Operators

| Operator     | Meaning               |
| ------------ | --------------------- |
| `=`          | Equal                 |
| `!=` or `<>` | Not equal             |
| `<`          | Less than             |
| `<=`         | Less than or equal    |
| `>`          | Greater than          |
| `>=`         | Greater than or equal |

```sql
SELECT * FROM users WHERE age >= 18 AND name != 'admin';
```

The value is converted to the type of the column before it is compared. Text
is compared character by character, and case matters.

### IN

`IN` is true when the column equals any value of the list. The list needs at
least one value. `NOT IN` is its negation.

```sql
SELECT * FROM users WHERE id IN (1, 2, 3);
SELECT * FROM users WHERE city NOT IN ('Rome', ?);
```

### LIKE

`LIKE` matches a text column against a pattern. `NOT LIKE` is its negation.
The pattern is a string literal or a placeholder.

| Pattern | Matches                  |
| ------- | ------------------------ |
| `%`     | Any number of characters |
| `_`     | Exactly one character    |
| `\%`    | A literal `%`            |
| `\_`    | A literal `_`            |
| `\\`    | A literal `\`            |

```sql
SELECT * FROM users WHERE email LIKE '%@example.com';
SELECT * FROM products WHERE code LIKE 'A_-%';
```

Matching is case-sensitive. A pattern that ends with a single backslash is an
error.

### IS NULL

`IS NULL` is true when the column has no value, and `IS NOT NULL` when it has
one. They are the only way to test for `NULL`.

```sql
SELECT * FROM users WHERE email IS NULL;
SELECT * FROM users WHERE email IS NOT NULL;
```

### AND, OR, NOT

From highest to lowest precedence:

1. Parentheses
2. `NOT`
3. `AND`
4. `OR`

`AND` and `OR` group from left to right.

```sql
-- read as: a = 1 OR (b = 2 AND c = 3)
SELECT * FROM t WHERE a = 1 OR b = 2 AND c = 3;

-- read as: (NOT a = 1) AND b = 2
SELECT * FROM t WHERE NOT a = 1 AND b = 2;

SELECT * FROM t WHERE (a = 1 OR b = 2) AND NOT (c = 3 OR d = 4);
```

### NULL Handling

A condition is either true or false for a row: there is no third "unknown"
result as in standard SQL. For a column that is `NULL`:

- `=`, `IN`, and `LIKE` are false.
- `!=`, `NOT IN`, `NOT LIKE`, and `NOT (...)` around a false predicate are
  true. `WHERE email != 'a@example.com'` therefore also returns the rows where
  `email` is `NULL`.
- The result of `<`, `<=`, `>`, and `>=` is not defined.

Add `AND column IS NOT NULL` when rows without a value must be left out. The
position of `NULL` values in an `ORDER BY` is not defined either.

---

## Aggregate Functions

| Function        | Result                                    | Result type        | On no values |
| --------------- | ----------------------------------------- | ------------------ | ------------ |
| `COUNT(*)`      | Number of rows                            | `Uint64`           | `0`          |
| `COUNT(column)` | Number of rows where `column` is not null | `Uint64`           | `0`          |
| `SUM(column)`   | Sum of the non-null values                | `Decimal`          | `NULL`       |
| `AVG(column)`   | Mean of the non-null values               | `Decimal`          | `NULL`       |
| `MIN(column)`   | Smallest non-null value                   | Type of the column | `NULL`       |
| `MAX(column)`   | Largest non-null value                    | Type of the column | `NULL`       |

- `SUM` and `AVG` need a numeric column: any integer type or `Decimal`.
- The argument is a column. `COUNT(DISTINCT column)`, expressions, and nested
  aggregates are not supported.
- In the result, an aggregate column is named after the function call with the
  plain column name, in upper case: `COUNT(*)`, `SUM(price)`. Use `AS` to give
  it another name.
- A literal compared with an aggregate in `HAVING` is converted to the result
  type of the aggregate.

```sql
SELECT COUNT(*), COUNT(email), AVG(age), MIN(age), MAX(age) FROM users;
```

---

## Type Conversion

A value is always used with a column: it is assigned to it, or compared with
it. The value is converted to the exact type of that column. A value that
cannot be converted is an error, and the statement does nothing.

### Literals by Column Type

| Column type                           | Integer          | Decimal | String                              | `TRUE` / `FALSE` |
| ------------------------------------- | ---------------- | ------- | ----------------------------------- | ---------------- |
| `Int8`, `Int16`, `Int32`, `Int64`     | Yes, if in range | No      | No                                  | No               |
| `Uint8`, `Uint16`, `Uint32`, `Uint64` | Yes, if in range | No      | No                                  | No               |
| `Decimal`                             | Yes              | Yes     | No                                  | No               |
| `Text`                                | No               | No      | Yes                                 | No               |
| `Boolean`                             | No               | No      | No                                  | Yes              |
| `Date`                                | No               | No      | [Date format](#string-formats)      | No               |
| `DateTime`                            | No               | No      | [Date-time format](#string-formats) | No               |
| `Uuid`                                | No               | No      | [UUID format](#string-formats)      | No               |
| `Blob`                                | No               | No      | [Hexadecimal](#string-formats)      | No               |
| `Json`                                | No               | No      | JSON text                           | No               |
| Custom types                          | No               | No      | No                                  | No               |

- "No" is reported as a type mismatch. A value of the right kind that does not
  fit, such as `300` for a `Uint8` column or `'2026-02-30'` for a `Date`
  column, is reported as an invalid value.
- `NULL` can be written for any column type. Whether the column accepts it is
  checked by the DBMS.
- `Nullable<T>` columns convert like `T`.
- A [custom data type](../guides/custom-data-types.md) has no literal form. Its
  values are passed as [parameters](#parameters).

### String Formats

| Column type | Format                                                        | Examples                                                  |
| ----------- | ------------------------------------------------------------- | --------------------------------------------------------- |
| `Date`      | `YYYY-MM-DD`                                                  | `'2026-04-24'`                                            |
| `DateTime`  | `YYYY-MM-DD`, `T` or a space, `HH:MM:SS`, then optional parts | `'2026-04-24T10:30:00Z'`, `'2026-04-24 10:30:00.5+02:00'` |
| `Uuid`      | 32 hexadecimal digits in groups of 8-4-4-4-12                 | `'550e8400-e29b-41d4-a716-446655440000'`                  |
| `Blob`      | Hexadecimal digits, two per byte, optional `0x` prefix        | `'deadbeef'`, `'0x00FF'`, `''`                            |
| `Json`      | Any JSON document                                             | `'{"tags": ["a", "b"]}'`, `'null'`                        |

- A date must exist in the calendar: `'2025-02-29'` is rejected.
- The optional parts of a date-time are a fraction of a second with one to six
  digits (`.5`, `.123456`), followed by a time zone: `Z` for UTC, or an offset
  such as `+02:00` or `-08:00`. Without a time zone, the value is UTC.
- Hexadecimal digits can be upper or lower case.

```sql
INSERT INTO events (day, at, token, payload, meta)
VALUES ('2026-04-24', '2026-04-24T10:30:00Z',
        '550e8400-e29b-41d4-a716-446655440000', 'deadbeef',
        '{"kind": "signup"}');

SELECT * FROM events WHERE day >= '2026-01-01';
```

---

## Parameters

Parameters are the values passed to `SqlEngine::execute` next to the SQL text.
There must be exactly one per `?` placeholder.

A parameter is never read as SQL: its content cannot change the statement.
Use parameters for every value that comes from outside the program.

A parameter is converted to the type of its column like a literal:

| Parameter value                                               | Converted as                                                         |
| ------------------------------------------------------------- | -------------------------------------------------------------------- |
| Any integer type                                              | An integer literal: any integer column or `Decimal`, if in range     |
| `Text`                                                        | A string literal: `Text`, `Date`, `DateTime`, `Uuid`, `Blob`, `Json` |
| `Boolean`                                                     | `TRUE` / `FALSE`                                                     |
| `Null`                                                        | `NULL`                                                               |
| `Decimal`, `Date`, `DateTime`, `Uuid`, `Blob`, `Json`, custom | Used as it is; the column must have the same type                    |

For example, a `Value::Uint64(5)` parameter can be compared with a `Uint8`
column, and a `Value::Text("2026-04-24")` parameter with a `Date` column.

A `Null` parameter is accepted in `INSERT` and `UPDATE ... SET`, and rejected
in a comparison or an `IN` list. A `LIMIT` or `OFFSET` parameter must be a
non-negative integer. A `LIKE` parameter must be `Text`.

---

## Results

`SqlEngine::execute` returns a `SqlResult`:

| Variant           | Returned by                  | Content                    |
| ----------------- | ---------------------------- | -------------------------- |
| `Rows(rows)`      | `SELECT`                     | The rows, possibly none    |
| `RowsAffected(n)` | `INSERT`, `UPDATE`, `DELETE` | The number of rows written |
| `TxBegin`         | `BEGIN`                      |                            |
| `TxCommit`        | `COMMIT`                     |                            |
| `TxRollback`      | `ROLLBACK`                   |                            |

A row is a list of `(JoinColumnDef, Value)` pairs, one per column of the
select list, in order. The column definition describes the column:

| Field         | Content                                                            |
| ------------- | ------------------------------------------------------------------ |
| `name`        | The alias if one was given, otherwise the column or aggregate name |
| `table`       | The table the column comes from in a join; `None` otherwise        |
| `data_type`   | The type of the value                                              |
| `nullable`    | Whether the value can be `NULL`                                    |
| `primary_key` | Whether the column is the primary key of its table                 |
| `foreign_key` | The foreign key of the column, if it has one                       |

`table` is always the real table name, even when the statement uses an alias.

---

## Errors

A failed statement returns a `SqlError`. Syntax errors carry the line and
column of the offending token, both starting at 1. The variants are described
in the [Errors Reference](./errors.md#sql-errors).

---

## Limits

| Limit                                    | Value                  |
| ---------------------------------------- | ---------------------- |
| Statements per call                      | 1                      |
| Terms in the conditions of one statement | 256                    |
| Largest unsigned integer literal         | `18446744073709551615` |

A term is one predicate, one `AND`, `OR`, or `NOT`, or one pair of
parentheses. The conditions of `WHERE` and `HAVING` are counted together. The
values of an `IN` list are not counted, so a long list of alternatives is best
written with `IN`.

---

## Reserved Words

These words cannot be used as names unless they are written between double
quotes. Some are reserved for clauses that are not supported yet.

```text
AND          AS           ASC          BEGIN        BY
CASCADE      COMMIT       CROSS        DELETE       DESC
DISTINCT     EXCEPT       FALSE        FROM         FULL
GROUP        HAVING       IN           INNER        INSERT
INTERSECT    INTO         IS           JOIN         LEFT
LIKE         LIMIT        NATURAL      NOT          NULL
OFFSET       ON           OR           ORDER        OUTER
RESTRICT     RIGHT        ROLLBACK     SELECT       SET
TRANSACTION  TRUE         UNION        UPDATE       USING
VALUES       WHERE
```

---

## Not Supported

- Schema statements: `CREATE`, `ALTER`, `DROP`, `TRUNCATE`. Use
  [migrations](./migrations.md).
- Subqueries, `UNION`, `INTERSECT`, `EXCEPT`, and `WITH`.
- Expressions and scalar functions: arithmetic, string concatenation, `CASE`,
  `COALESCE`, `LOWER`, and so on.
- Comparisons between two columns, `BETWEEN`, and `EXISTS`.
- Self-joins, `CROSS JOIN`, `NATURAL JOIN`, and `USING`.
- `DISTINCT` or aggregate functions together with a join.
- `COUNT(DISTINCT ...)`.
- Multi-row `INSERT`, `INSERT ... SELECT`, and `RETURNING`.
- Several statements in one call.
- Named and numbered placeholders.
- Filters on the content of `Json` columns. Use the
  [JSON filters](./json.md) of the query builder.
- Eager loading of relations. Use a `JOIN`, or the
  [query builder](./query.md).
