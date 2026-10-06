//! `SELECT` statements.

use wasm_dbms_api::prelude::SqlError;

use super::Parser;
use super::condition::Aggregates;
use crate::ast::{
    ColumnRef, JoinClause, JoinKind, OrderItem, Projection, RowCount, Select, SelectItem, TableRef,
};
use crate::lexer::{Keyword, Token};

impl Parser {
    /// Parses a `SELECT` statement.
    ///
    /// ```text
    /// SELECT [DISTINCT] select_list FROM table_ref { join }
    ///   [WHERE condition] [GROUP BY columns] [HAVING condition]
    ///   [ORDER BY order_items] [LIMIT count] [OFFSET count]
    /// ```
    pub(super) fn select(&mut self) -> Result<Select, SqlError> {
        self.advance();
        let distinct = self.eat_keyword(Keyword::Distinct);
        let projection = self.projection()?;
        self.expect_keyword(Keyword::From)?;
        let from = self.table_ref()?;

        let mut joins = Vec::new();
        while let Some(kind) = self.join_kind()? {
            joins.push(self.join(kind)?);
        }

        let filter = if self.eat_keyword(Keyword::Where) {
            Some(self.condition(Aggregates::Forbidden)?)
        } else {
            None
        };

        let group_by = if self.eat_keyword(Keyword::Group) {
            self.expect_keyword(Keyword::By)?;
            self.group_by_columns()?
        } else {
            Vec::new()
        };

        let having = if self.eat_keyword(Keyword::Having) {
            Some(self.condition(Aggregates::Allowed)?)
        } else {
            None
        };

        let order_by = if self.eat_keyword(Keyword::Order) {
            self.expect_keyword(Keyword::By)?;
            self.order_items()?
        } else {
            Vec::new()
        };

        // `LIMIT` and `OFFSET` are accepted in either order, once each
        let mut limit = None;
        let mut offset = None;
        loop {
            if limit.is_none() && self.eat_keyword(Keyword::Limit) {
                limit = Some(self.row_count("`LIMIT`")?);
            } else if offset.is_none() && self.eat_keyword(Keyword::Offset) {
                offset = Some(self.row_count("`OFFSET`")?);
            } else {
                break;
            }
        }

        Ok(Select {
            distinct,
            projection,
            from,
            joins,
            filter,
            group_by,
            having,
            order_by,
            limit,
            offset,
        })
    }

    /// Parses `*` or `operand [AS alias], ...`.
    fn projection(&mut self) -> Result<Projection, SqlError> {
        if self.eat(&Token::Star) {
            if *self.peek() == Token::Comma {
                return Err(self.error("`*` cannot be combined with other select items"));
            }
            return Ok(Projection::All);
        }
        let mut items = Vec::new();
        loop {
            let operand = self.operand(Aggregates::Allowed)?;
            let alias = if self.eat_keyword(Keyword::As) {
                Some(self.identifier("an alias")?)
            } else {
                None
            };
            items.push(SelectItem { operand, alias });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(Projection::Items(items))
    }

    /// Parses `table [[AS] alias]`.
    fn table_ref(&mut self) -> Result<TableRef, SqlError> {
        let name = self.identifier("a table name")?;
        let has_alias =
            self.eat_keyword(Keyword::As) || matches!(self.peek(), Token::Identifier(_));
        let alias = if has_alias {
            Some(self.identifier("an alias")?)
        } else {
            None
        };
        Ok(TableRef { name, alias })
    }

    /// Consumes the keywords that open a join and returns the join type.
    ///
    /// Returns `None`, consuming nothing, when no join starts here.
    fn join_kind(&mut self) -> Result<Option<JoinKind>, SqlError> {
        let (kind, outer_allowed) = match self.peek() {
            Token::Keyword(Keyword::Join) => (JoinKind::Inner, false),
            Token::Keyword(Keyword::Inner) => {
                self.advance();
                (JoinKind::Inner, false)
            }
            Token::Keyword(Keyword::Left) => {
                self.advance();
                (JoinKind::Left, true)
            }
            Token::Keyword(Keyword::Right) => {
                self.advance();
                (JoinKind::Right, true)
            }
            Token::Keyword(Keyword::Full) => {
                self.advance();
                (JoinKind::Full, true)
            }
            _ => return Ok(None),
        };
        if outer_allowed {
            self.eat_keyword(Keyword::Outer);
        }
        self.expect_keyword(Keyword::Join)?;
        Ok(Some(kind))
    }

    /// Parses `table_ref ON column = column`, after the join keywords.
    fn join(&mut self, kind: JoinKind) -> Result<JoinClause, SqlError> {
        let table = self.table_ref()?;
        self.expect_keyword(Keyword::On)?;
        let left = self.column_ref()?;
        self.expect(&Token::Eq, "`=`")?;
        let right = self.column_ref()?;
        if self.at_keyword(Keyword::And) || self.at_keyword(Keyword::Or) {
            return Err(self.error("a join condition must be a single `left = right` comparison"));
        }
        Ok(JoinClause {
            kind,
            table,
            left,
            right,
        })
    }

    /// Parses the `column, ...` list that follows `GROUP BY`.
    fn group_by_columns(&mut self) -> Result<Vec<ColumnRef>, SqlError> {
        let mut columns = Vec::new();
        loop {
            columns.push(self.column_ref()?);
            if !self.eat(&Token::Comma) {
                return Ok(columns);
            }
        }
    }

    /// Parses the `operand [ASC | DESC], ...` list that follows `ORDER BY`.
    fn order_items(&mut self) -> Result<Vec<OrderItem>, SqlError> {
        let mut items = Vec::new();
        loop {
            let operand = self.operand(Aggregates::Allowed)?;
            let descending = self.eat_keyword(Keyword::Desc);
            if !descending {
                self.eat_keyword(Keyword::Asc);
            }
            items.push(OrderItem {
                operand,
                descending,
            });
            if !self.eat(&Token::Comma) {
                return Ok(items);
            }
        }
    }

    /// Parses the argument of `LIMIT` or `OFFSET`; `clause` names it in errors.
    fn row_count(&mut self, clause: &str) -> Result<RowCount, SqlError> {
        let count = match self.peek() {
            Token::Integer(value) => RowCount::Value(*value),
            Token::Placeholder => {
                self.advance();
                return Ok(RowCount::Parameter(self.next_parameter()));
            }
            _ => {
                return Err(
                    self.unexpected(&format!("a non-negative integer or `?` after {clause}"))
                );
            }
        };
        self.advance();
        Ok(count)
    }
}
