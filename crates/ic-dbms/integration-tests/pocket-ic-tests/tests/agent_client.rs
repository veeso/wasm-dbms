use ic_dbms_api::prelude::{
    DeleteBehavior, Filter, Query, TablePerms, TableSchema, Text, Uint32, Value,
};
use ic_dbms_client::prelude::{Client as _, IcDbmsAgentClient};
use pocket_ic_harness::PocketIcTestEnv;
use pocket_ic_tests::table::{Post, PostInsertRequest, User, UserInsertRequest, UserUpdateRequest};
use pocket_ic_tests::{TestCanisterSetup, TestEnvExt as _, admin, bob, init_new_agent};

#[pocket_ic_harness::test]
async fn test_agent_client_should_return_principal(env: PocketIcTestEnv<TestCanisterSetup>) {
    let e = &mut env;
    e.pic.make_live(None).await;

    let agent = init_new_agent(e, true).await;
    let client = IcDbmsAgentClient::new(&agent, e.dbms_canister());

    assert_eq!(client.principal(), e.dbms_canister());
}

#[pocket_ic_harness::test]
async fn test_agent_client_should_grant_admin(env: PocketIcTestEnv<TestCanisterSetup>) {
    let e = &mut env;
    e.pic.make_live(None).await;

    let agent = init_new_agent(e, true).await;
    let client = IcDbmsAgentClient::new(&agent, e.dbms_canister());

    client
        .grant_admin(bob())
        .await
        .expect("failed to call canister")
        .expect("failed to grant admin");
}

#[pocket_ic_harness::test]
async fn test_agent_client_should_revoke_admin(env: PocketIcTestEnv<TestCanisterSetup>) {
    let e = &mut env;
    e.pic.make_live(None).await;

    let agent = init_new_agent(e, true).await;
    let client = IcDbmsAgentClient::new(&agent, e.dbms_canister());

    client
        .grant_admin(bob())
        .await
        .expect("failed to call canister")
        .expect("failed to grant admin");

    client
        .revoke_admin(bob())
        .await
        .expect("failed to call canister")
        .expect("failed to revoke admin");

    let identities = client
        .list_identities()
        .await
        .expect("failed to call canister")
        .expect("failed to list identities");
    let bob_perms = identities
        .iter()
        .find(|(p, _)| *p == bob())
        .map(|(_, perms)| perms);
    assert!(bob_perms.map(|p| !p.admin).unwrap_or(true));
}

#[pocket_ic_harness::test]
async fn test_agent_client_should_list_identities(env: PocketIcTestEnv<TestCanisterSetup>) {
    let e = &mut env;
    e.pic.make_live(None).await;

    let agent = init_new_agent(e, true).await;
    let client = IcDbmsAgentClient::new(&agent, e.dbms_canister());

    let identities = client
        .list_identities()
        .await
        .expect("failed to call canister")
        .expect("failed to list identities");

    assert!(identities.iter().any(|(p, _)| *p == admin()));
}

#[pocket_ic_harness::test]
async fn test_agent_client_should_grant_table_perms(env: PocketIcTestEnv<TestCanisterSetup>) {
    let e = &mut env;
    e.pic.make_live(None).await;

    let agent = init_new_agent(e, true).await;
    let client = IcDbmsAgentClient::new(&agent, e.dbms_canister());

    client
        .grant_table_perms(bob(), "users", TablePerms::READ)
        .await
        .expect("failed to call canister")
        .expect("failed to grant table perms");
}

#[pocket_ic_harness::test]
async fn test_agent_client_should_insert_and_select(env: PocketIcTestEnv<TestCanisterSetup>) {
    let e = &mut env;
    e.pic.make_live(None).await;

    let agent = init_new_agent(e, true).await;
    let client = IcDbmsAgentClient::new(&agent, e.dbms_canister());

    let insert_request = UserInsertRequest {
        id: Uint32::from(100),
        name: "AgentAlice".into(),
        email: "agent.alice@example.com".into(),
    };
    client
        .insert::<User>(User::table_name(), insert_request, None)
        .await
        .expect("failed to call canister")
        .expect("failed to insert user");

    // Query the inserted user
    let query = Query::builder()
        .all()
        .and_where(Filter::eq("id", Value::Uint32(100.into())))
        .build();
    let users = client
        .select::<User>(User::table_name(), query, None)
        .await
        .expect("failed to call canister")
        .expect("failed to query user");

    assert_eq!(users.len(), 1);
    let user = &users[0];
    assert_eq!(user.id.unwrap(), Uint32::from(100));
    assert_eq!(user.name.as_ref().unwrap(), &Text::from("AgentAlice"));
    assert_eq!(
        user.email.as_ref().unwrap(),
        &Text::from("agent.alice@example.com")
    );
}

#[pocket_ic_harness::test]
async fn test_agent_client_should_update(env: PocketIcTestEnv<TestCanisterSetup>) {
    let e = &mut env;
    e.pic.make_live(None).await;

    let agent = init_new_agent(e, true).await;
    let client = IcDbmsAgentClient::new(&agent, e.dbms_canister());

    // Insert a user first
    let insert_request = UserInsertRequest {
        id: Uint32::from(101),
        name: "AgentBob".into(),
        email: "agent.bob@example.com".into(),
    };
    client
        .insert::<User>(User::table_name(), insert_request, None)
        .await
        .expect("failed to call canister")
        .expect("failed to insert user");

    // Update the user's name
    let patch = UserUpdateRequest {
        id: None,
        name: Some("AgentRobert".into()),
        email: None,
        where_clause: Some(Filter::eq("id", Value::Uint32(101.into()))),
    };
    client
        .update::<User>(User::table_name(), patch, None)
        .await
        .expect("failed to call canister")
        .expect("failed to update user");

    // Verify the update
    let query = Query::builder()
        .all()
        .and_where(Filter::eq("id", Value::Uint32(101.into())))
        .build();
    let users = client
        .select::<User>(User::table_name(), query, None)
        .await
        .expect("failed to call canister")
        .expect("failed to query user");

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].name.as_ref().unwrap(), &Text::from("AgentRobert"));
}

