//! Checks that keep the SQL documentation in step with the parser.

use crate::lexer::Keyword;
use crate::parser::parse;

const SQL_REFERENCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../docs/reference/sql.md"
));
const SQL_GUIDE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../docs/guides/sql.md"
));

/// Returns the body of every fenced code block of `markdown` tagged `language`.
fn code_blocks(markdown: &str, language: &str) -> Vec<String> {
    let opening = format!("```{language}");
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in markdown.lines() {
        match (&mut current, line.trim_end()) {
            (Some(lines), "```") => {
                blocks.push(lines.join("\n"));
                current = None;
            }
            (Some(lines), _) => lines.push(line),
            (None, fence) if fence == opening => current = Some(Vec::new()),
            (None, _) => {}
        }
    }
    assert!(current.is_none(), "unterminated `{language}` code block");
    blocks
}

/// Splits a code block into statements; each one ends with a `;` at the end
/// of a line.
fn statements(block: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in block.lines() {
        current.push(line);
        if line.trim_end().ends_with(';') {
            statements.push(current.join("\n"));
            current.clear();
        }
    }
    assert!(
        current.iter().all(|line| line.trim().is_empty()),
        "SQL example does not end with `;`:\n{block}"
    );
    statements
}

/// Returns the text of the `## heading` section of `markdown`.
fn section<'a>(markdown: &'a str, heading: &str) -> &'a str {
    let start = markdown
        .find(&format!("\n## {heading}\n"))
        .unwrap_or_else(|| panic!("section `{heading}` not found"));
    let body = &markdown[start + 1..];
    let end = body[1..].find("\n## ").map_or(body.len(), |end| end + 1);
    &body[..end]
}

#[test]
fn test_every_sql_example_in_the_docs_parses() {
    for (file, markdown) in [
        ("reference/sql.md", SQL_REFERENCE),
        ("guides/sql.md", SQL_GUIDE),
    ] {
        let blocks = code_blocks(markdown, "sql");
        assert!(!blocks.is_empty(), "{file} has no SQL examples");
        for statement in blocks.iter().flat_map(|block| statements(block)) {
            if let Err(error) = parse(&statement) {
                panic!("SQL example in docs/{file} does not parse: {error}\n{statement}");
            }
        }
    }
}

#[test]
fn test_reserved_words_in_the_reference_match_the_lexer() {
    let listed = code_blocks(section(SQL_REFERENCE, "Reserved Words"), "text");
    assert_eq!(listed.len(), 1, "expected one list of reserved words");
    let mut documented: Vec<&str> = listed[0].split_whitespace().collect();
    documented.sort_unstable();

    let mut reserved: Vec<&str> = Keyword::ALL
        .iter()
        .map(|keyword| keyword.as_str())
        .collect();
    reserved.sort_unstable();

    assert_eq!(documented, reserved);
}

#[test]
fn test_grammar_in_the_reference_mentions_every_keyword_of_the_dialect() {
    let grammar = code_blocks(section(SQL_REFERENCE, "Grammar"), "text");
    assert_eq!(grammar.len(), 1, "expected one grammar block");
    // reserved only so that unsupported clauses are rejected clearly
    let unsupported = ["CROSS", "EXCEPT", "INTERSECT", "NATURAL", "UNION", "USING"];
    for keyword in Keyword::ALL.iter().map(|keyword| keyword.as_str()) {
        let quoted = format!("\"{keyword}\"");
        assert_eq!(
            grammar[0].contains(&quoted),
            !unsupported.contains(&keyword),
            "keyword {keyword} in the grammar of docs/reference/sql.md"
        );
    }
}

#[test]
fn test_code_block_helpers() {
    let markdown =
        "intro\n```sql\nSELECT 1;\n```\n```text\nnot sql\n```\n```sql\nA;\nB\n  C;\n```\n";
    assert_eq!(
        code_blocks(markdown, "sql"),
        vec!["SELECT 1;".to_string(), "A;\nB\n  C;".to_string()]
    );
    assert_eq!(
        statements("A;\nB\n  C;\n"),
        vec!["A;".to_string(), "B\n  C;".to_string()]
    );
    assert_eq!(
        section("# T\n\n## One\nfirst\n\n## Two\nsecond\n", "One"),
        "## One\nfirst\n"
    );
}
