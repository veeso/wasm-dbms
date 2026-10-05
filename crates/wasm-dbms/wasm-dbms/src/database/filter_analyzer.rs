// Rust guideline compliant 2026-03-29
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Filter analyzer for choosing a single-column index execution plan.

use wasm_dbms_api::prelude::{Filter, IndexDef, Value};

/// A single-column index execution plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexPlan {
    /// Exact lookup on one indexed column.
    Eq { column: &'static str, value: Value },
    /// Range lookup on one indexed column.
    Range {
        column: &'static str,
        start: Option<Value>,
        end: Option<Value>,
    },
    /// IN lookup on one indexed column.
    In {
        column: &'static str,
        values: Vec<Value>,
    },
}

impl IndexPlan {
    /// Returns the indexed column name for the plan.
    pub fn column(&self) -> &'static str {
        match self {
            Self::Eq { column, .. } | Self::Range { column, .. } | Self::In { column, .. } => {
                column
            }
        }
    }
}

/// Result of filter analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedFilter {
    /// The chosen index plan.
    pub plan: IndexPlan,
    /// Filter still needing evaluation after the index lookup.
    pub remaining_filter: Option<Filter>,
}

/// Analyzes a filter and returns an indexable plan when a single-column index can help.
pub fn analyze_filter(filter: &Filter, indexes: &[IndexDef]) -> Option<AnalyzedFilter> {
    let indexed_columns: Vec<&'static str> = indexes
        .iter()
        .filter_map(|index| (index.columns().len() == 1).then_some(index.columns()[0]))
        .collect();

    analyze_inner(filter, &indexed_columns)
}

