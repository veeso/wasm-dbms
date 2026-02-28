mod env;

use std::io::Read as _;
use std::path::PathBuf;

use candid::{CandidType, Decode, Encode, Principal};
use ic_dbms_api::prelude::{IcDbmsCanisterArgs, IcDbmsCanisterInitArgs};
use ic_dbms_client::prelude::{Client, IcDbmsPocketIcClient};
use pocket_ic::nonblocking::PocketIc;
use serde::de::DeserializeOwned;

use crate::TestEnv;
use crate::actor::{admin, alice, bob};
use crate::wasm::Canister;

const DEFAULT_CYCLES: u128 = 2_000_000_000_000_000;

/// Test environment
pub struct PocketIcTestEnv {
    pub pic: PocketIc,
    dbms_canister: Principal,
    dbms_canister_client_integration: Principal,
}

impl TestEnv for PocketIcTestEnv {
    async fn query<R>(
        &self,
        canister: Principal,
        caller: Principal,
        method: &str,
        payload: Vec<u8>,
    ) -> anyhow::Result<R>
    where
        R: DeserializeOwned + CandidType,
    {
        let reply = match self.pic.query_call(canister, caller, method, payload).await {
            Ok(result) => result,
            Err(e) => anyhow::bail!("Error calling {}: {:?}", method, e),
        };
        let ret_type = Decode!(&reply, R)?;

        Ok(ret_type)
    }

    async fn update<R>(
        &self,
        canister: Principal,
        caller: Principal,
        method: &str,
        payload: Vec<u8>,
    ) -> anyhow::Result<R>
    where
        R: DeserializeOwned + CandidType,
    {
        let reply = if self.is_live() {
            let id = self
                .pic
                .submit_call(canister, caller, method, payload)
                .await
                .map_err(|e| anyhow::anyhow!("Error submitting call {}: {:?}", method, e))?;
            self.pic.await_call_no_ticks(id).await
        } else {
            self.pic
                .update_call(canister, caller, method, payload)
                .await
        };

        let reply = match reply {
            Ok(r) => r,
            Err(r) => anyhow::bail!("{} was rejected: {:?}", method, r),
        };
        let ret_type = Decode!(&reply, R)?;

        Ok(ret_type)
    }

    fn admin(&self) -> Principal {
        admin()
    }
    fn bob(&self) -> Principal {
        bob()
    }

    fn alice(&self) -> Principal {
        alice()
    }

    fn dbms_canister(&self) -> Principal {
        self.dbms_canister
    }

    fn dbms_canister_client_integration(&self) -> Principal {
        self.dbms_canister_client_integration
    }

    fn endpoint(&self) -> Option<url::Url> {
        self.pic.url()
    }
}

impl PocketIcTestEnv {
    /// Install the canisters needed for the tests
    pub async fn init() -> Self {
        let pic = env::init_pocket_ic()
            .await
            .with_nns_subnet()
            .with_ii_subnet()
            .with_fiduciary_subnet()
            .with_application_subnet()
            .with_max_request_time_ms(Some(30_000))
            .build_async()
            .await;

        // create canisters
        let dbms_canister = pic.create_canister_with_settings(Some(admin()), None).await;
        println!("DBMS Canister: {dbms_canister}",);
        let dbms_canister_client_integration =
            pic.create_canister_with_settings(Some(admin()), None).await;
        println!("DBMS Canister Client Integration: {dbms_canister_client_integration}",);

        // install canisters
        Self::install_dbms_canister(&pic, dbms_canister).await;
        Self::install_dbms_canister_client_integration(
            &pic,
            dbms_canister_client_integration,
            dbms_canister,
        )
        .await;

        Self {
            pic,
            dbms_canister,
            dbms_canister_client_integration,
        }
    }

    /// Stop instance -  Should be called after each test
    pub async fn stop(self) {
        self.pic.drop().await
    }

    fn is_live(&self) -> bool {
        self.pic.url().is_some()
    }

    /// Install [`Canister::DbmsCanister`] canister
    async fn install_dbms_canister(pic: &PocketIc, canister_id: Principal) {
        pic.add_cycles(canister_id, DEFAULT_CYCLES).await;

        let wasm_bytes = Self::load_wasm(Canister::DbmsCanister);

        let init_args = IcDbmsCanisterArgs::Init(IcDbmsCanisterInitArgs {
            allowed_principals: vec![admin()],
        });

        let init_arg = Encode!(&init_args).expect("Failed to encode init arg");

        pic.install_canister(canister_id, wasm_bytes, init_arg, Some(admin()))
            .await;
    }

    /// Install [`Canister::DbmsCanister`] canister
    async fn install_dbms_canister_client_integration(
        pic: &PocketIc,
        canister_id: Principal,
        dbms_canister: Principal,
    ) {
        pic.add_cycles(canister_id, DEFAULT_CYCLES).await;

        let wasm_bytes = Self::load_wasm(Canister::DbmsCanisterClientIntegration);

        let init_arg = Encode!(&dbms_canister).expect("Failed to encode init arg");

        pic.install_canister(canister_id, wasm_bytes, init_arg, Some(admin()))
            .await;

        // add this canister to the DBMS canister's allowed principals
        let client = IcDbmsPocketIcClient::new(dbms_canister, admin(), pic);
        client
            .acl_add_principal(canister_id)
            .await
            .expect("failed to call canister")
            .expect("failed to add principal to ACL");
    }

    fn load_wasm(canister: Canister) -> Vec<u8> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push(canister.as_path());

        let mut file = std::fs::File::open(path).expect("Failed to open wasm file");
        let mut wasm_bytes = Vec::new();
        file.read_to_end(&mut wasm_bytes)
            .expect("Failed to read wasm file");

        wasm_bytes
    }

    pub async fn live(&mut self, live: bool) {
        if live {
            self.pic.make_live(None).await;
        } else {
            self.pic.stop_live().await;
        }
    }
}
