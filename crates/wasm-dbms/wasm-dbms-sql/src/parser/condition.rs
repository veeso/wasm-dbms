//! Conditions, operands, and values.

use wasm_dbms_api::prelude::SqlError;

use super::{MAX_CONDITION_TERMS, Parser};
use crate::ast::{
    Aggregate, AggregateKind, ColumnRef, CompareOp, Condition, Literal, Operand, ValueExpr,
};
use crate::lexer::{Keyword, Token};

/// Whether aggregate function calls may appear as operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Aggregates {
    /// `HAVING`, the select list, and `ORDER BY`.
    Allowed,
    /// `WHERE`.
    Forbidden,
}

impl Parser {
    /// Parses `and_condition { OR and_condition }`.
    pub(super) fn condition(&mut self, aggregates: Aggregates) -> Result<Condition, SqlError> {
        let mut left = self.and_condition(aggregates)?;
        while self.at_keyword(Keyword::Or) {
            self.count_condition_term()?;
            self.advance();
            let right = self.and_condition(aggregates)?;
            left = Condition::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// Parses `not_condition { AND not_condition }`.
    fn and_condition(&mut self, aggregates: Aggregates) -> Result<Condition, SqlError> {
        let mut left = self.not_condition(aggregates)?;
        while self.at_keyword(Keyword::And) {
            self.count_condition_term()?;
            self.advance();
            let right = self.not_condition(aggregates)?;
            left = Condition::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// Parses `NOT not_condition | "(" condition ")" | predicate`.
    fn not_condition(&mut self, aggregates: Aggregates) -> Result<Condition, SqlError> {
        self.count_condition_term()?;
        if self.eat_keyword(Keyword::Not) {
            let inner = self.not_condition(aggregates)?;
            return Ok(Condition::Not(Box::new(inner)));
        }
        if self.eat(&Token::LeftParen) {
            let inner = self.condition(aggregates)?;
            self.expect(&Token::RightParen, "`)`")?;
            return Ok(inner);
        }
        self.predicate(aggregates)
    }

    /// Counts one condition term, failing at the current token past the limit.
    fn count_condition_term(&mut self) -> Result<(), SqlError> {
        self.condition_terms += 1;
        if self.condition_terms > MAX_CONDITION_TERMS {
            return Err(self.error(format!(
                "condition is too complex: more than {MAX_CONDITION_TERMS} terms"
            )));
        }
        Ok(())
    }

    /// Parses a comparison, `[NOT] IN`, `[NOT] LIKE`, or `IS [NOT] NULL` test.
    fn predicate(&mut self, aggregates: Aggregates) -> Result<Condition, SqlError> {
        let operand = self.operand(aggregates)?;
        let op = match self.peek() {
            Token::Eq => CompareOp::Eq,
            Token::NotEq => CompareOp::NotEq,
            Token::Lt => CompareOp::Lt,
            Token::LtEq => CompareOp::LtEq,
            Token::Gt => CompareOp::Gt,
            Token::GtEq => CompareOp::GtEq,
            Token::Keyword(Keyword::Is) => {
                self.advance();
                let negated = self.eat_keyword(Keyword::Not);
                self.expect_keyword(Keyword::Null)?;
                return Ok(Condition::IsNull { operand, negated });
            }
            Token::Keyword(Keyword::In) => {
                self.advance();
                return self.in_list(operand, false);
            }
            Token::Keyword(Keyword::Like) => {
                self.advance();
                return self.like(operand, false);
            }
            Token::Keyword(Keyword::Not) => {
                self.advance();
                if self.eat_keyword(Keyword::In) {
                    return self.in_list(operand, true);
                }
                if self.eat_keyword(Keyword::Like) {
                    return self.like(operand, true);
                }
                return Err(self.unexpected("`IN` or `LIKE` after `NOT`"));
            }
            _ => return Err(self.unexpected("a comparison operator, `IN`, `LIKE` or `IS`")),
        };
        self.advance();
        let position = self.position();
        let value = self.value()?;
        if value == ValueExpr::Literal(Literal::Null) {
            return Err(
                position.error("cannot compare with `NULL`; use `IS NULL` or `IS NOT NULL`")
            );
        }
        Ok(Condition::Compare { operand, op, value })
    }

    /// Parses the `(value, ...)` list that follows `IN`.
    fn in_list(&mut self, operand: Operand, negated: bool) -> Result<Condition, SqlError> {
        self.expect(&Token::LeftParen, "`(`")?;
        let mut values = Vec::new();
        loop {
            let position = self.position();
            let value = self.value()?;
            if value == ValueExpr::Literal(Literal::Null) {
                return Err(position.error("`NULL` is not allowed in an `IN` list"));
            }
            values.push(value);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RightParen, "`,` or `)`")?;
        Ok(Condition::In {
            operand,
            values,
            negated,
        })
    }

    /// Parses the pattern that follows `LIKE`.
    fn like(&mut self, operand: Operand, negated: bool) -> Result<Condition, SqlError> {
        let position = self.position();
        let pattern = self.value()?;
        if !matches!(
            pattern,
            ValueExpr::Parameter(_) | ValueExpr::Literal(Literal::String(_))
        ) {
            return Err(position.error("`LIKE` requires a string literal or `?` pattern"));
        }
        Ok(Condition::Like {
            operand,
            pattern,
            negated,
        })
    }

    /// Parses a column reference or, where allowed, an aggregate function call.
    pub(super) fn operand(&mut self, aggregates: Aggregates) -> Result<Operand, SqlError> {
        let is_call =
            matches!(self.peek(), Token::Identifier(_)) && *self.peek_at(1) == Token::LeftParen;
        if is_call {
            self.aggregate(aggregates).map(Operand::Aggregate)
        } else {
            self.column_ref().map(Operand::Column)
        }
    }

    /// Parses `COUNT(*)`, `COUNT(column)`, `SUM(column)`, and friends.
    fn aggregate(&mut self, aggregates: Aggregates) -> Result<Aggregate, SqlError> {
        let position = self.position();
        let name = self.identifier("a function name")?;
        let function = match name.to_ascii_uppercase().as_str() {
            "COUNT" => AggregateKind::Count,
            "SUM" => AggregateKind::Sum,
            "AVG" => AggregateKind::Avg,
            "MIN" => AggregateKind::Min,
            "MAX" => AggregateKind::Max,
            _ => {
                return Err(position.error(format!(
                    "unknown function `{name}`; only COUNT, SUM, AVG, MIN and MAX are supported"
                )));
            }
        };
        if aggregates == Aggregates::Forbidden {
            return Err(position.error("aggregate functions are not allowed in `WHERE`"));
        }
        self.expect(&Token::LeftParen, "`(`")?;
        if self.at_keyword(Keyword::Distinct) {
            return Err(self.error("`DISTINCT` inside an aggregate function is not supported"));
        }
        let column = if function == AggregateKind::Count && self.eat(&Token::Star) {
            None
        } else {
            Some(self.column_ref()?)
        };
        self.expect(&Token::RightParen, "`)`")?;
        Ok(Aggregate { function, column })
    }

    /// Parses `column` or `table.column`.
    pub(super) fn column_ref(&mut self) -> Result<ColumnRef, SqlError> {
        let first = self.identifier("a column name")?;
        if !self.eat(&Token::Dot) {
            return Ok(ColumnRef {
                table: None,
                column: first,
            });
        }
        if *self.peek() == Token::Star {
            return Err(self.error("qualified wildcard `table.*` is not supported"));
        }
        let column = self.identifier("a column name")?;
        Ok(ColumnRef {
            table: Some(first),
            column,
        })
    }

    /// Parses a literal or a `?` placeholder.
    pub(super) fn value(&mut self) -> Result<ValueExpr, SqlError> {
        let negative = self.eat(&Token::Minus);
        let literal = match (self.peek().clone(), negative) {
            (Token::Integer(value), false) => Literal::Integer(i128::from(value)),
            (Token::Integer(value), true) => Literal::Integer(-i128::from(value)),
            (Token::Float(value), false) => Literal::Float(value),
            (Token::Float(value), true) => Literal::Float(format!("-{value}")),
            (_, true) => return Err(self.unexpected("a number after `-`")),
            (Token::String(value), false) => Literal::String(value),
            (Token::Keyword(Keyword::True), false) => Literal::Boolean(true),
            (Token::Keyword(Keyword::False), false) => Literal::Boolean(false),
            (Token::Keyword(Keyword::Null), false) => Literal::Null,
            (Token::Placeholder, false) => {
                self.advance();
                return Ok(ValueExpr::Parameter(self.next_parameter()));
            }
            (_, false) => return Err(self.unexpected("a literal or `?`")),
        };
        self.advance();
        Ok(ValueExpr::Literal(literal))
    }
}
