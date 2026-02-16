use std::time::Duration;

use candid::utils::ArgumentEncoder;
use candid::{CandidType, Principal};
use ic_dbms_api::prelude::IcDbmsResult;

use crate::client::Client;
use crate::prelude::IcDbmsCanisterClientResult;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// Client to interact with an IC DBMS Canister.
#[derive(Clone, Debug)]
pub struct IcDbmsCanisterClient {
    canister_id: Principal,
    timeout: Duration,
}

impl From<Principal> for IcDbmsCanisterClient {
    fn from(principal: Principal) -> Self {
        Self {
            canister_id: principal,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl IcDbmsCanisterClient {
    /// Creates a new IC DBMS Canister client from the given canister id as [`Principal`].
    pub fn new(canister_id: Principal) -> Self {
        Self::from(canister_id)
    }

    /// Sets the timeout duration for requests made by this client.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Calls a method on the IC DBMS Canister with the provided arguments and returns the result.
    async fn call<A, R>(&self, method: &str, args: &A) -> IcDbmsCanisterClientResult<R>
    where
        A: ArgumentEncoder,
        R: CandidType + for<'de> candid::Deserialize<'de>,
    {
        let response = ic_cdk::call::Call::bounded_wait(self.canister_id, method)
            .with_args(args)
            .change_timeout(self.timeout.as_secs() as u32)
            .into_future()
            .await?;

        let response: R = response.candid()?;
        Ok(response)
    }
}

impl Client for IcDbmsCanisterClient {
    fn principal(&self) -> Principal {
        self.canister_id
    }

    async fn acl_add_principal(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("acl_add_principal", &(principal,)).await
    }

    async fn acl_remove_principal(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("acl_remove_principal", &(principal,)).await
    }

    async fn acl_allowed_principals(&self) -> IcDbmsCanisterClientResult<Vec<Principal>> {
        self.call("acl_allowed_principals", &()).await
    }

    async fn begin_transaction(
        &self,
    ) -> IcDbmsCanisterClientResult<ic_dbms_api::prelude::TransactionId> {
        self.call("begin_transaction", &()).await
    }

    async fn commit(
        &self,
        transaction_id: ic_dbms_api::prelude::TransactionId,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("commit", &(transaction_id,)).await
    }

    async fn rollback(
        &self,
        transaction_id: ic_dbms_api::prelude::TransactionId,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("rollback", &(transaction_id,)).await
    }

    async fn select<T>(
        &self,
        table: &str,
        query: ic_dbms_api::prelude::Query,
        transaction_id: Option<ic_dbms_api::prelude::TransactionId>,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<Vec<T::Record>>>
    where
        T: ic_dbms_api::prelude::TableSchema,
    {
        self.call(
            &crate::utils::table_method(table, "select"),
            &(query, transaction_id),
        )
        .await
    }

    async fn insert<T>(
        &self,
        table: &str,
        record: T::Insert,
        transaction_id: Option<ic_dbms_api::prelude::TransactionId>,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>>
    where
        T: ic_dbms_api::prelude::TableSchema,
        T::Insert: ic_dbms_api::prelude::InsertRecord<Schema = T>,
    {
        self.call(
            &crate::utils::table_method(table, "insert"),
            &(record, transaction_id),
        )
        .await
    }

    async fn update<T>(
        &self,
        table: &str,
        patch: T::Update,
        transaction_id: Option<ic_dbms_api::prelude::TransactionId>,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<u64>>
    where
        T: ic_dbms_api::prelude::TableSchema,
        T::Update: ic_dbms_api::prelude::UpdateRecord<Schema = T>,
    {
        self.call(
            &crate::utils::table_method(table, "update"),
            &(patch, transaction_id),
        )
        .await
    }

    async fn delete<T>(
        &self,
        table: &str,
        behaviour: ic_dbms_api::prelude::DeleteBehavior,
        filter: Option<ic_dbms_api::prelude::Filter>,
        transaction_id: Option<ic_dbms_api::prelude::TransactionId>,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<u64>>
    where
        T: ic_dbms_api::prelude::TableSchema,
    {
        self.call(
            &crate::utils::table_method(table, "delete"),
            &(behaviour, filter, transaction_id),
        )
        .await
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_should_get_principal() {
        let principal = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let client = IcDbmsCanisterClient::new(principal);
        assert_eq!(client.principal(), principal);
    }

    #[test]
    fn test_should_set_timeout() {
        let principal = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let client = IcDbmsCanisterClient::new(principal).with_timeout(Duration::from_secs(600));
        assert_eq!(client.timeout, Duration::from_secs(600));
    }

    #[test]
    fn test_from_principal() {
        let principal = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let client: IcDbmsCanisterClient = principal.into();
        assert_eq!(client.principal(), principal);
    }
}
