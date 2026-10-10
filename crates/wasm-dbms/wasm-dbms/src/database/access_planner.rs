// Rust guideline compliant 2026-10-09
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Access-path planner over a table's declared indexes.
//!
//! The planner turns a filter into an [`AccessPath`] whose rows are a superset
//! of the rows matching the filter. Callers re-check the original filter on
//! every candidate, so a path never needs to be exact.

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

use wasm_dbms_api::prelude::{
    ColumnDef, DataTypeKind, Filter, IndexDef, Text, Uint8, Uint16, Uint32, Uint64, Value,
};

use super::index_reader::IndexScan;

/// Largest number of index ranges one path may read for IN lists and OR
/// branches. Larger requests fall back instead of being truncated.
pub(crate) const MAX_ALTERNATIVES: usize = 64;

/// Candidate-producing access path over persistent indexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessPath {
    /// No row can match the filter.
    Empty,
    /// Rows whose index key falls in one range.
    Scan(IndexScan),
    /// Rows produced by any of the paths.
    Union(Vec<AccessPath>),
    /// Rows produced by every one of the paths.
    Intersection(Vec<AccessPath>),
}

impl AccessPath {
    /// Returns whether a visible row belongs to this candidate path.
    pub fn matches_row(&self, row: &[(ColumnDef, Value)]) -> bool {
        match self {
            Self::Empty => false,
            Self::Scan(scan) => scan.matches_row(row),
            Self::Union(paths) => paths.iter().any(|path| path.matches_row(row)),
            Self::Intersection(paths) => paths.iter().all(|path| path.matches_row(row)),
        }
    }

    /// Collects the columns whose changes can move a row into this path.
    pub fn collect_constrained_columns(&self, columns: &mut BTreeSet<&'static str>) {
        match self {
            Self::Empty => {}
            Self::Scan(scan) => columns.extend(scan.constrained_columns()),
            Self::Union(paths) | Self::Intersection(paths) => {
                for path in paths {
                    path.collect_constrained_columns(columns);
                }
            }
        }
    }

    /// Returns the number of index ranges the path reads.
    pub fn scan_count(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Scan(_) => 1,
            Self::Union(paths) | Self::Intersection(paths) => {
                paths.iter().map(Self::scan_count).sum()
            }
        }
    }

    /// Combines the scans of one index into a single path.
    fn from_scans(mut scans: Vec<IndexScan>) -> Self {
        match scans.len() {
            0 => Self::Empty,
            1 => Self::Scan(scans.remove(0)),
            _ => Self::Union(scans.into_iter().map(Self::Scan).collect()),
        }
    }
}

/// Plans an access path for `filter`, or returns `None` when no index helps.
pub fn plan_filter(
    filter: &Filter,
    indexes: &'static [IndexDef],
    columns: &'static [ColumnDef],
) -> Option<AccessPath> {
    let mut conjuncts = Vec::new();
    flatten_and(filter, &mut conjuncts);
    plan_conjunction(&conjuncts, indexes, columns)
}

/// Returns whether evaluating `filter` can fail, which is the case for LIKE
/// and JSON predicates.
pub fn is_fallible(filter: &Filter) -> bool {
    match filter {
        Filter::Like(..) | Filter::Json(..) => true,
        Filter::And(left, right) | Filter::Or(left, right) => {
            is_fallible(left) || is_fallible(right)
        }
        Filter::Not(inner) => is_fallible(inner),
        _ => false,
    }
}

/// Adds every column referenced by `filter` to `out`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn collect_filter_columns(filter: &Filter, out: &mut BTreeSet<String>) {
    match filter {
        Filter::Eq(column, _)
        | Filter::Ne(column, _)
        | Filter::Gt(column, _)
        | Filter::Lt(column, _)
        | Filter::Ge(column, _)
        | Filter::Le(column, _)
        | Filter::In(column, _)
        | Filter::Json(column, _)
        | Filter::Like(column, _)
        | Filter::NotNull(column)
        | Filter::IsNull(column) => {
            out.insert(column.clone());
        }
        Filter::And(left, right) | Filter::Or(left, right) => {
            collect_filter_columns(left, out);
            collect_filter_columns(right, out);
        }
        Filter::Not(inner) => collect_filter_columns(inner, out),
    }
}

fn flatten_and<'f>(filter: &'f Filter, out: &mut Vec<&'f Filter>) {
    match filter {
        Filter::And(left, right) => {
            flatten_and(left, out);
            flatten_and(right, out);
        }
        other => out.push(other),
    }
}

