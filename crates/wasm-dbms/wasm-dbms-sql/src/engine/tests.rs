use wasm_dbms::prelude::{DbmsContext, WasmDbmsDatabase};
use wasm_dbms_api::prelude::{
    AggregatedRow, Blob, CandidDataTypeKind, Database as _, Date, DateTime, DbmsError, Decimal,
    Encode as _, JoinColumnDef, Json, MigrationError, Query, QueryError, SqlError, SqlResult,
    TableSchema as _, TransactionError, Uuid, Value,
};
use wasm_dbms_memory::prelude::HeapMemoryProvider;

use super::{SqlEngine, aggregate_row, join_row, table_row};
use crate::planner::{AggregateOutput, AggregateSource, OutputColumn};
use crate::test_schema::{TestSchema, User};

const ALICE: &[u8] = b"alice";
const BOB: &[u8] = b"bob";

/// A fresh database with its SQL engine.
struct TestDb {
    ctx: DbmsContext<HeapMemoryProvider>,
    engine: SqlEngine<TestSchema>,
}

impl TestDb {
    fn new() -> Self {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        TestSchema::register_tables(&ctx).expect("failed to register tables");
        Self {
            ctx,
            engine: SqlEngine::new(TestSchema),
        }
    }

    /// A database with a few users, posts, comments, and sales.
    fn seeded() -> Self {
        let db = Self::new();
        for sql in [
            "INSERT INTO users (id, name, email, age) VALUES (1, 'Alice', 'alice@example.com', 30)",
            "INSERT INTO users (id, name, email, age) VALUES (2, 'Bob', NULL, 25)",
            "INSERT INTO users (id, name, email, age) VALUES (3, 'Carol', 'carol@example.com', 41)",
            "INSERT INTO posts (id, title, published, user_id) VALUES (10, 'Hello', TRUE, 1)",
            "INSERT INTO posts (id, title, published, user_id) VALUES (11, 'Draft', FALSE, 1)",
            "INSERT INTO posts (id, title, published, user_id) VALUES (12, 'News', TRUE, 2)",
            "INSERT INTO comments (id, body, post_id) VALUES (100, 'Nice', 10)",
            "INSERT INTO comments (id, body, post_id) VALUES (101, 'First', 12)",
            "INSERT INTO sales (id, category, region, price, quantity, bonus) VALUES (1, 'book', 'eu', 10.50, 2, 5)",
            "INSERT INTO sales (id, category, region, price, quantity, bonus) VALUES (2, 'book', 'us', 20.00, 1, NULL)",
            "INSERT INTO sales (id, category, region, price, quantity, bonus) VALUES (3, 'game', 'eu', 60.00, 4, 9)",
            "INSERT INTO sales (id, category, region, price, quantity, bonus) VALUES (4, 'game', 'eu', 40.00, 6, NULL)",
            "INSERT INTO sales (id, category, region, price, quantity, bonus) VALUES (5, 'toy', 'us', 5.25, 10, 1)",
        ] {
            assert_eq!(db.run(sql), SqlResult::RowsAffected(1), "{sql}");
        }
        db
    }

    fn execute(&self, caller: &[u8], sql: &str, params: &[Value]) -> Result<SqlResult, SqlError> {
        self.engine.execute(&self.ctx, caller, sql, params)
    }

    /// Runs a statement without parameters as Alice; it must succeed.
    fn run(&self, sql: &str) -> SqlResult {
        self.execute(ALICE, sql, &[])
            .unwrap_or_else(|error| panic!("`{sql}` failed: {error}"))
    }

    /// Runs a statement without parameters as Alice; it must fail.
    fn fail(&self, sql: &str) -> SqlError {
        self.execute(ALICE, sql, &[])
            .expect_err("statement must fail")
    }

    /// Runs a `SELECT` and returns `(column name, value)` pairs per row.
    fn query_as(&self, caller: &[u8], sql: &str, params: &[Value]) -> Vec<Vec<(String, Value)>> {
        match self.execute(caller, sql, params) {
            Ok(SqlResult::Rows(rows)) => rows
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|(def, value)| (def.name, value))
                        .collect()
                })
                .collect(),
            other => panic!("`{sql}` did not return rows: {other:?}"),
        }
    }

    fn query(&self, sql: &str) -> Vec<Vec<(String, Value)>> {
        self.query_as(ALICE, sql, &[])
    }

    /// Runs a `SELECT` and returns only the values of each row.
    fn values(&self, sql: &str) -> Vec<Vec<Value>> {
        self.query(sql)
            .into_iter()
            .map(|row| row.into_iter().map(|(_, value)| value).collect())
            .collect()
    }

    /// Number of rows in `table`, read through the programmatic API.
    fn count(&self, table: &str) -> usize {
        WasmDbmsDatabase::oneshot(&self.ctx, TestSchema)
            .select_raw(table, Query::builder().all().build())
            .expect("select failed")
            .len()
    }
}

fn text(value: &str) -> Value {
    Value::from(value)
}

fn decimal(value: &str) -> Value {
    Value::Decimal(Decimal(value.parse().expect("valid decimal")))
}

fn named(name: &str, value: Value) -> (String, Value) {
    (name.to_string(), value)
}

fn query_error(error: SqlError) -> QueryError {
    match error {
        SqlError::Runtime(DbmsError::Query(error)) => error,
        other => panic!("expected a runtime query error, got {other:?}"),
    }
}

// -- SELECT -----------------------------------------------------------------

#[test]
fn test_select_star_returns_every_column_in_table_order() {
    let db = TestDb::seeded();
    let SqlResult::Rows(rows) = db.run("SELECT * FROM users WHERE id = 2") else {
        panic!("expected rows");
    };
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    let names: Vec<&str> = row.iter().map(|(def, _)| def.name.as_str()).collect();
    assert_eq!(names, vec!["id", "name", "email", "age"]);
    let values: Vec<Value> = row.iter().map(|(_, value)| value.clone()).collect();
    assert_eq!(
        values,
        vec![
            Value::from(2u32),
            text("Bob"),
            Value::Null,
            Value::from(25u8)
        ]
    );

    let (id, _) = &row[0];
    assert_eq!(id.table, None);
    assert_eq!(id.data_type, CandidDataTypeKind::Uint32);
    assert!(id.primary_key);
    assert!(!id.nullable);
    let (email, _) = &row[2];
    assert_eq!(email.data_type, CandidDataTypeKind::Text);
    assert!(email.nullable);
    assert!(!email.primary_key);
}

