use ic_dbms_api::prelude::{DeleteBehavior, Filter, Query, TableSchema, Text, Uint32, Value};
use ic_dbms_client::prelude::{Client as _, IcDbmsPocketIcClient};
use pocket_ic_harness::PocketIcTestEnv;
use pocket_ic_tests::table::{User, UserInsertRequest, UserUpdateRequest};
use pocket_ic_tests::{TestCanisterSetup, TestEnvExt as _, admin, alice};

#[pocket_ic_harness::test]
async fn test_should_insert_and_query_data(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    let insert_request = UserInsertRequest {
        id: Uint32::from(1),
        name: "Alice".into(),
        email: "alice@example.com".into(),
    };
    client
        .insert::<User>(User::table_name(), insert_request, None)
        .await
        .expect("failed to call canister")
        .expect("failed to insert user");

    // query user
    let query = Query::builder()
        .all()
        .and_where(Filter::eq("id", Value::Uint32(1.into())))
        .build();
    let users = client
        .select::<User>(User::table_name(), query, None)
        .await
        .expect("failed to call canister")
        .expect("failed to query user");

    assert_eq!(users.len(), 1);
    let user = &users[0];
    assert_eq!(user.id.unwrap(), Uint32::from(1));
    assert_eq!(user.name.as_ref().unwrap(), &Text::from("Alice"));
    assert_eq!(
        user.email.as_ref().unwrap(),
        &Text::from("alice@example.com")
    );
}

#[pocket_ic_harness::test]
async fn test_should_delete_a_user(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    let insert_request = UserInsertRequest {
        id: Uint32::from(2),
        name: "Bob".into(),
        email: "bob@example.com".into(),
    };
    client
        .insert::<User>(User::table_name(), insert_request, None)
        .await
        .expect("failed to call canister")
        .expect("failed to insert user");

    // delete user
    client
        .delete::<User>(
            User::table_name(),
            DeleteBehavior::Restrict,
            Some(Filter::eq("id", Value::Uint32(2.into()))),
            None,
        )
        .await
        .expect("failed to call canister")
        .expect("failed to delete user");
}

#[pocket_ic_harness::test]
async fn test_should_update_a_user(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    let insert_request = UserInsertRequest {
        id: Uint32::from(3),
        name: "Charlie".into(),
        email: "charlie@example.com".into(),
    };
    client
        .insert::<User>(User::table_name(), insert_request, None)
        .await
        .expect("failed to call canister")
        .expect("failed to insert user");

    let patch = UserUpdateRequest {
        id: None,
        name: Some("Charles".into()),
        email: None,
        where_clause: Some(Filter::eq("id", Value::Uint32(3.into()))),
    };

    client
        .update::<User>(User::table_name(), patch, None)
        .await
        .expect("failed to call canister")
        .expect("failed to update user");

    // select
    let query = Query::builder()
        .all()
        .and_where(Filter::eq("id", Value::Uint32(3.into())))
        .build();
    let users = client
        .select::<User>(User::table_name(), query, None)
        .await
        .expect("failed to call canister")
        .expect("failed to query user");

    assert_eq!(users.len(), 1);
    let user = &users[0];
    assert_eq!(user.id.unwrap(), Uint32::from(3));
    assert_eq!(user.name.as_ref().unwrap(), &Text::from("Charles"));
    assert_eq!(
        user.email.as_ref().unwrap(),
        &Text::from("charlie@example.com")
    );
}

#[pocket_ic_harness::test]
async fn test_should_not_allow_unauthorized_call(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), alice(), &env.pic);

    let insert_request = UserInsertRequest {
        id: Uint32::from(4),
        name: "Eve".into(),
        email: "eve@example.com".into(),
    };
    let result = client
        .insert::<User>(User::table_name(), insert_request, None)
        .await;
    assert!(result.is_err());
}
