use ic_dbms_client::prelude::{Client as _, IcDbmsPocketIcClient};
use pocket_ic_tests::TestEnv;

#[pocket_ic_tests_macro::test]
async fn test_should_add_and_remove_principal_to_acl(env: PocketIcTestEnv) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), env.admin(), &env.pic);

    // add alice to ACL
    client
        .acl_add_principal(env.bob())
        .await
        .expect("failed to call canister")
        .expect("failed to add principal to ACL");

    // get
    let acl = client
        .acl_allowed_principals()
        .await
        .expect("failed to call canister");
    assert!(acl.contains(&env.bob()));
    assert!(acl.contains(&env.admin()));

    // remove alice from ACL
    client
        .acl_remove_principal(env.bob())
        .await
        .expect("failed to call canister")
        .expect("failed to remove principal from ACL");

    // get
    let acl = client
        .acl_allowed_principals()
        .await
        .expect("failed to call canister");
    assert!(!acl.contains(&env.bob()));
    assert!(acl.contains(&env.admin()));
}
