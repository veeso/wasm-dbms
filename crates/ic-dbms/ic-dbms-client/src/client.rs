#[cfg(feature = "ic-agent")]
#[cfg_attr(docsrs, doc(cfg(feature = "ic-agent")))]
mod agent;
mod ic;
#[cfg(feature = "pocket-ic")]
#[cfg_attr(docsrs, doc(cfg(feature = "pocket-ic")))]
mod pocket_ic;
mod types;

use candid::{CandidType, Principal};
use ic_dbms_api::prelude::{
    AggregateFunction, AggregatedRow, DeleteBehavior, Filter, IcDbmsResult, InsertRecord,
    JoinColumnDef, MigrationOp, MigrationPolicy, Query, TableSchema, TransactionId, UpdateRecord,
    Value,
};

#[cfg(feature = "ic-agent")]
#[cfg_attr(docsrs, doc(cfg(feature = "ic-agent")))]
pub use self::agent::IcDbmsAgentClient;
pub use self::ic::IcDbmsCanisterClient;
#[cfg(feature = "pocket-ic")]
#[cfg_attr(docsrs, doc(cfg(feature = "pocket-ic")))]
pub use self::pocket_ic::IcDbmsPocketIcClient;
use crate::prelude::IcDbmsCanisterClientResult;

type RawRecords = Vec<Vec<(JoinColumnDef, Value)>>;

/// Trait for implementing a ic-dbms-client.
///
/// This is used so the library can expose also clients for pocket-ic.
///
/// If you're looking for the IC DBMS Canister client, see [`IcDbmsCanisterClient`].
pub trait Client {
    /// Returns the [`Principal`] of the IC DBMS Canister.
    fn principal(&self) -> Principal;

    /// Adds the given principal to the ACL of the canister.
    fn acl_add_principal(
        &self,
        principal: Principal,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<()>>>;

    /// Removes the given principal from the ACL of the canister.
    fn acl_remove_principal(
        &self,
        principal: Principal,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<()>>>;

    /// Lists all principals in the ACL of the canister.
    fn acl_allowed_principals(
        &self,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<Vec<Principal>>>;

    /// Begins a new transaction and returns its ID.
    fn begin_transaction(&self) -> impl Future<Output = IcDbmsCanisterClientResult<TransactionId>>;

    /// Commits the transaction with the given ID.
    fn commit(
        &self,
        transaction_id: TransactionId,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<()>>>;

    /// Rolls back the transaction with the given ID.
    fn rollback(
        &self,
        transaction_id: TransactionId,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<()>>>;

    /// Executes a `SELECT` query on the IC DBMS Canister.
    fn select<T>(
        &self,
        table: &str,
        query: Query,
        transaction_id: Option<TransactionId>,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<Vec<T::Record>>>>
    where
        T: TableSchema,
        T::Record: CandidType + for<'de> candid::Deserialize<'de>;

    /// Executes an aggregate query on the IC DBMS Canister.
    ///
    /// The `query` carries `WHERE`, `DISTINCT`, `GROUP BY`, `HAVING`,
    /// `ORDER BY`, `OFFSET`, and `LIMIT` clauses; `aggregates` lists the
    /// [`AggregateFunction`]s to compute per group.
    fn aggregate<T>(
        &self,
        table: &str,
        query: Query,
        aggregates: Vec<AggregateFunction>,
        transaction_id: Option<TransactionId>,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<Vec<AggregatedRow>>>>
    where
        T: TableSchema;

    /// Executes a `SELECT` query on the IC DBMS Canister and returns raw records (without deserialization).
    fn select_raw(
        &self,
        table: &str,
        query: Query,
        transaction_id: Option<TransactionId>,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<RawRecords>>>;

    /// Executes an `INSERT` query on the IC DBMS Canister.
    fn insert<T>(
        &self,
        table: &str,
        record: T::Insert,
        transaction_id: Option<TransactionId>,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<()>>>
    where
        T: TableSchema,
        T::Insert: InsertRecord<Schema = T> + CandidType;

    /// Executes an `UPDATE` query on the IC DBMS Canister.
    fn update<T>(
        &self,
        table: &str,
        patch: T::Update,
        transaction_id: Option<TransactionId>,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<u64>>>
    where
        T: TableSchema,
        T::Update: UpdateRecord<Schema = T> + CandidType;

    /// Executes a `DELETE` query on the IC DBMS Canister.
    fn delete<T>(
        &self,
        table: &str,
        behaviour: DeleteBehavior,
        filter: Option<Filter>,
        transaction_id: Option<TransactionId>,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<u64>>>
    where
        T: TableSchema;

    /// Returns `true` when the canister's persisted schema differs from the
    /// schema compiled into its binary.
    fn has_drift(&self) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<bool>>>;

    /// Returns the migration ops needed to bring the persisted schema in line
    /// with the compiled one, without applying anything.
    fn pending_migrations(
        &self,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<Vec<MigrationOp>>>>;

    /// Applies a planned migration under `policy`.
    fn migrate(
        &self,
        policy: MigrationPolicy,
    ) -> impl Future<Output = IcDbmsCanisterClientResult<IcDbmsResult<()>>>;
}
