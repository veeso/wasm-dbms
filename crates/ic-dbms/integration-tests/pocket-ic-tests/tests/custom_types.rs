use ic_dbms_api::prelude::{
    CustomValue, Filter, Principal, Query, TableSchema, Text, Uint32, Value,
};
use ic_dbms_client::prelude::{Client as _, IcDbmsPocketIcClient};
use pocket_ic_harness::PocketIcTestEnv;
use pocket_ic_tests::table::{Project, ProjectInsertRequest, ProjectUpdateRequest};
use pocket_ic_tests::{TestCanisterSetup, TestEnvExt as _, admin};

#[pocket_ic_harness::test]
async fn test_should_insert_and_query_project_with_custom_type(
    env: PocketIcTestEnv<TestCanisterSetup>,
) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    let owner = Principal(candid::Principal::from_text("aaaaa-aa").unwrap());
    let insert_request = ProjectInsertRequest {
        id: Uint32::from(1),
        name: "My Project".into(),
        owner: owner.clone(),
    };
    client
        .insert::<Project>(Project::table_name(), insert_request, None)
        .await
        .expect("failed to call canister")
        .expect("failed to insert project");

    let query = Query::builder()
        .all()
        .and_where(Filter::eq("id", Value::Uint32(1.into())))
        .build();
    let projects = client
        .select::<Project>(Project::table_name(), query, None)
        .await
        .expect("failed to call canister")
        .expect("failed to query project");

    assert_eq!(projects.len(), 1);
    let project = &projects[0];
    assert_eq!(project.id.unwrap(), Uint32::from(1));
    assert_eq!(project.name.as_ref().unwrap(), &Text::from("My Project"));
    assert_eq!(project.owner.as_ref().unwrap(), &owner);
}

#[pocket_ic_harness::test]
async fn test_should_filter_project_by_custom_type(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    let owner_a = Principal(candid::Principal::from_text("aaaaa-aa").unwrap());
    let owner_b = Principal(candid::Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap());

    // insert two projects with different owners
    for (id, name, owner) in [(1u32, "Project A", &owner_a), (2u32, "Project B", &owner_b)] {
        let insert_request = ProjectInsertRequest {
            id: Uint32::from(id),
            name: name.into(),
            owner: owner.clone(),
        };
        client
            .insert::<Project>(Project::table_name(), insert_request, None)
            .await
            .expect("failed to call canister")
            .expect("failed to insert project");
    }

    // filter by owner_a using Custom value
    let query = Query::builder()
        .all()
        .and_where(Filter::eq(
            "owner",
            Value::Custom(CustomValue::new(&owner_a)),
        ))
        .build();
    let projects = client
        .select::<Project>(Project::table_name(), query, None)
        .await
        .expect("failed to call canister")
        .expect("failed to query project");

    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name.as_ref().unwrap(), &Text::from("Project A"));
}

#[pocket_ic_harness::test]
async fn test_should_update_project_custom_type_field(env: PocketIcTestEnv<TestCanisterSetup>) {
    let client = IcDbmsPocketIcClient::new(env.dbms_canister(), admin(), &env.pic);

    let original_owner = Principal(candid::Principal::from_text("aaaaa-aa").unwrap());
    let insert_request = ProjectInsertRequest {
        id: Uint32::from(1),
        name: "Updatable Project".into(),
        owner: original_owner,
    };
    client
        .insert::<Project>(Project::table_name(), insert_request, None)
        .await
        .expect("failed to call canister")
        .expect("failed to insert project");

    // update the owner
    let new_owner = Principal(candid::Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap());
    let patch = ProjectUpdateRequest {
        id: None,
        name: None,
        owner: Some(new_owner.clone()),
        where_clause: Some(Filter::eq("id", Value::Uint32(1.into()))),
    };
    client
        .update::<Project>(Project::table_name(), patch, None)
        .await
        .expect("failed to call canister")
        .expect("failed to update project");

    // verify the update
    let query = Query::builder()
        .all()
        .and_where(Filter::eq("id", Value::Uint32(1.into())))
        .build();
    let projects = client
        .select::<Project>(Project::table_name(), query, None)
        .await
        .expect("failed to call canister")
        .expect("failed to query project");

    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].owner.as_ref().unwrap(), &new_owner);
}