/// Plans one conjunction of predicates.
///
/// A primary-key, unique, or full composite equality is read alone: fetching
/// its few records beats reading another index. Otherwise every useful
/// candidate joins an intersection, cheapest first; the executor stops early
/// once few candidates remain and skips members over budget.
fn plan_conjunction(
    conjuncts: &[&Filter],
    indexes: &'static [IndexDef],
    columns: &'static [ColumnDef],
) -> Option<AccessPath> {
    let Some(constraints) = ColumnConstraints::collect(conjuncts) else {
        return Some(AccessPath::Empty);
    };
    let mut candidates: Vec<Candidate> = indexes
        .iter()
        .enumerate()
        .filter_map(|(position, index)| index_candidate(index, position, &constraints, columns))
        .collect();
    candidates.extend(
        conjuncts
            .iter()
            .enumerate()
            .filter_map(|(position, conjunct)| {
                or_candidate(conjunct, indexes.len() + position, indexes, columns)
            }),
    );
    candidates.sort_by_key(Candidate::sort_key);
    if candidates
        .iter()
        .any(|candidate| candidate.path == AccessPath::Empty)
    {
        return Some(AccessPath::Empty);
    }

    let first = candidates.first()?;
    if matches!(first.rank, Rank::UniqueExact | Rank::CompositeExact) {
        return candidates
            .into_iter()
            .next()
            .map(|candidate| candidate.path);
    }

    let mut covered: BTreeSet<&'static str> = BTreeSet::new();
    let mut members = Vec::new();
    for candidate in candidates {
        let redundant = !candidate.columns.is_empty()
            && candidate
                .columns
                .iter()
                .all(|column| covered.contains(column));
        if redundant {
            continue;
        }
        covered.extend(candidate.columns.iter().copied());
        members.push(candidate.path);
    }
    match members.len() {
        1 => members.pop(),
        _ => Some(AccessPath::Intersection(members)),
    }
}

/// Expected cost class of a candidate path; lower ranks are cheaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    /// Equality on a primary-key or unique single-column index.
    UniqueExact,
    /// Equality on every column of a multi-column index.
    CompositeExact,
    /// Equality prefix, optionally followed by a range, including equality on
    /// a non-unique single-column index. Deeper prefixes sort first, so a
    /// composite prefix wins over a single-column index on its first column.
    Prefix,
    /// Range on the first index column.
    Range,
    /// Union of the branches of an OR conjunct.
    Union,
    /// Every non-NULL value of a nullable column; often dense.
    NotNull,
}

/// A usable access path on one index.
#[derive(Debug)]
struct Candidate {
    rank: Rank,
    /// Declaration position of the index, for deterministic ties.
    position: usize,
    /// Columns the path constrains.
    columns: Vec<&'static str>,
    path: AccessPath,
}

impl Candidate {
    fn sort_key(&self) -> (Rank, Reverse<usize>, usize) {
        (self.rank, Reverse(self.columns.len()), self.position)
    }
}

/// Matches the constraints against `index` in declaration order.
fn index_candidate(
    index: &IndexDef,
    position: usize,
    constraints: &ColumnConstraints,
    columns: &'static [ColumnDef],
) -> Option<Candidate> {
    let index_columns = index.columns();
    let mut prefixes: Vec<Vec<Value>> = vec![Vec::new()];
    let mut used = Vec::new();
    let mut bounds = None;
    let mut not_null_only = false;

    for column in index_columns {
        let Some(constraint) = constraints.get(column) else {
            break;
        };
        if let Some(value) = &constraint.eq {
            for prefix in &mut prefixes {
                prefix.push(value.clone());
            }
        } else if let Some(values) = &constraint.in_list {
            if prefixes.len() * values.len() > MAX_ALTERNATIVES {
                break;
            }
            prefixes = prefixes
                .iter()
                .flat_map(|prefix| {
                    values.iter().map(move |value| {
                        let mut extended = prefix.clone();
                        extended.push(value.clone());
                        extended
                    })
                })
                .collect();
        } else if let Some((lower, upper, from_not_null)) =
            constraint.range_bounds(column_def(column, columns))
        {
            used.push(*column);
            bounds = Some((lower, upper));
            not_null_only = from_not_null;
            break;
        } else {
            break;
        }
        used.push(*column);
    }

    if used.is_empty() {
        return None;
    }
    let depth = used.len();
    let exact = bounds.is_none() && depth == index_columns.len();
    let rank = if exact && index_columns.len() > 1 {
        Rank::CompositeExact
    } else if exact && is_unique(index_columns[0], columns) {
        Rank::UniqueExact
    } else if depth > 1 || bounds.is_none() {
        Rank::Prefix
    } else if not_null_only {
        Rank::NotNull
    } else {
        Rank::Range
    };
    let scans = prefixes
        .into_iter()
        .map(|prefix| match &bounds {
            Some((lower, upper)) => {
                IndexScan::range(index_columns, prefix, lower.clone(), upper.clone())
            }
            None => IndexScan::prefix(index_columns, prefix),
        })
        .collect();
    Some(Candidate {
        rank,
        position,
        columns: used,
        path: AccessPath::from_scans(scans),
    })
}

