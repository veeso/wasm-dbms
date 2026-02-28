use crate::prelude::{ColumnDef, Value, ValuesSource};

/// Helper function which takes a list of `(ValuesSource, Value)` tuples, takes only those with
/// [`ValuesSource::Foreign`] matching the provided table and column names, and returns a vector of
/// the corresponding `Value`s with the [`ValuesSource`] set to [`ValuesSource::This`].
pub fn self_reference_values(
    values: &[(ValuesSource, Vec<(ColumnDef, Value)>)],
    table: &'static str,
    local_column: &'static str,
) -> Vec<(ValuesSource, Vec<(ColumnDef, Value)>)> {
    values
        .iter()
        .filter(|(source, _)| {
            matches!(source, ValuesSource::Foreign { table: t, column } if t == table && column == local_column)
        })
        .map(|(_, value)| (ValuesSource::This, value.clone()))
        .collect()
}
