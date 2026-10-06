//! `INSERT`, `UPDATE`, and `DELETE` statements.

use wasm_dbms_api::prelude::SqlError;

use super::Parser;
use super::condition::Aggregates;
use crate::ast::{Assignment, Condition, Delete, Insert, Update};
use crate::lexer::{Keyword, Token};

impl Parser {
    /// Parses `INSERT INTO table (column, ...) VALUES (value, ...)`.
    pub(super) fn insert(&mut self) -> Result<Insert, SqlError> {
        self.advance();
        self.expect_keyword(Keyword::Into)?;
        let table = self.identifier("a table name")?;

        if self.at_keyword(Keyword::Values) {
            return Err(self.error("INSERT requires an explicit column list"));
        }
        self.expect(&Token::LeftParen, "`(`")?;
        let mut columns: Vec<String> = Vec::new();
        loop {
            let position = self.position();
            let column = self.identifier("a column name")?;
            if columns.contains(&column) {
                return Err(position.error(format!("duplicate column `{column}`")));
            }
            columns.push(column);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RightParen, "`,` or `)`")?;

        let values_position = self.position();
        self.expect_keyword(Keyword::Values)?;
        self.expect(&Token::LeftParen, "`(`")?;
        let mut values = Vec::new();
        loop {
            values.push(self.value()?);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RightParen, "`,` or `)`")?;

        if columns.len() != values.len() {
            return Err(values_position.error(format!(
                "column count ({columns}) does not match value count ({values})",
                columns = columns.len(),
                values = values.len()
            )));
        }
        if *self.peek() == Token::Comma {
            return Err(self.error("multi-row INSERT is not supported"));
        }
        Ok(Insert {
            table,
            columns,
            values,
        })
    }

    /// Parses `UPDATE table SET column = value, ... WHERE condition`.
    pub(super) fn update(&mut self) -> Result<Update, SqlError> {
        self.advance();
        let table = self.identifier("a table name")?;
        self.expect_keyword(Keyword::Set)?;
        let mut assignments: Vec<Assignment> = Vec::new();
        loop {
            let position = self.position();
            let column = self.identifier("a column name")?;
            if assignments.iter().any(|a| a.column == column) {
                return Err(position.error(format!("duplicate column `{column}`")));
            }
            self.expect(&Token::Eq, "`=`")?;
            let value = self.value()?;
            assignments.push(Assignment { column, value });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        let filter = self.required_where()?;
        Ok(Update {
            table,
            assignments,
            filter,
        })
    }

    /// Parses `DELETE FROM table WHERE condition [CASCADE | RESTRICT]`.
    pub(super) fn delete(&mut self) -> Result<Delete, SqlError> {
        self.advance();
        self.expect_keyword(Keyword::From)?;
        let table = self.identifier("a table name")?;
        let filter = self.required_where()?;
        let cascade = self.eat_keyword(Keyword::Cascade);
        if !cascade {
            self.eat_keyword(Keyword::Restrict);
        }
        Ok(Delete {
            table,
            filter,
            cascade,
        })
    }

    /// Parses the mandatory `WHERE` clause of an `UPDATE` or `DELETE`.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::MissingWhereClause`] when the statement ends where
    /// `WHERE` should be, so that a write can never silently hit every row.
    fn required_where(&mut self) -> Result<Condition, SqlError> {
        if self.eat_keyword(Keyword::Where) {
            return self.condition(Aggregates::Forbidden);
        }
        Err(match self.peek() {
            Token::Eof
            | Token::Semicolon
            | Token::Keyword(Keyword::Cascade | Keyword::Restrict) => SqlError::MissingWhereClause,
            _ => self.unexpected("`WHERE`"),
        })
    }
}