/// Plans an OR conjunct as a union when every branch is indexable.
fn or_candidate(
    conjunct: &Filter,
    position: usize,
    indexes: &'static [IndexDef],
    columns: &'static [ColumnDef],
) -> Option<Candidate> {
    if !matches!(conjunct, Filter::Or(..)) {
        return None;
    }
    let mut branches = Vec::new();
    flatten_or(conjunct, &mut branches);

    let mut paths = Vec::new();
    for branch in branches {
        match plan_filter(branch, indexes, columns)? {
            AccessPath::Empty => {}
            path => paths.push(path),
        }
    }
    let path = match paths.len() {
        0 => AccessPath::Empty,
        1 => paths.remove(0),
        _ => AccessPath::Union(paths),
    };
    if path.scan_count() > MAX_ALTERNATIVES {
        return None;
    }
    Some(Candidate {
        rank: Rank::Union,
        position,
        columns: Vec::new(),
        path,
    })
}

fn flatten_or<'f>(filter: &'f Filter, out: &mut Vec<&'f Filter>) {
    match filter {
        Filter::Or(left, right) => {
            flatten_or(left, out);
            flatten_or(right, out);
        }
        other => out.push(other),
    }
}

fn column_def(name: &str, columns: &'static [ColumnDef]) -> Option<&'static ColumnDef> {
    columns.iter().find(|column| column.name == name)
}

fn is_unique(name: &str, columns: &'static [ColumnDef]) -> bool {
    column_def(name, columns).is_some_and(|column| column.primary_key || column.unique)
}

/// Per-column constraints extracted from one conjunction.
#[derive(Debug, Default)]
struct ColumnConstraints(BTreeMap<String, ColumnConstraint>);

impl ColumnConstraints {
    /// Collects the constraints, returning `None` when they contradict.
    fn collect(conjuncts: &[&Filter]) -> Option<Self> {
        let mut constraints = Self::default();
        for conjunct in conjuncts {
            let (column, update) = match conjunct {
                Filter::Eq(column, value) => (column, Update::Eq(value.clone())),
                Filter::IsNull(column) => (column, Update::Eq(Value::Null)),
                Filter::NotNull(column) => (column, Update::NotNull),
                Filter::In(column, values) => (column, Update::In(values.clone())),
                Filter::Ge(column, value) => {
                    (column, Update::Lower(Bound::Included(value.clone())))
                }
                Filter::Gt(column, value) => {
                    (column, Update::Lower(Bound::Excluded(value.clone())))
                }
                Filter::Le(column, value) => {
                    (column, Update::Upper(Bound::Included(value.clone())))
                }
                Filter::Lt(column, value) => {
                    (column, Update::Upper(Bound::Excluded(value.clone())))
                }
                _ => continue,
            };
            constraints
                .0
                .entry(column.clone())
                .or_default()
                .apply(update)?;
        }
        for constraint in constraints.0.values_mut() {
            constraint.normalize()?;
        }
        Some(constraints)
    }

    fn get(&self, column: &str) -> Option<&ColumnConstraint> {
        self.0.get(column)
    }
}

/// One predicate's effect on a column constraint.
enum Update {
    Eq(Value),
    In(Vec<Value>),
    NotNull,
    Lower(Bound<Value>),
    Upper(Bound<Value>),
}

/// The values one column may take under a conjunction.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ColumnConstraint {
    eq: Option<Value>,
    /// Sorted, distinct allowed values.
    in_list: Option<Vec<Value>>,
    not_null: bool,
    lower: Bound<Value>,
    upper: Bound<Value>,
}

impl Default for ColumnConstraint {
    fn default() -> Self {
        Self {
            eq: None,
            in_list: None,
            not_null: false,
            lower: Bound::Unbounded,
            upper: Bound::Unbounded,
        }
    }
}