#[test]
fn test_select_list_sets_column_order_and_names() {
    let db = TestDb::seeded();
    assert_eq!(
        db.query("SELECT name AS who, id, id AS key FROM users WHERE id = 1"),
        vec![vec![
            named("who", text("Alice")),
            named("id", Value::from(1u32)),
            named("key", Value::from(1u32)),
        ]]
    );
}

#[test]
fn test_select_where_order_limit_offset() {
    let db = TestDb::seeded();
    assert_eq!(
        db.values("SELECT name FROM users WHERE age >= 25 ORDER BY age DESC"),
        vec![vec![text("Carol")], vec![text("Alice")], vec![text("Bob")]]
    );
    assert_eq!(
        db.values("SELECT name FROM users ORDER BY name LIMIT 2"),
        vec![vec![text("Alice")], vec![text("Bob")]]
    );
    assert_eq!(
        db.values("SELECT name FROM users ORDER BY name LIMIT 1 OFFSET 1"),
        vec![vec![text("Bob")]]
    );
    assert_eq!(
        db.values("SELECT name FROM users ORDER BY name OFFSET 2"),
        vec![vec![text("Carol")]]
    );
    assert_eq!(
        db.values("SELECT name FROM users ORDER BY name LIMIT 0"),
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn test_select_zero_limit_without_order_returns_no_rows() {
    let db = TestDb::seeded();
    assert_eq!(
        db.values("SELECT id FROM users LIMIT 0"),
        Vec::<Vec<Value>>::new()
    );
    assert_eq!(
        db.query_as(ALICE, "SELECT id FROM users LIMIT ?", &[Value::from(0u8)]),
        Vec::<Vec<(String, Value)>>::new()
    );
}

#[test]
fn test_select_maximum_limit_returns_available_rows() {
    let empty = TestDb::new();
    assert_eq!(
        empty.values("SELECT id FROM users LIMIT 4294967295"),
        Vec::<Vec<Value>>::new()
    );

    let seeded = TestDb::seeded();
    assert_eq!(
        seeded.values("SELECT id FROM users LIMIT 4294967295").len(),
        3
    );
}

#[test]
fn test_select_can_sort_and_filter_by_columns_that_are_not_selected() {
    let db = TestDb::seeded();
    assert_eq!(
        db.values("SELECT name FROM users WHERE email IS NOT NULL ORDER BY age DESC"),
        vec![vec![text("Carol")], vec![text("Alice")]]
    );
}

#[test]
fn test_select_predicates() {
    let db = TestDb::seeded();
    let names = |sql: &str| -> Vec<Value> { db.values(sql).into_iter().flatten().collect() };
    assert_eq!(
        names("SELECT name FROM users WHERE name LIKE 'A%' OR name LIKE '%ol' ORDER BY id"),
        vec![text("Alice"), text("Carol")]
    );
    assert_eq!(
        names("SELECT name FROM users WHERE name NOT LIKE '_ob' ORDER BY id"),
        vec![text("Alice"), text("Carol")]
    );
    assert_eq!(
        names("SELECT name FROM users WHERE id IN (1, 3, 99) ORDER BY id"),
        vec![text("Alice"), text("Carol")]
    );
    assert_eq!(
        names("SELECT name FROM users WHERE id NOT IN (1, 3) ORDER BY id"),
        vec![text("Bob")]
    );
    assert_eq!(
        names("SELECT name FROM users WHERE email IS NULL"),
        vec![text("Bob")]
    );
    assert_eq!(
        names("SELECT name FROM users WHERE NOT (age < 30 OR age > 40) ORDER BY id"),
        vec![text("Alice")]
    );
    assert_eq!(
        names("SELECT name FROM users WHERE id != 2 AND id <> 3"),
        vec![text("Alice")]
    );
    assert_eq!(
        names("SELECT name FROM users WHERE age > 25 AND age <= 41 AND id < 3"),
        vec![text("Alice")]
    );
}

#[test]
fn test_select_with_parameters() {
    let db = TestDb::seeded();
    assert_eq!(
        db.query_as(
            ALICE,
            "SELECT name FROM users WHERE age > ? AND name != ? ORDER BY name LIMIT ? OFFSET ?",
            &[
                Value::from(20i64),
                text("Bob"),
                Value::from(5u32),
                Value::from(1u8)
            ],
        ),
        vec![vec![named("name", text("Carol"))]]
    );
}

#[test]
fn test_select_from_empty_table() {
    let db = TestDb::new();
    assert_eq!(db.run("SELECT * FROM users"), SqlResult::Rows(Vec::new()));
}

#[test]
fn test_select_distinct() {
    let db = TestDb::seeded();
    assert_eq!(
        db.values("SELECT DISTINCT region FROM sales ORDER BY region"),
        vec![vec![text("eu")], vec![text("us")]]
    );
    assert_eq!(
        db.values("SELECT DISTINCT category, region FROM sales ORDER BY category, region"),
        vec![
            vec![text("book"), text("eu")],
            vec![text("book"), text("us")],
            vec![text("game"), text("eu")],
            vec![text("toy"), text("us")],
        ]
    );
    assert_eq!(db.values("SELECT DISTINCT * FROM sales").len(), 5);
    assert_eq!(
        db.values("SELECT DISTINCT category FROM sales ORDER BY category DESC LIMIT 1 OFFSET 1"),
        vec![vec![text("game")]]
    );
}

// -- JOIN -------------------------------------------------------------------

#[test]
fn test_inner_join() {
    let db = TestDb::seeded();
    let SqlResult::Rows(rows) = db.run(
        "SELECT users.name, posts.title AS headline FROM users JOIN posts ON users.id = \
         posts.user_id WHERE posts.published = TRUE ORDER BY posts.title",
    ) else {
        panic!("expected rows");
    };
    let flat: Vec<Vec<(Option<&str>, &str, &Value)>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|(def, value)| (def.table.as_deref(), def.name.as_str(), value))
                .collect()
        })
        .collect();
    assert_eq!(
        flat,
        vec![
            vec![
                (Some("users"), "name", &text("Alice")),
                (Some("posts"), "headline", &text("Hello"))
            ],
            vec![
                (Some("users"), "name", &text("Bob")),
                (Some("posts"), "headline", &text("News"))
            ],
        ]
    );
}

