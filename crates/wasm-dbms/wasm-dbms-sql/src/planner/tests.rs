use wasm_dbms_api::prelude::{
    AggregateFunction, CandidDataTypeKind, DeleteBehavior, Filter, Query, SqlError, Value,
};

use super::{AggregateSource, SelectPlan, delete, insert, select, update};
use crate::ast::Statement;
use crate::parser::parse;
use crate::test_schema::catalog;

fn parse_statement(sql: &str) -> Statement {
    parse(sql).expect("statement must parse").statement
}

fn plan_select(sql: &str, params: &[Value]) -> Result<SelectPlan, SqlError> {
    match parse_statement(sql) {
        Statement::Select(statement) => select(&statement, &catalog, params),
        other => panic!("not a SELECT: {other:?}"),
    }
}

/// `(source table, source column, output name)` of every projected column;
/// `None` for `SELECT *`.
type Outputs = Option<Vec<(Option<String>, String, String)>>;

/// Plans a `SELECT` that reads one table without aggregates.
fn table_plan(sql: &str) -> (String, Query, Outputs) {
    match plan_select(sql, &[]).expect("statement must plan") {
        SelectPlan::Table(plan) => (plan.table, plan.query, output_columns(plan.projection)),
        other => panic!("expected a table plan, got {other:?}"),
    }
}

/// Plans a `SELECT` with at least one `JOIN`.
fn join_plan(sql: &str) -> (String, Query, Outputs) {
    match plan_select(sql, &[]).expect("statement must plan") {
        SelectPlan::Join(plan) => (plan.table, plan.query, output_columns(plan.projection)),
        other => panic!("expected a join plan, got {other:?}"),
    }
}

fn output_columns(projection: Option<Vec<super::OutputColumn>>) -> Outputs {
    projection.map(|columns| {
        columns
            .into_iter()
            .map(|column| (column.table, column.column, column.name))
            .collect()
    })
}

fn output(table: Option<&str>, column: &str, name: &str) -> (Option<String>, String, String) {
    (
        table.map(str::to_string),
        column.to_string(),
        name.to_string(),
    )
}

fn unsupported(sql: &str) -> String {
    match plan_select(sql, &[]) {
        Err(SqlError::Unsupported(msg)) => msg,
        other => panic!("expected an unsupported error, got {other:?}"),
    }
}

fn select_error(sql: &str) -> SqlError {
    plan_select(sql, &[]).expect_err("statement must not plan")
}

// -- single-table SELECT ----------------------------------------------------

#[test]
fn test_select_star_reads_every_column_without_projection() {
    let (table, query, projection) = table_plan("SELECT * FROM users");
    assert_eq!(table, "users");
    assert_eq!(query, Query::builder().all().build());
    assert_eq!(projection, None);
}

#[test]
fn test_select_columns_keeps_the_select_list_order() {
    let (_, query, projection) = table_plan("SELECT name, id FROM users");
    // the query always reads every column; the projection picks and orders them
    assert_eq!(query, Query::builder().all().build());
    assert_eq!(
        projection,
        Some(vec![output(None, "name", "name"), output(None, "id", "id")])
    );
}

#[test]
fn test_select_alias_renames_the_output_column() {
    let (_, _, projection) = table_plan("SELECT name AS full_name, users.id AS key FROM users");
    assert_eq!(
        projection,
        Some(vec![
            output(None, "name", "full_name"),
            output(None, "id", "key")
        ])
    );
}

#[test]
fn test_select_where_coerces_literals_to_the_column_type() {
    let (_, query, _) = table_plan("SELECT * FROM users WHERE id = 7 AND age >= 18");
    assert_eq!(
        query.filter,
        Some(Filter::eq("id", Value::from(7u32)).and(Filter::ge("age", Value::from(18u8))))
    );
}

