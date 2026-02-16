mod acid;
mod acl;
mod agent_client;
mod crud;
mod ic_dbms_canister_client;
mod select_raw;

#[pocket_ic_tests_macro::test]
async fn test_should_init_dbms_canister(env: PocketIcTestEnv) {}
