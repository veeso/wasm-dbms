mod fixtures;
mod sexpr;

use wasm_dbms_api::prelude::SqlError;

use super::{MAX_CONDITION_TERMS, parse};
use crate::ast::Statement;

/// Declares one test per fixture file and records the registered file names.
macro_rules! fixture_tests {
    ($($test:ident => $file:literal),+ $(,)?) => {
        /// Fixture files that have a test.
        const REGISTERED_FIXTURES: &[&str] = &[$($file),+];

        $(
            #[test]
            fn $test() {
                fixtures::run(
                    $file,
                    include_str!(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/tests/fixtures/parser/",
                        $file
                    )),
                );
            }
        )+
    };
}

fixture_tests! {
    test_fixture_aggregate => "aggregate.sqltest",
    test_fixture_condition => "condition.sqltest",
    test_fixture_delete => "delete.sqltest",
    test_fixture_insert => "insert.sqltest",
    test_fixture_join => "join.sqltest",
    test_fixture_lexical => "lexical.sqltest",
    test_fixture_select => "select.sqltest",
    test_fixture_transaction => "transaction.sqltest",
    test_fixture_update => "update.sqltest",
}

/// `(line, col, msg)` of the parse error raised for `sql`.
fn parse_error(sql: &str) -> (usize, usize, String) {
    match parse(sql) {
        Err(SqlError::Parse { line, col, msg }) => (line, col, msg),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

#[test]
fn test_every_fixture_file_is_registered() {
    let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/parser");
    let mut on_disk: Vec<String> = std::fs::read_dir(directory)
        .expect("fixture directory must exist")
        .map(|entry| {
            entry
                .expect("unreadable fixture directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    on_disk.sort();
    let mut registered: Vec<String> = REGISTERED_FIXTURES
        .iter()
        .map(|file| file.to_string())
        .collect();
    registered.sort();
    assert_eq!(
        on_disk, registered,
        "every file in {directory} needs an entry in `fixture_tests!`"
    );
}

#[test]
fn test_fixture_loader_splits_cases() {
    let source = "# comment\n\n=== first\nBEGIN\n---\n(begin)\n\n=== second\n\nUPDATE t\n  SET a = 1\n---\nerror\n  missing-where\n";
    let cases = fixtures::load("inline", source);
    assert_eq!(
        cases,
        vec![
            fixtures::Case {
                name: "first".to_string(),
                line: 3,
                sql: "BEGIN".to_string(),
                expected: "(begin)".to_string(),
            },
            fixtures::Case {
                name: "second".to_string(),
                line: 8,
                sql: "\nUPDATE t\n  SET a = 1".to_string(),
                expected: "error missing-where".to_string(),
            },
        ]
    );
}

#[test]
#[should_panic(expected = "duplicate case name `same`")]
fn test_fixture_loader_rejects_duplicate_names() {
    fixtures::load(
        "inline",
        "=== same\nBEGIN\n---\n(begin)\n=== same\nBEGIN\n---\n(begin)\n",
    );
}

#[test]
#[should_panic(expected = "previous case has no `---` separator")]
fn test_fixture_loader_rejects_a_missing_separator() {
    fixtures::load(
        "inline",
        "=== first\nBEGIN\n=== second\nBEGIN\n---\n(begin)\n",
    );
}

#[test]
#[should_panic(expected = "case `first` has no expectation")]
fn test_fixture_loader_rejects_a_missing_expectation() {
    fixtures::load("inline", "=== first\nBEGIN\n---\n\n");
}

#[test]
#[should_panic(expected = "1 of 2 fixture cases failed")]
fn test_fixture_runner_reports_mismatches() {
    fixtures::run(
        "inline",
        "=== right\nBEGIN\n---\n(begin)\n=== wrong\nBEGIN\n---\n(commit)\n",
    );
}

#[test]
fn test_parameter_count_matches_placeholders() {
    for (sql, expected) in [
        ("BEGIN", 0),
        ("DELETE FROM t WHERE a = 1", 0),
        ("DELETE FROM t WHERE a = ?", 1),
        ("INSERT INTO t (a, b, c) VALUES (?, 1, ?)", 2),
        (
            "UPDATE t SET a = ?, b = ? WHERE c IN (?, ?) AND d LIKE ?",
            5,
        ),
        ("DELETE FROM t WHERE a = '?' /* ? */ -- ?", 0),
        ("SELECT * FROM t", 0),
        (
            "SELECT a FROM t WHERE b = ? GROUP BY a HAVING COUNT(*) > ? LIMIT ? OFFSET ?",
            4,
        ),
    ] {
        let parsed = parse(sql).expect("statement must parse");
        assert_eq!(parsed.parameter_count, expected, "{sql}");
    }
}

#[test]
fn test_statement_kinds() {
    assert_eq!(parse("BEGIN").unwrap().statement, Statement::Begin);
    assert_eq!(parse("COMMIT").unwrap().statement, Statement::Commit);
    assert_eq!(parse("ROLLBACK").unwrap().statement, Statement::Rollback);
    assert!(matches!(
        parse("SELECT * FROM t").unwrap().statement,
        Statement::Select(_)
    ));
    assert!(matches!(
        parse("INSERT INTO t (a) VALUES (1)").unwrap().statement,
        Statement::Insert(_)
    ));
    assert!(matches!(
        parse("UPDATE t SET a = 1 WHERE a = 2").unwrap().statement,
        Statement::Update(_)
    ));
    assert!(matches!(
        parse("DELETE FROM t WHERE a = 2").unwrap().statement,
        Statement::Delete(_)
    ));
}

#[test]
fn test_whitespace_only_and_trailing_whitespace() {
    assert_eq!(
        parse_error("  \n\t "),
        (
            2,
            3,
            "expected a statement (SELECT, INSERT, UPDATE, DELETE, BEGIN, COMMIT or ROLLBACK), \
             found end of input"
                .to_string()
        )
    );
    assert_eq!(
        parse("\n\n  BEGIN  ;  \n\n").unwrap().statement,
        Statement::Begin
    );
    assert_eq!(parse("BEGIN\r\n").unwrap().statement, Statement::Begin);
}

#[test]
fn test_lexer_errors_surface_through_parse() {
    assert_eq!(
        parse_error("DELETE FROM t WHERE a = 'oops"),
        (1, 25, "unterminated string literal".to_string())
    );
}

/// A condition of `terms` comparisons joined by `joiner`.
fn chained_condition(terms: usize, joiner: &str) -> String {
    vec!["a = 1"; terms].join(joiner)
}

#[test]
fn test_condition_at_the_term_limit_parses() {
    // n comparisons joined by n - 1 operators count as 2n - 1 terms
    let comparisons = MAX_CONDITION_TERMS.div_ceil(2);
    let sql = format!(
        "DELETE FROM t WHERE {condition}",
        condition = chained_condition(comparisons, " AND ")
    );
    assert!(parse(&sql).is_ok());
}

#[test]
fn test_condition_over_the_term_limit_is_rejected() {
    let comparisons = MAX_CONDITION_TERMS.div_ceil(2) + 1;
    let sql = format!(
        "DELETE FROM t WHERE {condition}",
        condition = chained_condition(comparisons, " OR ")
    );
    let (line, _, msg) = parse_error(&sql);
    assert_eq!(line, 1);
    assert_eq!(
        msg,
        format!("condition is too complex: more than {MAX_CONDITION_TERMS} terms")
    );
}

#[test]
fn test_deeply_nested_parentheses_are_rejected_without_overflowing_the_stack() {
    let depth = 100_000;
    let sql = format!(
        "DELETE FROM t WHERE {open}a = 1{close}",
        open = "(".repeat(depth),
        close = ")".repeat(depth)
    );
    let (_, col, msg) = parse_error(&sql);
    assert_eq!(
        msg,
        format!("condition is too complex: more than {MAX_CONDITION_TERMS} terms")
    );
    // "DELETE FROM t WHERE " is 20 characters; the first rejected `(` follows
    // MAX_CONDITION_TERMS accepted ones
    assert_eq!(col, 20 + MAX_CONDITION_TERMS + 1);
}

#[test]
fn test_long_not_chain_is_rejected_without_overflowing_the_stack() {
    let sql = format!(
        "DELETE FROM t WHERE {nots}a = 1",
        nots = "NOT ".repeat(100_000)
    );
    let (_, _, msg) = parse_error(&sql);
    assert_eq!(
        msg,
        format!("condition is too complex: more than {MAX_CONDITION_TERMS} terms")
    );
}

#[test]
fn test_large_in_list_is_not_limited() {
    let values = vec!["1"; 10_000].join(", ");
    let sql = format!("DELETE FROM t WHERE a IN ({values})");
    assert!(parse(&sql).is_ok());
}

#[test]
fn test_where_and_having_share_the_condition_term_limit() {
    // each clause alone is under the limit, together they exceed it
    let comparisons = MAX_CONDITION_TERMS.div_ceil(4) + 1;
    let sql = format!(
        "SELECT a FROM t WHERE {filter} GROUP BY a HAVING {having}",
        filter = chained_condition(comparisons, " AND "),
        having = chained_condition(comparisons, " AND ")
    );
    let (_, _, msg) = parse_error(&sql);
    assert_eq!(
        msg,
        format!("condition is too complex: more than {MAX_CONDITION_TERMS} terms")
    );
}

#[test]
fn test_long_select_and_order_lists_are_not_limited() {
    let columns = vec!["a"; 5_000].join(", ");
    let sql = format!("SELECT {columns} FROM t ORDER BY {columns}");
    assert!(parse(&sql).is_ok());
}