#[test]
fn test_select_where_every_predicate_kind() {
    let (_, query, _) = table_plan(
        "SELECT * FROM users WHERE id != 1 AND id < 2 AND id <= 3 AND id > 4 AND name IN ('a', \
         'b') AND name NOT IN ('c') AND name LIKE 'A%' AND name NOT LIKE '%z' AND email IS NULL \
         AND NOT email IS NOT NULL OR id = 5",
    );
    let expected = Filter::ne("id", Value::from(1u32))
        .and(Filter::lt("id", Value::from(2u32)))
        .and(Filter::le("id", Value::from(3u32)))
        .and(Filter::gt("id", Value::from(4u32)))
        .and(Filter::in_list(
            "name",
            vec![Value::from("a"), Value::from("b")],
        ))
        .and(Filter::in_list("name", vec![Value::from("c")]).not())
        .and(Filter::like("name", "A%"))
        .and(Filter::like("name", "%z").not())
        .and(Filter::is_null("email"))
        .and(Filter::not_null("email").not())
        .or(Filter::eq("id", Value::from(5u32)));
    assert_eq!(query.filter, Some(expected));
}

#[test]
fn test_select_where_binds_parameters_in_order() {
    let plan = plan_select(
        "SELECT * FROM users WHERE name = ? AND age > ? LIMIT ? OFFSET ?",
        &[
            Value::from("Alice"),
            Value::from(30i64),
            Value::from(10u32),
            Value::from(20u64),
        ],
    )
    .expect("statement must plan");
    let SelectPlan::Table(plan) = plan else {
        panic!("expected a table plan");
    };
    assert_eq!(
        plan.query,
        Query::builder()
            .all()
            .filter(Some(
                Filter::eq("name", Value::from("Alice")).and(Filter::gt("age", Value::from(30u8)))
            ))
            .limit(10)
            .offset(20)
            .build()
    );
}

#[test]
fn test_select_order_limit_offset() {
    let (_, query, _) =
        table_plan("SELECT * FROM users ORDER BY age DESC, users.name LIMIT 5 OFFSET 2");
    assert_eq!(
        query,
        Query::builder()
            .all()
            .order_by_desc("age")
            .order_by_asc("name")
            .limit(5)
            .offset(2)
            .build()
    );
}

#[test]
fn test_select_order_by_alias_sorts_by_the_aliased_column() {
    let (_, query, _) = table_plan("SELECT name AS n, age FROM users ORDER BY n");
    assert_eq!(
        query.order_by,
        Query::builder().order_by_asc("name").build().order_by
    );
}

#[test]
fn test_select_order_by_alias_wins_over_a_column_of_the_same_name() {
    // `name` is both a column and the alias of `email`
    let (_, query, _) = table_plan("SELECT email AS name FROM users ORDER BY name");
    assert_eq!(
        query.order_by,
        Query::builder().order_by_asc("email").build().order_by
    );
}

#[test]
fn test_select_table_alias_qualifies_columns() {
    let (table, query, projection) =
        table_plan("SELECT u.name FROM users AS u WHERE u.id = 1 ORDER BY u.age");
    assert_eq!(table, "users");
    assert_eq!(query.filter, Some(Filter::eq("id", Value::from(1u32))));
    assert_eq!(
        query.order_by,
        Query::builder().order_by_asc("age").build().order_by
    );
    assert_eq!(projection, Some(vec![output(None, "name", "name")]));
}

#[test]
fn test_select_distinct_uses_the_select_list() {
    let (_, query, projection) = table_plan("SELECT DISTINCT region, category FROM sales");
    assert_eq!(query.distinct_by, vec!["region", "category"]);
    assert_eq!(
        projection,
        Some(vec![
            output(None, "region", "region"),
            output(None, "category", "category")
        ])
    );
}

#[test]
fn test_select_distinct_star_uses_every_column() {
    let (_, query, projection) = table_plan("SELECT DISTINCT * FROM users");
    assert_eq!(query.distinct_by, vec!["id", "name", "email", "age"]);
    assert_eq!(projection, None);
}

#[test]
fn test_select_distinct_rejects_ordering_by_a_column_outside_the_select_list() {
    assert_eq!(
        unsupported("SELECT DISTINCT region FROM sales ORDER BY price"),
        "ORDER BY column `price` must appear in the select list of a SELECT DISTINCT"
    );
    // ordering by a selected column is fine
    let (_, query, _) = table_plan("SELECT DISTINCT region FROM sales ORDER BY region DESC");
    assert_eq!(query.distinct_by, vec!["region"]);
}

