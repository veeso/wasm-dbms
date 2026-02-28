use candid::{CandidType, Decode, Encode, Principal};
use ic_dbms_api::prelude::IcDbmsResult;
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

    async fn acl_add_principal(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "acl_add_principal",
            Encode!(&principal).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn acl_remove_principal(
        &self,
        principal: Principal,
    ) -> IcDbmsCanisterClientResult<IcDbmsResult<()>> {
        self.update(
            self.principal,
            self.caller,
            "acl_remove_principal",
            Encode!(&principal).map_err(PocketIcError::Candid)?,
        )
        .await
    }

    async fn acl_allowed_principals(&self) -> IcDbmsCanisterClientResult<Vec<Principal>> {
        self.query(
            self.principal,
            self.caller,
            "acl_allowed_principals",
            Vec::new(),
        )
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
}
