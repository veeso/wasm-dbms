//! Runner for the `.sqltest` parser fixtures.
//!
//! A fixture file is a list of cases. Each case has a name, a SQL statement,
//! and the expected outcome of parsing it:
//!
//! ```text
//! # Lines before the first case are comments.
//!
//! === case name
//! SELECT id FROM users
//! ---
//! (select (columns id) (from users))
//! ```
//!
//! The SQL is every line between the `=== name` header and the `---`
//! separator, joined with `\n` and otherwise untouched, so its line and column
//! numbers are the ones a parse error reports. The expectation is everything
//! up to the next header; whitespace runs in it are not significant. It is
//! one of:
//!
//! - an S-expression, as rendered by [`super::sexpr`], for SQL that must parse;
//! - `error <line>:<col>: <message>` for SQL that must fail with that exact
//!   [`SqlError::Parse`];
//! - `error missing-where` for SQL that must fail with
//!   [`SqlError::MissingWhereClause`].

use wasm_dbms_api::prelude::SqlError;

use super::sexpr;
use crate::parser::parse;

const CASE_HEADER: &str = "=== ";
const SEPARATOR: &str = "---";

/// One fixture case.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Case {
    pub(super) name: String,
    /// 1-based line of the `=== name` header in the fixture file.
    pub(super) line: usize,
    pub(super) sql: String,
    pub(super) expected: String,
}

/// Splits a fixture file into its cases.
///
/// # Panics
///
/// Panics when the file is malformed: text before the first case that is not
/// a `#` comment, a case without a `---` separator, an empty or duplicate
/// case name, a missing expectation, or no case at all.
pub(super) fn load(file: &str, source: &str) -> Vec<Case> {
    /// A case whose sections are still lists of lines.
    struct Raw<'a> {
        name: String,
        line: usize,
        sql: Vec<&'a str>,
        expected: Vec<&'a str>,
    }

    let mut cases: Vec<Raw<'_>> = Vec::new();
    let mut in_sql = false;
    for (index, line) in source.lines().enumerate() {
        let number = index + 1;
        if let Some(name) = line.strip_prefix(CASE_HEADER) {
            let name = name.trim().to_string();
            assert!(!name.is_empty(), "{file}:{number}: empty case name");
            assert!(
                cases.iter().all(|case| case.name != name),
                "{file}:{number}: duplicate case name `{name}`"
            );
            assert!(
                !in_sql,
                "{file}:{number}: previous case has no `---` separator"
            );
            cases.push(Raw {
                name,
                line: number,
                sql: Vec::new(),
                expected: Vec::new(),
            });
            in_sql = true;
            continue;
        }
        let Some(case) = cases.last_mut() else {
            assert!(
                line.trim().is_empty() || line.starts_with('#'),
                "{file}:{number}: text before the first case must be a `#` comment"
            );
            continue;
        };
        if in_sql && line == SEPARATOR {
            in_sql = false;
        } else if in_sql {
            case.sql.push(line);
        } else {
            case.expected.push(line);
        }
    }
    assert!(!in_sql, "{file}: last case has no `---` separator");
    assert!(!cases.is_empty(), "{file}: no cases found");

    cases
        .into_iter()
        .map(|raw| {
            let expected = normalize(&raw.expected.join(" "));
            assert!(
                !expected.is_empty(),
                "{file}:{line}: case `{name}` has no expectation",
                line = raw.line,
                name = raw.name
            );
            Case {
                name: raw.name,
                line: raw.line,
                sql: raw.sql.join("\n"),
                expected,
            }
        })
        .collect()
}

/// Collapses every whitespace run into one space and trims both ends.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Renders the outcome of parsing `sql` in the notation of the fixtures.
pub(super) fn outcome(sql: &str) -> String {
    match parse(sql) {
        Ok(parsed) => sexpr::statement(&parsed.statement),
        Err(SqlError::Parse { line, col, msg }) => format!("error {line}:{col}: {msg}"),
        Err(SqlError::MissingWhereClause) => "error missing-where".to_string(),
        Err(other) => format!("unexpected error kind: {other:?}"),
    }
}

/// Runs every case of a fixture file and reports all mismatches at once.
///
/// # Panics
///
/// Panics when at least one case does not produce its expected outcome.
pub(super) fn run(file: &str, source: &str) {
    let cases = load(file, source);
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|case| {
            let actual = normalize(&outcome(&case.sql));
            (actual != case.expected).then(|| {
                format!(
                    "{file}:{line}: case `{name}`\n       sql: {sql:?}\n  expected: {expected}\n    actual: {actual}",
                    line = case.line,
                    name = case.name,
                    sql = case.sql,
                    expected = case.expected,
                )
            })
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{count} of {total} fixture cases failed:\n\n{report}\n",
        count = failures.len(),
        total = cases.len(),
        report = failures.join("\n\n")
    );
}
