mod acid;
mod acl;
mod agent_client;
mod crud;
mod custom_types;
mod ic_dbms_canister_client;
mod select_raw;

#[pocket_ic_harness::test]
async fn test_should_init_dbms_canister(
    _env: pocket_ic_harness::PocketIcTestEnv<pocket_ic_tests::TestCanisterSetup>,
) {
}