#[test]
fn test_select_unknown_names() {
    assert!(matches!(
        select_error("SELECT * FROM ghosts"),
        SqlError::UnknownTable(table) if table == "ghosts"
    ));
    assert!(matches!(
        select_error("SELECT nope FROM users"),
        SqlError::UnknownColumn { table, column } if table == "users" && column == "nope"
    ));
    assert!(matches!(
        select_error("SELECT * FROM users WHERE nope = 1"),
        SqlError::UnknownColumn { table, column } if table == "users" && column == "nope"
    ));
    assert!(matches!(
        select_error("SELECT * FROM users ORDER BY nope"),
        SqlError::UnknownColumn { table, column } if table == "users" && column == "nope"
    ));
    assert!(matches!(
        select_error("SELECT posts.id FROM users"),
        SqlError::UnknownTable(table) if table == "posts"
    ));
}

#[test]
fn test_select_table_name_is_hidden_by_its_alias() {
    assert!(matches!(
        select_error("SELECT users.id FROM users AS u"),
        SqlError::UnknownTable(table) if table == "users"
    ));
}

#[test]
fn test_select_names_are_case_sensitive() {
    assert!(matches!(
        select_error("SELECT * FROM Users"),
        SqlError::UnknownTable(table) if table == "Users"
    ));
    assert!(matches!(
        select_error("SELECT Name FROM users"),
        SqlError::UnknownColumn { column, .. } if column == "Name"
    ));
}

#[test]
fn test_select_where_type_errors_name_the_column() {
    assert!(matches!(
        select_error("SELECT * FROM users WHERE age = 'old'"),
        SqlError::TypeMismatch { column, expected: CandidDataTypeKind::Uint8, got }
            if column == "age" && got == "String"
    ));
    assert!(matches!(
        select_error("SELECT * FROM users WHERE age IN (1, 300)"),
        SqlError::InvalidLiteral { column, expected: CandidDataTypeKind::Uint8, .. }
            if column == "age"
    ));
    assert!(matches!(
        select_error("SELECT * FROM users WHERE name LIKE ?"),
        SqlError::ParameterCountMismatch {
            expected: 1,
            got: 0
        }
    ));
}

#[test]
fn test_select_null_parameter_in_comparison_is_rejected() {
    let result = plan_select("SELECT * FROM users WHERE email = ?", &[Value::Null]);
    assert!(matches!(
        result,
        Err(SqlError::TypeMismatch { column, got, .. }) if column == "email" && got == "Null"
    ));
}

// -- JOIN -------------------------------------------------------------------

#[test]
fn test_join_qualifies_every_column_with_its_table() {
    let (table, query, projection) = join_plan(
        "SELECT name, title FROM users JOIN posts ON users.id = posts.user_id WHERE published = \
         TRUE AND users.id > 1 ORDER BY title DESC LIMIT 3",
    );
    assert_eq!(table, "users");
    assert_eq!(
        query,
        Query::builder()
            .all()
            .inner_join("posts", "users.id", "user_id")
            .filter(Some(
                Filter::eq("posts.published", Value::from(true))
                    .and(Filter::gt("users.id", Value::from(1u32)))
            ))
            .order_by_desc("posts.title")
            .limit(3)
            .build()
    );
    assert_eq!(
        projection,
        Some(vec![
            output(Some("users"), "name", "name"),
            output(Some("posts"), "title", "title")
        ])
    );
}

#[test]
fn test_join_star_has_no_projection() {
    let (_, query, projection) = join_plan("SELECT * FROM users JOIN posts ON users.id = user_id");
    assert_eq!(
        query,
        Query::builder()
            .all()
            .inner_join("posts", "users.id", "user_id")
            .build()
    );
    assert_eq!(projection, None);
}

#[test]
fn test_join_condition_sides_can_be_written_in_either_order() {
    let (_, query, _) =
        join_plan("SELECT * FROM users LEFT JOIN posts ON posts.user_id = users.id");
    assert_eq!(
        query,
        Query::builder()
            .all()
            .left_join("posts", "users.id", "user_id")
            .build()
    );
}

