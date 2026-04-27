use std::time::Duration;

use candid::utils::ArgumentEncoder;
use candid::{CandidType, Principal};
use ic_dbms_api::prelude::{IcDbmsResult, IdentityPerms, TablePerms};

use crate::client::{Client, RawRecords};
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

    async fn grant_admin(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("grant_admin", &(principal,)).await
    }

    async fn revoke_admin(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("revoke_admin", &(principal,)).await
    }

    async fn grant_manage_acl(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("grant_manage_acl", &(principal,)).await
    }

    async fn revoke_manage_acl(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("revoke_manage_acl", &(principal,)).await
    }

    async fn grant_migrate(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("grant_migrate", &(principal,)).await
    }

    async fn revoke_migrate(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("revoke_migrate", &(principal,)).await
    }

    async fn grant_all_tables_perms(
        &self,
        principal: Principal,
        perms: TablePerms,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("grant_all_tables_perms", &(principal, perms))
            .await
    }

    async fn revoke_all_tables_perms(
        &self,
        principal: Principal,
        perms: TablePerms,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("revoke_all_tables_perms", &(principal, perms))
            .await
    }

    async fn grant_table_perms(
        &self,
        principal: Principal,
        table: &str,
        perms: TablePerms,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("grant_table_perms", &(principal, table.to_string(), perms))
            .await
    }

    async fn revoke_table_perms(
        &self,
        principal: Principal,
        table: &str,
        perms: TablePerms,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("revoke_table_perms", &(principal, table.to_string(), perms))
            .await
    }

    async fn remove_identity(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("remove_identity", &(principal,)).await
    }

    async fn list_identities(
        &self,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<Vec<(Principal, IdentityPerms)>>> {
        self.call("list_identities", &()).await
    }

    async fn my_perms(&self) -> IcDbmsCanisterClientResult<IdentityPerms> {
        self.call("my_perms", &()).await
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
        T::Record: CandidType + for<'de> candid::Deserialize<'de>,
    {
        self.call(
            &crate::utils::table_method(table, "select"),
            &(query, transaction_id),
        )
        .await
    }

    async fn select_raw(
        &self,
        table: &str,
        query: ic_dbms_api::prelude::Query,
        transaction_id: Option<ic_dbms_api::prelude::TransactionId>,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<RawRecords>> {
        self.call("select", &(table, query, transaction_id)).await
    }

    async fn aggregate<T>(
        &self,
        table: &str,
        query: ic_dbms_api::prelude::Query,
        aggregates: Vec<ic_dbms_api::prelude::AggregateFunction>,
        transaction_id: Option<ic_dbms_api::prelude::TransactionId>,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<Vec<ic_dbms_api::prelude::AggregatedRow>>>
    where
        T: ic_dbms_api::prelude::TableSchema,
    {
        self.call(
            &crate::utils::table_method(table, "aggregate"),
            &(query, aggregates, transaction_id),
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
        T::Insert: ic_dbms_api::prelude::InsertRecord<Schema = T> + CandidType,
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
        T::Update: ic_dbms_api::prelude::UpdateRecord<Schema = T> + CandidType,
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

    async fn has_drift(&self) -> IcDbmsCanisterClientResult<IcDbmsResult<bool>> {
        self.call("has_drift", &()).await
    }

    async fn pending_migrations(
        &self,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<Vec<ic_dbms_api::prelude::MigrationOp>>> {
        self.call("pending_migrations", &()).await
    }

    async fn migrate(
        &self,
        policy: ic_dbms_api::prelude::MigrationPolicy,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.call("migrate", &(policy,)).await
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
