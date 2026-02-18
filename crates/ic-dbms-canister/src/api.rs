//! API generic interface to be used by different DBMS canisters.

mod inspect;

use candid::Principal;
use ic_dbms_api::prelude::{
    CandidColumnDef, ColumnDef, Database as _, DeleteBehavior, Filter, IcDbmsError, IcDbmsResult,
    InsertRecord, Query, TableSchema, TransactionId, UpdateRecord, Value,
};

pub use self::inspect::inspect;
use crate::dbms::IcDbmsDatabase;
use crate::memory::ACL;
use crate::prelude::{DatabaseSchema, TRANSACTION_SESSION};
use crate::trap;

/// Adds the given principal to the ACL of the canister.
pub fn acl_add_principal(principal: Principal) -> IcDbmsResult<()> {
    assert_caller_is_allowed();
    ACL.with_borrow_mut(|acl| acl.add_principal(principal))
        .map_err(IcDbmsError::from)
}

/// Removes the given principal from the ACL of the canister.
pub fn acl_remove_principal(principal: Principal) -> IcDbmsResult<()> {
    assert_caller_is_allowed();
    ACL.with_borrow_mut(|acl| acl.remove_principal(&principal))
        .map_err(IcDbmsError::from)
}

/// Lists all principals in the ACL of the canister.
pub fn acl_allowed_principals() -> Vec<Principal> {
    assert_caller_is_allowed();
    ACL.with_borrow(|acl| acl.allowed_principals().to_vec())
}

/// Begins a new transaction and returns its ID.
pub fn begin_transaction() -> TransactionId {
    assert_caller_is_allowed();
    let owner = crate::utils::caller();
    TRANSACTION_SESSION.with_borrow_mut(|ts| ts.begin_transaction(owner))
}

/// Commits the transaction with the given ID.
pub fn commit(
    transaction_id: TransactionId,
    database_schema: impl DatabaseSchema + 'static,
) -> IcDbmsResult<()> {
    assert_caller_is_allowed();
    assert_caller_owns_transaction(Some(&transaction_id));
    let mut database = IcDbmsDatabase::from_transaction(database_schema, transaction_id);
    database.commit()
}

/// Rolls back the transaction with the given ID.
pub fn rollback(
    transaction_id: TransactionId,
    database_schema: impl DatabaseSchema + 'static,
) -> IcDbmsResult<()> {
    assert_caller_is_allowed();
    assert_caller_owns_transaction(Some(&transaction_id));
    let mut database = IcDbmsDatabase::from_transaction(database_schema, transaction_id);
    database.rollback()
}

/// Executes a select query against the database schema, optionally within a transaction.
pub fn select<T>(
    query: Query,
    transaction_id: Option<TransactionId>,
    database_schema: impl DatabaseSchema + 'static,
) -> IcDbmsResult<Vec<T::Record>>
where
    T: TableSchema,
{
    assert_caller_is_allowed();
    assert_caller_owns_transaction(transaction_id.as_ref());
    let database = database(transaction_id, database_schema);
    database.select::<T>(query)
}

/// Executes a generic select query by table name, optionally within a transaction.
///
/// Unlike [`select`], this method does not require a concrete table type.
/// It takes a table name as a string and dispatches internally, returning
/// rows as column-value pairs.
///
/// # Errors
///
/// Returns an error if the table does not exist or the query is invalid.
pub fn select_raw(
    table: &str,
    query: Query,
    transaction_id: Option<TransactionId>,
    database_schema: impl DatabaseSchema + 'static,
) -> IcDbmsResult<Vec<Vec<(ColumnDef, Value)>>> {
    assert_caller_is_allowed();
    assert_caller_owns_transaction(transaction_id.as_ref());
    let database = database(transaction_id, database_schema);
    database.select_raw(table, query)
}

/// Executes a join query through the raw/untyped select path.
///
/// Returns rows with [`CandidColumnDef`] that include the source table name.
pub fn select_join(
    table: &str,
    query: Query,
    transaction_id: Option<TransactionId>,
    database_schema: impl DatabaseSchema + 'static,
) -> IcDbmsResult<Vec<Vec<(CandidColumnDef, Value)>>> {
    assert_caller_is_allowed();
    assert_caller_owns_transaction(transaction_id.as_ref());
    let db = database(transaction_id, database_schema);
    db.select_join(table, query)
}

/// Executes an insert query against the database schema, optionally within a transaction.
pub fn insert<T>(
    record: T::Insert,
    transaction_id: Option<TransactionId>,
    database_schema: impl DatabaseSchema + 'static,
) -> IcDbmsResult<()>
where
    T: TableSchema,
    T::Insert: InsertRecord<Schema = T>,
{
    assert_caller_is_allowed();
    assert_caller_owns_transaction(transaction_id.as_ref());
    let database = database(transaction_id, database_schema);
    database.insert::<T>(record)
}

/// Executes an update query against the database schema, optionally within a transaction.
pub fn update<T>(
    patch: T::Update,
    transaction_id: Option<TransactionId>,
    database_schema: impl DatabaseSchema + 'static,
) -> IcDbmsResult<u64>
where
    T: TableSchema,
    T::Update: UpdateRecord<Schema = T>,
{
    assert_caller_is_allowed();
    assert_caller_owns_transaction(transaction_id.as_ref());
    let database = database(transaction_id, database_schema);
    database.update::<T>(patch)
}

