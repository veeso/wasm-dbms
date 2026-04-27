use candid::{Encode, Principal};
use ic_dbms_api::prelude::{
    DeleteBehavior, Filter, IcDbmsResult, JoinColumnDef, Query, TransactionId, Value,
};
use pocket_ic_harness::PocketIcTestEnv;
use pocket_ic_tests::table::{UserInsertRequest, UserRecord, UserUpdateRequest};
use pocket_ic_tests::{PocketIcClient, TestCanisterSetup, TestEnvExt as _, admin, bob};

#[pocket_ic_harness::test]
async fn test_should_grant_and_revoke_admin(env: PocketIcTestEnv<TestCanisterSetup>) {
    use ic_dbms_api::prelude::IdentityPerms;

    let client = PocketIcClient::new(env.dbms_canister_client_integration(), admin(), &env.pic);

    // Grant admin
    let res: Result<IcDbmsResult<()>, String> = client
        .update("grant_admin", Encode!(&bob()).expect("Failed to encode"))
        .await
        .expect("Can't update");

    res.expect("Client error").expect("Failed to grant admin");

    // Verify via list_identities
    let identities: Result<IcDbmsResult<Vec<(Principal, IdentityPerms)>>, String> = client
        .update("list_identities", Encode!().expect("Failed to encode"))
        .await
        .expect("Can't query");
    let identities = identities.expect("Client error").expect("list ok");
    assert!(
        identities
            .iter()
            .any(|(p, perms)| *p == bob() && perms.admin)
    );

    // Revoke admin
    let res: Result<IcDbmsResult<()>, String> = client
        .update("revoke_admin", Encode!(&bob()).expect("Failed to encode"))
        .await
        .expect("Can't update");

    res.expect("Client error").expect("Failed to revoke admin");
}

#[pocket_ic_harness::test]
async fn test_should_begin_commit_transaction(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = PocketIcClient::new(env.dbms_canister_client_integration(), admin(), &env.pic);

    // Begin transaction
    let res: Result<TransactionId, String> = client
        .update("begin_transaction", Encode!().expect("Failed to encode"))
        .await
        .expect("Can't update");

    let transaction_id = res.expect("Failed to begin transaction");

    // Commit transaction
    let res: Result<IcDbmsResult<()>, String> = client
        .update(
            "commit",
            Encode!(&transaction_id).expect("Failed to encode"),
        )
        .await
        .expect("Can't update");

    res.expect("Client error")
        .expect("Failed to commit transaction");
}

#[pocket_ic_harness::test]
async fn test_should_begin_rollback_transaction(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = PocketIcClient::new(env.dbms_canister_client_integration(), admin(), &env.pic);

    // Begin transaction
    let res: Result<TransactionId, String> = client
        .update("begin_transaction", Encode!().expect("Failed to encode"))
        .await
        .expect("Can't update");

    let transaction_id = res.expect("Failed to begin transaction");

    // Rollback transaction
    let res: Result<IcDbmsResult<()>, String> = client
        .update(
            "rollback",
            Encode!(&transaction_id).expect("Failed to encode"),
        )
        .await
        .expect("Can't update");

    res.expect("Client error")
        .expect("Failed to rollback transaction");
}

#[pocket_ic_harness::test]
async fn test_should_insert_select_update_delete(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = PocketIcClient::new(env.dbms_canister_client_integration(), admin(), &env.pic);

    // Insert a record
    let insert_request = UserInsertRequest {
        id: 1u32.into(),
        name: "Alice".into(),
        email: "alice@example.com".into(),
    };
    let transaction_id: Option<TransactionId> = None;

    let res: Result<IcDbmsResult<()>, String> = client
        .update(
            "insert",
            Encode!(&insert_request, &transaction_id).expect("Failed to encode"),
        )
        .await
        .expect("Can't update");

    res.expect("Client error").expect("Failed to insert record");

    // Select the record
    let query = Query::builder()
        .all()
        .and_where(Filter::eq("id", Value::Uint32(1.into())))
        .build();

    let res: Result<IcDbmsResult<Vec<UserRecord>>, String> = client
        .update(
            "select",
            Encode!(&query, &transaction_id).expect("Failed to encode"),
        )
        .await
        .expect("Can't query");

    let records = res
        .expect("Client error")
        .expect("Failed to select records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id.unwrap(), 1u32.into());
    assert_eq!(records[0].name.as_ref().unwrap(), &"Alice".into());
    assert_eq!(
        records[0].email.as_ref().unwrap(),
        &"alice@example.com".into()
    );

    // select raw
    let res: Result<IcDbmsResult<Vec<Vec<(JoinColumnDef, Value)>>>, String> = client
        .update(
            "select_raw",
            Encode!(&query, &transaction_id).expect("Failed to encode"),
        )
        .await
        .expect("Can't query");
    assert!(res.is_ok());

    // Update the record
    let update_request = UserUpdateRequest {
        id: None,
        name: Some("Alice Updated".into()),
        email: None,
        where_clause: Some(Filter::eq("id", Value::Uint32(1.into()))),
    };

    let res: Result<IcDbmsResult<u64>, String> = client
        .update(
            "update",
            Encode!(&update_request, &transaction_id).expect("Failed to encode"),
        )
        .await
        .expect("Can't update");

    let updated_count = res.expect("Client error").expect("Failed to update record");
    assert_eq!(updated_count, 1);

    // Select again to verify update
    let res: Result<IcDbmsResult<Vec<UserRecord>>, String> = client
        .update(
            "select",
            Encode!(&query, &transaction_id).expect("Failed to encode"),
        )
        .await
        .expect("Can't query");

    let records = res
        .expect("Client error")
        .expect("Failed to select records");
    assert_eq!(records[0].name.as_ref().unwrap(), &"Alice Updated".into());

    // Delete the record
    let behaviour = DeleteBehavior::Restrict;
    let filter: Option<Filter> = None;

    let res: Result<IcDbmsResult<u64>, String> = client
        .update(
            "delete",
            Encode!(&behaviour, &filter, &transaction_id).expect("Failed to encode"),
        )
        .await
        .expect("Can't update");

    let deleted_count = res.expect("Client error").expect("Failed to delete record");
    assert_eq!(deleted_count, 1);

    // Verify deletion
    let res: Result<IcDbmsResult<Vec<UserRecord>>, String> = client
        .update(
            "select",
            Encode!(&query, &transaction_id).expect("Failed to encode"),
        )
        .await
        .expect("Can't query");

    let records = res
        .expect("Client error")
        .expect("Failed to select records");
    assert!(records.is_empty());
}
