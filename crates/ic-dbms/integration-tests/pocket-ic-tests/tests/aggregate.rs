use candid::Encode;
use ic_dbms_api::prelude::{
    AggregateFunction, AggregatedRow, AggregatedValue, Filter, IcDbmsResult, Query, TableSchema,
    TransactionId, Uint32, Uint64, Value,
};
use ic_dbms_client::prelude::{Client as _, IcDbmsPocketIcClient};
use pocket_ic_harness::PocketIcTestEnv;
use pocket_ic_tests::table::{User, UserInsertRequest};
use pocket_ic_tests::{PocketIcClient, TestCanisterSetup, TestEnvExt as _, admin};

async fn seed_users(client: &IcDbmsPocketIcClient<'_>) {
    for (id, name, email) in [
        (1u32, "alice", "alice@example.com"),
        (2u32, "bob", "bob@example.com"),
        (3u32, "carol", "carol@example.com"),
    ] {
        let req = UserInsertRequest {
            id: Uint32::from(id),
            name: name.into(),
            email: email.into(),
        };
        client
            .insert::<User>(User::table_name(), req, None)
            .await
            .expect("call failed")
            .expect("insert failed");
    }
}

#[pocket_ic_harness::test]
async fn test_aggregate_count_all_via_pocket_ic_client(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);
    seed_users(&client).await;

    let result = client
        .aggregate::<User>(
            User::table_name(),
            Query::default(),
            vec![AggregateFunction::Count(None)],
            None,
        )
        .await
        .expect("call failed")
        .expect("aggregate failed");

    assert_eq!(result.len(), 1);
    assert!(result[0].group_keys.is_empty());
    assert_eq!(result[0].values, vec![AggregatedValue::Count(3)]);
}

#[pocket_ic_harness::test]
async fn test_aggregate_sum_min_max_no_group(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);
    seed_users(&client).await;

    let result = client
        .aggregate::<User>(
            User::table_name(),
            Query::default(),
            vec![
                AggregateFunction::Sum("id".into()),
                AggregateFunction::Min("id".into()),
                AggregateFunction::Max("id".into()),
            ],
            None,
        )
        .await
        .expect("call failed")
        .expect("aggregate failed");

    assert_eq!(result.len(), 1);
    let v = &result[0].values;
    // SUM is returned as Decimal
    assert!(matches!(v[0], AggregatedValue::Sum(Value::Decimal(_))));
    assert_eq!(
        v[1],
        AggregatedValue::Min(Value::Uint32(Uint32::from(1u32)))
    );
    assert_eq!(
        v[2],
        AggregatedValue::Max(Value::Uint32(Uint32::from(3u32)))
    );
}

#[pocket_ic_harness::test]
async fn test_aggregate_group_by_with_having(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);
    seed_users(&client).await;

    // Group by id (each row is its own group), HAVING agg0 >= 1 keeps every group.
    let query = Query::builder()
        .group_by(&["id"])
        .having(Filter::ge("agg0", Value::Uint64(Uint64(1))))
        .order_by_asc("id")
        .build();
    let result = client
        .aggregate::<User>(
            User::table_name(),
            query,
            vec![AggregateFunction::Count(None)],
            None,
        )
        .await
        .expect("call failed")
        .expect("aggregate failed");

    assert_eq!(result.len(), 3);
    assert_eq!(
        result[0].group_keys,
        vec![Value::Uint32(Uint32::from(1u32))]
    );
    assert_eq!(result[0].values, vec![AggregatedValue::Count(1)]);
    assert_eq!(
        result[2].group_keys,
        vec![Value::Uint32(Uint32::from(3u32))]
    );
}

#[pocket_ic_harness::test]
async fn test_aggregate_having_filters_out_groups(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);
    seed_users(&client).await;

    // HAVING agg0 > 1 — no group has count > 1, expect empty result.
    let query = Query::builder()
        .group_by(&["id"])
        .having(Filter::gt("agg0", Value::Uint64(Uint64(1))))
        .build();
    let result = client
        .aggregate::<User>(
            User::table_name(),
            query,
            vec![AggregateFunction::Count(None)],
            None,
        )
        .await
        .expect("call failed")
        .expect("aggregate failed");

    assert!(result.is_empty());
}

#[pocket_ic_harness::test]
async fn test_aggregate_invalid_query_returns_err(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    // SUM on Text column — must be rejected at planning.
    let result = client
        .aggregate::<User>(
            User::table_name(),
            Query::default(),
            vec![AggregateFunction::Sum("name".into())],
            None,
        )
        .await
        .expect("call must reach canister");

    assert!(result.is_err(), "expected planning error, got {result:?}");
}

#[pocket_ic_harness::test]
async fn test_aggregate_via_integration_canister(env: PocketIcTestEnv<TestCanisterSetup>) {
    let dbms = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);
    seed_users(&dbms).await;

    // Add the integration canister as caller through ACL — already done in setup.
    let client = PocketIcClient::new(env.dbms_canister_client_integration(), admin(), &env.pic);

    let payload = Encode!(
        &Query::default(),
        &vec![AggregateFunction::Count(None)],
        &Option::<TransactionId>::None
    )
    .expect("encode");
    let res: Result<IcDbmsResult<Vec<AggregatedRow>>, String> = client
        .update("aggregate", payload)
        .await
        .expect("call failed");

    let rows = res.expect("client error").expect("aggregate failed");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].values, vec![AggregatedValue::Count(3)]);
}
