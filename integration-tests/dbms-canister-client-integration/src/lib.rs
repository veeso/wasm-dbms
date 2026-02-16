//! This is a canister used for integration tests of the [`IcDbmsCanisterClient`].
//! It exposes methods that wrap the client methods, so that they can be called from
//! the test environment.

use std::cell::RefCell;

use candid::{CandidType, Deserialize, Principal};
use ic_dbms_api::prelude::{
    DeleteBehavior, Filter, IcDbmsResult, Query, Table, Text, TransactionId, Uint32,
};
use ic_dbms_client::prelude::{Client as _, IcDbmsCanisterClient};

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
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
pub async fn acl_add_principal(principal: Principal) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .acl_add_principal(principal)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn acl_remove_principal(principal: Principal) -> Result<IcDbmsResult<()>, String> {
    let client = new_client();
    client
        .acl_remove_principal(principal)
        .await
        .map_err(|e| e.to_string())
}

#[ic_cdk::update]
pub async fn acl_allowed_principals() -> Result<Vec<Principal>, String> {
    let client = new_client();
    client
        .acl_allowed_principals()
        .await
        .map_err(|e| e.to_string())
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

#[inline]
fn new_client() -> IcDbmsCanisterClient {
    let canister_id = IC_DBMS_CANISTER.with_borrow(|c| *c);
    IcDbmsCanisterClient::new(canister_id)
}

ic_cdk::export_candid!();
