mod agent;
mod client;
pub mod table;

use candid::{Encode, Principal};
use ic_dbms_api::prelude::{IcDbmsCanisterArgs, IcDbmsCanisterInitArgs};
use pocket_ic_harness::{Canister, CanisterSetup, PocketIcTestEnv};
pub use pocket_ic_harness::{admin, alice, bob};

pub use self::agent::init_new_agent;
pub use self::client::PocketIcClient;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum TestCanister {
    DbmsCanister,
    DbmsCanisterClientIntegration,
}

impl Canister for TestCanister {
    fn as_path(&self) -> &'static std::path::Path {
        match self {
            TestCanister::DbmsCanister => std::path::Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../../.artifact/example.wasm.gz"
            )),
            TestCanister::DbmsCanisterClientIntegration => std::path::Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../../.artifact/dbms_canister_client_integration.wasm.gz"
            )),
        }
    }

    fn all_canisters() -> &'static [Self] {
        &[Self::DbmsCanister, Self::DbmsCanisterClientIntegration]
    }
}

pub trait TestEnvExt {
    fn dbms_canister(&self) -> Principal;
    fn dbms_canister_client_integration(&self) -> Principal;
}

impl TestEnvExt for PocketIcTestEnv<TestCanisterSetup> {
    fn dbms_canister(&self) -> Principal {
        self.canister_id(&TestCanister::DbmsCanister)
    }

    fn dbms_canister_client_integration(&self) -> Principal {
        self.canister_id(&TestCanister::DbmsCanisterClientIntegration)
    }
}

pub struct TestCanisterSetup;

impl CanisterSetup for TestCanisterSetup {
    type Canister = TestCanister;

    async fn setup(env: &mut pocket_ic_harness::PocketIcTestEnv<Self>)
    where
        Self: Sized,
    {
        let dbms_canister = env.canister_id(&TestCanister::DbmsCanister);
        let dbms_canister_client_integration_canister =
            env.canister_id(&TestCanister::DbmsCanisterClientIntegration);
        // install dbms-canister
        let init_arg = Encode!(&IcDbmsCanisterArgs::Init(IcDbmsCanisterInitArgs {
            allowed_principals: vec![admin(), dbms_canister_client_integration_canister],
        }))
        .expect("failed to encode dbms canister init args");
        env.install_canister(TestCanister::DbmsCanister, init_arg)
            .await;

        // install dbms-canister-client-integration canister
        let init_arg = Encode!(&dbms_canister).expect("Failed to encode init arg");
        env.install_canister(TestCanister::DbmsCanisterClientIntegration, init_arg)
            .await;
    }
}
