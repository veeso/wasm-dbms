// Rust guideline compliant 2026-04-27
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! API generic interface to be used by different DBMS canisters.

mod inspect;

use std::collections::HashSet;

use candid::Principal;
use ic_dbms_api::prelude::{
    AggregateFunction, AggregatedRow, ColumnDef, Database, DbmsError, DeleteBehavior, Filter,
    IcDbmsResult, IdentityPerms, InsertRecord, JoinColumnDef, MigrationOp, MigrationPolicy,
    PermGrant, PermRevoke, Query, QueryError, RequiredPerm, TableFingerprint, TablePerms,
    TableSchema, TransactionId, UpdateRecord, Value, fingerprint_for_name,
};
use wasm_dbms::prelude::{DatabaseSchema, WasmDbmsDatabase};

pub use self::inspect::inspect;
use crate::memory::{DBMS_CONTEXT, IcAccessControlList, IcMemoryProvider};
use crate::trap;

// --- ACL: grants -----------------------------------------------------------

/// Grants the `admin` bypass flag to `target`. Caller must hold
/// `manage_acl`.
pub fn grant_admin(target: Principal) -> IcDbmsResult<()> {
    apply_grant(target, PermGrant::Admin)
}

/// Revokes the `admin` bypass flag from `target`.
pub fn revoke_admin(target: Principal) -> IcDbmsResult<()> {
    apply_revoke(target, PermRevoke::Admin)
}

/// Grants the `manage_acl` operational flag to `target`.
pub fn grant_manage_acl(target: Principal) -> IcDbmsResult<()> {
    apply_grant(target, PermGrant::ManageAcl)
}

/// Revokes the `manage_acl` operational flag from `target`.
pub fn revoke_manage_acl(target: Principal) -> IcDbmsResult<()> {
    apply_revoke(target, PermRevoke::ManageAcl)
}

/// Grants the `migrate` operational flag to `target`.
pub fn grant_migrate(target: Principal) -> IcDbmsResult<()> {
    apply_grant(target, PermGrant::Migrate)
}

/// Revokes the `migrate` operational flag from `target`.
pub fn revoke_migrate(target: Principal) -> IcDbmsResult<()> {
    apply_revoke(target, PermRevoke::Migrate)
}

/// Grants `perms` on every table to `target`.
pub fn grant_all_tables_perms(target: Principal, perms: TablePerms) -> IcDbmsResult<()> {
    apply_grant(target, PermGrant::AllTables(perms))
}

/// Revokes `perms` on every table from `target` (does not affect
/// per-table grants).
pub fn revoke_all_tables_perms(target: Principal, perms: TablePerms) -> IcDbmsResult<()> {
    apply_revoke(target, PermRevoke::AllTables(perms))
}

/// Grants `perms` on the named `table` to `target`.
pub fn grant_table_perms(target: Principal, table: String, perms: TablePerms) -> IcDbmsResult<()> {
    let fingerprint = resolve_table_fingerprint(&table)?;
    apply_grant(target, PermGrant::Table(fingerprint, perms))
}

/// Revokes `perms` on the named `table` from `target`.
pub fn revoke_table_perms(target: Principal, table: String, perms: TablePerms) -> IcDbmsResult<()> {
    let fingerprint = resolve_table_fingerprint(&table)?;
    apply_revoke(target, PermRevoke::Table(fingerprint, perms))
}

/// Removes `target` entirely from the ACL. Caller must hold `manage_acl`.
pub fn remove_identity(target: Principal) -> IcDbmsResult<()> {
    let caller = crate::utils::caller();
    DBMS_CONTEXT.with(|ctx| {
        if !ctx.granted_manage_acl(&caller) {
            return Err(DbmsError::AccessDenied {
                table: None,
                required: RequiredPerm::ManageAcl,
            });
        }
        ctx.acl_remove_identity(&target)
    })
}

/// Lists every identity together with its [`IdentityPerms`]. Caller must
/// hold `manage_acl`.
pub fn list_identities() -> IcDbmsResult<Vec<(Principal, IdentityPerms)>> {
    let caller = crate::utils::caller();
    DBMS_CONTEXT.with(|ctx| {
        if !ctx.granted_manage_acl(&caller) {
            return Err(DbmsError::AccessDenied {
                table: None,
                required: RequiredPerm::ManageAcl,
            });
        }
        Ok(ctx.acl_identities())
    })
}

