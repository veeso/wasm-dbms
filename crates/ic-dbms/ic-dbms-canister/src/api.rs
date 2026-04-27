// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! API generic interface to be used by different DBMS canisters.

mod inspect;

use candid::Principal;
use ic_dbms_api::prelude::{
    AggregateFunction, AggregatedRow, ColumnDef, Database, DeleteBehavior, Filter, IcDbmsResult,
    InsertRecord, JoinColumnDef, Query, TableSchema, TransactionId, UpdateRecord, Value,
};
use wasm_dbms::prelude::{DatabaseSchema, WasmDbmsDatabase};

pub use self::inspect::inspect;
use crate::memory::{DBMS_CONTEXT, IcAccessControlList, IcMemoryProvider};
use crate::trap;

/// Adds the given principal to the ACL of the canister.
pub fn acl_add_principal(principal: Principal) -> IcDbmsResult<()> {
    assert_caller_is_allowed();
    DBMS_CONTEXT.with(|ctx| ctx.acl_add(principal))
}

/// Removes the given principal from the ACL of the canister.
pub fn acl_remove_principal(principal: Principal) -> IcDbmsResult<()> {
    assert_caller_is_allowed();
    DBMS_CONTEXT.with(|ctx| ctx.acl_remove(&principal))
}

/// Lists all principals in the ACL of the canister.
pub fn acl_allowed_principals() -> Vec<Principal> {
    assert_caller_is_allowed();
    DBMS_CONTEXT.with(|ctx| ctx.acl_allowed())
}

/// Begins a new transaction and returns its ID.
pub fn begin_transaction() -> TransactionId {
    assert_caller_is_allowed();
    let owner = crate::utils::caller();
    DBMS_CONTEXT.with(|ctx| ctx.begin_transaction(owner.as_slice().to_vec()))
}

/// Commits the transaction with the given ID.
pub fn commit<S>(transaction_id: TransactionId, database_schema: S) -> IcDbmsResult<()>
where
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    assert_caller_is_allowed();
    assert_caller_owns_transaction(Some(&transaction_id));
    DBMS_CONTEXT.with(|ctx| {
        let mut db = WasmDbmsDatabase::from_transaction(ctx, database_schema, transaction_id);
        db.commit()
    })
}

/// Rolls back the transaction with the given ID.
pub fn rollback<S>(transaction_id: TransactionId, database_schema: S) -> IcDbmsResult<()>
where
    S: DatabaseSchema<IcMemoryProvider, IcAccessControlList> + 'static,
{
    assert_caller_is_allowed();
    assert_caller_owns_transaction(Some(&transaction_id));
    DBMS_CONTEXT.with(|ctx| {
        let mut db = WasmDbmsDatabase::from_transaction(ctx, database_schema, transaction_id);
        db.rollback()
    })
}

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
    assert_caller_is_allowed();
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
    assert_caller_is_allowed();
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
    assert_caller_is_allowed();
    assert_caller_owns_transaction(transaction_id.as_ref());
    with_database(transaction_id, database_schema, |db| {
        db.select_join(table, query)
    })
}

/// Executes an aggregate query against the database schema, optionally within
/// a transaction.
///
/// See [`Database::aggregate`] for the pipeline (`WHERE` -> `DISTINCT` ->
/// `GROUP BY` -> aggregate computation -> `HAVING` -> `ORDER BY` ->
/// `OFFSET`/`LIMIT`).
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
    assert_caller_is_allowed();
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
    assert_caller_is_allowed();
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
    assert_caller_is_allowed();
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
    assert_caller_is_allowed();
    assert_caller_owns_transaction(transaction_id.as_ref());
    with_database(transaction_id, database_schema, |db| {
        db.delete::<T>(behaviour, filter)
    })
}

/// Closure-based helper that creates a [`WasmDbmsDatabase`] inside the
/// `DBMS_CONTEXT` thread-local and invokes `f` on it.
///
/// Because [`WasmDbmsDatabase`] borrows [`DbmsContext`], the database
/// cannot outlive the `with` closure.
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

/// Asserts that the caller is in the ACL of the canister.
///
/// Traps if the caller is not allowed.
fn assert_caller_is_allowed() {
    let caller = crate::utils::caller();
    if !DBMS_CONTEXT.with(|ctx| ctx.acl_is_allowed(&caller)) {
        trap!("Caller {caller} is not allowed to perform this operation");
    }
}

/// Asserts that the caller owns the given transaction ID.
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

    #[test]
    fn test_should_insert_into_acl() {
        init_acl();
        let bob = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
        assert!(acl_add_principal(bob).is_ok());
        let allowed = acl_allowed_principals();
        assert!(allowed.contains(&bob));
        assert!(allowed.contains(&alice()));
    }

    #[test]
    fn test_should_remove_from_acl() {
        init_acl();
        let bob = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
        assert!(acl_add_principal(bob).is_ok());
        assert!(acl_remove_principal(bob).is_ok());
        let allowed = acl_allowed_principals();
        assert!(!allowed.contains(&bob));
        assert!(allowed.contains(&alice()));
    }

    #[test]
    fn test_should_list_acl_principals() {
        init_acl();
        let allowed = acl_allowed_principals();
        assert!(allowed.contains(&alice()));
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

    fn alice() -> Principal {
        crate::utils::caller()
    }

    fn init_acl() {
        DBMS_CONTEXT.with(|ctx| {
            crate::tests::TestDatabaseSchema::register_tables(ctx)
                .expect("failed to register tables");
            ctx.acl_add(alice()).unwrap();
        });
    }
}