#[test]
fn test_join_star_returns_the_columns_of_every_table() {
    let db = TestDb::seeded();
    let SqlResult::Rows(rows) =
        db.run("SELECT * FROM users JOIN posts ON users.id = posts.user_id WHERE posts.id = 12")
    else {
        panic!("expected rows");
    };
    assert_eq!(rows.len(), 1);
    let columns: Vec<(Option<&str>, &str)> = rows[0]
        .iter()
        .map(|(def, _)| (def.table.as_deref(), def.name.as_str()))
        .collect();
    assert_eq!(
        columns,
        vec![
            (Some("users"), "id"),
            (Some("users"), "name"),
            (Some("users"), "email"),
            (Some("users"), "age"),
            (Some("posts"), "id"),
            (Some("posts"), "title"),
            (Some("posts"), "published"),
            (Some("posts"), "user_id"),
        ]
    );
}

#[test]
fn test_left_join_pads_missing_rows_with_null() {
    let db = TestDb::seeded();
    assert_eq!(
        db.values(
            "SELECT u.name, p.title FROM users u LEFT JOIN posts p ON u.id = p.user_id ORDER BY \
             u.id, p.id"
        ),
        vec![
            vec![text("Alice"), text("Hello")],
            vec![text("Alice"), text("Draft")],
            vec![text("Bob"), text("News")],
            vec![text("Carol"), Value::Null],
        ]
    );
}

#[test]
fn test_right_and_full_join() {
    let db = TestDb::seeded();
    // every post has a user, so a right join equals the inner join
    assert_eq!(
        db.values(
            "SELECT users.name FROM users RIGHT JOIN posts ON users.id = posts.user_id ORDER BY \
             posts.id"
        ),
        vec![vec![text("Alice")], vec![text("Alice")], vec![text("Bob")]]
    );
    assert_eq!(
        db.values("SELECT users.name FROM users FULL JOIN posts ON users.id = posts.user_id")
            .len(),
        4
    );
}

#[test]
fn test_chained_join_does_not_match_outer_join_null_padding() {
    let db = TestDb::seeded();
    let rows = db.values(
        "SELECT users.name, sales.id FROM users LEFT JOIN posts ON users.id = posts.user_id \
         JOIN sales ON posts.user_id = sales.bonus WHERE users.id = 3",
    );
    assert!(rows.is_empty());
}

#[test]
fn test_three_table_join() {
    let db = TestDb::seeded();
    assert_eq!(
        db.values(
            "SELECT users.name, posts.title, comments.body FROM users JOIN posts ON users.id = \
             posts.user_id JOIN comments ON comments.post_id = posts.id ORDER BY comments.id"
        ),
        vec![
            vec![text("Alice"), text("Hello"), text("Nice")],
            vec![text("Bob"), text("News"), text("First")],
        ]
    );
}

#[test]
fn test_join_with_limit_and_parameters() {
    let db = TestDb::seeded();
    assert_eq!(
        db.query_as(
            ALICE,
            "SELECT posts.title FROM users JOIN posts ON users.id = posts.user_id WHERE \
             users.name = ? ORDER BY posts.title DESC LIMIT ?",
            &[text("Alice"), Value::from(1u32)],
        ),
        vec![vec![named("title", text("Hello"))]]
    );
}

// -- aggregates -------------------------------------------------------------

#[test]
fn test_aggregates_over_the_whole_table() {
    let db = TestDb::seeded();
    assert_eq!(
        db.query(
            "SELECT COUNT(*), COUNT(bonus) AS with_bonus, SUM(quantity), MIN(price), MAX(price) \
             AS top FROM sales"
        ),
        vec![vec![
            named("COUNT(*)", Value::from(5u64)),
            named("with_bonus", Value::from(3u64)),
            named("SUM(quantity)", decimal("23")),
            named("MIN(price)", decimal("5.25")),
            named("top", decimal("60.00")),
        ]]
    );
}

#[test]
fn test_aggregate_column_definitions() {
    let db = TestDb::seeded();
    let SqlResult::Rows(rows) =
        db.run("SELECT category, COUNT(*), AVG(price) FROM sales GROUP BY category LIMIT 1")
    else {
        panic!("expected rows");
    };
    let definitions: Vec<(&str, &CandidDataTypeKind, bool, bool)> = rows[0]
        .iter()
        .map(|(def, _)| {
            (
                def.name.as_str(),
                &def.data_type,
                def.nullable,
                def.table.is_none(),
            )
        })
        .collect();
    assert_eq!(
        definitions,
        vec![
            ("category", &CandidDataTypeKind::Text, false, true),
            ("COUNT(*)", &CandidDataTypeKind::Uint64, false, true),
            ("AVG(price)", &CandidDataTypeKind::Decimal, true, true),
        ]
    );
}

#[test]
fn test_group_by_having_order_by() {
    let db = TestDb::seeded();
    assert_eq!(
        db.values(
            "SELECT category, COUNT(*) AS n, SUM(quantity) FROM sales GROUP BY category ORDER BY \
             category"
        ),
        vec![
            vec![text("book"), Value::from(2u64), decimal("3")],
            vec![text("game"), Value::from(2u64), decimal("10")],
            vec![text("toy"), Value::from(1u64), decimal("10")],
        ]
    );
    assert_eq!(
        db.values(
            "SELECT category FROM sales GROUP BY category HAVING COUNT(*) >= 2 AND SUM(quantity) \
             > 3 ORDER BY category"
        ),
        vec![vec![text("game")]]
    );
    assert_eq!(
        db.values(
            "SELECT category, SUM(quantity) AS total FROM sales GROUP BY category ORDER BY total \
             DESC, category DESC LIMIT 2"
        ),
        vec![
            vec![text("toy"), decimal("10")],
            vec![text("game"), decimal("10")],
        ]
    );
    assert_eq!(
        db.values(
            "SELECT region, category, MAX(price) FROM sales WHERE quantity > 1 GROUP BY region, \
             category ORDER BY MAX(price) DESC"
        ),
        vec![
            vec![text("eu"), text("game"), decimal("60.00")],
            vec![text("eu"), text("book"), decimal("10.50")],
            vec![text("us"), text("toy"), decimal("5.25")],
        ]
    );
}