/// Executes a delete query against the database schema, optionally within a transaction.
pub fn delete<T>(
    behaviour: DeleteBehavior,
    filter: Option<Filter>,
    transaction_id: Option<TransactionId>,
    database_schema: impl DatabaseSchema + 'static,
) -> IcDbmsResult<u64>
where
    T: TableSchema,
{
    assert_caller_is_allowed();
    assert_caller_owns_transaction(transaction_id.as_ref());
    let database = database(transaction_id, database_schema);
    database.delete::<T>(behaviour, filter)
}

/// Helper function to get the database, either in a transaction or as a one-shot.
#[inline]
fn database(
    transaction_id: Option<TransactionId>,
    database_schema: impl DatabaseSchema + 'static,
) -> IcDbmsDatabase {
    match transaction_id {
        Some(tx_id) => IcDbmsDatabase::from_transaction(database_schema, tx_id),
        None => IcDbmsDatabase::oneshot(database_schema),
    }
}

/// Asserts that the caller is in the ACL of the canister.
///
/// If not it traps.
fn assert_caller_is_allowed() {
    let caller = crate::utils::caller();
    if !ACL.with_borrow(|acl| acl.is_allowed(&caller)) {
        trap!("Caller {caller} is not allowed to perform this operation");
    }
}

/// Asserts that the caller owns the given transaction ID.
fn assert_caller_owns_transaction(transaction_id: Option<&TransactionId>) {
    let Some(tx_id) = transaction_id else {
        return;
    };
    let caller = crate::utils::caller();
    TRANSACTION_SESSION.with_borrow(|ts| {
        if !ts.has_transaction(tx_id, caller) {
            trap!("Caller {caller} does not own transaction {tx_id}");
        }
    });
}

#[cfg(test)]
mod tests {

    use ic_dbms_api::prelude::Uint32;

    use super::*;
    use crate::tests::{UserInsertRequest, load_fixtures};

    #[test]
    fn test_should_insert_into_acl() {
        // init ACL
        init_acl();
        // try to add a new principal
        let bob = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
        assert!(acl_add_principal(bob).is_ok());
        // check if bob is in the ACL
        let allowed = acl_allowed_principals();
        assert!(allowed.contains(&bob));
        assert!(allowed.contains(&alice()));
    }

    #[test]
    fn test_should_remove_from_acl() {
        // init ACL
        init_acl();
        // add a new principal
        let bob = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
        assert!(acl_add_principal(bob).is_ok());
        // remove bob
        assert!(acl_remove_principal(bob).is_ok());
        // check if bob is not in the ACL
        let allowed = acl_allowed_principals();
        assert!(!allowed.contains(&bob));
        assert!(allowed.contains(&alice()));
    }

    #[test]
    fn test_should_list_acl_principals() {
        // init ACL
        init_acl();
        // list principals
        let allowed = acl_allowed_principals();
        // check if alice is in the ACL
        assert!(allowed.contains(&alice()));
    }

    #[test]
    fn test_should_begin_transaction() {
        // init ACL
        init_acl();
        // begin transaction
        let _tx_id = begin_transaction();
    }

    #[test]
    fn test_should_commit_transaction() {
        // init ACL
        init_acl();
        // begin transaction
        let tx_id = begin_transaction();
        // commit transaction
        let res = commit(tx_id, crate::tests::TestDatabaseSchema);
        assert!(res.is_ok());
    }

    #[test]
    fn test_should_rollback_transaction() {
        // init ACL
        init_acl();
        // begin transaction
        let tx_id = begin_transaction();
        // rollback transaction
        let res = rollback(tx_id, crate::tests::TestDatabaseSchema);
        assert!(res.is_ok());
    }

    #[test]
    fn test_should_insert_record() {
        load_fixtures();
        // init ACL
        init_acl();
        // insert record
        let record = UserInsertRequest {
            id: 100u32.into(),
            name: "Alice".to_string().into(),
            email: "alice@example.com".into(),
            age: 25u32.into(),
        };

        let res = insert::<crate::tests::User>(record, None, crate::tests::TestDatabaseSchema);
        assert!(res.is_ok());
    }

    #[test]
    fn test_should_select_record() {
        // init ACL
        init_acl();
        load_fixtures();
        // select record
        let query = Query::builder().all().limit(10).build();
        let res = select::<crate::tests::User>(query, None, crate::tests::TestDatabaseSchema);
        assert!(res.is_ok());
        let records = res.unwrap();
        assert!(!records.is_empty());
    }

    #[test]
    fn test_should_update_record() {
        // init ACL
        init_acl();
        load_fixtures();

        // update record
        let patch = crate::tests::UserUpdateRequest {
            id: None,
            name: Some("Robert".into()),
            email: Some("robert@example.com".into()),
            age: None,
            where_clause: Some(Filter::Eq("id".to_string(), Uint32::from(1u32).into())),
        };
        let res = update::<crate::tests::User>(patch, None, crate::tests::TestDatabaseSchema);
        assert!(res.is_ok());
    }

    #[test]
    fn test_should_delete_record() {
        // init ACL
        init_acl();
        load_fixtures();

        // delete record
        let filter = Some(Filter::Eq("id".to_string(), Uint32::from(2u32).into()));
        let res = delete::<crate::tests::User>(
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
        // init ACL
        init_acl();
        load_fixtures();

        let tx_id = TRANSACTION_SESSION.with_borrow_mut(|ts| {
            ts.begin_transaction(Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap())
        });

        // try to commit the transaction started by alice
        let _ = commit(tx_id, crate::tests::TestDatabaseSchema);
    }

    fn alice() -> Principal {
        crate::utils::caller()
    }

    fn init_acl() {
        ACL.with_borrow_mut(|acl| {
            acl.add_principal(alice()).unwrap();
        });
    }
}
