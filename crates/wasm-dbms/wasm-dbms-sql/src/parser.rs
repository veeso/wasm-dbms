//! Recursive-descent parser for the supported SQL dialect.

mod condition;
mod dml;
mod select;
#[cfg(test)]
mod tests;

use wasm_dbms_api::prelude::SqlError;

use crate::ast::Statement;
use crate::lexer::{Keyword, Position, Spanned, Token, tokenize};

/// Maximum number of terms in the conditions of one statement.
///
/// Every comparison, `IN`, `LIKE`, and `IS NULL` test, every `AND`, `OR`, and
/// `NOT`, and every parenthesized group counts as one term. The bound keeps
/// the recursion depth of the parser, and of everything that later walks the
/// condition tree, small enough for the limited stack of a WASM runtime.
pub(crate) const MAX_CONDITION_TERMS: usize = 256;

const STATEMENT_START: &str =
    "a statement (SELECT, INSERT, UPDATE, DELETE, BEGIN, COMMIT or ROLLBACK)";

/// A parsed statement and the number of `?` placeholders it contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedStatement {
    /// The statement.
    pub statement: Statement,
    /// Number of `?` placeholders; the statement needs that many parameters.
    pub parameter_count: usize,
}

/// Parses one SQL statement.
///
/// The statement may end with a single `;`. Keywords are case-insensitive and
/// identifiers are case-sensitive. Names are not checked against a schema:
/// that happens when the statement is executed.
///
/// # Errors
///
/// - [`SqlError::Parse`] when `sql` is not a single statement of the
///   supported grammar. The error carries the 1-based line and column of the
///   offending token.
/// - [`SqlError::MissingWhereClause`] when an `UPDATE` or `DELETE` has no
///   `WHERE` clause.
///
/// # Examples
///
/// ```
/// use wasm_dbms_sql::ast::Statement;
///
/// let parsed = wasm_dbms_sql::parse("DELETE FROM users WHERE id = ?")?;
/// assert!(matches!(parsed.statement, Statement::Delete(_)));
/// assert_eq!(parsed.parameter_count, 1);
/// # Ok::<(), wasm_dbms_api::prelude::SqlError>(())
/// ```
pub fn parse(sql: &str) -> Result<ParsedStatement, SqlError> {
    let mut parser = Parser::new(sql)?;
    let statement = parser.statement()?;
    parser.finish()?;
    Ok(ParsedStatement {
        statement,
        parameter_count: parser.parameters,
    })
}

/// Token cursor shared by the statement parsers.
struct Parser {
    /// The tokens of the statement; the last one is always [`Token::Eof`].
    tokens: Vec<Spanned>,
    cursor: usize,
    /// Number of `?` placeholders consumed so far.
    parameters: usize,
    /// Number of condition terms consumed so far.
    condition_terms: usize,
}

impl Parser {
    fn new(sql: &str) -> Result<Self, SqlError> {
        Ok(Self {
            tokens: tokenize(sql)?,
            cursor: 0,
            parameters: 0,
            condition_terms: 0,
        })
    }

    /// Returns the token `offset` places after the cursor, or `Eof` past the end.
    fn peek_at(&self, offset: usize) -> &Token {
        let index = (self.cursor + offset).min(self.tokens.len() - 1);
        &self.tokens[index].token
    }

    fn peek(&self) -> &Token {
        self.peek_at(0)
    }

    /// Returns the position of the current token.
    fn position(&self) -> Position {
        self.tokens[self.cursor].position
    }

    /// Moves past the current token; the cursor never moves past `Eof`.
    fn advance(&mut self) {
        if self.cursor < self.tokens.len() - 1 {
            self.cursor += 1;
        }
    }

    fn at_keyword(&self, keyword: Keyword) -> bool {
        *self.peek() == Token::Keyword(keyword)
    }

    /// Consumes the current token if it equals `token`.
    fn eat(&mut self, token: &Token) -> bool {
        let found = self.peek() == token;
        if found {
            self.advance();
        }
        found
    }

    fn eat_keyword(&mut self, keyword: Keyword) -> bool {
        self.eat(&Token::Keyword(keyword))
    }

    /// Builds a parse error located at the current token.
    fn error(&self, msg: impl Into<String>) -> SqlError {
        self.position().error(msg)
    }

    /// Builds an `expected ..., found ...` error located at the current token.
    fn unexpected(&self, expected: &str) -> SqlError {
        self.error(format!(
            "expected {expected}, found {found}",
            found = self.peek().describe()
        ))
    }

    /// Consumes `token`, or fails with `expected {expected}, found ...`.
    fn expect(&mut self, token: &Token, expected: &str) -> Result<(), SqlError> {
        if self.eat(token) {
            Ok(())
        } else {
            Err(self.unexpected(expected))
        }
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> Result<(), SqlError> {
        let expected = format!("`{keyword}`", keyword = keyword.as_str());
        self.expect(&Token::Keyword(keyword), &expected)
    }

    /// Consumes an identifier; `expected` names its role, e.g. `a table name`.
    fn identifier(&mut self, expected: &str) -> Result<String, SqlError> {
        match self.peek() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            Token::Keyword(_) => Err(self.error(format!(
                "expected {expected}, found {found} (reserved words must be double-quoted to be \
                 used as names)",
                found = self.peek().describe()
            ))),
            _ => Err(self.unexpected(expected)),
        }
    }

    /// Returns the 0-based index of the `?` placeholder being consumed.
    fn next_parameter(&mut self) -> usize {
        let index = self.parameters;
        self.parameters += 1;
        index
    }

    fn statement(&mut self) -> Result<Statement, SqlError> {
        match self.peek() {
            Token::Keyword(Keyword::Select) => self
                .select()
                .map(|select| Statement::Select(Box::new(select))),
            Token::Keyword(Keyword::Insert) => self.insert().map(Statement::Insert),
            Token::Keyword(Keyword::Update) => self.update().map(Statement::Update),
            Token::Keyword(Keyword::Delete) => self.delete().map(Statement::Delete),
            Token::Keyword(Keyword::Begin) => Ok(self.transaction(Statement::Begin)),
            Token::Keyword(Keyword::Commit) => Ok(self.transaction(Statement::Commit)),
            Token::Keyword(Keyword::Rollback) => Ok(self.transaction(Statement::Rollback)),
            _ => Err(self.unexpected(STATEMENT_START)),
        }
    }

    /// Consumes `BEGIN`, `COMMIT`, or `ROLLBACK` and an optional `TRANSACTION`.
    fn transaction(&mut self, statement: Statement) -> Statement {
        self.advance();
        self.eat_keyword(Keyword::Transaction);
        statement
    }

    /// Consumes the optional trailing `;` and checks nothing follows.
    fn finish(&mut self) -> Result<(), SqlError> {
        let terminated = self.eat(&Token::Semicolon);
        match self.peek() {
            Token::Eof => Ok(()),
            _ if terminated => Err(self.error("multiple statements are not supported")),
            _ => Err(self.unexpected("end of statement")),
        }
    }
}