#[test]
fn test_join_rejects_incompatible_column_types_in_either_order() {
    for condition in ["users.name = posts.user_id", "posts.user_id = users.name"] {
        let error = select_error(&format!("SELECT * FROM users JOIN posts ON {condition}"));
        let SqlError::TypeMismatch {
            column,
            expected,
            got,
        } = error
        else {
            panic!("expected a type mismatch, got {error:?}");
        };
        assert_eq!(column, "posts.user_id".to_string());
        assert_eq!(expected, CandidDataTypeKind::Text);
        assert_eq!(got, "Uint32".to_string());
    }
}

#[test]
fn test_join_accepts_matching_nullable_column_type() {
    let (table, query, projection) =
        join_plan("SELECT * FROM users JOIN sales ON users.id = sales.bonus");
    assert_eq!(table, "users");
    assert_eq!(
        query,
        Query::builder()
            .all()
            .inner_join("sales", "users.id", "bonus")
            .build()
    );
    assert_eq!(projection, None);
}

#[test]
fn test_join_types() {
    for (keyword, expected) in [
        (
            "JOIN",
            Query::builder().inner_join("posts", "users.id", "user_id"),
        ),
        (
            "LEFT JOIN",
            Query::builder().left_join("posts", "users.id", "user_id"),
        ),
        (
            "RIGHT JOIN",
            Query::builder().right_join("posts", "users.id", "user_id"),
        ),
        (
            "FULL JOIN",
            Query::builder().full_join("posts", "users.id", "user_id"),
        ),
    ] {
        let (_, query, _) = join_plan(&format!(
            "SELECT * FROM users {keyword} posts ON users.id = posts.user_id"
        ));
        assert_eq!(query, expected.all().build());
    }
}

#[test]
fn test_join_aliases_resolve_to_real_table_names() {
    let (table, query, projection) = join_plan(
        "SELECT u.name AS author, p.title FROM users u JOIN posts AS p ON u.id = p.user_id WHERE \
         p.id = 9 ORDER BY u.name",
    );
    assert_eq!(table, "users");
    assert_eq!(
        query,
        Query::builder()
            .all()
            .inner_join("posts", "users.id", "user_id")
            .filter(Some(Filter::eq("posts.id", Value::from(9u32))))
            .order_by_asc("users.name")
            .build()
    );
    assert_eq!(
        projection,
        Some(vec![
            output(Some("users"), "name", "author"),
            output(Some("posts"), "title", "title")
        ])
    );
}

#[test]
fn test_join_chain_can_join_on_any_earlier_table() {
    let (_, query, _) = join_plan(
        "SELECT * FROM users JOIN posts ON users.id = posts.user_id LEFT JOIN comments ON \
         comments.post_id = posts.id",
    );
    assert_eq!(
        query,
        Query::builder()
            .all()
            .inner_join("posts", "users.id", "user_id")
            .left_join("comments", "posts.id", "post_id")
            .build()
    );
}

#[test]
fn test_join_order_by_alias() {
    let (_, query, _) = join_plan(
        "SELECT posts.title AS headline FROM users JOIN posts ON users.id = posts.user_id ORDER \
         BY headline",
    );
    assert_eq!(
        query.order_by,
        Query::builder()
            .order_by_asc("posts.title")
            .build()
            .order_by
    );
}

#[test]
fn test_join_ambiguous_and_unknown_columns() {
    assert!(matches!(
        select_error("SELECT id FROM users JOIN posts ON users.id = posts.user_id"),
        SqlError::AmbiguousColumn(column) if column == "id"
    ));
    assert!(matches!(
        select_error("SELECT * FROM users JOIN posts ON id = user_id"),
        SqlError::AmbiguousColumn(column) if column == "id"
    ));
    assert!(matches!(
        select_error("SELECT * FROM users JOIN posts ON users.id = posts.user_id WHERE id = 1"),
        SqlError::AmbiguousColumn(column) if column == "id"
    ));
    assert!(matches!(
        select_error("SELECT nope FROM users JOIN posts ON users.id = posts.user_id"),
        SqlError::UnknownColumn { table, column }
            if table == "users or posts" && column == "nope"
    ));
    assert!(matches!(
        select_error("SELECT * FROM users JOIN ghosts ON users.id = ghosts.user_id"),
        SqlError::UnknownTable(table) if table == "ghosts"
    ));
}