/// Returns the caller's own [`IdentityPerms`]. Always permitted.
pub fn my_perms() -> IdentityPerms {
    let caller = crate::utils::caller();
    DBMS_CONTEXT.with(|ctx| ctx.acl_perms(&caller))
}

fn apply_grant(target: Principal, g: PermGrant) -> IcDbmsResult<()> {
    let caller = crate::utils::caller();
    DBMS_CONTEXT.with(|ctx| {
        if !ctx.granted_manage_acl(&caller) {
            return Err(DbmsError::AccessDenied {
                table: None,
                required: RequiredPerm::ManageAcl,
            });
        }
        ctx.acl_grant(target, g)
    })
}

fn apply_revoke(target: Principal, r: PermRevoke) -> IcDbmsResult<()> {
    let caller = crate::utils::caller();
    DBMS_CONTEXT.with(|ctx| {
        if !ctx.granted_manage_acl(&caller) {
            return Err(DbmsError::AccessDenied {
                table: None,
                required: RequiredPerm::ManageAcl,
            });
        }
        ctx.acl_revoke(&target, r)
    })
}

// --- Transactions ----------------------------------------------------------

/// Begins a new transaction owned by the caller and returns its ID.
///
/// Opening a transaction is unconditional — per-CRUD perm checks gate
/// the data accesses inside the transaction.
pub fn begin_transaction() -> TransactionId {
    let owner = crate::utils::caller();
    DBMS_CONTEXT.with(|ctx| ctx.begin_transaction(owner.as_slice().to_vec()))
}

/// Commits the transaction with the given ID. Caller must own the
/// transaction.
pub fn commit<S>(transaction_id: TransactionId, database_schema: S) -> IcDbmsResult<()>
where
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    assert_caller_owns_transaction(Some(&transaction_id));
    DBMS_CONTEXT.with(|ctx| {
        let mut db = WasmDbmsDatabase::from_transaction(ctx, database_schema, transaction_id);
        db.commit()
    })
}

/// Rolls back the transaction with the given ID. Caller must own the
/// transaction.
pub fn rollback<S>(transaction_id: TransactionId, database_schema: S) -> IcDbmsResult<()>
where
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    assert_caller_owns_transaction(Some(&transaction_id));
    DBMS_CONTEXT.with(|ctx| {
        let mut db = WasmDbmsDatabase::from_transaction(ctx, database_schema, transaction_id);
        db.rollback()
    })
}

// --- CRUD ------------------------------------------------------------------

/// Executes a select query against the database schema, optionally within a transaction.
pub fn select<T, S>(
    query: Query,
    transaction_id: Option<TransactionId>,
    database_schema: S,
) -> IcDbmsResult<Vec<T::Record>>
where
    T: TableSchema,
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    check_table_perm(T::fingerprint(), TablePerms::READ)?;
    assert_caller_owns_transaction(transaction_id.as_ref());
    with_database(transaction_id, database_schema, |db| db.select::<T>(query))
}

/// Executes a generic select query by table name, optionally within a transaction.
///
/// Unlike [`select`], this method does not require a concrete table type.
/// It takes a table name as a string and dispatches internally, returning
/// rows as column-value pairs.
pub fn select_raw<S>(
    table: &str,
    query: Query,
    transaction_id: Option<TransactionId>,
    database_schema: S,
) -> IcDbmsResult<Vec<Vec<(ColumnDef, Value)>>>
where
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    check_table_read_by_name(table)?;
    assert_caller_owns_transaction(transaction_id.as_ref());
    with_database(transaction_id, database_schema, |db| {
        db.select_raw(table, query)
    })
}

/// Executes a join query through the raw/untyped select path.
///
/// Returns rows with [`JoinColumnDef`] that include the source table name.
pub fn select_join<S>(
    table: &str,
    query: Query,
    transaction_id: Option<TransactionId>,
    database_schema: S,
) -> IcDbmsResult<Vec<Vec<(JoinColumnDef, Value)>>>
where
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    check_join_read_perms(table, &query)?;
    assert_caller_owns_transaction(transaction_id.as_ref());
    with_database(transaction_id, database_schema, |db| {
        db.select_join(table, query)
    })
}

