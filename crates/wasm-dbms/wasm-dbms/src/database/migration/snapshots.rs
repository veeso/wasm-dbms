//! Schema snapshot canonicalisation and persisted schema hashing.
//!
//! The hash function and the canonicalisation rules below are part of the
//! on-disk contract: changing either invalidates the drift comparison for every
//! existing deployment, forcing a one-time false-positive drift on the next
//! boot. The hash is seeded with [`TableSchemaSnapshot::latest_version`] so
//! bumping the snapshot wire format automatically invalidates old hashes; any
//! change to the canonicalisation logic in this file that does **not** also
//! bump the snapshot version requires manual coordination across deployments.

use wasm_dbms_api::prelude::{Encode, TableSchemaSnapshot};
use xxhash_rust::xxh3::Xxh3;

/// Computes the drift hash for a set of snapshots.
///
/// Snapshots are sorted by `name` so two equivalent sets hash identically
/// regardless of input order, then each is encoded through [`Encode::encode`]
/// (the same wire format used for stable-memory persistence) and folded into a
/// single `xxh3` digest seeded with [`TableSchemaSnapshot::latest_version`].
pub(crate) fn compute_hash(mut snapshots: Vec<TableSchemaSnapshot>) -> u64 {
    snapshots.sort_by(|a, b| a.name.cmp(&b.name));

    let mut hasher = Xxh3::new();
    hasher.update(&[TableSchemaSnapshot::latest_version()]);
    for snapshot in &snapshots {
        let bytes = snapshot.encode();
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    hasher.digest()
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{
        ColumnSnapshot, DataTypeSnapshot, IndexSnapshot, TableSchema, TableSchemaSnapshot, Text,
        Uint32,
    };
    use wasm_dbms_macros::{DatabaseSchema, Table};
    use wasm_dbms_memory::MemoryAccess;
    use wasm_dbms_memory::prelude::HeapMemoryProvider;

    use super::*;
    use crate::context::DbmsContext;
    use crate::schema::DatabaseSchema;

    fn snapshot(name: &str, columns: Vec<ColumnSnapshot>) -> TableSchemaSnapshot {
        TableSchemaSnapshot {
            version: TableSchemaSnapshot::latest_version(),
            name: name.to_string(),
            primary_key: "id".to_string(),
            alignment: 8,
            columns,
            indexes: Vec::<IndexSnapshot>::new(),
        }
    }

    fn id_column() -> ColumnSnapshot {
        ColumnSnapshot {
            name: "id".to_string(),
            data_type: DataTypeSnapshot::Uint32,
            nullable: false,
            auto_increment: false,
            unique: true,
            primary_key: true,
            foreign_key: None,
            default: None,
        }
    }

    #[test]
    fn test_hash_is_order_independent() {
        let a = snapshot("alpha", vec![id_column()]);
        let b = snapshot("bravo", vec![id_column()]);

        let one = compute_hash(vec![a.clone(), b.clone()]);
        let two = compute_hash(vec![b, a]);
        assert_eq!(one, two);
    }

    #[test]
    fn test_hash_changes_when_column_added() {
        let baseline = snapshot("alpha", vec![id_column()]);
        let mut extended_columns = vec![id_column()];
        extended_columns.push(ColumnSnapshot {
            name: "email".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: true,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        let extended = snapshot("alpha", extended_columns);

        assert_ne!(
            compute_hash(vec![baseline]),
            compute_hash(vec![extended]),
            "adding a column must change the drift hash"
        );
    }

    #[test]
    fn test_hash_empty_input_is_stable() {
        // Two empty schemas hash equal — important so a fresh registry against
        // an empty compiled schema does not spuriously report drift.
        assert_eq!(compute_hash(vec![]), compute_hash(vec![]));
    }

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "users"]
    pub struct User {
        #[primary_key]
        pub id: Uint32,
        pub name: Text,
    }

    #[derive(DatabaseSchema)]
    #[tables(User = "users")]
    pub struct UserSchema;

    #[test]
    fn test_schema_registry_hash_matches_compiled_schema() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        UserSchema::register_tables(&ctx).unwrap();

        let compiled_hash =
            compute_hash(<UserSchema as DatabaseSchema<HeapMemoryProvider>>::compiled_snapshots());
        assert_eq!(ctx.schema_registry.borrow().schema_hash(), compiled_hash);
    }

    #[test]
    fn test_schema_registry_hash_changes_when_persisted_snapshot_diverges() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        UserSchema::register_tables(&ctx).unwrap();

        // Tamper with the persisted snapshot so its column-set diverges from
        // the compiled definition.
        let snapshot_page = {
            let sr = ctx.schema_registry.borrow();
            sr.table_registry_page::<User>()
                .unwrap()
                .schema_snapshot_page
        };
        let mut tampered = User::schema_snapshot();
        tampered.columns.push(ColumnSnapshot {
            name: "email".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: true,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        ctx.mm
            .borrow_mut()
            .write_at(snapshot_page, 0, &tampered)
            .unwrap();
        {
            let mut schema_registry = ctx.schema_registry.borrow_mut();
            let mut mm = ctx.mm.borrow_mut();
            schema_registry.refresh_schema_hash(&mut *mm).unwrap();
            schema_registry.save(&mut *mm).unwrap();
        }

        let compiled_hash =
            compute_hash(<UserSchema as DatabaseSchema<HeapMemoryProvider>>::compiled_snapshots());
        assert_ne!(ctx.schema_registry.borrow().schema_hash(), compiled_hash);
    }
}
