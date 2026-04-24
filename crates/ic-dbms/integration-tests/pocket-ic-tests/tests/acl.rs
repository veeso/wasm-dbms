use ic_dbms_client::prelude::{Client as _, IcDbmsPocketIcClient};
use pocket_ic_harness::PocketIcTestEnv;
use pocket_ic_tests::{TestCanisterSetup, TestEnvExt as _, admin, bob};

#[pocket_ic_harness::test]
async fn test_should_add_and_remove_principal_to_acl(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    // add alice to ACL
    client
        .acl_add_principal(bob())
        .await
        .expect("failed to call canister")
        .expect("failed to add principal to ACL");

    // get
    let acl = client
        .acl_allowed_principals()
        .await
        .expect("failed to call canister");
    assert!(acl.contains(&bob()));
    assert!(acl.contains(&admin()));

    // remove alice from ACL
    client
        .acl_remove_principal(bob())
        .await
        .expect("failed to call canister")
        .expect("failed to remove principal from ACL");

    // get
    let acl = client
        .acl_allowed_principals()
        .await
        .expect("failed to call canister");
    assert!(!acl.contains(&bob()));
    assert!(acl.contains(&admin()));
}