/// Executes an aggregate query against the database schema, optionally within
/// a transaction.
pub fn aggregate<T, S>(
    query: Query,
    aggregates: Vec<AggregateFunction>,
    transaction_id: Option<TransactionId>,
    database_schema: S,
) -> IcDbmsResult<Vec<AggregatedRow>>
where
    T: TableSchema,
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    check_table_perm(T::fingerprint(), TablePerms::READ)?;
    assert_caller_owns_transaction(transaction_id.as_ref());
    with_database(transaction_id, database_schema, |db| {
        db.aggregate::<T>(query, &aggregates)
    })
}

/// Executes an insert query against the database schema, optionally within a transaction.
pub fn insert<T, S>(
    record: T::Insert,
    transaction_id: Option<TransactionId>,
    database_schema: S,
) -> IcDbmsResult<()>
where
    T: TableSchema,
    T::Insert: InsertRecord<Schema = T>,
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    check_table_perm(T::fingerprint(), TablePerms::INSERT)?;
    assert_caller_owns_transaction(transaction_id.as_ref());
    with_database(transaction_id, database_schema, |db| db.insert::<T>(record))
}

/// Executes an update query against the database schema, optionally within a transaction.
pub fn update<T, S>(
    patch: T::Update,
    transaction_id: Option<TransactionId>,
    database_schema: S,
) -> IcDbmsResult<u64>
where
    T: TableSchema,
    T::Update: UpdateRecord<Schema = T>,
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    check_table_perm(T::fingerprint(), TablePerms::UPDATE)?;
    assert_caller_owns_transaction(transaction_id.as_ref());
    with_database(transaction_id, database_schema, |db| db.update::<T>(patch))
}

/// Executes a delete query against the database schema, optionally within a transaction.
pub fn delete<T, S>(
    behaviour: DeleteBehavior,
    filter: Option<Filter>,
    transaction_id: Option<TransactionId>,
    database_schema: S,
) -> IcDbmsResult<u64>
where
    T: TableSchema,
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    check_table_perm(T::fingerprint(), TablePerms::DELETE)?;
    assert_caller_owns_transaction(transaction_id.as_ref());
    with_database(transaction_id, database_schema, |db| {
        db.delete::<T>(behaviour, filter)
    })
}

// --- Migration -------------------------------------------------------------

/// Returns `true` if the persisted schema differs from the compiled one.
/// Caller must hold the `migrate` flag.
pub fn has_drift<S>(database_schema: S) -> IcDbmsResult<bool>
where
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    check_migrate()?;
    with_database(None, database_schema, |db| db.has_drift())
}

/// Returns the migration ops needed to bring the persisted schema in line
/// with the compiled one, without applying anything.
pub fn pending_migrations<S>(database_schema: S) -> IcDbmsResult<Vec<MigrationOp>>
where
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    check_migrate()?;
    with_database(None, database_schema, |db| db.pending_migrations())
}

/// Applies a planned migration under `policy`. Transactional: on failure the
/// stored schema and data are unchanged.
pub fn migrate<S>(policy: MigrationPolicy, database_schema: S) -> IcDbmsResult<()>
where
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    check_migrate()?;
    DBMS_CONTEXT.with(|ctx| {
        let mut db = WasmDbmsDatabase::oneshot(ctx, database_schema);
        db.migrate(policy)
    })
}

// --- Helpers ---------------------------------------------------------------

fn check_table_perm(table: TableFingerprint, required: TablePerms) -> IcDbmsResult<()> {
    let caller = crate::utils::caller();
    DBMS_CONTEXT.with(|ctx| {
        if ctx.granted(&caller, table, required) {
            Ok(())
        } else {
            Err(DbmsError::AccessDenied {
                table: Some(table),
                required: RequiredPerm::Table(required),
            })
        }
    })
}

fn check_table_read_by_name(table: &str) -> IcDbmsResult<()> {
    let fingerprint = resolve_table_fingerprint(table)?;
    check_table_perm(fingerprint, TablePerms::READ)
}

fn check_join_read_perms(root_table: &str, query: &Query) -> IcDbmsResult<()> {
    let mut checked = HashSet::new();
    for table in
        std::iter::once(root_table).chain(query.joins.iter().map(|join| join.table.as_str()))
    {
        let fingerprint = resolve_table_fingerprint(table)?;
        if checked.insert(fingerprint) {
            check_table_perm(fingerprint, TablePerms::READ)?;
        }
    }
    Ok(())
}

