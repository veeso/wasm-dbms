use wasm_dbms_api::prelude::SqlError;

use super::{Keyword, Position, Token, tokenize};

/// Tokens of `sql`, without positions and without the trailing `Eof`.
fn tokens(sql: &str) -> Vec<Token> {
    let mut tokens: Vec<Token> = tokenize(sql)
        .expect("tokenize failed")
        .into_iter()
        .map(|spanned| spanned.token)
        .collect();
    assert_eq!(tokens.pop(), Some(Token::Eof));
    tokens
}

/// Position of every token of `sql`, including the trailing `Eof`.
fn positions(sql: &str) -> Vec<(usize, usize)> {
    tokenize(sql)
        .expect("tokenize failed")
        .into_iter()
        .map(|spanned| (spanned.position.line, spanned.position.col))
        .collect()
}

/// `(line, col, msg)` of the parse error raised for `sql`.
fn error(sql: &str) -> (usize, usize, String) {
    match tokenize(sql) {
        Err(SqlError::Parse { line, col, msg }) => (line, col, msg),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

fn identifier(name: &str) -> Token {
    Token::Identifier(name.to_string())
}

#[test]
fn test_empty_input_is_only_eof() {
    let all = tokenize("").expect("tokenize failed");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].token, Token::Eof);
    assert_eq!(all[0].position, Position { line: 1, col: 1 });
}

#[test]
fn test_keywords_are_case_insensitive() {
    assert_eq!(
        tokens("select SELECT SeLeCt"),
        vec![
            Token::Keyword(Keyword::Select),
            Token::Keyword(Keyword::Select),
            Token::Keyword(Keyword::Select),
        ]
    );
}

#[test]
fn test_every_keyword_round_trips_through_its_spelling() {
    for keyword in Keyword::ALL {
        let upper = keyword.as_str();
        assert_eq!(upper, upper.to_ascii_uppercase());
        assert_eq!(Keyword::from_word(upper), Some(*keyword));
        assert_eq!(
            Keyword::from_word(&upper.to_ascii_lowercase()),
            Some(*keyword)
        );
        assert_eq!(tokens(upper), vec![Token::Keyword(*keyword)]);
    }
}

#[test]
fn test_keyword_list_has_no_duplicates() {
    for (index, keyword) in Keyword::ALL.iter().enumerate() {
        assert!(
            Keyword::ALL[index + 1..]
                .iter()
                .all(|other| other.as_str() != keyword.as_str()),
            "duplicate keyword {keyword:?}"
        );
    }
}

#[test]
fn test_identifiers_keep_their_case() {
    assert_eq!(
        tokens("users Users _tmp col_1 selected"),
        vec![
            identifier("users"),
            identifier("Users"),
            identifier("_tmp"),
            identifier("col_1"),
            identifier("selected"),
        ]
    );
}

#[test]
fn test_aggregate_names_are_plain_identifiers() {
    assert_eq!(
        tokens("count SUM avg Min MAX"),
        vec![
            identifier("count"),
            identifier("SUM"),
            identifier("avg"),
            identifier("Min"),
            identifier("MAX"),
        ]
    );
}