#[pocket_ic_harness::test]
async fn test_agent_client_should_delete(env: PocketIcTestEnv<TestCanisterSetup>) {
    let e = &mut env;
    e.pic.make_live(None).await;

    let agent = init_new_agent(e, true).await;
    let client = IcDbmsAgentClient::new(&agent, e.dbms_canister());

    // Insert a user first
    let insert_request = UserInsertRequest {
        id: Uint32::from(102),
        name: "AgentCharlie".into(),
        email: "agent.charlie@example.com".into(),
    };
    client
        .insert::<User>(User::table_name(), insert_request, None)
        .await
        .expect("failed to call canister")
        .expect("failed to insert user");

    // Delete the user
    client
        .delete::<User>(
            User::table_name(),
            DeleteBehavior::Restrict,
            Some(Filter::eq("id", Value::Uint32(102.into()))),
            None,
        )
        .await
        .expect("failed to call canister")
        .expect("failed to delete user");

    // Verify deletion
    let query = Query::builder()
        .all()
        .and_where(Filter::eq("id", Value::Uint32(102.into())))
        .build();
    let users = client
        .select::<User>(User::table_name(), query, None)
        .await
        .expect("failed to call canister")
        .expect("failed to query user");

    assert!(users.is_empty());
}

#[pocket_ic_harness::test]
async fn test_agent_client_should_begin_transaction_and_commit(
    env: PocketIcTestEnv<TestCanisterSetup>,
) {
    let e = &mut env;
    e.pic.make_live(None).await;

    let agent = init_new_agent(e, true).await;
    let client = IcDbmsAgentClient::new(&agent, e.dbms_canister());

    // Begin transaction
    let transaction_id = client
        .begin_transaction()
        .await
        .expect("failed to call canister");

    // Insert user within transaction
    let insert_request = UserInsertRequest {
        id: Uint32::from(103),
        name: "AgentDave".into(),
        email: "agent.dave@example.com".into(),
    };
    client
        .insert::<User>(
            User::table_name(),
            insert_request,
            Some(transaction_id.clone()),
        )
        .await
        .expect("failed to call canister")
        .expect("failed to insert user");

    // Insert post within transaction
    let insert_request = PostInsertRequest {
        id: Uint32::from(100),
        user: Uint32::from(103),
        title: "Agent Post".into(),
        content: "This is a post from the agent test.".into(),
    };
    client
        .insert::<Post>(
            Post::table_name(),
            insert_request,
            Some(transaction_id.clone()),
        )
        .await
        .expect("failed to call canister")
        .expect("failed to insert post");

    // Commit transaction
    client
        .commit(transaction_id)
        .await
        .expect("failed to call canister")
        .expect("failed to commit transaction");

    // Verify user was committed
    let query = Query::builder()
        .all()
        .and_where(Filter::eq("id", Value::Uint32(103.into())))
        .build();
    let users = client
        .select::<User>(User::table_name(), query, None)
        .await
        .expect("failed to call canister")
        .expect("failed to query user");

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].name.as_ref().unwrap(), &Text::from("AgentDave"));

    // Verify post was committed
    let query = Query::builder()
        .all()
        .and_where(Filter::eq("user", Value::Uint32(103.into())))
        .build();
    let posts = client
        .select::<Post>(Post::table_name(), query, None)
        .await
        .expect("failed to call canister")
        .expect("failed to query post");

    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title.as_ref().unwrap(), &Text::from("Agent Post"));
}

#[pocket_ic_harness::test]
async fn test_agent_client_should_rollback_transaction(env: PocketIcTestEnv<TestCanisterSetup>) {
    let e = &mut env;
    e.pic.make_live(None).await;

    let agent = init_new_agent(e, true).await;
    let client = IcDbmsAgentClient::new(&agent, e.dbms_canister());

    // Begin transaction
    let transaction_id = client
        .begin_transaction()
        .await
        .expect("failed to call canister");

    // Insert user within transaction
    let insert_request = UserInsertRequest {
        id: Uint32::from(104),
        name: "AgentEve".into(),
        email: "agent.eve@example.com".into(),
    };
    client
        .insert::<User>(
            User::table_name(),
            insert_request,
            Some(transaction_id.clone()),
        )
        .await
        .expect("failed to call canister")
        .expect("failed to insert user");

    // Rollback transaction
    client
        .rollback(transaction_id)
        .await
        .expect("failed to call canister")
        .expect("failed to rollback transaction");

    // Verify user was NOT committed
    let query = Query::builder()
        .all()
        .and_where(Filter::eq("id", Value::Uint32(104.into())))
        .build();
    let users = client
        .select::<User>(User::table_name(), query, None)
        .await
        .expect("failed to call canister")
        .expect("failed to query user");

    assert!(users.is_empty());
}
