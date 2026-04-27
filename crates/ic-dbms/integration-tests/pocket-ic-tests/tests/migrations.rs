use candid::Encode;
use ic_dbms_api::prelude::{IcDbmsResult, MigrationOp, MigrationPolicy};
use ic_dbms_client::prelude::{Client as _, IcDbmsPocketIcClient};
use pocket_ic_harness::PocketIcTestEnv;
use pocket_ic_tests::{TestCanisterSetup, TestEnvExt as _, admin};

#[pocket_ic_harness::test]
async fn test_should_report_no_drift_on_fresh_canister(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    let drift = client
        .has_drift()
        .await
        .expect("failed to call canister")
        .expect("has_drift should succeed");

    assert!(!drift, "fresh canister must not be in drift");
}

#[pocket_ic_harness::test]
async fn test_should_return_empty_pending_migrations(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    let ops: Vec<MigrationOp> = client
        .pending_migrations()
        .await
        .expect("failed to call canister")
        .expect("pending_migrations should succeed");

    assert!(ops.is_empty(), "fresh canister should have no pending ops");
}

#[pocket_ic_harness::test]
async fn test_should_migrate_noop_when_no_drift(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    client
        .migrate(MigrationPolicy::default())
        .await
        .expect("failed to call canister")
        .expect("migrate should succeed as no-op");

    let drift = client
        .has_drift()
        .await
        .expect("failed to call canister")
        .expect("has_drift should succeed");
    assert!(!drift);
}

#[pocket_ic_harness::test]
async fn test_should_call_through_wrapper_canister(env: PocketIcTestEnv<TestCanisterSetup>) {
    let wrapper = env.dbms_canister_client_integration();

    let drift: Result<IcDbmsResult<bool>, String> = env
        .update(wrapper, admin(), "has_drift", Encode!().unwrap())
        .await
        .expect("failed to call wrapper canister");
    let drift = drift.expect("wrapper has_drift").expect("inner Ok");
    assert!(!drift);

    let ops: Result<IcDbmsResult<Vec<MigrationOp>>, String> = env
        .update(wrapper, admin(), "pending_migrations", Encode!().unwrap())
        .await
        .expect("failed to call wrapper canister");
    let ops = ops.expect("wrapper pending_migrations").expect("inner Ok");
    assert!(ops.is_empty());

    let policy = MigrationPolicy::default();
    let migrate_res: Result<IcDbmsResult<()>, String> = env
        .update(wrapper, admin(), "migrate", Encode!(&policy).unwrap())
        .await
        .expect("failed to call wrapper canister");
    migrate_res.expect("wrapper migrate").expect("inner Ok");
}