#[test]
fn test_quoted_identifiers() {
    assert_eq!(
        tokens(r#""order" "Select" "my col" "a""b" "é""#),
        vec![
            identifier("order"),
            identifier("Select"),
            identifier("my col"),
            identifier("a\"b"),
            identifier("é"),
        ]
    );
}

#[test]
fn test_string_literals() {
    assert_eq!(
        tokens("'Alice' '' 'it''s' 'a\nb' '-- no comment' '\"q\"'"),
        vec![
            Token::String("Alice".to_string()),
            Token::String(String::new()),
            Token::String("it's".to_string()),
            Token::String("a\nb".to_string()),
            Token::String("-- no comment".to_string()),
            Token::String("\"q\"".to_string()),
        ]
    );
}

#[test]
fn test_numbers() {
    assert_eq!(
        tokens("0 42 007 18446744073709551615 1.5 0.25 10.0"),
        vec![
            Token::Integer(0),
            Token::Integer(42),
            Token::Integer(7),
            Token::Integer(u64::MAX),
            Token::Float("1.5".to_string()),
            Token::Float("0.25".to_string()),
            Token::Float("10.0".to_string()),
        ]
    );
}

#[test]
fn test_number_followed_by_dot_without_digits_is_integer_then_dot() {
    assert_eq!(tokens("1."), vec![Token::Integer(1), Token::Dot]);
    assert_eq!(
        tokens("1.x"),
        vec![Token::Integer(1), Token::Dot, identifier("x")]
    );
}

#[test]
fn test_minus_is_a_separate_token() {
    assert_eq!(tokens("-5"), vec![Token::Minus, Token::Integer(5)]);
    assert_eq!(
        tokens("- 1.5"),
        vec![Token::Minus, Token::Float("1.5".to_string())]
    );
}

#[test]
fn test_punctuation_and_operators() {
    assert_eq!(
        tokens(", . ( ) * ; ? = != <> < <= > >="),
        vec![
            Token::Comma,
            Token::Dot,
            Token::LeftParen,
            Token::RightParen,
            Token::Star,
            Token::Semicolon,
            Token::Placeholder,
            Token::Eq,
            Token::NotEq,
            Token::NotEq,
            Token::Lt,
            Token::LtEq,
            Token::Gt,
            Token::GtEq,
        ]
    );
}

#[test]
fn test_tokens_do_not_need_whitespace_between_them() {
    assert_eq!(
        tokens("a.b>=1,c<>'x'"),
        vec![
            identifier("a"),
            Token::Dot,
            identifier("b"),
            Token::GtEq,
            Token::Integer(1),
            Token::Comma,
            identifier("c"),
            Token::NotEq,
            Token::String("x".to_string()),
        ]
    );
}

#[test]
fn test_line_comments_are_skipped() {
    assert_eq!(
        tokens("a -- trailing comment\nb --no space\n-- only comment"),
        vec![identifier("a"), identifier("b")]
    );
}

#[test]
fn test_block_comments_are_skipped() {
    assert_eq!(
        tokens("a /* one */ b /* multi\nline */ c /**/ d /* -- */ e"),
        vec![
            identifier("a"),
            identifier("b"),
            identifier("c"),
            identifier("d"),
            identifier("e"),
        ]
    );
}

#[test]
fn test_block_comments_do_not_nest() {
    // the first `*/` closes the comment, leaving `b */` behind
    assert_eq!(tokens("/* a /* inner */ b"), vec![identifier("b")]);
    assert_eq!(error("/* a /* inner */ b */").2, "unexpected character `/`");
}

#[test]
fn test_positions_are_one_based_lines_and_columns() {
    assert_eq!(
        positions("SELECT *\n  FROM users;"),
        vec![(1, 1), (1, 8), (2, 3), (2, 8), (2, 13), (2, 14)]
    );
}

#[test]
fn test_positions_count_characters_not_bytes() {
    // `é` is two bytes but one column
    assert_eq!(positions("'é' x"), vec![(1, 1), (1, 5), (1, 6)]);
}

#[test]
fn test_positions_after_multi_line_tokens() {
    assert_eq!(positions("'a\nb' x"), vec![(1, 1), (2, 4), (2, 5)]);
    assert_eq!(positions("/* a\nb */ x"), vec![(2, 6), (2, 7)]);
    assert_eq!(positions("\t\r\n x"), vec![(2, 2), (2, 3)]);
}

#[test]
fn test_unterminated_string_reports_opening_quote() {
    assert_eq!(
        error("SELECT 'abc"),
        (1, 8, "unterminated string literal".to_string())
    );
    assert_eq!(
        error("'it''s"),
        (1, 1, "unterminated string literal".to_string())
    );
}

#[test]
fn test_unterminated_quoted_identifier() {
    assert_eq!(
        error("SELECT \"abc"),
        (1, 8, "unterminated quoted identifier".to_string())
    );
}

#[test]
fn test_empty_quoted_identifier() {
    assert_eq!(
        error("SELECT \"\""),
        (1, 8, "quoted identifier cannot be empty".to_string())
    );
}

#[test]
fn test_unterminated_block_comment() {
    assert_eq!(
        error("a\n /* never closed"),
        (2, 2, "unterminated block comment".to_string())
    );
    assert_eq!(
        error("/*/"),
        (1, 1, "unterminated block comment".to_string())
    );
}

#[test]
fn test_backticks_are_rejected() {
    assert_eq!(
        error("SELECT `name`"),
        (
            1,
            8,
            "backtick-quoted identifiers are not supported; use double quotes".to_string()
        )
    );
}

#[test]
fn test_named_and_numbered_placeholders_are_rejected() {
    let msg = "named and numbered placeholders are not supported; use `?`".to_string();
    assert_eq!(error("id = :id"), (1, 6, msg.clone()));
    assert_eq!(error("id = $1"), (1, 6, msg.clone()));
    assert_eq!(error("id = @id"), (1, 6, msg));
}

#[test]
fn test_invalid_numeric_literals() {
    assert_eq!(
        error("x = 12abc"),
        (1, 5, "invalid numeric literal".to_string())
    );
    assert_eq!(error("1e5"), (1, 1, "invalid numeric literal".to_string()));
    assert_eq!(error("1.5x"), (1, 1, "invalid numeric literal".to_string()));
    assert_eq!(error("7_"), (1, 1, "invalid numeric literal".to_string()));
}

#[test]
fn test_integer_literal_overflow() {
    assert_eq!(
        error("18446744073709551616"),
        (1, 1, "integer literal is too large".to_string())
    );
}

#[test]
fn test_unexpected_characters() {
    assert_eq!(
        error("a + b"),
        (1, 3, "unexpected character `+`".to_string())
    );
    assert_eq!(
        error("a ! b"),
        (1, 3, "unexpected character `!`".to_string())
    );
    assert_eq!(error("é"), (1, 1, "unexpected character `é`".to_string()));
    assert_eq!(
        error("a / b"),
        (1, 3, "unexpected character `/`".to_string())
    );
    assert_eq!(error("[a]"), (1, 1, "unexpected character `[`".to_string()));
}

#[test]
fn test_token_descriptions() {
    let described: Vec<String> = tokenize("SELECT name 42 1.5 'x' ? , . ( ) * ; - = != < <= > >=")
        .expect("tokenize failed")
        .iter()
        .map(|spanned| spanned.token.describe())
        .collect();
    assert_eq!(
        described,
        vec![
            "keyword `SELECT`",
            "identifier `name`",
            "number `42`",
            "number `1.5`",
            "string literal",
            "`?`",
            "`,`",
            "`.`",
            "`(`",
            "`)`",
            "`*`",
            "`;`",
            "`-`",
            "`=`",
            "`!=`",
            "`<`",
            "`<=`",
            "`>`",
            "`>=`",
            "end of input",
        ]
    );
}
