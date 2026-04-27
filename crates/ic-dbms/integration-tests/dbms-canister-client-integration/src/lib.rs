//! This is a canister used for integration tests of the [`IcDbmsCanisterClient`].
//! It exposes methods that wrap the client methods, so that they can be called from
//! the test environment.

use std::cell::RefCell;

use candid::{CandidType, Deserialize, Principal};
use ic_dbms_api::prelude::{
    AggregateFunction, AggregatedRow, DeleteBehavior, Filter, IcDbmsResult, IdentityPerms,
    JoinColumnDef, MigrationOp, MigrationPolicy, Query, Table, TablePerms, Text, TransactionId,
    Uint32, Value,
};
use ic_dbms_client::prelude::{Client as _, IcDbmsCanisterClient};

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    #[validate(ic_dbms_api::prelude::EmailValidator)]
    pub email: Text,
}

thread_local! {
    static IC_DBMS_CANISTER: RefCell<Principal> = const { RefCell::new(Principal::anonymous()) };
}

#[ic_cdk::init]
pub fn init(ic_dbms_canister: Principal) {
    IC_DBMS_CANISTER.with_borrow_mut(|c| {
        *c = ic_dbms_canister;
    });
}

#[ic_cdk::update]
pub async fn grant_admin(principal: Principal) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .grant_admin(principal)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn revoke_admin(principal: Principal) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .revoke_admin(principal)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn grant_manage_acl(principal: Principal) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .grant_manage_acl(principal)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn revoke_manage_acl(principal: Principal) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .revoke_manage_acl(principal)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn grant_migrate(principal: Principal) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .grant_migrate(principal)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn revoke_migrate(principal: Principal) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .revoke_migrate(principal)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn grant_all_tables_perms(
    principal: Principal,
    perms: TablePerms,
) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .grant_all_tables_perms(principal, perms)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn revoke_all_tables_perms(
    principal: Principal,
    perms: TablePerms,
) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .revoke_all_tables_perms(principal, perms)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn grant_table_perms(
    principal: Principal,
    table: String,
    perms: TablePerms,
) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .grant_table_perms(principal, &table, perms)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn revoke_table_perms(
    principal: Principal,
    table: String,
    perms: TablePerms,
) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .revoke_table_perms(principal, &table, perms)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn remove_identity(principal: Principal) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .remove_identity(principal)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn list_identities() -> Result<IcDbmsResult<Vec<(Principal, IdentityPerms)>>, String> {
    let client = new_client();
    client.list_identities().await.map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn my_perms() -> Result<IdentityPerms, String> {
    let client = new_client();
    client.my_perms().await.map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn begin_transaction() -> Result<ic_dbms_api::prelude::TransactionId, String> {
    let client = new_client();
    client.begin_transaction().await.map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn commit(
    transaction_id: ic_dbms_api::prelude::TransactionId,
) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .commit(transaction_id)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn rollback(
    transaction_id: ic_dbms_api::prelude::TransactionId,
) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .rollback(transaction_id)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn select(
    query: Query,
    transaction_id: Option<TransactionId>,
) -> Result<IcDbmsResult<Vec<UserRecord>>, String> {
    let client = new_client();
    client
        .select::<User>("users", query, transaction_id)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn select_raw(
    query: Query,
    transaction_id: Option<TransactionId>,
) -> Result<IcDbmsResult<Vec<Vec<(JoinColumnDef, Value)>>>, String> {
    let client = new_client();
    client
        .select_raw("users", query, transaction_id)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn aggregate(
    query: Query,
    aggregates: Vec<AggregateFunction>,
    transaction_id: Option<TransactionId>,
) -> Result<IcDbmsResult<Vec<AggregatedRow>>, String> {
    let client = new_client();
    client
        .aggregate::<User>("users", query, aggregates, transaction_id)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn insert(
    record: UserInsertRequest,
    transaction_id: Option<TransactionId>,
) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .insert::<User>("users", record, transaction_id)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn update(
    patch: UserUpdateRequest,
    transaction_id: Option<TransactionId>,
) -> Result<IcDbmsResult<u64>, String> {
    let client = new_client();
    client
        .update::<User>("users", patch, transaction_id)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn delete(
    behaviour: DeleteBehavior,
    filter: Option<Filter>,
    transaction_id: Option<TransactionId>,
) -> Result<IcDbmsResult<u64>, String> {
    let client = new_client();
    client
        .delete::<User>("users", behaviour, filter, transaction_id)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn has_drift() -> Result<IcDbmsResult<bool>, String> {
    let client = new_client();
    client.has_drift().await.map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn pending_migrations() -> Result<IcDbmsResult<Vec<MigrationOp>>, String> {
    let client = new_client();
    client.pending_migrations().await.map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn migrate(policy: MigrationPolicy) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client.migrate(policy).await.map_err(|e| e.to_string())
}

#[inline]
fn new_client() -> IcDbmsCanisterClient {
    let canister_id = IC_DBMS_CANISTER.with_borrow(|c| *c);
    IcDbmsCanisterClient::new(canister_id)
}

ic_cdk::export_candid!();