fn analyze_inner(filter: &Filter, indexed_columns: &[&'static str]) -> Option<AnalyzedFilter> {
    match filter {
        Filter::Eq(column, value) => {
            resolve_column(column, indexed_columns).map(|column| AnalyzedFilter {
                plan: IndexPlan::Eq {
                    column,
                    value: value.clone(),
                },
                remaining_filter: None,
            })
        }
        Filter::Ge(column, value) => {
            resolve_column(column, indexed_columns).map(|column| AnalyzedFilter {
                plan: IndexPlan::Range {
                    column,
                    start: Some(value.clone()),
                    end: None,
                },
                remaining_filter: None,
            })
        }
        Filter::Le(column, value) => {
            resolve_column(column, indexed_columns).map(|column| AnalyzedFilter {
                plan: IndexPlan::Range {
                    column,
                    start: None,
                    end: Some(value.clone()),
                },
                remaining_filter: None,
            })
        }
        Filter::Gt(column, value) => {
            resolve_column(column, indexed_columns).map(|column| AnalyzedFilter {
                plan: IndexPlan::Range {
                    column,
                    start: Some(value.clone()),
                    end: None,
                },
                // `IndexPlan::Range` is inclusive, so `>` keeps the original filter as a
                // post-index predicate to trim away equal-bound matches.
                remaining_filter: Some(filter.clone()),
            })
        }
        Filter::Lt(column, value) => {
            resolve_column(column, indexed_columns).map(|column| AnalyzedFilter {
                plan: IndexPlan::Range {
                    column,
                    start: None,
                    end: Some(value.clone()),
                },
                // `IndexPlan::Range` is inclusive, so `<` keeps the original filter as a
                // post-index predicate to trim away equal-bound matches.
                remaining_filter: Some(filter.clone()),
            })
        }
        Filter::In(column, values) => {
            resolve_column(column, indexed_columns).map(|column| AnalyzedFilter {
                plan: IndexPlan::In {
                    column,
                    values: values.clone(),
                },
                remaining_filter: None,
            })
        }
        Filter::And(left, right) => analyze_and(left, right, indexed_columns),
        _ => None,
    }
}

fn analyze_and(
    left: &Filter,
    right: &Filter,
    indexed_columns: &[&'static str],
) -> Option<AnalyzedFilter> {
    let left_analysis = analyze_inner(left, indexed_columns);
    let right_analysis = analyze_inner(right, indexed_columns);

    match (left_analysis, right_analysis) {
        (
            Some(AnalyzedFilter {
                plan:
                    IndexPlan::Range {
                        column: left_column,
                        start: left_start,
                        end: left_end,
                    },
                remaining_filter: left_remaining,
            }),
            Some(AnalyzedFilter {
                plan:
                    IndexPlan::Range {
                        column: right_column,
                        start: right_start,
                        end: right_end,
                    },
                remaining_filter: right_remaining,
            }),
        ) if left_column == right_column => Some(AnalyzedFilter {
            plan: IndexPlan::Range {
                column: left_column,
                start: stricter_bound(left_start, right_start, std::cmp::max),
                end: stricter_bound(left_end, right_end, std::cmp::min),
            },
            remaining_filter: combine_filters(left_remaining, right_remaining),
        }),
        (Some(left_analysis), Some(right_analysis)) => Some(AnalyzedFilter {
            plan: left_analysis.plan,
            remaining_filter: combine_filters(
                combine_filters(left_analysis.remaining_filter, Some(right.to_owned())),
                right_analysis.remaining_filter,
            ),
        }),
        (Some(analysis), None) => Some(AnalyzedFilter {
            plan: analysis.plan,
            remaining_filter: combine_filters(analysis.remaining_filter, Some(right.to_owned())),
        }),
        (None, Some(analysis)) => Some(AnalyzedFilter {
            plan: analysis.plan,
            remaining_filter: combine_filters(Some(left.to_owned()), analysis.remaining_filter),
        }),
        (None, None) => None,
    }
}

/// Intersects two optional range bounds on the same column.
///
/// When both bounds are present, `pick` selects the stricter one: `max` for
/// lower bounds and `min` for upper bounds. Exclusive comparisons (`>` and
/// `<`) are kept in the residual filter by [`analyze_inner`], so choosing the
/// inclusive value here never admits a row that an exclusive bound rejects.
fn stricter_bound<F>(left: Option<Value>, right: Option<Value>, pick: F) -> Option<Value>
where
    F: FnOnce(Value, Value) -> Value,
{
    match (left, right) {
        (Some(left), Some(right)) => Some(pick(left, right)),
        (left, right) => left.or(right),
    }
}

fn resolve_column(column: &str, indexed_columns: &[&'static str]) -> Option<&'static str> {
    indexed_columns
        .iter()
        .find(|candidate| **candidate == column)
        .copied()
}

fn combine_filters(left: Option<Filter>, right: Option<Filter>) -> Option<Filter> {
    match (left, right) {
        (Some(left), Some(right)) => Some(Filter::And(Box::new(left), Box::new(right))),
        (Some(filter), None) | (None, Some(filter)) => Some(filter),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {

    use wasm_dbms_api::prelude::{Filter, IndexDef, Value};

    use super::{AnalyzedFilter, IndexPlan, analyze_filter};

    fn single_index() -> &'static [IndexDef] {
        &[IndexDef(&["name"])]
    }

    #[test]
    fn test_eq_on_indexed_column() {
        let filter = Filter::eq("name", Value::Text("alice".to_string().into()));
        let analyzed = analyze_filter(&filter, single_index()).expect("analysis should exist");

        assert_eq!(
            analyzed,
            AnalyzedFilter {
                plan: IndexPlan::Eq {
                    column: "name",
                    value: Value::Text("alice".to_string().into()),
                },
                remaining_filter: None,
            }
        );
    }

    #[test]
    fn test_eq_on_non_indexed_column_returns_none() {
        let filter = Filter::eq("age", Value::Uint32(25.into()));
        assert!(analyze_filter(&filter, single_index()).is_none());
    }

    #[test]
    fn test_gt_on_indexed_column_keeps_remaining_filter() {
        let filter = Filter::gt("name", Value::Text("alice".to_string().into()));
        let analyzed = analyze_filter(&filter, single_index()).expect("analysis should exist");

        assert_eq!(
            analyzed.plan,
            IndexPlan::Range {
                column: "name",
                start: Some(Value::Text("alice".to_string().into())),
                end: None,
            }
        );
        assert_eq!(analyzed.remaining_filter, Some(filter));
    }

    #[test]
    fn test_in_on_indexed_column() {
        let filter = Filter::in_list(
            "name",
            vec![
                Value::Text("alice".to_string().into()),
                Value::Text("bob".to_string().into()),
            ],
        );
        let analyzed = analyze_filter(&filter, single_index()).expect("analysis should exist");

        assert_eq!(
            analyzed.plan,
            IndexPlan::In {
                column: "name",
                values: vec![
                    Value::Text("alice".to_string().into()),
                    Value::Text("bob".to_string().into()),
                ],
            }
        );
        assert!(analyzed.remaining_filter.is_none());
    }

    #[test]
    fn test_and_with_indexed_and_non_indexed() {
        let filter = Filter::And(
            Box::new(Filter::eq("name", Value::Text("alice".to_string().into()))),
            Box::new(Filter::eq("age", Value::Uint32(25.into()))),
        );
        let analyzed = analyze_filter(&filter, single_index()).expect("analysis should exist");

        assert_eq!(
            analyzed.plan,
            IndexPlan::Eq {
                column: "name",
                value: Value::Text("alice".to_string().into()),
            }
        );
        assert_eq!(
            analyzed.remaining_filter,
            Some(Filter::eq("age", Value::Uint32(25.into())))
        );
    }

    #[test]
    fn test_and_range_merge_on_same_column() {
        let filter = Filter::And(
            Box::new(Filter::ge("name", Value::Text("a".to_string().into()))),
            Box::new(Filter::lt("name", Value::Text("z".to_string().into()))),
        );
        let analyzed = analyze_filter(&filter, single_index()).expect("analysis should exist");

        assert_eq!(
            analyzed.plan,
            IndexPlan::Range {
                column: "name",
                start: Some(Value::Text("a".to_string().into())),
                end: Some(Value::Text("z".to_string().into())),
            }
        );
        assert_eq!(
            analyzed.remaining_filter,
            Some(Filter::lt("name", Value::Text("z".to_string().into())))
        );
    }

    fn text(value: &str) -> Value {
        Value::Text(value.to_string().into())
    }

    /// Asserts that both operand orders of `left AND right` plan the given
    /// range bounds on `name`.
    fn assert_range_in_both_orders(
        left: Filter,
        right: Filter,
        start: Option<Value>,
        end: Option<Value>,
    ) {
        let expected = IndexPlan::Range {
            column: "name",
            start,
            end,
        };
        for filter in [
            left.clone().and(right.clone()),
            right.clone().and(left.clone()),
        ] {
            let analyzed = analyze_filter(&filter, single_index()).expect("analysis should exist");
            assert_eq!(analyzed.plan, expected, "unexpected plan for {filter:?}");
        }
    }

    #[test]
    fn test_and_inclusive_lower_bounds_use_maximum() {
        assert_range_in_both_orders(
            Filter::ge("name", text("b")),
            Filter::ge("name", text("m")),
            Some(text("m")),
            None,
        );
    }

    #[test]
    fn test_and_exclusive_lower_bounds_use_maximum() {
        let left = Filter::gt("name", text("b"));
        let right = Filter::gt("name", text("m"));
        assert_range_in_both_orders(left.clone(), right.clone(), Some(text("m")), None);

        // Exclusive bounds stay in the residual filter so equal keys are trimmed.
        let analyzed = analyze_filter(&left.clone().and(right.clone()), single_index())
            .expect("analysis should exist");
        assert_eq!(analyzed.remaining_filter, Some(left.and(right)));
    }

    #[test]
    fn test_and_inclusive_upper_bounds_use_minimum() {
        assert_range_in_both_orders(
            Filter::le("name", text("z")),
            Filter::le("name", text("m")),
            None,
            Some(text("m")),
        );
    }

    #[test]
    fn test_and_exclusive_upper_bounds_use_minimum() {
        let left = Filter::lt("name", text("z"));
        let right = Filter::lt("name", text("m"));
        assert_range_in_both_orders(left.clone(), right.clone(), None, Some(text("m")));

        let analyzed = analyze_filter(&left.clone().and(right.clone()), single_index())
            .expect("analysis should exist");
        assert_eq!(analyzed.remaining_filter, Some(left.and(right)));
    }

    #[test]
    fn test_and_mixed_bounds_on_equal_value_keep_exclusive_residual() {
        for (inclusive, exclusive) in [
            (Filter::ge("name", text("m")), Filter::gt("name", text("m"))),
            (Filter::le("name", text("m")), Filter::lt("name", text("m"))),
        ] {
            for filter in [
                inclusive.clone().and(exclusive.clone()),
                exclusive.clone().and(inclusive.clone()),
            ] {
                let analyzed =
                    analyze_filter(&filter, single_index()).expect("analysis should exist");
                assert_eq!(
                    analyzed.remaining_filter,
                    Some(exclusive.clone()),
                    "exclusive bound must survive as residual for {filter:?}"
                );
            }
        }
    }

    #[test]
    fn test_or_returns_none() {
        let filter = Filter::Or(
            Box::new(Filter::eq("name", Value::Text("alice".to_string().into()))),
            Box::new(Filter::eq("name", Value::Text("bob".to_string().into()))),
        );
        assert!(analyze_filter(&filter, single_index()).is_none());
    }
}