#[test]
fn test_aggregate_with_parameters() {
    let db = TestDb::seeded();
    assert_eq!(
        db.query_as(
            ALICE,
            "SELECT category FROM sales WHERE region = ? GROUP BY category HAVING COUNT(*) > ? \
             ORDER BY category LIMIT ?",
            &[text("eu"), Value::from(1u32), Value::from(10u32)],
        ),
        vec![vec![named("category", text("game"))]]
    );
}

#[test]
fn test_aggregates_over_no_rows() {
    let db = TestDb::new();
    assert_eq!(
        db.values("SELECT COUNT(*), SUM(quantity), MAX(price) FROM sales"),
        vec![vec![Value::from(0u64), Value::Null, Value::Null]]
    );
    assert_eq!(
        db.values("SELECT category, COUNT(*) FROM sales GROUP BY category"),
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn test_sum_of_a_text_column_is_a_runtime_error() {
    let db = TestDb::seeded();
    assert!(matches!(
        query_error(db.fail("SELECT SUM(category) FROM sales")),
        QueryError::InvalidQuery(_)
    ));
}

// -- INSERT -----------------------------------------------------------------

#[test]
fn test_insert_is_visible_through_the_programmatic_api() {
    let db = TestDb::new();
    assert_eq!(
        db.execute(
            ALICE,
            "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
            &[Value::from(7u32), text("Dan"), Value::from(50u8)],
        )
        .unwrap(),
        SqlResult::RowsAffected(1)
    );
    let users = WasmDbmsDatabase::oneshot(&db.ctx, TestSchema)
        .select::<User>(Query::builder().all().build())
        .expect("select failed");
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].id, Some(7u32.into()));
    assert_eq!(users[0].name, Some("Dan".into()));
    // the omitted nullable column is NULL
    assert_eq!(
        db.values("SELECT email FROM users WHERE id = 7"),
        vec![vec![Value::Null]]
    );
}

#[test]
fn test_insert_constraint_violations_are_runtime_errors() {
    let db = TestDb::seeded();
    assert!(matches!(
        query_error(db.fail("INSERT INTO users (id, name, age) VALUES (1, 'Dup', 1)")),
        QueryError::PrimaryKeyConflict
    ));
    assert!(matches!(
        query_error(
            db.fail(
                "INSERT INTO posts (id, title, published, user_id) VALUES (50, 'x', TRUE, 999)"
            )
        ),
        QueryError::BrokenForeignKeyReference { .. }
    ));
    assert!(matches!(
        query_error(db.fail("INSERT INTO users (id, name) VALUES (9, 'NoAge')")),
        QueryError::MissingNonNullableField(column) if column == "age"
    ));
    assert_eq!(db.count("users"), 3);
    assert_eq!(db.count("posts"), 3);
}

#[test]
fn test_insert_typed_values_from_literals() {
    let db = TestDb::new();
    db.run(
        "INSERT INTO events (day, at, token, payload, meta, delta) VALUES ('2026-04-24', \
         '2026-04-24T10:30:00.5+02:00', '550e8400-e29b-41d4-a716-446655440000', 'DEADbeef', \
         '{\"kind\": \"signup\"}', -9000000000)",
    );
    let rows = db.query("SELECT * FROM events");
    assert_eq!(rows.len(), 1);
    let uuid_bytes = [
        0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44, 0x00,
        0x00,
    ];
    assert_eq!(
        rows[0],
        vec![
            // the autoincrement key was generated
            named("id", Value::from(1u64)),
            named(
                "day",
                Value::Date(Date {
                    year: 2026,
                    month: 4,
                    day: 24
                })
            ),
            named(
                "at",
                Value::DateTime(DateTime {
                    year: 2026,
                    month: 4,
                    day: 24,
                    hour: 10,
                    minute: 30,
                    second: 0,
                    microsecond: 500_000,
                    timezone_offset_minutes: 120,
                })
            ),
            named("payload", Value::Blob(Blob(vec![0xde, 0xad, 0xbe, 0xef]))),
            named(
                "meta",
                Value::Json("{\"kind\": \"signup\"}".parse::<Json>().unwrap())
            ),
            named("delta", Value::from(-9_000_000_000i64)),
            named(
                "token",
                Value::Uuid(Uuid::decode(std::borrow::Cow::Owned(uuid_bytes.to_vec())).unwrap())
            ),
        ]
    );
}

#[test]
fn test_typed_values_from_parameters_and_in_filters() {
    let db = TestDb::new();
    let day = Value::Date(Date {
        year: 2025,
        month: 12,
        day: 31,
    });
    let at = Value::DateTime(DateTime {
        year: 2025,
        month: 12,
        day: 31,
        hour: 23,
        minute: 59,
        second: 59,
        microsecond: 0,
        timezone_offset_minutes: 0,
    });
    let token = Value::Uuid(Uuid::decode(std::borrow::Cow::Owned(vec![7; 16])).unwrap());
    let payload = Value::Blob(Blob(vec![1, 2, 3]));
    let meta = Value::Json("[1, 2]".parse::<Json>().unwrap());
    db.execute(
        ALICE,
        "INSERT INTO events (day, at, token, payload, meta, delta) VALUES (?, ?, ?, ?, ?, ?)",
        &[
            day.clone(),
            at.clone(),
            token.clone(),
            payload.clone(),
            meta,
            Value::from(5u8),
        ],
    )
    .unwrap();
    db.run(
        "INSERT INTO events (day, at, token, payload, meta, delta) VALUES ('2026-06-01', \
         '2026-06-01 08:00:00', '00000000-0000-0000-0000-000000000001', '', 'null', 6)",
    );

    let ids = |sql: &str, params: &[Value]| -> Vec<Value> {
        db.query_as(ALICE, sql, params)
            .into_iter()
            .flatten()
            .map(|(_, value)| value)
            .collect()
    };
    let first = vec![Value::from(1u64)];
    let second = vec![Value::from(2u64)];
    assert_eq!(
        ids("SELECT id FROM events WHERE day < '2026-01-01'", &[]),
        first
    );
    assert_eq!(ids("SELECT id FROM events WHERE day = ?", &[day]), first);
    assert_eq!(
        ids(
            "SELECT id FROM events WHERE at >= '2026-01-01T00:00:00Z'",
            &[]
        ),
        second
    );
    assert_eq!(ids("SELECT id FROM events WHERE at = ?", &[at]), first);
    assert_eq!(
        ids(
            "SELECT id FROM events WHERE token = '07070707-0707-0707-0707-070707070707'",
            &[]
        ),
        first
    );
    assert_eq!(
        ids("SELECT id FROM events WHERE token != ?", &[token]),
        second
    );
    assert_eq!(
        ids("SELECT id FROM events WHERE payload = '010203'", &[]),
        first
    );
    assert_eq!(
        ids("SELECT id FROM events WHERE payload = ?", &[payload]),
        first
    );
    assert_eq!(
        ids("SELECT id FROM events WHERE delta IN (6, 7)", &[]),
        second
    );
    // a text parameter is converted like a string literal
    assert_eq!(
        ids("SELECT id FROM events WHERE day = ?", &[text("2026-06-01")]),
        second
    );
}