#[test]
fn test_join_condition_cannot_reference_a_later_table() {
    assert!(matches!(
        select_error(
            "SELECT * FROM users JOIN posts ON comments.post_id = posts.id JOIN comments ON \
             comments.post_id = posts.id"
        ),
        SqlError::UnknownTable(table) if table == "comments"
    ));
}

#[test]
fn test_join_condition_must_link_the_joined_table_to_an_earlier_one() {
    let expected = "the JOIN condition of `posts` must compare one of its columns with a column \
                    of a table that precedes it";
    assert_eq!(
        unsupported("SELECT * FROM users JOIN posts ON users.id = users.age"),
        expected
    );
    assert_eq!(
        unsupported("SELECT * FROM users JOIN posts ON posts.id = posts.user_id"),
        expected
    );
}

#[test]
fn test_join_rejects_self_joins_and_duplicate_aliases() {
    assert_eq!(
        unsupported("SELECT * FROM users a JOIN users b ON a.id = b.id"),
        "table `users` appears more than once; self-joins are not supported"
    );
    assert_eq!(
        unsupported("SELECT * FROM users x JOIN posts x ON x.id = x.user_id"),
        "table alias `x` is used more than once"
    );
}

#[test]
fn test_join_rejects_distinct() {
    assert_eq!(
        unsupported("SELECT DISTINCT name FROM users JOIN posts ON users.id = posts.user_id"),
        "DISTINCT is not supported together with JOIN"
    );
}

// -- aggregates -------------------------------------------------------------

/// `(output name, data type, nullable, source)` of every aggregate output.
type AggregateColumns = Vec<(String, CandidDataTypeKind, bool, AggregateSource)>;

fn aggregate_plan(sql: &str) -> (String, Query, Vec<AggregateFunction>, AggregateColumns) {
    match plan_select(sql, &[]).expect("statement must plan") {
        SelectPlan::Aggregate(plan) => (
            plan.table,
            plan.query,
            plan.aggregates,
            plan.outputs
                .into_iter()
                .map(|output| {
                    (
                        output.def.name,
                        output.def.data_type,
                        output.def.nullable,
                        output.source,
                    )
                })
                .collect(),
        ),
        other => panic!("expected an aggregate plan, got {other:?}"),
    }
}

#[test]
fn test_aggregate_without_group_by() {
    let (table, query, aggregates, outputs) = aggregate_plan("SELECT COUNT(*) FROM sales");
    assert_eq!(table, "sales");
    assert_eq!(query, Query::builder().all().build());
    assert_eq!(aggregates, vec![AggregateFunction::Count(None)]);
    assert_eq!(
        outputs,
        vec![(
            "COUNT(*)".to_string(),
            CandidDataTypeKind::Uint64,
            false,
            AggregateSource::Aggregate(0)
        )]
    );
}

#[test]
fn test_aggregate_output_names_and_types() {
    let (_, _, aggregates, outputs) = aggregate_plan(
        "SELECT COUNT(bonus), SUM(quantity), AVG(price), MIN(category), MAX(sales.bonus) AS best \
         FROM sales",
    );
    assert_eq!(
        aggregates,
        vec![
            AggregateFunction::Count(Some("bonus".to_string())),
            AggregateFunction::Sum("quantity".to_string()),
            AggregateFunction::Avg("price".to_string()),
            AggregateFunction::Min("category".to_string()),
            AggregateFunction::Max("bonus".to_string()),
        ]
    );
    assert_eq!(
        outputs,
        vec![
            (
                "COUNT(bonus)".to_string(),
                CandidDataTypeKind::Uint64,
                false,
                AggregateSource::Aggregate(0)
            ),
            (
                "SUM(quantity)".to_string(),
                CandidDataTypeKind::Decimal,
                true,
                AggregateSource::Aggregate(1)
            ),
            (
                "AVG(price)".to_string(),
                CandidDataTypeKind::Decimal,
                true,
                AggregateSource::Aggregate(2)
            ),
            (
                "MIN(category)".to_string(),
                CandidDataTypeKind::Text,
                true,
                AggregateSource::Aggregate(3)
            ),
            (
                "best".to_string(),
                CandidDataTypeKind::Uint32,
                true,
                AggregateSource::Aggregate(4)
            ),
        ]
    );
}