fn resolve_table_fingerprint(table: &str) -> IcDbmsResult<TableFingerprint> {
    DBMS_CONTEXT.with(|ctx| {
        if ctx.has_table(table) {
            Ok(fingerprint_for_name(table))
        } else {
            Err(DbmsError::Query(QueryError::TableNotFound(
                table.to_string(),
            )))
        }
    })
}

fn check_migrate() -> IcDbmsResult<()> {
    let caller = crate::utils::caller();
    DBMS_CONTEXT.with(|ctx| {
        if ctx.granted_migrate(&caller) {
            Ok(())
        } else {
            Err(DbmsError::AccessDenied {
                table: None,
                required: RequiredPerm::Migrate,
            })
        }
    })
}

/// Closure-based helper that creates a [`WasmDbmsDatabase`] inside the
/// `DBMS_CONTEXT` thread-local and invokes `f` on it.
fn with_database<S, F, R>(transaction_id: Option<TransactionId>, database_schema: S, f: F) -> R
where
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
    F: for<'a> FnOnce(&WasmDbmsDatabase<'a, IcMemoryProvider, IcAccessControlList>) -> R,
{
    DBMS_CONTEXT.with(|ctx| {
        let db = match transaction_id {
            Some(tx_id) => WasmDbmsDatabase::from_transaction(ctx, database_schema, tx_id),
            None => WasmDbmsDatabase::oneshot(ctx, database_schema),
        };
        f(&db)
    })
}

/// Asserts that the caller owns the given transaction ID. Traps on
/// mismatch.
fn assert_caller_owns_transaction(transaction_id: Option<&TransactionId>) {
    let Some(tx_id) = transaction_id else {
        return;
    };
    let caller = crate::utils::caller();
    if !DBMS_CONTEXT.with(|ctx| ctx.has_transaction(tx_id, caller.as_slice())) {
        trap!("Caller {caller} does not own transaction {tx_id}");
    }
}

#[cfg(test)]
mod tests {

    use ic_dbms_api::prelude::Uint32;

    use super::*;
    use crate::tests::{UserInsertRequest, load_fixtures};

    fn alice() -> Principal {
        crate::utils::caller()
    }

    fn init_acl() {
        DBMS_CONTEXT.with(|ctx| {
            crate::tests::TestDatabaseSchema::register_tables(ctx)
                .expect("failed to register tables");
            ctx.acl_grant(alice(), PermGrant::Admin).unwrap();
            ctx.acl_grant(alice(), PermGrant::ManageAcl).unwrap();
            ctx.acl_grant(alice(), PermGrant::Migrate).unwrap();
            ctx.acl_grant(alice(), PermGrant::AllTables(TablePerms::all()))
                .unwrap();
        });
    }

    #[test]
    fn test_should_grant_admin() {
        init_acl();
        let bob = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
        grant_admin(bob).expect("failed to grant admin");
        DBMS_CONTEXT.with(|ctx| assert!(ctx.granted_admin(&bob)));
    }

    #[test]
    fn test_should_revoke_admin() {
        init_acl();
        let bob = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
        grant_admin(bob).unwrap();
        revoke_admin(bob).unwrap();
        DBMS_CONTEXT.with(|ctx| assert!(!ctx.granted_admin(&bob)));
    }

    #[test]
    fn test_should_list_identities() {
        init_acl();
        let identities = list_identities().expect("failed to list identities");
        assert!(identities.iter().any(|(p, _)| *p == alice()));
    }

    #[test]
    fn test_should_my_perms_returns_callers() {
        init_acl();
        let perms = my_perms();
        assert!(perms.admin && perms.manage_acl && perms.migrate);
    }

    #[test]
    fn test_should_reject_unknown_table_acl_grant() {
        init_acl();
        let bob = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
        let err = grant_table_perms(bob, "missing".to_string(), TablePerms::READ)
            .expect_err("unknown table must be rejected");
        assert!(matches!(
            err,
            DbmsError::Query(QueryError::TableNotFound(table)) if table == "missing"
        ));
    }