#[test]
fn test_value_errors_are_reported_before_anything_is_written() {
    let db = TestDb::new();
    assert!(matches!(
        db.fail("INSERT INTO users (id, name, age) VALUES (1, 'A', 256)"),
        SqlError::InvalidLiteral { column, expected: CandidDataTypeKind::Uint8, .. }
            if column == "age"
    ));
    assert!(matches!(
        db.fail("INSERT INTO users (id, name, age) VALUES ('1', 'A', 2)"),
        SqlError::TypeMismatch { column, expected: CandidDataTypeKind::Uint32, got }
            if column == "id" && got == "String"
    ));
    assert!(matches!(
        db.fail(
            "INSERT INTO events (day, at, token, payload, meta, delta) VALUES ('2026-02-30', \
             '2026-01-01T00:00:00Z', '00000000-0000-0000-0000-000000000001', '', '{}', 1)"
        ),
        SqlError::InvalidLiteral { column, expected: CandidDataTypeKind::Date, .. }
            if column == "day"
    ));
    assert_eq!(db.count("users"), 0);
    assert_eq!(db.count("events"), 0);
}

#[test]
fn test_null_for_a_required_column_is_rejected() {
    let db = TestDb::seeded();
    assert!(matches!(
        query_error(db.fail("INSERT INTO users (id, name, age) VALUES (9, NULL, 1)")),
        QueryError::MissingNonNullableField(column) if column == "name"
    ));
    assert!(matches!(
        query_error(db.fail("UPDATE users SET name = NULL WHERE id = 1")),
        QueryError::InvalidQuery(_)
    ));
    assert_eq!(db.count("users"), 3);
    assert_eq!(
        db.values("SELECT name FROM users WHERE id = 1"),
        vec![vec![text("Alice")]]
    );
}

// -- UPDATE -----------------------------------------------------------------

#[test]
fn test_update_with_where() {
    let db = TestDb::seeded();
    assert_eq!(
        db.run("UPDATE users SET name = 'Bobby', email = 'bob@example.com' WHERE id = 2"),
        SqlResult::RowsAffected(1)
    );
    assert_eq!(
        db.values("SELECT name, email, age FROM users WHERE id = 2"),
        vec![vec![
            text("Bobby"),
            text("bob@example.com"),
            Value::from(25u8)
        ]]
    );
    // several rows, with a parameter
    assert_eq!(
        db.execute(
            ALICE,
            "UPDATE users SET age = ? WHERE age >= 30",
            &[Value::from(99u8)]
        )
        .unwrap(),
        SqlResult::RowsAffected(2)
    );
    assert_eq!(
        db.values("SELECT age FROM users ORDER BY id"),
        vec![
            vec![Value::from(99u8)],
            vec![Value::from(25u8)],
            vec![Value::from(99u8)]
        ]
    );
    // no row matches
    assert_eq!(
        db.run("UPDATE users SET age = 1 WHERE id = 404"),
        SqlResult::RowsAffected(0)
    );
    // setting a nullable column to NULL
    assert_eq!(
        db.run("UPDATE users SET email = NULL WHERE id = 1"),
        SqlResult::RowsAffected(1)
    );
    assert_eq!(
        db.values("SELECT email FROM users WHERE id = 1"),
        vec![vec![Value::Null]]
    );
}

#[test]
fn test_update_without_where_is_rejected_and_changes_nothing() {
    let db = TestDb::seeded();
    assert!(matches!(
        db.fail("UPDATE users SET age = 0"),
        SqlError::MissingWhereClause
    ));
    assert_eq!(
        db.values("SELECT age FROM users ORDER BY id"),
        vec![
            vec![Value::from(30u8)],
            vec![Value::from(25u8)],
            vec![Value::from(41u8)]
        ]
    );
}

// -- DELETE -----------------------------------------------------------------

#[test]
fn test_delete_with_where() {
    let db = TestDb::seeded();
    assert_eq!(
        db.run("DELETE FROM comments WHERE post_id = 10"),
        SqlResult::RowsAffected(1)
    );
    assert_eq!(db.count("comments"), 1);
    assert_eq!(
        db.run("DELETE FROM comments WHERE id = 404"),
        SqlResult::RowsAffected(0)
    );
    assert_eq!(
        db.execute(ALICE, "DELETE FROM users WHERE name = ?", &[text("Carol")])
            .unwrap(),
        SqlResult::RowsAffected(1)
    );
    assert_eq!(db.count("users"), 2);
}

#[test]
fn test_delete_without_where_is_rejected_and_deletes_nothing() {
    let db = TestDb::seeded();
    assert!(matches!(
        db.fail("DELETE FROM comments"),
        SqlError::MissingWhereClause
    ));
    assert!(matches!(
        db.fail("DELETE FROM comments CASCADE"),
        SqlError::MissingWhereClause
    ));
    assert_eq!(db.count("comments"), 2);
}

#[test]
fn test_delete_restricts_referenced_rows_by_default() {
    let db = TestDb::seeded();
    for sql in [
        "DELETE FROM users WHERE id = 1",
        "DELETE FROM users WHERE id = 1 RESTRICT",
    ] {
        assert!(matches!(
            query_error(db.fail(sql)),
            QueryError::ForeignKeyConstraintViolation { .. }
        ));
    }
    assert_eq!(db.count("users"), 3);
    assert_eq!(db.count("posts"), 3);
}