#[test]
fn test_aggregate_group_by_with_where_and_pagination() {
    let (_, query, aggregates, outputs) = aggregate_plan(
        "SELECT region AS area, category, COUNT(*) AS n FROM sales WHERE quantity > 0 GROUP BY \
         category, region ORDER BY area DESC, n LIMIT 4 OFFSET 1",
    );
    assert_eq!(
        query,
        Query::builder()
            .all()
            .filter(Some(Filter::gt("quantity", Value::from(0u32))))
            .group_by(&["category", "region"])
            .order_by_desc("region")
            .order_by_asc("agg0")
            .limit(4)
            .offset(1)
            .build()
    );
    assert_eq!(aggregates, vec![AggregateFunction::Count(None)]);
    assert_eq!(
        outputs,
        vec![
            (
                "area".to_string(),
                CandidDataTypeKind::Text,
                false,
                AggregateSource::GroupKey(1)
            ),
            (
                "category".to_string(),
                CandidDataTypeKind::Text,
                false,
                AggregateSource::GroupKey(0)
            ),
            (
                "n".to_string(),
                CandidDataTypeKind::Uint64,
                false,
                AggregateSource::Aggregate(0)
            ),
        ]
    );
}

#[test]
fn test_aggregate_having_and_order_by_reuse_and_add_aggregates() {
    let (_, query, aggregates, outputs) = aggregate_plan(
        "SELECT category, SUM(quantity) FROM sales GROUP BY category HAVING SUM(quantity) > 10 \
         AND COUNT(*) >= 2 AND category != 'misc' ORDER BY MAX(bonus) DESC, SUM(quantity)",
    );
    // SUM(quantity) is computed once; COUNT(*) and MAX(bonus) are added for
    // HAVING and ORDER BY without becoming output columns
    assert_eq!(
        aggregates,
        vec![
            AggregateFunction::Sum("quantity".to_string()),
            AggregateFunction::Count(None),
            AggregateFunction::Max("bonus".to_string()),
        ]
    );
    assert_eq!(outputs.len(), 2);
    assert_eq!(
        query,
        Query::builder()
            .all()
            .group_by(&["category"])
            .having(
                Filter::gt("agg0", Value::from(rust_decimal::Decimal::from(10)))
                    .and(Filter::ge("agg1", Value::from(2u64)))
                    .and(Filter::ne("category", Value::from("misc")))
            )
            .order_by_desc("agg2")
            .order_by_asc("agg0")
            .build()
    );
}

#[test]
fn test_aggregate_having_coerces_to_the_aggregate_result_type() {
    let (_, query, _, _) = aggregate_plan(
        "SELECT category FROM sales GROUP BY category HAVING AVG(price) < 2.5 AND MIN(bonus) IN \
         (1, 2) AND MAX(category) = 'z' AND COUNT(bonus) IS NOT NULL",
    );
    assert_eq!(
        query.having,
        Some(
            Filter::lt(
                "agg0",
                Value::from("2.5".parse::<rust_decimal::Decimal>().unwrap())
            )
            .and(Filter::in_list(
                "agg1",
                vec![Value::from(1u32), Value::from(2u32)]
            ))
            .and(Filter::eq("agg2", Value::from("z")))
            .and(Filter::not_null("agg3"))
        )
    );
}

#[test]
fn test_aggregate_having_without_group_by() {
    let (_, query, aggregates, _) =
        aggregate_plan("SELECT COUNT(*) FROM sales HAVING COUNT(*) > 0");
    assert_eq!(aggregates, vec![AggregateFunction::Count(None)]);
    assert_eq!(query.having, Some(Filter::gt("agg0", Value::from(0u64))));
    assert!(query.group_by.is_empty());
}

