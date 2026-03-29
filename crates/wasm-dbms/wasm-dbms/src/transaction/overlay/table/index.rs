//! Table overlay index tracking for uncommitted transaction changes.

use std::collections::{BTreeMap, HashMap, HashSet};

use wasm_dbms_api::prelude::Value;

/// Overlay for tracking uncommited transaction changes to indexes on a table.
///
/// 1 Table has 1 [`IndexOverlay`], which has n BTreeMaps for each index
#[derive(Debug, Default, Clone)]
pub struct IndexOverlay(HashMap<Vec<&'static str>, IndexColumnOverlay>);

/// Overlay for tracking uncommited transaction changes to an indexed column.
#[derive(Debug, Default, Clone)]
struct IndexColumnOverlay {
    /// Maps from added indexed values to their corresponding primary key values.
    added: BTreeMap<Vec<Value>, HashSet<Value>>,
    /// Maps from removed indexed values to their corresponding primary key values.
    removed: BTreeMap<Vec<Value>, HashSet<Value>>,
}

impl IndexOverlay {
    /// Inserts a record into the index overlay for the specified indexed columns, indexed values, and primary key.
    ///
    /// The primary key is added to the set of primary keys associated with the indexed values in the `added` map.
    /// If the primary key was previously in the `removed` set for these values, it is removed from there.
    pub fn insert(
        &mut self,
        indexed_columns: &[&'static str],
        indexed_values: Vec<Value>,
        pk: Value,
    ) {
        let column_overlay = self.0.entry(indexed_columns.to_vec()).or_default();
        column_overlay.insert(indexed_values, pk);
    }

    /// Removes a record from the index overlay for the specified indexed columns, indexed values, and primary key.
    ///
    /// The primary key is added to the set of primary keys associated with the indexed values in the `removed` map.
    /// If the primary key was previously in the `added` set for these values, it is removed from there.
    pub fn delete(
        &mut self,
        indexed_columns: &[&'static str],
        indexed_values: Vec<Value>,
        pk: Value,
    ) {
        let column_overlay = self.0.entry(indexed_columns.to_vec()).or_default();
        column_overlay.delete(indexed_values, pk);
    }

    /// Returns the set of primary keys added for the given index columns and indexed values.
    ///
    /// Returns an empty set if no entries exist.
    pub fn added_pks(
        &self,
        indexed_columns: &[&'static str],
        indexed_values: &[Value],
    ) -> HashSet<Value> {
        self.0
            .get(indexed_columns)
            .and_then(|overlay| overlay.added.get(indexed_values))
            .cloned()
            .unwrap_or_default()
    }

    /// Returns the set of primary keys removed for the given index columns and indexed values.
    ///
    /// Returns an empty set if no entries exist.
    pub fn removed_pks(
        &self,
        indexed_columns: &[&'static str],
        indexed_values: &[Value],
    ) -> HashSet<Value> {
        self.0
            .get(indexed_columns)
            .and_then(|overlay| overlay.removed.get(indexed_values))
            .cloned()
            .unwrap_or_default()
    }

    /// Returns the set of primary keys added within the given inclusive key range.
    pub fn added_pks_in_range(
        &self,
        indexed_columns: &[&'static str],
        start: Option<&[Value]>,
        end: Option<&[Value]>,
    ) -> HashSet<Value> {
        self.pks_in_range_from_map(indexed_columns, start, end, |overlay| &overlay.added)
    }

    /// Returns the set of primary keys removed within the given inclusive key range.
    pub fn removed_pks_in_range(
        &self,
        indexed_columns: &[&'static str],
        start: Option<&[Value]>,
        end: Option<&[Value]>,
    ) -> HashSet<Value> {
        self.pks_in_range_from_map(indexed_columns, start, end, |overlay| &overlay.removed)
    }

    /// Updates a record in the index overlay by removing the old indexed values and inserting the new ones.
    ///
    /// This is equivalent to calling [`delete`](Self::delete) with the old values
    /// followed by [`insert`](Self::insert) with the new values.
    pub fn update(
        &mut self,
        indexed_columns: &[&'static str],
        old_indexed_values: Vec<Value>,
        new_indexed_values: Vec<Value>,
        pk: Value,
    ) {
        let column_overlay = self.0.entry(indexed_columns.to_vec()).or_default();
        column_overlay.delete(old_indexed_values, pk.clone());
        column_overlay.insert(new_indexed_values, pk);
    }

    fn pks_in_range_from_map(
        &self,
        indexed_columns: &[&'static str],
        start: Option<&[Value]>,
        end: Option<&[Value]>,
        map_selector: impl Fn(&IndexColumnOverlay) -> &BTreeMap<Vec<Value>, HashSet<Value>>,
    ) -> HashSet<Value> {
        use std::ops::Bound;

        let Some(column_overlay) = self.0.get(indexed_columns) else {
            return HashSet::new();
        };

        let start_bound = match start {
            Some(start_key) => Bound::Included(start_key.to_vec()),
            None => Bound::Unbounded,
        };
        let end_bound = match end {
            Some(end_key) => Bound::Included(end_key.to_vec()),
            None => Bound::Unbounded,
        };

        map_selector(column_overlay)
            .range((start_bound, end_bound))
            .flat_map(|(_, pks)| pks.iter().cloned())
            .collect()
    }
}

impl IndexColumnOverlay {
    /// Inserts a record into the index overlay for the specified indexed values and primary key.
    ///
    /// The primary key is added to the `added` set and removed from `removed` if present,
    /// keeping the two sets consistent.
    fn insert(&mut self, indexed_values: Vec<Value>, pk: Value) {
        if let Some(removed_pks) = self.removed.get_mut(&indexed_values) {
            removed_pks.remove(&pk);
        }
        self.added.entry(indexed_values).or_default().insert(pk);
    }

    /// Removes a record from the index overlay for the specified indexed values and primary key.
    ///
    /// If the PK was in the `added` set (overlay-only entry), it is simply removed from `added`
    /// without adding to `removed`, since the base index never had this entry.
    /// If the PK was **not** in `added`, it is added to `removed` to mark a base index entry
    /// for exclusion.
    fn delete(&mut self, indexed_values: Vec<Value>, pk: Value) {
        let was_in_added = self
            .added
            .get_mut(&indexed_values)
            .is_some_and(|added_pks| added_pks.remove(&pk));

        if !was_in_added {
            self.removed.entry(indexed_values).or_default().insert(pk);
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn pk(id: u32) -> Value {
        Value::Uint32(id.into())
    }

    fn text_val(s: &str) -> Value {
        Value::Text(s.to_string().into())
    }

    // -- IndexColumnOverlay tests --

    #[test]
    fn test_column_overlay_insert_should_add_to_added() {
        let mut overlay = IndexColumnOverlay::default();
        overlay.insert(vec![text_val("alice")], pk(1));

        assert!(overlay.added[&vec![text_val("alice")]].contains(&pk(1)));
        assert!(overlay.removed.is_empty());
    }

    #[test]
    fn test_column_overlay_delete_should_add_to_removed() {
        let mut overlay = IndexColumnOverlay::default();
        overlay.delete(vec![text_val("alice")], pk(1));

        assert!(overlay.removed[&vec![text_val("alice")]].contains(&pk(1)));
        assert!(overlay.added.is_empty());
    }

    #[test]
    fn test_column_overlay_insert_after_delete_should_move_pk_from_removed_to_added() {
        let mut overlay = IndexColumnOverlay::default();
        let key = vec![text_val("alice")];

        overlay.delete(key.clone(), pk(1));
        assert!(overlay.removed[&key].contains(&pk(1)));

        overlay.insert(key.clone(), pk(1));
        assert!(overlay.added[&key].contains(&pk(1)));
        assert!(!overlay.removed[&key].contains(&pk(1)));
    }

    #[test]
    fn test_column_overlay_delete_after_insert_should_remove_from_added_without_stale_removed() {
        let mut overlay = IndexColumnOverlay::default();
        let key = vec![text_val("alice")];

        overlay.insert(key.clone(), pk(1));
        assert!(overlay.added[&key].contains(&pk(1)));

        // Delete an overlay-only entry: should remove from added but NOT add to removed,
        // since this key+pk was never in the base index.
        overlay.delete(key.clone(), pk(1));
        assert!(!overlay.added[&key].contains(&pk(1)));
        assert!(
            overlay
                .removed
                .get(&key)
                .map_or(true, |pks| !pks.contains(&pk(1)))
        );
    }

    #[test]
    fn test_column_overlay_delete_base_entry_goes_to_removed() {
        // Simulates deleting a record that exists in the base index (not overlay-inserted).
        // Since it was never in `added`, it should appear in `removed`.
        let mut overlay = IndexColumnOverlay::default();
        let key = vec![text_val("alice")];

        overlay.delete(key.clone(), pk(1));

        assert!(overlay.removed[&key].contains(&pk(1)));
        assert!(overlay.added.is_empty());
    }

    #[test]
    fn test_column_overlay_delete_base_then_reinsert_cancels_removed() {
        // Delete a base entry, then re-insert the same key+pk.
        // The re-insert should cancel the removal.
        let mut overlay = IndexColumnOverlay::default();
        let key = vec![text_val("alice")];

        overlay.delete(key.clone(), pk(1));
        assert!(overlay.removed[&key].contains(&pk(1)));

        overlay.insert(key.clone(), pk(1));
        assert!(overlay.added[&key].contains(&pk(1)));
        assert!(!overlay.removed[&key].contains(&pk(1)));
    }

    #[test]
    fn test_column_overlay_multiple_pks_per_key() {
        let mut overlay = IndexColumnOverlay::default();
        let key = vec![text_val("alice")];

        overlay.insert(key.clone(), pk(1));
        overlay.insert(key.clone(), pk(2));
        overlay.insert(key.clone(), pk(3));

        let added_pks = &overlay.added[&key];
        assert_eq!(added_pks.len(), 3);
        assert!(added_pks.contains(&pk(1)));
        assert!(added_pks.contains(&pk(2)));
        assert!(added_pks.contains(&pk(3)));
    }

    #[test]
    fn test_column_overlay_delete_one_pk_should_not_affect_others() {
        let mut overlay = IndexColumnOverlay::default();
        let key = vec![text_val("alice")];

        overlay.insert(key.clone(), pk(1));
        overlay.insert(key.clone(), pk(2));
        overlay.delete(key.clone(), pk(1));

        assert!(!overlay.added[&key].contains(&pk(1)));
        assert!(overlay.added[&key].contains(&pk(2)));
        // pk(1) was overlay-only, so it should NOT be in removed
        assert!(
            overlay
                .removed
                .get(&key)
                .map_or(true, |pks| !pks.contains(&pk(1)))
        );
    }

    // -- IndexOverlay tests --

    #[test]
    fn test_index_overlay_insert() {
        let mut overlay = IndexOverlay::default();
        let cols: &[&str] = &["name"];

        overlay.insert(cols, vec![text_val("alice")], pk(1));

        let column_overlay = &overlay.0[&vec!["name"]];
        assert!(column_overlay.added[&vec![text_val("alice")]].contains(&pk(1)));
    }

    #[test]
    fn test_index_overlay_delete() {
        let mut overlay = IndexOverlay::default();
        let cols: &[&str] = &["name"];

        overlay.delete(cols, vec![text_val("alice")], pk(1));

        let column_overlay = &overlay.0[&vec!["name"]];
        assert!(column_overlay.removed[&vec![text_val("alice")]].contains(&pk(1)));
    }

    #[test]
    fn test_index_overlay_update_should_remove_old_and_add_new() {
        let mut overlay = IndexOverlay::default();
        let cols: &[&str] = &["name"];

        overlay.update(cols, vec![text_val("alice")], vec![text_val("bob")], pk(1));

        let column_overlay = &overlay.0[&vec!["name"]];
        assert!(column_overlay.removed[&vec![text_val("alice")]].contains(&pk(1)));
        assert!(column_overlay.added[&vec![text_val("bob")]].contains(&pk(1)));
    }

    #[test]
    fn test_index_overlay_update_same_value_should_be_consistent() {
        let mut overlay = IndexOverlay::default();
        let cols: &[&str] = &["name"];

        // Update where old == new: delete removes from added, insert adds back
        overlay.update(
            cols,
            vec![text_val("alice")],
            vec![text_val("alice")],
            pk(1),
        );

        let column_overlay = &overlay.0[&vec!["name"]];
        // PK should be in added (insert happened after delete)
        assert!(column_overlay.added[&vec![text_val("alice")]].contains(&pk(1)));
        // PK should not be in removed (insert cleaned it)
        assert!(!column_overlay.removed[&vec![text_val("alice")]].contains(&pk(1)));
    }

    #[test]
    fn test_index_overlay_multiple_indexes() {
        let mut overlay = IndexOverlay::default();
        let name_cols: &[&str] = &["name"];
        let email_cols: &[&str] = &["email"];

        overlay.insert(name_cols, vec![text_val("alice")], pk(1));
        overlay.insert(email_cols, vec![text_val("alice@example.com")], pk(1));

        assert!(overlay.0.contains_key(&vec!["name"]));
        assert!(overlay.0.contains_key(&vec!["email"]));

        let name_overlay = &overlay.0[&vec!["name"]];
        assert!(name_overlay.added[&vec![text_val("alice")]].contains(&pk(1)));

        let email_overlay = &overlay.0[&vec!["email"]];
        assert!(email_overlay.added[&vec![text_val("alice@example.com")]].contains(&pk(1)));
    }

    #[test]
    fn test_index_overlay_composite_index() {
        let mut overlay = IndexOverlay::default();
        let cols: &[&str] = &["first_name", "last_name"];

        overlay.insert(cols, vec![text_val("alice"), text_val("smith")], pk(1));
        overlay.insert(cols, vec![text_val("alice"), text_val("jones")], pk(2));

        let column_overlay = &overlay.0[&vec!["first_name", "last_name"]];
        assert!(column_overlay.added[&vec![text_val("alice"), text_val("smith")]].contains(&pk(1)));
        assert!(column_overlay.added[&vec![text_val("alice"), text_val("jones")]].contains(&pk(2)));
    }

    #[test]
    fn test_index_overlay_insert_delete_insert_cycle() {
        let mut overlay = IndexOverlay::default();
        let cols: &[&str] = &["name"];
        let key = vec![text_val("alice")];

        overlay.insert(cols, key.clone(), pk(1));
        overlay.delete(cols, key.clone(), pk(1));
        overlay.insert(cols, key.clone(), pk(1));

        let column_overlay = &overlay.0[&vec!["name"]];
        assert!(column_overlay.added[&key].contains(&pk(1)));
        // After insert→delete→insert, PK was never in the base index,
        // so removed should not contain it.
        assert!(
            column_overlay
                .removed
                .get(&key)
                .map_or(true, |pks| !pks.contains(&pk(1)))
        );
    }

    #[test]
    fn test_added_pks_in_range_returns_matching_entries() {
        let mut overlay = IndexOverlay::default();
        let cols: &[&str] = &["age"];
        overlay.insert(cols, vec![Value::Uint32(20.into())], pk(1));
        overlay.insert(cols, vec![Value::Uint32(25.into())], pk(2));
        overlay.insert(cols, vec![Value::Uint32(30.into())], pk(3));
        overlay.insert(cols, vec![Value::Uint32(35.into())], pk(4));

        let pks = overlay.added_pks_in_range(
            cols,
            Some(&[Value::Uint32(25.into())]),
            Some(&[Value::Uint32(30.into())]),
        );

        assert_eq!(pks.len(), 2);
        assert!(pks.contains(&pk(2)));
        assert!(pks.contains(&pk(3)));
    }

    #[test]
    fn test_removed_pks_in_range_returns_matching_entries() {
        let mut overlay = IndexOverlay::default();
        let cols: &[&str] = &["age"];
        overlay.delete(cols, vec![Value::Uint32(20.into())], pk(1));
        overlay.delete(cols, vec![Value::Uint32(25.into())], pk(2));

        let pks = overlay.removed_pks_in_range(
            cols,
            Some(&[Value::Uint32(20.into())]),
            Some(&[Value::Uint32(25.into())]),
        );

        assert_eq!(pks.len(), 2);
        assert!(pks.contains(&pk(1)));
        assert!(pks.contains(&pk(2)));
    }

    #[test]
    fn test_added_pks_in_range_open_ended() {
        let mut overlay = IndexOverlay::default();
        let cols: &[&str] = &["age"];
        overlay.insert(cols, vec![Value::Uint32(20.into())], pk(1));
        overlay.insert(cols, vec![Value::Uint32(30.into())], pk(2));

        let pks = overlay.added_pks_in_range(cols, Some(&[Value::Uint32(25.into())]), None);
        assert_eq!(pks.len(), 1);
        assert!(pks.contains(&pk(2)));

        let pks = overlay.added_pks_in_range(cols, None, Some(&[Value::Uint32(25.into())]));
        assert_eq!(pks.len(), 1);
        assert!(pks.contains(&pk(1)));
    }
}