#[test]
fn test_delete_cascade_removes_referencing_rows() {
    let db = TestDb::seeded();
    // user 1, its two posts, and the comment on one of them
    assert_eq!(
        db.run("DELETE FROM users WHERE id = 1 CASCADE"),
        SqlResult::RowsAffected(4)
    );
    assert_eq!(db.count("users"), 2);
    assert_eq!(
        db.values("SELECT id FROM posts"),
        vec![vec![Value::from(12u32)]]
    );
    assert_eq!(
        db.values("SELECT id FROM comments"),
        vec![vec![Value::from(101u32)]]
    );
}

// -- transactions -----------------------------------------------------------

#[test]
fn test_begin_commit_cycle() {
    let db = TestDb::seeded();
    assert!(!db.engine.in_transaction(&db.ctx, ALICE));
    assert_eq!(db.run("BEGIN"), SqlResult::TxBegin);
    assert!(db.engine.in_transaction(&db.ctx, ALICE));
    assert!(!db.engine.in_transaction(&db.ctx, BOB));

    db.run("INSERT INTO users (id, name, age) VALUES (4, 'Dan', 20)");
    db.run("UPDATE users SET age = 31 WHERE id = 1");
    db.run("DELETE FROM comments WHERE id = 100");

    // the transaction reads its own writes
    assert_eq!(
        db.values("SELECT name FROM users WHERE id = 4"),
        vec![vec![text("Dan")]]
    );
    assert_eq!(
        db.values("SELECT age FROM users WHERE name = 'Alice'"),
        vec![vec![Value::from(31u8)]]
    );
    assert_eq!(db.values("SELECT id FROM comments").len(), 1);
    assert_eq!(
        db.values("SELECT COUNT(*) FROM users"),
        vec![vec![Value::from(4u64)]]
    );

    // other callers and the programmatic API do not see them yet
    assert_eq!(
        db.query_as(BOB, "SELECT name FROM users WHERE id = 4", &[]),
        Vec::<Vec<(String, Value)>>::new()
    );
    assert_eq!(db.count("users"), 3);
    assert_eq!(db.count("comments"), 2);

    assert_eq!(db.run("COMMIT"), SqlResult::TxCommit);
    assert!(!db.engine.in_transaction(&db.ctx, ALICE));
    assert_eq!(db.count("users"), 4);
    assert_eq!(db.count("comments"), 1);
    assert_eq!(
        db.query_as(BOB, "SELECT age FROM users WHERE id = 1", &[]),
        vec![vec![named("age", Value::from(31u8))]]
    );
}

#[test]
fn test_begin_rollback_cycle() {
    let db = TestDb::seeded();
    assert_eq!(db.run("BEGIN TRANSACTION"), SqlResult::TxBegin);
    db.run("INSERT INTO users (id, name, age) VALUES (4, 'Dan', 20)");
    db.run("DELETE FROM users WHERE id = 3");
    assert_eq!(
        db.values("SELECT COUNT(*) FROM users"),
        vec![vec![Value::from(3u64)]]
    );
    assert_eq!(db.run("ROLLBACK"), SqlResult::TxRollback);
    assert!(!db.engine.in_transaction(&db.ctx, ALICE));
    assert_eq!(
        db.values("SELECT id FROM users ORDER BY id"),
        vec![
            vec![Value::from(1u32)],
            vec![Value::from(2u32)],
            vec![Value::from(3u32)]
        ]
    );
    // a new transaction can start afterwards
    assert_eq!(db.run("BEGIN"), SqlResult::TxBegin);
    assert_eq!(db.run("COMMIT TRANSACTION"), SqlResult::TxCommit);
}

#[test]
fn test_nested_begin_is_rejected_and_keeps_the_transaction() {
    let db = TestDb::seeded();
    db.run("BEGIN");
    db.run("INSERT INTO users (id, name, age) VALUES (4, 'Dan', 20)");
    assert!(matches!(
        db.fail("BEGIN"),
        SqlError::TransactionAlreadyActive
    ));
    assert!(db.engine.in_transaction(&db.ctx, ALICE));
    db.run("COMMIT");
    assert_eq!(db.count("users"), 4);
}

#[test]
fn test_commit_and_rollback_without_begin() {
    let db = TestDb::new();
    for sql in ["COMMIT", "ROLLBACK"] {
        assert!(matches!(
            db.fail(sql),
            SqlError::Runtime(DbmsError::Transaction(
                TransactionError::NoActiveTransaction
            ))
        ));
    }
}

#[test]
fn test_each_caller_has_its_own_transaction() {
    let db = TestDb::seeded();
    db.execute(ALICE, "BEGIN", &[]).unwrap();
    db.execute(BOB, "BEGIN", &[]).unwrap();
    db.execute(
        ALICE,
        "INSERT INTO users (id, name, age) VALUES (4, 'A', 1)",
        &[],
    )
    .unwrap();
    db.execute(
        BOB,
        "INSERT INTO users (id, name, age) VALUES (5, 'B', 2)",
        &[],
    )
    .unwrap();

    let ids = |caller: &[u8]| -> Vec<Value> {
        db.query_as(caller, "SELECT id FROM users WHERE id > 3 ORDER BY id", &[])
            .into_iter()
            .flatten()
            .map(|(_, value)| value)
            .collect()
    };
    assert_eq!(ids(ALICE), vec![Value::from(4u32)]);
    assert_eq!(ids(BOB), vec![Value::from(5u32)]);

    db.execute(BOB, "ROLLBACK", &[]).unwrap();
    assert!(db.engine.in_transaction(&db.ctx, ALICE));
    assert!(!db.engine.in_transaction(&db.ctx, BOB));
    db.execute(ALICE, "COMMIT", &[]).unwrap();
    assert_eq!(ids(BOB), vec![Value::from(4u32)]);
}

