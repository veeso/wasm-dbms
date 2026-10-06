//! Conversion of a parsed condition into a [`Filter`].

use wasm_dbms_api::prelude::{DataTypeKind, Filter, SqlError, Value};

use super::coerce::{coerce_filter_value, like_pattern};
use crate::ast::{CompareOp, Condition, Operand};

/// What a condition operand refers to.
pub(super) struct Target {
    /// The name the DBMS knows the value by: a column name, `table.column`,
    /// or `agg{N}`.
    pub(super) field: String,
    /// The name used for the operand in error messages.
    pub(super) label: String,
    /// The type literals compared with the operand are converted to.
    pub(super) kind: DataTypeKind,
}

/// The clause a condition belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Clause {
    Where,
    Having,
}

/// Converts `condition` into a [`Filter`].
///
/// `target` resolves each operand; it is called once per predicate, in source
/// order.
///
/// # Errors
///
/// Propagates the errors of `target` and of value conversion, and returns
/// [`SqlError::Unsupported`] for a `LIKE` test in `HAVING`.
pub(super) fn filter(
    condition: &Condition,
    params: &[Value],
    clause: Clause,
    target: &mut dyn FnMut(&Operand) -> Result<Target, SqlError>,
) -> Result<Filter, SqlError> {
    Ok(match condition {
        Condition::And(left, right) => {
            filter(left, params, clause, target)?.and(filter(right, params, clause, target)?)
        }
        Condition::Or(left, right) => {
            filter(left, params, clause, target)?.or(filter(right, params, clause, target)?)
        }
        Condition::Not(inner) => filter(inner, params, clause, target)?.not(),
        Condition::Compare { operand, op, value } => {
            let target = target(operand)?;
            let value = coerce_filter_value(value, params, &target.label, target.kind)?;
            match op {
                CompareOp::Eq => Filter::Eq(target.field, value),
                CompareOp::NotEq => Filter::Ne(target.field, value),
                CompareOp::Lt => Filter::Lt(target.field, value),
                CompareOp::LtEq => Filter::Le(target.field, value),
                CompareOp::Gt => Filter::Gt(target.field, value),
                CompareOp::GtEq => Filter::Ge(target.field, value),
            }
        }
        Condition::In {
            operand,
            values,
            negated,
        } => {
            let target = target(operand)?;
            let values = values
                .iter()
                .map(|value| coerce_filter_value(value, params, &target.label, target.kind))
                .collect::<Result<Vec<_>, _>>()?;
            negate(Filter::In(target.field, values), *negated)
        }
        Condition::Like {
            operand,
            pattern,
            negated,
        } => {
            if clause == Clause::Having {
                return Err(SqlError::Unsupported(
                    "LIKE is not supported in HAVING".to_string(),
                ));
            }
            let target = target(operand)?;
            let pattern = like_pattern(pattern, params, &target.label)?;
            negate(Filter::Like(target.field, pattern), *negated)
        }
        Condition::IsNull { operand, negated } => {
            let target = target(operand)?;
            if *negated {
                Filter::NotNull(target.field)
            } else {
                Filter::IsNull(target.field)
            }
        }
    })
}

fn negate(filter: Filter, negated: bool) -> Filter {
    if negated { filter.not() } else { filter }
}