    #[test]
    fn test_should_deny_join_without_joined_table_read_perm() {
        init_acl();
        let query = Query::builder()
            .all()
            .inner_join("posts", "users.id", "posts.user")
            .build();
        let res = DBMS_CONTEXT.with(|ctx| {
            ctx.acl_revoke(&alice(), PermRevoke::Admin)
                .expect("should revoke bootstrap admin bypass");
            ctx.acl_revoke(&alice(), PermRevoke::AllTables(TablePerms::all()))
                .expect("should revoke bootstrap table perms");
            ctx.acl_grant(
                alice(),
                PermGrant::Table(crate::tests::User::fingerprint(), TablePerms::READ),
            )
            .expect("should grant root-table read");
            select_join("users", query, None, crate::tests::TestDatabaseSchema)
        });
        assert!(matches!(
            res,
            Err(DbmsError::AccessDenied {
                required: RequiredPerm::Table(perms),
                table: Some(table),
            }) if perms == TablePerms::READ && table == crate::tests::Post::fingerprint()
        ));
    }

    #[test]
    fn test_should_begin_transaction() {
        init_acl();
        let _tx_id = begin_transaction();
    }

    #[test]
    fn test_should_commit_transaction() {
        init_acl();
        let tx_id = begin_transaction();
        let res = commit(tx_id, crate::tests::TestDatabaseSchema);
        assert!(res.is_ok());
    }

    #[test]
    fn test_should_rollback_transaction() {
        init_acl();
        let tx_id = begin_transaction();
        let res = rollback(tx_id, crate::tests::TestDatabaseSchema);
        assert!(res.is_ok());
    }

    #[test]
    fn test_should_insert_record() {
        load_fixtures();
        init_acl();
        let record = UserInsertRequest {
            id: 100u32.into(),
            name: "Alice".to_string().into(),
            email: "alice@example.com".into(),
            age: 25u32.into(),
        };

        let res = insert::<crate::tests::User, _>(record, None, crate::tests::TestDatabaseSchema);
        assert!(res.is_ok());
    }

    #[test]
    fn test_should_select_record() {
        init_acl();
        load_fixtures();
        let query = Query::builder().all().limit(10).build();
        let res = select::<crate::tests::User, _>(query, None, crate::tests::TestDatabaseSchema);
        assert!(res.is_ok());
        let records = res.unwrap();
        assert!(!records.is_empty());
    }

    #[test]
    fn test_should_update_record() {
        init_acl();
        load_fixtures();

        let patch = crate::tests::UserUpdateRequest {
            id: None,
            name: Some("Robert".into()),
            email: Some("robert@example.com".into()),
            age: None,
            where_clause: Some(Filter::Eq("id".to_string(), Uint32::from(1u32).into())),
        };
        let res = update::<crate::tests::User, _>(patch, None, crate::tests::TestDatabaseSchema);
        assert!(res.is_ok());
    }

    #[test]
    fn test_should_delete_record() {
        init_acl();
        load_fixtures();

        let filter = Some(Filter::Eq("id".to_string(), Uint32::from(2u32).into()));
        let res = delete::<crate::tests::User, _>(
            DeleteBehavior::Cascade,
            filter,
            None,
            crate::tests::TestDatabaseSchema,
        );
        assert!(res.is_ok());
    }

    #[test]
    fn test_should_select_raw_record() {
        init_acl();
        load_fixtures();
        let query = Query::builder().all().limit(10).build();
        let res = select_raw("users", query, None, crate::tests::TestDatabaseSchema);
        assert!(res.is_ok());
        let rows = res.unwrap();
        assert!(!rows.is_empty());
        for row in &rows {
            assert!(row.iter().any(|(col, _)| col.name == "id"));
            assert!(row.iter().any(|(col, _)| col.name == "name"));
        }
    }

    #[test]
    fn test_should_fail_select_raw_unknown_table() {
        init_acl();
        load_fixtures();
        let query = Query::builder().all().build();
        let res = select_raw("nonexistent", query, None, crate::tests::TestDatabaseSchema);
        assert!(res.is_err());
    }

    #[test]
    #[should_panic = "Caller ghsi2-tqaaa-aaaan-aaaca-cai does not own transaction 0"]
    fn test_should_not_allow_operating_wrong_tx() {
        init_acl();
        load_fixtures();

        let bob = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
        let tx_id = DBMS_CONTEXT.with(|ctx| ctx.begin_transaction(bob.as_slice().to_vec()));

        // try to commit the transaction started by bob (we are alice)
        let _ = commit(tx_id, crate::tests::TestDatabaseSchema);
    }
}