#[test]
fn test_transactions_are_scoped_to_the_context() {
    let first = TestDb::new();
    let second = TestDb::new();

    first
        .engine
        .execute(&first.ctx, ALICE, "BEGIN", &[])
        .unwrap();
    let second_transaction = second.ctx.begin_transaction(ALICE.to_vec());

    assert_eq!(
        first
            .engine
            .execute(
                &second.ctx,
                ALICE,
                "INSERT INTO users (id, name, age) VALUES (1, 'Alice', 30)",
                &[],
            )
            .unwrap(),
        SqlResult::RowsAffected(1)
    );
    assert_eq!(second.count("users"), 1);

    let mut transaction =
        WasmDbmsDatabase::from_transaction(&second.ctx, TestSchema, second_transaction);
    transaction.rollback().unwrap();

    assert_eq!(
        first
            .engine
            .execute(&second.ctx, ALICE, "BEGIN", &[])
            .unwrap(),
        SqlResult::TxBegin
    );
    assert!(first.engine.in_transaction(&first.ctx, ALICE));
    assert!(first.engine.in_transaction(&second.ctx, ALICE));
    first
        .engine
        .execute(&second.ctx, ALICE, "ROLLBACK", &[])
        .unwrap();
    first
        .engine
        .execute(&first.ctx, ALICE, "ROLLBACK", &[])
        .unwrap();
}

#[test]
fn test_transactions_of_dropped_contexts_are_evicted() {
    let engine = SqlEngine::new(TestSchema);
    for _ in 0..3 {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        TestSchema::register_tables(&ctx).expect("failed to register tables");
        assert_eq!(
            engine.execute(&ctx, ALICE, "BEGIN", &[]).unwrap(),
            SqlResult::TxBegin
        );
    }
    assert_eq!(engine.transactions.borrow().len(), 1);

    let active = DbmsContext::new(HeapMemoryProvider::default());
    TestSchema::register_tables(&active).expect("failed to register tables");
    assert!(!engine.in_transaction(&active, ALICE));
    assert!(engine.transactions.borrow().is_empty());
}

#[test]
fn test_closed_programmatic_transaction_does_not_lock_sql_caller() {
    let db = TestDb::new();
    db.engine.execute(&db.ctx, ALICE, "BEGIN", &[]).unwrap();

    let transaction_id = db
        .engine
        .transactions
        .borrow()
        .get(&(db.ctx.id(), ALICE.to_vec()))
        .copied()
        .expect("SQL transaction must be tracked");
    let mut transaction = WasmDbmsDatabase::from_transaction(&db.ctx, TestSchema, transaction_id);
    transaction.rollback().unwrap();

    assert!(!db.engine.in_transaction(&db.ctx, ALICE));
    assert_eq!(
        db.engine.execute(&db.ctx, ALICE, "BEGIN", &[]).unwrap(),
        SqlResult::TxBegin
    );
    db.engine.execute(&db.ctx, ALICE, "ROLLBACK", &[]).unwrap();
}

#[test]
fn test_failed_statement_leaves_the_transaction_open() {
    let db = TestDb::seeded();
    db.run("BEGIN");
    db.run("INSERT INTO users (id, name, age) VALUES (4, 'Dan', 20)");
    assert!(matches!(
        db.fail("INSERT INTO nowhere (id) VALUES (1)"),
        SqlError::UnknownTable(_)
    ));
    assert!(matches!(db.fail("SELEC 1"), SqlError::Parse { .. }));
    assert!(db.engine.in_transaction(&db.ctx, ALICE));
    db.run("COMMIT");
    assert_eq!(db.count("users"), 4);
}

#[test]
fn test_failed_commit_ends_the_transaction() {
    let db = TestDb::seeded();
    db.execute(ALICE, "BEGIN", &[]).unwrap();
    db.execute(
        ALICE,
        "INSERT INTO users (id, name, age) VALUES (4, 'A', 1)",
        &[],
    )
    .unwrap();
    // Bob takes the same key first
    db.execute(
        BOB,
        "INSERT INTO users (id, name, age) VALUES (4, 'B', 2)",
        &[],
    )
    .unwrap();

    assert!(matches!(
        query_error(db.execute(ALICE, "COMMIT", &[]).unwrap_err()),
        QueryError::PrimaryKeyConflict
    ));
    assert!(!db.engine.in_transaction(&db.ctx, ALICE));
    assert_eq!(
        db.values("SELECT name FROM users WHERE id = 4"),
        vec![vec![text("B")]]
    );
    // the caller can start over
    assert_eq!(db.run("BEGIN"), SqlResult::TxBegin);
    assert_eq!(db.run("ROLLBACK"), SqlResult::TxRollback);
}

#[test]
fn test_schema_drift_is_reported_and_keeps_the_transaction_until_rollback() {
    /// A schema that does not match the tables stored by [`TestSchema`].
    #[derive(Debug, Clone, Copy, wasm_dbms_macros::DatabaseSchema)]
    #[tables(User = "users")]
    struct UsersOnlySchema;

    let db = TestDb::seeded();
    let engine = SqlEngine::new(UsersOnlySchema);
    let drift = |result: Result<SqlResult, SqlError>| {
        assert!(matches!(
            result,
            Err(SqlError::Runtime(DbmsError::Migration(
                MigrationError::SchemaDrift
            )))
        ));
    };

    drift(engine.execute(&db.ctx, ALICE, "SELECT * FROM users", &[]));
    drift(engine.execute(
        &db.ctx,
        ALICE,
        "INSERT INTO users (id, name, age) VALUES (9, 'Zed', 1)",
        &[],
    ));

    assert_eq!(
        engine.execute(&db.ctx, ALICE, "BEGIN", &[]).unwrap(),
        SqlResult::TxBegin
    );
    // the commit is refused, but the transaction is still there to roll back
    drift(engine.execute(&db.ctx, ALICE, "COMMIT", &[]));
    assert!(engine.in_transaction(&db.ctx, ALICE));
    assert_eq!(
        engine.execute(&db.ctx, ALICE, "ROLLBACK", &[]).unwrap(),
        SqlResult::TxRollback
    );
    assert!(!engine.in_transaction(&db.ctx, ALICE));
    assert_eq!(db.count("users"), 3);
}

