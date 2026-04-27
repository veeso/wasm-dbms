mod acid;
mod agent_client;
mod aggregate;
mod crud;
mod custom_types;
mod granular_acl;
mod ic_dbms_canister_client;
mod migrations;
mod select_raw;

#[pocket_ic_harness::test]
async fn test_should_init_dbms_canister(
    _env: pocket_ic_harness::PocketIcTestEnv<pocket_ic_tests::TestCanisterSetup>,
) {
}