impl ColumnConstraint {
    /// Narrows the constraint, returning `None` when nothing can match.
    fn apply(&mut self, update: Update) -> Option<()> {
        match update {
            Update::Eq(value) => {
                if self.eq.as_ref().is_some_and(|existing| *existing != value) {
                    return None;
                }
                self.eq = Some(value);
            }
            Update::In(mut values) => {
                values.sort();
                values.dedup();
                let values = match self.in_list.take() {
                    Some(existing) => existing
                        .into_iter()
                        .filter(|value| values.binary_search(value).is_ok())
                        .collect(),
                    None => values,
                };
                self.in_list = Some(values);
            }
            Update::NotNull => self.not_null = true,
            Update::Lower(bound) => {
                let current = std::mem::replace(&mut self.lower, Bound::Unbounded);
                self.lower = stricter(current, bound, Ordering::Greater);
            }
            Update::Upper(bound) => {
                let current = std::mem::replace(&mut self.upper, Bound::Unbounded);
                self.upper = stricter(current, bound, Ordering::Less);
            }
        }
        Some(())
    }

    /// Reduces the constraint to its simplest form, returning `None` when
    /// nothing can match.
    fn normalize(&mut self) -> Option<()> {
        if let Some(list) = self.in_list.take() {
            let list: Vec<Value> = list
                .into_iter()
                .filter(|value| self.admits(value))
                .collect();
            let list: Vec<Value> = list
                .into_iter()
                .filter(|value| !(self.not_null && value.is_null()))
                .collect();
            match &self.eq {
                Some(eq) if !list.contains(eq) => return None,
                Some(_) => {}
                None if list.is_empty() => return None,
                None if list.len() == 1 => self.eq = list.into_iter().next(),
                None => self.in_list = Some(list),
            }
        }
        if let Some(eq) = &self.eq {
            if !self.admits(eq) || (self.not_null && eq.is_null()) {
                return None;
            }
            self.lower = Bound::Unbounded;
            self.upper = Bound::Unbounded;
        } else if is_empty_interval(&self.lower, &self.upper) {
            return None;
        }
        Some(())
    }

    /// Returns whether `value` lies within the bounds.
    fn admits(&self, value: &Value) -> bool {
        let above = match &self.lower {
            Bound::Included(lower) => value >= lower,
            Bound::Excluded(lower) => value > lower,
            Bound::Unbounded => true,
        };
        let below = match &self.upper {
            Bound::Included(upper) => value <= upper,
            Bound::Excluded(upper) => value < upper,
            Bound::Unbounded => true,
        };
        above && below
    }

    /// Returns range bounds and whether they come from `IS NOT NULL` alone.
    fn range_bounds(
        &self,
        column: Option<&ColumnDef>,
    ) -> Option<(Bound<Value>, Bound<Value>, bool)> {
        let unbounded = matches!(
            (&self.lower, &self.upper),
            (Bound::Unbounded, Bound::Unbounded)
        );
        if !unbounded {
            return Some((self.lower.clone(), self.upper.clone(), false));
        }
        if !self.not_null {
            return None;
        }
        let column = column.filter(|column| column.nullable)?;
        let (lower, upper) = not_null_bounds(column.data_type)?;
        Some((lower, upper, true))
    }
}

/// Returns bounds holding every non-NULL value of a column of `data_type`, or
/// `None` when no complete bound is known.
///
/// `Value` orders by variant first: variants declared before `Null` sort below
/// it and the others above it. Values above `Null` start at the smallest value
/// of their type, so no NULL key is read.
fn not_null_bounds(data_type: DataTypeKind) -> Option<(Bound<Value>, Bound<Value>)> {
    let below_null = (Bound::Unbounded, Bound::Excluded(Value::Null));
    let from = |minimum: Value| (Bound::Included(minimum), Bound::Unbounded);
    match data_type {
        DataTypeKind::Blob
        | DataTypeKind::Boolean
        | DataTypeKind::Date
        | DataTypeKind::DateTime
        | DataTypeKind::Decimal
        | DataTypeKind::Int8
        | DataTypeKind::Int16
        | DataTypeKind::Int32
        | DataTypeKind::Int64
        | DataTypeKind::Json => Some(below_null),
        DataTypeKind::Text => Some(from(Value::Text(Text(String::new())))),
        DataTypeKind::Uint8 => Some(from(Value::Uint8(Uint8(0)))),
        DataTypeKind::Uint16 => Some(from(Value::Uint16(Uint16(0)))),
        DataTypeKind::Uint32 => Some(from(Value::Uint32(Uint32(0)))),
        DataTypeKind::Uint64 => Some(from(Value::Uint64(Uint64(0)))),
        DataTypeKind::Uuid | DataTypeKind::Custom { .. } => None,
    }
}

