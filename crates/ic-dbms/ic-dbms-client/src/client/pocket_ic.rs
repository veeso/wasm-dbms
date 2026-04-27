use candid::{CandidType, Decode, Encode, Principal};
use ic_dbms_api::prelude::{IcDbmsResult, IdentityPerms, TablePerms};
use pocket_ic::nonblocking::PocketIc;

use crate::client::{Client, RawRecords};
use crate::errors::{IcDbmsCanisterClientResult, PocketIcError};

/// IC DBMS Canister client implementation for pocket-ic.
pub struct IcDbmsPocketIcClient<'a> {
    caller: Principal,
    principal: Principal,
    pocket_ic: &'a PocketIc,
}

impl<'a> IcDbmsPocketIcClient<'a> {
    /// Creates a new IC DBMS Canister client for pocket-ic.
    pub fn new(principal: Principal, caller: Principal, pocket_ic: &'a PocketIc) -> Self {
        Self {
            caller,
            principal,
            pocket_ic,
        }
    }

    async fn query<R>(
        &self,
        canister: Principal,
        caller: Principal,
        method: &str,
        payload: Vec<u8>,
    ) -> IcDbmsCanisterClientResult<R>
    where
        R: for<'de> candid::Deserialize<'de> + CandidType,
    {
        let reply = match self
            .pocket_ic
            .query_call(canister, caller, method, payload)
            .await
        {
            Ok(result) => result,
            Err(err) => return Err(PocketIcError::from(err).into()),
        };
        let ret_type = Decode!(&reply, R).map_err(PocketIcError::from)?;

        Ok(ret_type)
    }

    async fn update<R>(
        &self,
        canister: Principal,
        caller: Principal,
        method: &str,
        payload: Vec<u8>,
    ) -> IcDbmsCanisterClientResult<R>
    where
        R: for<'de> candid::Deserialize<'de> + CandidType,
    {
        let is_live = self.pocket_ic.url().is_some();
        let reply = if is_live {
            let id = self
                .pocket_ic
                .submit_call(canister, caller, method, payload)
                .await
                .map_err(PocketIcError::from)?;
            self.pocket_ic.await_call_no_ticks(id).await
        } else {
            self.pocket_ic
                .update_call(canister, caller, method, payload)
                .await
        };

        let reply = match reply {
            Ok(r) => r,
            Err(err) => return Err(PocketIcError::from(err).into()),
        };
        let ret_type = candid::Decode!(&reply, R).map_err(PocketIcError::from)?;

        Ok(ret_type)
    }
}

impl Client for IcDbmsPocketIcClient<'_> {
    fn principal(&self) -> Principal {
        self.principal
    }

    async fn grant_admin(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "grant_admin",
            Encode!(&principal).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn revoke_admin(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "revoke_admin",
            Encode!(&principal).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn grant_manage_acl(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "grant_manage_acl",
            Encode!(&principal).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn revoke_manage_acl(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "revoke_manage_acl",
            Encode!(&principal).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn grant_migrate(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "grant_migrate",
            Encode!(&principal).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn revoke_migrate(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "revoke_migrate",
            Encode!(&principal).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn grant_all_tables_perms(
        &self,
        principal: Principal,
        perms: TablePerms,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "grant_all_tables_perms",
            Encode!(&principal, &perms).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn revoke_all_tables_perms(
        &self,
        principal: Principal,
        perms: TablePerms,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "revoke_all_tables_perms",
            Encode!(&principal, &perms).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn grant_table_perms(
        &self,
        principal: Principal,
        table: &str,
        perms: TablePerms,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        let table_str = table.to_string();
        self.update(
            self.principal,
            self.caller,
            "grant_table_perms",
            Encode!(&principal, &table_str, &perms).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn revoke_table_perms(
        &self,
        principal: Principal,
        table: &str,
        perms: TablePerms,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        let table_str = table.to_string();
        self.update(
            self.principal,
            self.caller,
            "revoke_table_perms",
            Encode!(&principal, &table_str, &perms).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn remove_identity(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "remove_identity",
            Encode!(&principal).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn list_identities(
        &self,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<Vec<(Principal, IdentityPerms)>>> {
        self.query(self.principal, self.caller, "list_identities", Vec::new())
            .await
    }

    async fn my_perms(&self) -> IcDbmsCanisterClientResult<IdentityPerms> {
        self.query(self.principal, self.caller, "my_perms", Vec::new())
            .await
    }

    async fn begin_transaction(
        &self,
    ) -> IcDbmsCanisterClientResult<ic_dbms_api::prelude::TransactionId> {
        self.update(self.principal, self.caller, "begin_transaction", Vec::new())
            .await
    }

    async fn commit(
        &self,
        transaction_id: ic_dbms_api::prelude::TransactionId,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "commit",
            Encode!(&transaction_id).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn rollback(
        &self,
        transaction_id: ic_dbms_api::prelude::TransactionId,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "rollback",
            Encode!(&transaction_id).map_err(PocketIcError::Candid)?,
        )
        .await
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
        self.query(
            self.principal,
            self.caller,
            &crate::utils::table_method(table, "select"),
            Encode!(&query, &transaction_id).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn select_raw(
        &self,
        table: &str,
        query: ic_dbms_api::prelude::Query,
        transaction_id: Option<ic_dbms_api::prelude::TransactionId>,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<RawRecords>> {
        self.query(
            self.principal,
            self.caller,
            "select",
            Encode!(&table, &query, &transaction_id).map_err(PocketIcError::Candid)?,
        )
        .await
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
        self.query(
            self.principal,
            self.caller,
            &crate::utils::table_method(table, "aggregate"),
            Encode!(&query, &aggregates, &transaction_id).map_err(PocketIcError::Candid)?,
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
        self.update(
            self.principal,
            self.caller,
            &crate::utils::table_method(table, "insert"),
            Encode!(&record, &transaction_id).map_err(PocketIcError::Candid)?,
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
        self.update(
            self.principal,
            self.caller,
            &crate::utils::table_method(table, "update"),
            Encode!(&patch, &transaction_id).map_err(PocketIcError::Candid)?,
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
        self.update(
            self.principal,
            self.caller,
            &crate::utils::table_method(table, "delete"),
            Encode!(&behaviour, &filter, &transaction_id).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn has_drift(&self) -> IcDbmsCanisterClientResult<IcDbmsResult<bool>> {
        self.query(self.principal, self.caller, "has_drift", Vec::new())
            .await
    }

    async fn pending_migrations(
        &self,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<Vec<ic_dbms_api::prelude::MigrationOp>>> {
        self.query(
            self.principal,
            self.caller,
            "pending_migrations",
            Vec::new(),
        )
        .await
    }

    async fn migrate(
        &self,
        policy: ic_dbms_api::prelude::MigrationPolicy,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "migrate",
            Encode!(&policy).map_err(PocketIcError::Candid)?,
        )
        .await
    }
}