#[test]
fn test_aggregate_group_by_only_is_an_aggregate_query() {
    let (_, query, aggregates, outputs) =
        aggregate_plan("SELECT category FROM sales GROUP BY category");
    assert!(aggregates.is_empty());
    assert_eq!(query.group_by, vec!["category"]);
    assert_eq!(outputs.len(), 1);
}

#[test]
fn test_aggregate_rejections() {
    for (sql, expected) in [
        (
            "SELECT * FROM sales GROUP BY category",
            "`SELECT *` is not allowed in an aggregate query",
        ),
        (
            "SELECT region, COUNT(*) FROM sales GROUP BY category",
            "column `region` must appear in GROUP BY or be used in an aggregate function",
        ),
        (
            "SELECT category, COUNT(*) FROM sales",
            "column `category` must appear in GROUP BY or be used in an aggregate function",
        ),
        (
            "SELECT COUNT(*) FROM sales GROUP BY category HAVING region = 'eu'",
            "column `region` must appear in GROUP BY or be used in an aggregate function",
        ),
        (
            "SELECT COUNT(*) FROM sales GROUP BY category ORDER BY region",
            "column `region` must appear in GROUP BY or be used in an aggregate function",
        ),
        (
            "SELECT category FROM sales GROUP BY category HAVING category LIKE 'a%'",
            "LIKE is not supported in HAVING",
        ),
        (
            "SELECT DISTINCT category, COUNT(*) FROM sales GROUP BY category",
            "DISTINCT is not supported in an aggregate query",
        ),
        (
            "SELECT COUNT(*) FROM users JOIN posts ON users.id = posts.user_id",
            "aggregate functions, GROUP BY and HAVING are not supported together with JOIN",
        ),
        (
            "SELECT name FROM users ORDER BY COUNT(*)",
            "column `name` must appear in GROUP BY or be used in an aggregate function",
        ),
    ] {
        assert_eq!(unsupported(sql), expected, "{sql}");
    }
}

#[test]
fn test_aggregate_unknown_columns() {
    assert!(matches!(
        select_error("SELECT SUM(nope) FROM sales"),
        SqlError::UnknownColumn { table, column } if table == "sales" && column == "nope"
    ));
    assert!(matches!(
        select_error("SELECT COUNT(*) FROM sales GROUP BY nope"),
        SqlError::UnknownColumn { table, column } if table == "sales" && column == "nope"
    ));
}

#[test]
fn test_aggregate_having_type_errors_name_the_aggregate() {
    assert!(matches!(
        select_error("SELECT COUNT(*) FROM sales HAVING COUNT(*) > 'many'"),
        SqlError::TypeMismatch { column, expected: CandidDataTypeKind::Uint64, got }
            if column == "COUNT(*)" && got == "String"
    ));
    assert!(matches!(
        select_error("SELECT COUNT(*) FROM sales HAVING COUNT(*) > -1"),
        SqlError::InvalidLiteral { column, .. } if column == "COUNT(*)"
    ));
}

// -- INSERT / UPDATE / DELETE -----------------------------------------------

#[test]
fn test_insert_pairs_columns_with_coerced_values() {
    let Statement::Insert(statement) =
        parse_statement("INSERT INTO users (name, id, age, email) VALUES ('Al', 1, ?, NULL)")
    else {
        panic!("not an INSERT");
    };
    let plan = insert(&statement, &catalog, &[Value::from(44u64)]).expect("statement must plan");
    assert_eq!(plan.table, "users");
    let values: Vec<(&str, Value)> = plan
        .values
        .iter()
        .map(|(column, value)| (column.name, value.clone()))
        .collect();
    assert_eq!(
        values,
        vec![
            ("name", Value::from("Al")),
            ("id", Value::from(1u32)),
            ("age", Value::from(44u8)),
            ("email", Value::Null),
        ]
    );
}

