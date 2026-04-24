use candid::{Encode, Principal};
use ic_dbms_api::prelude::{
    DeleteBehavior, Filter, IcDbmsResult, JoinColumnDef, Query, TransactionId, Value,
};
use pocket_ic_harness::PocketIcTestEnv;
use pocket_ic_tests::table::{UserInsertRequest, UserRecord, UserUpdateRequest};
use pocket_ic_tests::{PocketIcClient, TestCanisterSetup, TestEnvExt as _, admin, bob};

#[pocket_ic_harness::test]
async fn test_should_add_and_remove_principal_to_acl(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = PocketIcClient::new(env.dbms_canister_client_integration(), admin(), &env.pic);

    // Add principal
    let res: Result<IcDbmsResult<()>, String> = client
        .update(
            "acl_add_principal",
            Encode!(&bob()).expect("Failed to encode"),
        )
        .await
        .expect("Can't update");

    res.expect("Client error")
        .expect("Failed to add principal to ACL");

    // Verify principal was added
    let principals: Result<Vec<Principal>, String> = client
        .update(
            "acl_allowed_principals",
            Encode!().expect("Failed to encode"),
        )
        .await
        .expect("Can't query");

    let principals = principals.expect("Client error");
    assert!(principals.contains(&bob()));

    // Remove principal
    let res: Result<IcDbmsResult<()>, String> = client
        .update(
            "acl_remove_principal",
            Encode!(&bob()).expect("Failed to encode"),
        )
        .await
        .expect("Can't update");

    res.expect("Client error")
        .expect("Failed to remove principal from ACL");

    // Verify principal was removed
    let principals: Result<Vec<Principal>, String> = client
        .update(
            "acl_allowed_principals",
            Encode!().expect("Failed to encode"),
        )
        .await
        .expect("Can't query");

    let principals = principals.expect("Client error");
    assert!(!principals.contains(&bob()));
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