/// Returns the bound admitting fewer values. `tighter` is the ordering of a
/// stricter bound value relative to the other: greater for lower bounds,
/// less for upper bounds. On equal values the exclusive bound wins.
fn stricter(current: Bound<Value>, new: Bound<Value>, tighter: Ordering) -> Bound<Value> {
    let ordering = match (&current, &new) {
        (Bound::Unbounded, _) => return new,
        (_, Bound::Unbounded) => return current,
        (
            Bound::Included(current_value) | Bound::Excluded(current_value),
            Bound::Included(new_value) | Bound::Excluded(new_value),
        ) => current_value.cmp(new_value),
    };
    if ordering == tighter {
        current
    } else if ordering == tighter.reverse() {
        new
    } else if matches!(current, Bound::Excluded(_)) {
        current
    } else {
        new
    }
}

fn is_empty_interval(lower: &Bound<Value>, upper: &Bound<Value>) -> bool {
    match (lower, upper) {
        (Bound::Included(lower), Bound::Included(upper)) => lower > upper,
        (Bound::Included(lower), Bound::Excluded(upper))
        | (Bound::Excluded(lower), Bound::Included(upper))
        | (Bound::Excluded(lower), Bound::Excluded(upper)) => lower >= upper,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use wasm_dbms_api::prelude::{ColumnDef, DataTypeKind, Filter, IndexDef, Text, Uint32, Value};

    use super::{AccessPath, MAX_ALTERNATIVES, plan_filter};
    use crate::database::index_reader::IndexScan;

    const fn column(
        name: &'static str,
        data_type: DataTypeKind,
        nullable: bool,
        primary_key: bool,
    ) -> ColumnDef {
        ColumnDef {
            name,
            data_type,
            auto_increment: false,
            nullable,
            primary_key,
            unique: false,
            foreign_key: None,
            default: None,
            renamed_from: &[],
        }
    }

    const COLUMNS: &[ColumnDef] = &[
        column("id", DataTypeKind::Uint32, false, true),
        column("category", DataTypeKind::Text, false, false),
        column("brand", DataTypeKind::Uint32, false, false),
        column("price", DataTypeKind::Uint32, false, false),
        column("note", DataTypeKind::Text, false, false),
        column("stock", DataTypeKind::Int32, true, false),
        column("label", DataTypeKind::Text, true, false),
        column("rank", DataTypeKind::Uint32, true, false),
        column("token", DataTypeKind::Uuid, true, false),
        column("shard", DataTypeKind::Uint32, false, false),
    ];

    const COMPOSITE: &[&str] = &["category", "brand", "price"];

    const INDEXES: &[IndexDef] = &[
        IndexDef(&["id"]),
        IndexDef(COMPOSITE),
        IndexDef(&["brand"]),
        IndexDef(&["stock"]),
        IndexDef(&["label"]),
        IndexDef(&["rank"]),
        IndexDef(&["token"]),
        IndexDef(&["label", "stock"]),
        IndexDef(&["shard"]),
    ];

    fn text(value: &str) -> Value {
        Value::Text(Text(value.to_string()))
    }

    fn uint(value: u32) -> Value {
        Value::Uint32(Uint32(value))
    }

    fn plan(filter: Filter) -> Option<AccessPath> {
        plan_filter(&filter, INDEXES, COLUMNS)
    }

    fn scan(columns: &'static [&'static str], prefix: Vec<Value>) -> AccessPath {
        AccessPath::Scan(IndexScan::prefix(columns, prefix))
    }

    fn range(
        columns: &'static [&'static str],
        prefix: Vec<Value>,
        lower: Bound<Value>,
        upper: Bound<Value>,
    ) -> AccessPath {
        AccessPath::Scan(IndexScan::range(columns, prefix, lower, upper))
    }

    #[test]
    fn test_composite_equality_in_any_and_order_is_one_exact_key() {
        let expected = Some(scan(COMPOSITE, vec![text("book"), uint(1), uint(10)]));
        let category = || Filter::eq("category", text("book"));
        let brand = || Filter::eq("brand", uint(1));
        let price = || Filter::eq("price", uint(10));
        for filter in [
            category().and(brand()).and(price()),
            price().and(brand()).and(category()),
            brand().and(price().and(category())),
        ] {
            assert_eq!(plan(filter.clone()), expected, "{filter:?}");
        }
    }

    #[test]
    fn test_leading_equalities_scan_a_composite_prefix() {
        assert_eq!(
            plan(Filter::eq("category", text("book"))),
            Some(scan(COMPOSITE, vec![text("book")]))
        );
        assert_eq!(
            plan(Filter::eq("category", text("book")).and(Filter::eq("brand", uint(1)))),
            Some(scan(COMPOSITE, vec![text("book"), uint(1)]))
        );
    }

    #[test]
    fn test_prefix_plus_range_keeps_bound_kinds() {
        assert_eq!(
            plan(
                Filter::eq("category", text("book"))
                    .and(Filter::ge("brand", uint(1)))
                    .and(Filter::lt("brand", uint(3)))
            ),
            Some(range(
                COMPOSITE,
                vec![text("book")],
                Bound::Included(uint(1)),
                Bound::Excluded(uint(3)),
            ))
        );
        // The range stops matching; the trailing price condition is a residual.
        assert_eq!(
            plan(
                Filter::eq("category", text("book"))
                    .and(Filter::gt("brand", uint(1)))
                    .and(Filter::eq("price", uint(5)))
            ),
            Some(range(
                COMPOSITE,
                vec![text("book")],
                Bound::Excluded(uint(1)),
                Bound::Unbounded,
            ))
        );
    }

    #[test]
    fn test_gap_after_a_prefix_and_trailing_only_predicates() {
        assert_eq!(
            plan(Filter::eq("category", text("book")).and(Filter::eq("price", uint(5)))),
            Some(scan(COMPOSITE, vec![text("book")]))
        );
        assert_eq!(plan(Filter::eq("price", uint(5))), None);
        assert_eq!(plan(Filter::ne("category", text("book"))), None);
    }

    #[test]
    fn test_repeated_bounds_keep_the_strictest_in_both_orders() {
        let cases = [
            (
                Filter::ge("brand", uint(1)),
                Filter::ge("brand", uint(3)),
                Bound::Included(uint(3)),
                Bound::Unbounded,
            ),
            (
                Filter::gt("brand", uint(1)),
                Filter::ge("brand", uint(3)),
                Bound::Included(uint(3)),
                Bound::Unbounded,
            ),
            (
                Filter::ge("brand", uint(3)),
                Filter::gt("brand", uint(3)),
                Bound::Excluded(uint(3)),
                Bound::Unbounded,
            ),
            (
                Filter::le("brand", uint(9)),
                Filter::lt("brand", uint(9)),
                Bound::Unbounded,
                Bound::Excluded(uint(9)),
            ),
            (
                Filter::le("brand", uint(4)),
                Filter::lt("brand", uint(9)),
                Bound::Unbounded,
                Bound::Included(uint(4)),
            ),
        ];
        for (left, right, lower, upper) in cases {
            let expected = Some(range(&["brand"], vec![], lower, upper));
            assert_eq!(
                plan(left.clone().and(right.clone())),
                expected,
                "{left:?} AND {right:?}"
            );
            assert_eq!(
                plan(right.clone().and(left.clone())),
                expected,
                "{right:?} AND {left:?}"
            );
        }
    }

    #[test]
    fn test_contradictions_plan_empty() {
        for filter in [
            Filter::eq("brand", uint(1)).and(Filter::eq("brand", uint(2))),
            Filter::in_list("brand", vec![]),
            Filter::in_list("brand", vec![uint(1)]).and(Filter::in_list("brand", vec![uint(2)])),
            Filter::ge("brand", uint(5)).and(Filter::lt("brand", uint(5))),
            Filter::gt("brand", uint(5)).and(Filter::le("brand", uint(5))),
            Filter::eq("brand", uint(3)).and(Filter::gt("brand", uint(3))),
            Filter::eq("note", text("a")).and(Filter::eq("note", text("b"))),
        ] {
            assert_eq!(plan(filter.clone()), Some(AccessPath::Empty), "{filter:?}");
        }
        assert_eq!(
            plan(Filter::ge("brand", uint(5)).and(Filter::le("brand", uint(5)))),
            Some(range(
                &["brand"],
                vec![],
                Bound::Included(uint(5)),
                Bound::Included(uint(5)),
            ))
        );
    }

    #[test]
    fn test_in_lists_enumerate_sorted_distinct_keys() {
        assert_eq!(
            plan(
                Filter::eq("category", text("book"))
                    .and(Filter::in_list("brand", vec![uint(2), uint(1), uint(2)]))
                    .and(Filter::eq("price", uint(3)))
            ),
            Some(AccessPath::Union(vec![
                scan(COMPOSITE, vec![text("book"), uint(1), uint(3)]),
                scan(COMPOSITE, vec![text("book"), uint(2), uint(3)]),
            ]))
        );
        assert_eq!(
            plan(Filter::in_list("brand", vec![uint(4)])),
            Some(scan(&["brand"], vec![uint(4)]))
        );
    }

    #[test]
    fn test_in_list_over_the_cap_keeps_a_narrower_prefix() {
        let values = |count: u32| (0..count).map(uint).collect::<Vec<_>>();
        let capped = plan(
            Filter::eq("category", text("book"))
                .and(Filter::in_list("brand", values(MAX_ALTERNATIVES as u32))),
        )
        .expect("plan expected");
        assert_eq!(capped.scan_count(), MAX_ALTERNATIVES);
        assert_eq!(
            plan(Filter::eq("category", text("book")).and(Filter::in_list(
                "brand",
                values(MAX_ALTERNATIVES as u32 + 1),
            )),),
            Some(scan(COMPOSITE, vec![text("book")]))
        );
        assert_eq!(
            plan(Filter::in_list(
                "shard",
                values(MAX_ALTERNATIVES as u32 + 1)
            )),
            None
        );
    }

    #[test]
    fn test_primary_key_equality_beats_every_other_index() {
        assert_eq!(
            plan(
                Filter::eq("category", text("book"))
                    .and(Filter::eq("brand", uint(1)))
                    .and(Filter::eq("price", uint(3)))
                    .and(Filter::eq("id", uint(4)))
            ),
            Some(scan(&["id"], vec![uint(4)]))
        );
    }

    #[test]
    fn test_full_composite_equality_beats_single_column_equality() {
        assert_eq!(
            plan(
                Filter::eq("brand", uint(1))
                    .and(Filter::eq("price", uint(3)))
                    .and(Filter::eq("category", text("book")))
            ),
            Some(scan(COMPOSITE, vec![text("book"), uint(1), uint(3)]))
        );
    }

    #[test]
    fn test_fallible_detection_and_filter_columns() {
        use std::collections::BTreeSet;

        use wasm_dbms_api::prelude::JsonFilter;

        use super::{collect_filter_columns, is_fallible};

        assert!(!is_fallible(
            &Filter::eq("brand", uint(1)).or(Filter::is_null("label").not())
        ));
        assert!(is_fallible(
            &Filter::eq("brand", uint(1)).and(Filter::like("note", "a%"))
        ));
        assert!(is_fallible(
            &Filter::eq("brand", uint(1))
                .or(Filter::json("note", JsonFilter::extract_eq("a", uint(1)),))
        ));

        let mut columns = BTreeSet::new();
        collect_filter_columns(
            &Filter::eq("brand", uint(1))
                .or(Filter::like("note", "a%").and(Filter::is_null("label").not())),
            &mut columns,
        );
        assert_eq!(
            columns.into_iter().collect::<Vec<_>>(),
            vec!["brand", "label", "note"]
        );
    }

    #[test]
    fn test_or_branches_become_a_union() {
        assert_eq!(
            plan(Filter::eq("shard", uint(1)).or(Filter::eq("brand", uint(2)))),
            Some(AccessPath::Union(vec![
                scan(&["shard"], vec![uint(1)]),
                scan(&["brand"], vec![uint(2)]),
            ]))
        );
        assert_eq!(
            plan(
                Filter::eq("shard", uint(1))
                    .or(Filter::eq("brand", uint(1)).and(Filter::eq("brand", uint(2))))
            ),
            Some(scan(&["shard"], vec![uint(1)]))
        );
    }

    #[test]
    fn test_an_unindexable_branch_disables_the_union() {
        assert_eq!(
            plan(Filter::eq("shard", uint(1)).or(Filter::eq("note", text("a")))),
            None
        );
        assert_eq!(
            plan(Filter::eq("shard", uint(1)).or(Filter::eq("brand", uint(2)).not())),
            None
        );
        // Another conjunct outside the OR still supplies candidates.
        assert_eq!(
            plan(
                Filter::eq("brand", uint(3))
                    .and(Filter::eq("shard", uint(1)).or(Filter::eq("note", text("a"))))
            ),
            Some(scan(&["brand"], vec![uint(3)]))
        );
    }

    #[test]
    fn test_or_over_the_alternative_cap_is_not_planned() {
        let or_of = |count: u32| {
            (0..count)
                .map(|value| Filter::eq("shard", uint(value)))
                .reduce(Filter::or)
                .expect("at least one branch")
        };
        assert_eq!(
            plan(or_of(MAX_ALTERNATIVES as u32)).map(|path| path.scan_count()),
            Some(MAX_ALTERNATIVES)
        );
        assert_eq!(plan(or_of(MAX_ALTERNATIVES as u32 + 1)), None);
    }

    #[test]
    fn test_separate_indexes_intersect_independently_of_and_order() {
        let expected = Some(AccessPath::Intersection(vec![
            scan(&["brand"], vec![uint(1)]),
            scan(&["shard"], vec![uint(2)]),
        ]));
        assert_eq!(
            plan(Filter::eq("brand", uint(1)).and(Filter::eq("shard", uint(2)))),
            expected
        );
        assert_eq!(
            plan(Filter::eq("shard", uint(2)).and(Filter::eq("brand", uint(1)))),
            expected
        );
    }

    #[test]
    fn test_intersection_orders_cheaper_paths_first_and_drops_redundant_ones() {
        // The composite prefix already constrains `brand`, so the brand index adds nothing.
        assert_eq!(
            plan(
                Filter::eq("category", text("book"))
                    .and(Filter::eq("brand", uint(1)))
                    .and(Filter::lt("shard", uint(3)))
            ),
            Some(AccessPath::Intersection(vec![
                scan(COMPOSITE, vec![text("book"), uint(1)]),
                range(
                    &["shard"],
                    vec![],
                    Bound::Unbounded,
                    Bound::Excluded(uint(3)),
                ),
            ]))
        );
        // An OR conjunct joins the intersection after the index ranges.
        assert_eq!(
            plan(
                Filter::eq("shard", uint(4))
                    .and(Filter::eq("brand", uint(1)).or(Filter::eq("brand", uint(2))))
            ),
            Some(AccessPath::Intersection(vec![
                scan(&["shard"], vec![uint(4)]),
                AccessPath::Union(vec![
                    scan(&["brand"], vec![uint(1)]),
                    scan(&["brand"], vec![uint(2)]),
                ]),
            ]))
        );
    }

    #[test]
    fn test_is_null_is_an_exact_null_key() {
        assert_eq!(
            plan(Filter::is_null("stock")),
            Some(scan(&["stock"], vec![Value::Null]))
        );
        assert_eq!(
            plan(Filter::is_null("label").and(Filter::eq("stock", Value::Int32(2.into()),))),
            Some(scan(
                &["label", "stock"],
                vec![Value::Null, Value::Int32(2.into())],
            ))
        );
        assert_eq!(
            plan(Filter::is_null("label").and(Filter::not_null("label"))),
            Some(AccessPath::Empty)
        );
        assert_eq!(
            plan(Filter::is_null("stock").and(Filter::eq("stock", Value::Int32(1.into()),))),
            Some(AccessPath::Empty)
        );
    }

    #[test]
    fn test_not_null_bounds_follow_the_declared_type() {
        assert_eq!(
            plan(Filter::not_null("stock")),
            Some(range(
                &["stock"],
                vec![],
                Bound::Unbounded,
                Bound::Excluded(Value::Null),
            ))
        );
        assert_eq!(
            plan(Filter::not_null("label")),
            Some(range(
                &["label"],
                vec![],
                Bound::Included(text("")),
                Bound::Unbounded,
            ))
        );
        assert_eq!(
            plan(Filter::not_null("rank")),
            Some(range(
                &["rank"],
                vec![],
                Bound::Included(uint(0)),
                Bound::Unbounded,
            ))
        );
        assert_eq!(
            plan(Filter::eq("label", text("a")).and(Filter::not_null("stock"))),
            Some(range(
                &["label", "stock"],
                vec![text("a")],
                Bound::Unbounded,
                Bound::Excluded(Value::Null),
            ))
        );
        // No proven minimum, and non-nullable columns match every row.
        assert_eq!(plan(Filter::not_null("token")), None);
        assert_eq!(plan(Filter::not_null("brand")), None);
        // Explicit bounds win over the NOT NULL bounds.
        assert_eq!(
            plan(Filter::not_null("rank").and(Filter::ge("rank", uint(3)))),
            Some(range(
                &["rank"],
                vec![],
                Bound::Included(uint(3)),
                Bound::Unbounded,
            ))
        );
    }

    #[test]
    fn test_not_null_candidates_are_read_last() {
        assert_eq!(
            plan(Filter::not_null("stock").and(Filter::eq("shard", uint(1)))),
            Some(AccessPath::Intersection(vec![
                scan(&["shard"], vec![uint(1)]),
                range(
                    &["stock"],
                    vec![],
                    Bound::Unbounded,
                    Bound::Excluded(Value::Null),
                ),
            ]))
        );
    }
}