#[test]
fn test_insert_errors() {
    let plan_insert = |sql: &str| {
        let Statement::Insert(statement) = parse_statement(sql) else {
            panic!("not an INSERT");
        };
        insert(&statement, &catalog, &[]).expect_err("statement must not plan")
    };
    assert!(matches!(
        plan_insert("INSERT INTO ghosts (id) VALUES (1)"),
        SqlError::UnknownTable(table) if table == "ghosts"
    ));
    assert!(matches!(
        plan_insert("INSERT INTO users (id, nope) VALUES (1, 2)"),
        SqlError::UnknownColumn { table, column } if table == "users" && column == "nope"
    ));
    assert!(matches!(
        plan_insert("INSERT INTO users (id) VALUES ('one')"),
        SqlError::TypeMismatch { column, .. } if column == "id"
    ));
    assert!(matches!(
        plan_insert("INSERT INTO users (id, age) VALUES (1, 256)"),
        SqlError::InvalidLiteral { column, .. } if column == "age"
    ));
}

#[test]
fn test_update_plan() {
    let Statement::Update(statement) =
        parse_statement("UPDATE users SET name = ?, email = NULL WHERE users.id = ? AND age < 30")
    else {
        panic!("not an UPDATE");
    };
    let plan = update(
        &statement,
        &catalog,
        &[Value::from("Bo"), Value::from(3u32)],
    )
    .expect("statement must plan");
    assert_eq!(plan.table, "users");
    let values: Vec<(&str, Value)> = plan
        .values
        .iter()
        .map(|(column, value)| (column.name, value.clone()))
        .collect();
    assert_eq!(
        values,
        vec![("name", Value::from("Bo")), ("email", Value::Null)]
    );
    assert_eq!(
        plan.filter,
        Filter::eq("id", Value::from(3u32)).and(Filter::lt("age", Value::from(30u8)))
    );
}

#[test]
fn test_update_errors() {
    let plan_update = |sql: &str| {
        let Statement::Update(statement) = parse_statement(sql) else {
            panic!("not an UPDATE");
        };
        update(&statement, &catalog, &[]).expect_err("statement must not plan")
    };
    assert!(matches!(
        plan_update("UPDATE ghosts SET a = 1 WHERE b = 2"),
        SqlError::UnknownTable(table) if table == "ghosts"
    ));
    assert!(matches!(
        plan_update("UPDATE users SET nope = 1 WHERE id = 2"),
        SqlError::UnknownColumn { column, .. } if column == "nope"
    ));
    assert!(matches!(
        plan_update("UPDATE users SET age = 1 WHERE nope = 2"),
        SqlError::UnknownColumn { column, .. } if column == "nope"
    ));
    assert!(matches!(
        plan_update("UPDATE users SET age = 1 WHERE posts.id = 2"),
        SqlError::UnknownTable(table) if table == "posts"
    ));
    assert!(matches!(
        plan_update("UPDATE users SET age = 'x' WHERE id = 2"),
        SqlError::TypeMismatch { column, .. } if column == "age"
    ));
}

#[test]
fn test_delete_plan() {
    let plan_delete = |sql: &str| {
        let Statement::Delete(statement) = parse_statement(sql) else {
            panic!("not a DELETE");
        };
        delete(&statement, &catalog, &[])
    };
    let plan = plan_delete("DELETE FROM posts WHERE user_id = 4").expect("statement must plan");
    assert_eq!(plan.table, "posts");
    assert_eq!(plan.filter, Filter::eq("user_id", Value::from(4u32)));
    assert_eq!(plan.behavior, DeleteBehavior::Restrict);

    let plan =
        plan_delete("DELETE FROM posts WHERE user_id = 4 RESTRICT").expect("statement must plan");
    assert_eq!(plan.behavior, DeleteBehavior::Restrict);

    let plan =
        plan_delete("DELETE FROM posts WHERE user_id = 4 CASCADE").expect("statement must plan");
    assert_eq!(plan.behavior, DeleteBehavior::Cascade);

    assert!(matches!(
        plan_delete("DELETE FROM ghosts WHERE id = 1"),
        Err(SqlError::UnknownTable(table)) if table == "ghosts"
    ));
    assert!(matches!(
        plan_delete("DELETE FROM posts WHERE nope = 1"),
        Err(SqlError::UnknownColumn { column, .. }) if column == "nope"
    ));
}
