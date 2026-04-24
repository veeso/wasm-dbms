use candid::Encode;
use ic_dbms_api::prelude::{
    Filter, IcDbmsResult, JoinColumnDef, Query, TableSchema, Uint32, Value,
};
use ic_dbms_client::prelude::{Client as _, IcDbmsPocketIcClient};
use pocket_ic_harness::PocketIcTestEnv;
use pocket_ic_tests::table::{User, UserInsertRequest};
use pocket_ic_tests::{TestCanisterSetup, TestEnvExt as _, admin};

#[pocket_ic_harness::test]
async fn test_should_select_raw_all_columns(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    let insert_request = UserInsertRequest {
        id: Uint32::from(100),
        name: "RawAlice".into(),
        email: "rawalice@example.com".into(),
    };
    client
        .insert::<User>(User::table_name(), insert_request, None)
        .await
        .expect("failed to call canister")
        .expect("failed to insert user");

    // Call the generic `select` endpoint with all columns
    let query = Query::builder()
        .all()
        .and_where(Filter::eq("id", Value::Uint32(100.into())))
        .build();

    let result = client
        .select_raw("users", query, None)
        .await
        .expect("failed to call canister");

    let rows = result.expect("select_raw should succeed");
    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    assert_eq!(row.len(), 3); // id, name, email

    let id_col = row.iter().find(|(col, _)| col.name == "id").unwrap();
    assert_eq!(id_col.1, Value::Uint32(100.into()));

    let name_col = row.iter().find(|(col, _)| col.name == "name").unwrap();
    assert_eq!(name_col.1, Value::Text("RawAlice".to_string().into()));

    let email_col = row.iter().find(|(col, _)| col.name == "email").unwrap();
    assert_eq!(
        email_col.1,
        Value::Text("rawalice@example.com".to_string().into())
    );
}

#[pocket_ic_harness::test]
async fn test_should_select_raw_specific_columns(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    let insert_request = UserInsertRequest {
        id: Uint32::from(101),
        name: "RawBob".into(),
        email: "rawbob@example.com".into(),
    };
    client
        .insert::<User>(User::table_name(), insert_request, None)
        .await
        .expect("failed to call canister")
        .expect("failed to insert user");

    // Select only the "name" column
    let query = Query::builder()
        .field("name")
        .and_where(Filter::eq("id", Value::Uint32(101.into())))
        .build();

    let payload =
        Encode!(&"users".to_string(), &query, &None::<u64>).expect("failed to encode payload");

    let result: IcDbmsResult<Vec<Vec<(JoinColumnDef, Value)>>> = env
        .query(env.dbms_canister(), admin(), "select", payload)
        .await
        .expect("failed to call canister");

    let rows = result.expect("select_raw should succeed");
    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    assert_eq!(row.len(), 1); // only "name"
    assert_eq!(row[0].0.name, "name");
    assert_eq!(row[0].1, Value::Text("RawBob".to_string().into()));
}

#[pocket_ic_harness::test]
async fn test_should_select_raw_with_limit_and_offset(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    // Insert 3 users
    for (i, name) in [(102, "LimitA"), (103, "LimitB"), (104, "LimitC")] {
        let insert_request = UserInsertRequest {
            id: Uint32::from(i),
            name: name.into(),
            email: format!("{name}@example.com").into(),
        };
        client
            .insert::<User>(User::table_name(), insert_request, None)
            .await
            .expect("failed to call canister")
            .expect("failed to insert user");
    }

    // Select with limit 2, offset 1, ordered by id asc
    let query = Query::builder()
        .all()
        .and_where(Filter::ge("id", Value::Uint32(102.into())))
        .order_by_asc("id")
        .limit(2)
        .offset(1)
        .build();

    let payload =
        Encode!(&"users".to_string(), &query, &None::<u64>).expect("failed to encode payload");

    let result: IcDbmsResult<Vec<Vec<(JoinColumnDef, Value)>>> = env
        .query(env.dbms_canister(), admin(), "select", payload)
        .await
        .expect("failed to call canister");

    let rows = result.expect("select_raw should succeed");
    assert_eq!(rows.len(), 2);

    // Should skip LimitA (offset=1), return LimitB and LimitC
    let name0 = rows[0].iter().find(|(col, _)| col.name == "name").unwrap();
    assert_eq!(name0.1, Value::Text("LimitB".to_string().into()));

    let name1 = rows[1].iter().find(|(col, _)| col.name == "name").unwrap();
    assert_eq!(name1.1, Value::Text("LimitC".to_string().into()));
}

#[pocket_ic_harness::test]
async fn test_should_fail_select_raw_unknown_table(env: PocketIcTestEnv<TestCanisterSetup>) {
    let query = Query::builder().all().build();

    let payload = Encode!(&"nonexistent".to_string(), &query, &None::<u64>)
        .expect("failed to encode payload");

    let result: IcDbmsResult<Vec<Vec<(JoinColumnDef, Value)>>> = env
        .query(env.dbms_canister(), admin(), "select", payload)
        .await
        .expect("failed to call canister");

    assert!(result.is_err(), "should fail for unknown table");
}