#[test]
fn test_null_never_equals_and_negations_are_two_valued() {
    let db = TestDb::seeded();
    let names = |sql: &str| -> Vec<Value> { db.values(sql).into_iter().flatten().collect() };
    // Bob has no email
    assert_eq!(
        names("SELECT name FROM users WHERE email = 'alice@example.com'"),
        vec![text("Alice")]
    );
    assert_eq!(
        names("SELECT name FROM users WHERE email IN ('alice@example.com', 'x')"),
        vec![text("Alice")]
    );
    assert_eq!(
        names("SELECT name FROM users WHERE email LIKE '%' ORDER BY id"),
        vec![text("Alice"), text("Carol")]
    );
    // a negation is true for a row without a value
    for condition in [
        "email != 'alice@example.com'",
        "NOT email = 'alice@example.com'",
        "email NOT IN ('alice@example.com')",
        "email NOT LIKE 'alice%'",
    ] {
        assert_eq!(
            names(&format!(
                "SELECT name FROM users WHERE {condition} ORDER BY id"
            )),
            vec![text("Bob"), text("Carol")],
            "{condition}"
        );
    }
    // which is why rows without a value are excluded explicitly
    assert_eq!(
        names("SELECT name FROM users WHERE email != 'alice@example.com' AND email IS NOT NULL"),
        vec![text("Carol")]
    );
}

// -- statement-level errors -------------------------------------------------

#[test]
fn test_parameter_count_must_match() {
    let db = TestDb::seeded();
    assert!(matches!(
        db.execute(
            ALICE,
            "SELECT * FROM users WHERE id = ? AND age = ?",
            &[Value::from(1u32)]
        ),
        Err(SqlError::ParameterCountMismatch {
            expected: 2,
            got: 1
        })
    ));
    assert!(matches!(
        db.execute(ALICE, "SELECT * FROM users", &[Value::from(1u32)]),
        Err(SqlError::ParameterCountMismatch {
            expected: 0,
            got: 1
        })
    ));
    assert!(matches!(
        db.execute(ALICE, "BEGIN", &[Value::Null]),
        Err(SqlError::ParameterCountMismatch {
            expected: 0,
            got: 1
        })
    ));
    assert!(!db.engine.in_transaction(&db.ctx, ALICE));
}

#[test]
fn test_parse_and_name_errors() {
    let db = TestDb::seeded();
    assert!(matches!(
        db.fail("SELECT * FROM"),
        SqlError::Parse {
            line: 1,
            col: 14,
            ..
        }
    ));
    assert!(matches!(
        db.fail("SELECT * FROM users; DELETE FROM users WHERE id = 1"),
        SqlError::Parse { .. }
    ));
    assert_eq!(db.count("users"), 3);
    assert!(matches!(
        db.fail("SELECT * FROM ghosts"),
        SqlError::UnknownTable(table) if table == "ghosts"
    ));
    assert!(matches!(
        db.fail("UPDATE users SET ghost = 1 WHERE id = 1"),
        SqlError::UnknownColumn { table, column } if table == "users" && column == "ghost"
    ));
    assert!(matches!(
        db.fail("DELETE FROM ghosts WHERE id = 1"),
        SqlError::UnknownTable(table) if table == "ghosts"
    ));
    assert!(matches!(
        db.fail("SELECT id FROM users JOIN posts ON users.id = posts.user_id"),
        SqlError::AmbiguousColumn(column) if column == "id"
    ));
    assert!(matches!(
        db.fail("SELECT DISTINCT users.name FROM users JOIN posts ON users.id = posts.user_id"),
        SqlError::Unsupported(_)
    ));
}

#[test]
fn test_text_is_never_interpreted_as_sql() {
    let db = TestDb::seeded();
    let hostile = "x'; DELETE FROM users WHERE id > 0; --";
    db.execute(
        ALICE,
        "UPDATE users SET name = ? WHERE id = 1",
        &[text(hostile)],
    )
    .unwrap();
    assert_eq!(db.count("users"), 3);
    assert_eq!(
        db.query_as(
            ALICE,
            "SELECT id FROM users WHERE name = ?",
            &[text(hostile)]
        ),
        vec![vec![named("id", Value::from(1u32))]]
    );
    // the same text as a literal needs its quote doubled
    assert_eq!(
        db.values("SELECT id FROM users WHERE name = 'x''; DELETE FROM users WHERE id > 0; --'"),
        vec![vec![Value::from(1u32)]]
    );
}

#[test]
fn test_statement_with_comments_and_semicolon() {
    let db = TestDb::seeded();
    assert_eq!(
        db.values(
            "-- the oldest user\nSELECT name /* only the name */ FROM users\nORDER BY age DESC \
             LIMIT 1;"
        ),
        vec![vec![text("Carol")]]
    );
}

#[test]
fn test_engine_debug_output() {
    let db = TestDb::new();
    db.run("BEGIN");
    let debug = format!("{engine:?}", engine = db.engine);
    assert!(debug.contains("SqlEngine"), "{debug}");
    assert!(debug.contains("active_transactions: 1"), "{debug}");
}

#[test]
#[should_panic(expected = "planned column must exist in the selected table row")]
fn test_table_row_rejects_missing_planned_column() {
    let projection = [OutputColumn {
        table: None,
        column: "missing".to_string(),
        name: "missing".to_string(),
    }];
    table_row(Vec::new(), Some(&projection));
}

#[test]
#[should_panic(expected = "planned column must exist in the selected join row")]
fn test_join_row_rejects_missing_planned_column() {
    let projection = [OutputColumn {
        table: Some("users".to_string()),
        column: "missing".to_string(),
        name: "missing".to_string(),
    }];
    join_row(Vec::new(), Some(&projection));
}

#[test]
#[should_panic(expected = "planned group key must exist in the aggregated row")]
fn test_aggregate_row_rejects_missing_planned_group_key() {
    let output = AggregateOutput {
        source: AggregateSource::GroupKey(0),
        def: JoinColumnDef::from(User::columns()[0]),
    };
    aggregate_row(
        &AggregatedRow {
            group_keys: Vec::new(),
            values: Vec::new(),
        },
        &[output],
    );
}

#[test]
#[should_panic(expected = "planned aggregate must exist in the aggregated row")]
fn test_aggregate_row_rejects_missing_planned_aggregate() {
    let output = AggregateOutput {
        source: AggregateSource::Aggregate(0),
        def: JoinColumnDef::from(User::columns()[0]),
    };
    aggregate_row(
        &AggregatedRow {
            group_keys: Vec::new(),
            values: Vec::new(),
        },
        &[output],
    );
}
