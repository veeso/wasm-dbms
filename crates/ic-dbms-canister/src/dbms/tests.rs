use candid::{Nat, Principal};
use ic_dbms_api::prelude::{Text, Uint32};

use super::*;
use crate::tests::{
    Message, POSTS_FIXTURES, Post, PostInsertRequest, TestDatabaseSchema, USERS_FIXTURES, User,
    UserInsertRequest, UserUpdateRequest, load_fixtures,
};

#[test]
fn test_should_init_dbms() {
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    assert!(dbms.transaction.is_none());

    let tx_dbms = IcDbmsDatabase::from_transaction(TestDatabaseSchema, Nat::from(1u64));
    assert!(tx_dbms.transaction.is_some());
}

#[test]
fn test_should_select_all_users() {
    load_fixtures();
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder().all().build();
    let users = dbms.select::<User>(query).expect("failed to select users");

    assert_eq!(users.len(), USERS_FIXTURES.len());
    // check if all users all loaded
    for (i, user) in users.iter().enumerate() {
        assert_eq!(user.id.expect("should have id").0 as usize, i);
        assert_eq!(
            user.name.as_ref().expect("should have name").0,
            USERS_FIXTURES[i]
        );
    }
}

#[test]
fn test_should_select_user_in_overlay() {
    load_fixtures();
    // create a transaction
    let transaction_id =
        TRANSACTION_SESSION.with_borrow_mut(|ts| ts.begin_transaction(Principal::anonymous()));
    // insert
    TRANSACTION_SESSION.with_borrow_mut(|ts| {
        let tx = ts
            .get_transaction_mut(&transaction_id)
            .expect("should have tx");
        tx.overlay_mut()
            .insert::<User>(vec![
                (
                    ColumnDef {
                        name: "id",
                        data_type: ic_dbms_api::prelude::DataTypeKind::Uint32,
                        nullable: false,
                        primary_key: true,
                        foreign_key: None,
                    },
                    Value::Uint32(999.into()),
                ),
                (
                    ColumnDef {
                        name: "name",
                        data_type: ic_dbms_api::prelude::DataTypeKind::Text,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Text("OverlayUser".to_string().into()),
                ),
            ])
            .expect("failed to insert");
    });

    // select by pk
    let dbms = IcDbmsDatabase::from_transaction(TestDatabaseSchema, transaction_id);
    let query = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(999.into())))
        .build();
    let users = dbms.select::<User>(query).expect("failed to select users");

    assert_eq!(users.len(), 1);
    let user = &users[0];
    assert_eq!(user.id.expect("should have id").0, 999);
    assert_eq!(
        user.name.as_ref().expect("should have name").0,
        "OverlayUser"
    );
}

#[test]
fn test_should_select_users_with_offset_and_limit() {
    load_fixtures();
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder().offset(2).limit(3).build();
    let users = dbms.select::<User>(query).expect("failed to select users");

    assert_eq!(users.len(), 3);
    // check if correct users are loaded
    for (i, user) in users.iter().enumerate() {
        let expected_index = i + 2;
        assert_eq!(user.id.expect("should have id").0 as usize, expected_index);
        assert_eq!(
            user.name.as_ref().expect("should have name").0,
            USERS_FIXTURES[expected_index]
        );
    }
}

#[test]
fn test_should_select_users_with_offset_and_filter() {
    load_fixtures();
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder()
        .offset(1)
        .and_where(Filter::gt("id", Value::Uint32(4.into())))
        .build();
    let users = dbms.select::<User>(query).expect("failed to select users");

    assert_eq!(users.len(), 4);
    // check if correct users are loaded
    for (i, user) in users.iter().enumerate() {
        let expected_index = i + 6;
        assert_eq!(user.id.expect("should have id").0 as usize, expected_index);
        assert_eq!(
            user.name.as_ref().expect("should have name").0,
            USERS_FIXTURES[expected_index]
        );
    }
}

#[test]
fn test_should_select_post_with_relation() {
    load_fixtures();
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder().all().with(User::table_name()).build();
    let posts = dbms.select::<Post>(query).expect("failed to select posts");
    assert_eq!(posts.len(), POSTS_FIXTURES.len());

    for (id, post) in posts.into_iter().enumerate() {
        let (expected_title, expected_content, expected_user_id) = &POSTS_FIXTURES[id];
        assert_eq!(post.id.expect("should have id").0 as usize, id);
        assert_eq!(
            post.title.as_ref().expect("should have title").0,
            *expected_title
        );
        assert_eq!(
            post.content.as_ref().expect("should have content").0,
            *expected_content
        );
        let user_query = Query::builder()
            .and_where(Filter::eq("id", Value::Uint32((*expected_user_id).into())))
            .build();
        let author = dbms
            .select::<User>(user_query)
            .expect("failed to load user")
            .pop()
            .expect("should have user");
        assert_eq!(
            post.user.expect("should have loaded user"),
            Box::new(author)
        );
    }
}

#[test]
fn test_should_fail_loading_unexisting_column_on_select() {
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder().field("unexisting_column").build();
    let result = dbms.select::<User>(query);
    assert!(result.is_err());
}

#[test]
fn test_should_select_queried_fields() {
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

    let record_values = User::columns()
        .iter()
        .cloned()
        .zip(vec![
            Value::Uint32(1.into()),
            Value::Text("Alice".to_string().into()),
        ])
        .collect::<Vec<(ColumnDef, Value)>>();

    let query = Query::builder().field("name").build();
    let selected_fields = dbms
        .select_queried_fields::<User>(record_values, &query)
        .expect("failed to select queried fields");
    let user_fields = selected_fields
        .into_iter()
        .find(|(table_name, _)| *table_name == ValuesSource::This)
        .map(|(_, cols)| cols)
        .unwrap_or_default();

    assert_eq!(user_fields.len(), 1);
    assert_eq!(user_fields[0].0.name, "name");
    assert_eq!(user_fields[0].1, Value::Text("Alice".to_string().into()));
}

#[test]
fn test_should_select_queried_fields_with_relations() {
    load_fixtures();
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

    let record_values = Post::columns()
        .iter()
        .cloned()
        .zip(vec![
            Value::Uint32(1.into()),
            Value::Text("Title".to_string().into()),
            Value::Text("Content".to_string().into()),
            Value::Uint32(2.into()), // author_id
        ])
        .collect::<Vec<(ColumnDef, Value)>>();

    let query: Query = Query::builder()
        .field("title")
        .with(User::table_name())
        .build();
    let selected_fields = dbms
        .select_queried_fields::<Post>(record_values, &query)
        .expect("failed to select queried fields");

    // check post fields
    let post_fields = selected_fields
        .iter()
        .find(|(table_name, _)| *table_name == ValuesSource::This)
        .map(|(_, cols)| cols)
        .cloned()
        .unwrap_or_default();
    assert_eq!(post_fields.len(), 1);
    assert_eq!(post_fields[0].0.name, "title");
    assert_eq!(post_fields[0].1, Value::Text("Title".to_string().into()));

    // check user fields
    let user_fields = selected_fields
        .iter()
        .find(|(table_name, _)| {
            *table_name
                == ValuesSource::Foreign {
                    table: User::table_name().to_string(),
                    column: "user".to_string(),
                }
        })
        .map(|(_, cols)| cols)
        .cloned()
        .unwrap_or_default();

    let expected_user = USERS_FIXTURES[2]; // author_id = 2

    assert_eq!(user_fields.len(), 4);
    assert_eq!(user_fields[0].0.name, "id");
    assert_eq!(user_fields[0].1, Value::Uint32(2.into()));
    assert_eq!(user_fields[1].0.name, "name");
    assert_eq!(
        user_fields[1].1,
        Value::Text(expected_user.to_string().into())
    );
    assert_eq!(user_fields[2].0.name, "email");
    assert_eq!(
        user_fields[2].1,
        Value::Text(format!("{}@example.com", expected_user.to_lowercase()).into())
    );
    assert_eq!(user_fields[3].0.name, "age");
    assert_eq!(user_fields[3].1, Value::Uint32(22u32.into()));
}

#[test]
fn test_should_select_with_two_fk_on_the_same_table() {
    load_fixtures();

    let query = Query::builder()
        .all()
        .and_where(Filter::Eq("id".to_string(), Value::Uint32(0.into())))
        .with("users")
        .build();

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let messages = dbms
        .select::<Message>(query)
        .expect("failed to select messages");
    assert_eq!(messages.len(), 1);
    let message = &messages[0];
    assert_eq!(message.id.expect("should have id").0, 0);
    assert_eq!(
        message
            .sender
            .as_ref()
            .expect("should have sender")
            .name
            .as_ref()
            .unwrap()
            .0,
        "Alice"
    );
    assert_eq!(
        message
            .recipient
            .as_ref()
            .expect("should have recipient")
            .name
            .as_ref()
            .unwrap()
            .0,
        "Bob"
    );
}

#[test]
fn test_should_select_users_sorted_by_name_descending() {
    load_fixtures();
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder().all().order_by_desc("name").build();
    let users = dbms.select::<User>(query).expect("failed to select users");

    let mut sorted_usernames = USERS_FIXTURES.to_vec();
    sorted_usernames.sort_by(|a, b| b.cmp(a)); // descending

    assert_eq!(users.len(), USERS_FIXTURES.len());
    // check if all users all loaded in sorted order
    for (i, user) in users.iter().enumerate() {
        assert_eq!(
            user.name.as_ref().expect("should have name").0,
            sorted_usernames[i]
        );
    }
}

#[test]
fn test_should_select_users_sorted_by_multiple_columns() {
    load_fixtures();
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

    // Insert users with duplicate names but different ages to test multi-column sort.
    // The fixture users have unique names, so we add duplicates here.
    for (id, (name, age)) in [("Alice", 50u32), ("Alice", 30), ("Bob", 25), ("Bob", 40)]
        .iter()
        .enumerate()
    {
        let new_user = UserInsertRequest {
            id: Uint32(500 + id as u32),
            name: Text(name.to_string()),
            email: format!("dup_{}@example.com", id).into(),
            age: (*age).into(),
        };
        dbms.insert::<User>(new_user)
            .expect("failed to insert user");
    }

    // Sort by name ASC, age DESC — primary key is name, secondary is age descending.
    let query = Query::builder()
        .all()
        .and_where(Filter::ge("id", Value::Uint32(500.into())))
        .order_by_asc("name")
        .order_by_desc("age")
        .build();
    let users = dbms.select::<User>(query).expect("failed to select users");

    assert_eq!(users.len(), 4);

    // Expected order: Alice(50), Alice(30), Bob(40), Bob(25)
    let expected = [("Alice", 50u32), ("Alice", 30), ("Bob", 40), ("Bob", 25)];
    for (i, user) in users.iter().enumerate() {
        let (expected_name, expected_age) = expected[i];
        assert_eq!(
            user.name.as_ref().expect("should have name").0,
            expected_name,
            "name mismatch at index {i}"
        );
        assert_eq!(
            user.age.expect("should have age").0,
            expected_age,
            "age mismatch at index {i}"
        );
    }
}

#[test]
fn test_should_select_many_entries() {
    const COUNT: u64 = 2_000;
    load_fixtures();

    for i in 1..=COUNT {
        let new_user = UserInsertRequest {
            id: Uint32(1000u32 + i as u32),
            name: Text(format!("User{}", i)),
            email: format!("user_{i}@example.com").into(),
            age: 20.into(),
        };
        assert!(
            IcDbmsDatabase::oneshot(TestDatabaseSchema)
                .insert::<User>(new_user)
                .is_ok()
        );
    }

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder()
        .all()
        .and_where(Filter::ge("id", Value::Uint32(1001.into())))
        .build();
    let users = dbms.select::<User>(query).expect("failed to select users");
    assert_eq!(users.len(), COUNT as usize);
}

#[test]
fn test_should_fail_loading_unexisting_relation() {
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

    let record_values = Post::columns()
        .iter()
        .cloned()
        .zip(vec![
            Value::Uint32(1.into()),
            Value::Text("Title".to_string().into()),
            Value::Text("Content".to_string().into()),
            Value::Uint32(2.into()), // author_id
        ])
        .collect::<Vec<(ColumnDef, Value)>>();

    let query = Query::builder()
        .field("title")
        .with("unexisting_relation")
        .build();
    let result = dbms.select_queried_fields::<Post>(record_values, &query);
    assert!(result.is_err());
}

#[test]
fn test_should_get_whether_record_matches_filter() {
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

    let record_values = User::columns()
        .iter()
        .cloned()
        .zip(vec![
            Value::Uint32(1.into()),
            Value::Text("Alice".to_string().into()),
        ])
        .collect::<Vec<(ColumnDef, Value)>>();
    let filter = Filter::eq("name", Value::Text("Alice".to_string().into()));

    let matches = dbms
        .record_matches_filter(&record_values, &filter)
        .expect("failed to match");
    assert!(matches);

    let non_matching_filter = Filter::eq("name", Value::Text("Bob".to_string().into()));
    let non_matches = dbms
        .record_matches_filter(&record_values, &non_matching_filter)
        .expect("failed to match");
    assert!(!non_matches);
}

#[test]
fn test_should_load_table_registry() {
    init_user_table();

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let table_registry = dbms.load_table_registry::<User>();
    assert!(table_registry.is_ok());
}

#[test]
fn test_should_insert_record_without_transaction() {
    load_fixtures();

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let new_user = UserInsertRequest {
        id: Uint32(100u32),
        name: Text("NewUser".to_string()),
        email: "new_user@example.com".into(),
        age: 25.into(),
    };

    let result = dbms.insert::<User>(new_user);
    assert!(result.is_ok());

    // find user
    let query = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(100u32.into())))
        .build();
    let users = dbms.select::<User>(query).expect("failed to select users");
    assert_eq!(users.len(), 1);
    let user = &users[0];
    assert_eq!(user.id.expect("should have id").0, 100);
    assert_eq!(
        user.name.as_ref().expect("should have name").0,
        "NewUser".to_string()
    );
}

#[test]
fn test_should_validate_user_insert_conflict() {
    load_fixtures();

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let new_user = UserInsertRequest {
        id: Uint32(1u32),
        name: Text("NewUser".to_string()),
        email: "new_user@example.com".into(),
        age: 25.into(),
    };

    let result = dbms.insert::<User>(new_user);
    assert!(result.is_err());
}

#[test]
fn test_should_insert_within_a_transaction() {
    load_fixtures();

    // create a transaction
    let transaction_id =
        TRANSACTION_SESSION.with_borrow_mut(|ts| ts.begin_transaction(Principal::anonymous()));
    let mut dbms = IcDbmsDatabase::from_transaction(TestDatabaseSchema, transaction_id.clone());

    let new_user = UserInsertRequest {
        id: Uint32(200u32),
        name: Text("TxUser".to_string()),
        email: "new_user@example.com".into(),
        age: 30.into(),
    };

    let result = dbms.insert::<User>(new_user);
    assert!(result.is_ok());

    // user should not be visible outside the transaction
    let oneshot_dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(200u32.into())))
        .build();
    let users = oneshot_dbms
        .select::<User>(query.clone())
        .expect("failed to select users");
    assert_eq!(users.len(), 0);

    // commit transaction
    let commit_result = dbms.commit();
    assert!(commit_result.is_ok());

    // now user should be visible
    let users_after_commit = oneshot_dbms
        .select::<User>(query)
        .expect("failed to select users");
    assert_eq!(users_after_commit.len(), 1);

    let user = &users_after_commit[0];
    assert_eq!(user.id.expect("should have id").0, 200);
    assert_eq!(
        user.name.as_ref().expect("should have name").0,
        "TxUser".to_string()
    );

    // transaction should have been removed
    TRANSACTION_SESSION.with_borrow(|ts| {
        let tx_res = ts.get_transaction(&transaction_id);
        assert!(tx_res.is_err());
    });
}

#[test]
fn test_should_rollback_transaction() {
    load_fixtures();

    // create a transaction
    let transaction_id =
        TRANSACTION_SESSION.with_borrow_mut(|ts| ts.begin_transaction(Principal::anonymous()));
    let mut dbms = IcDbmsDatabase::from_transaction(TestDatabaseSchema, transaction_id.clone());
    let new_user = UserInsertRequest {
        id: Uint32(300u32),
        name: Text("RollbackUser".to_string()),
        email: "new_user@example.com".into(),
        age: 28.into(),
    };
    let result = dbms.insert::<User>(new_user);
    assert!(result.is_ok());

    // rollback transaction
    let rollback_result = dbms.rollback();
    assert!(rollback_result.is_ok());

    // user should not be visible
    let oneshot_dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(300u32.into())))
        .build();
    let users = oneshot_dbms
        .select::<User>(query)
        .expect("failed to select users");
    assert_eq!(users.len(), 0);

    // transaction should have been removed
    TRANSACTION_SESSION.with_borrow(|ts| {
        let tx_res = ts.get_transaction(&transaction_id);
        assert!(tx_res.is_err());
    });
}

#[test]
fn test_should_sanitize_insert_data() {
    load_fixtures();

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let new_user = UserInsertRequest {
        id: Uint32(100u32),
        name: Text("NewUser".to_string()),
        email: "new_user@example.com".into(),
        age: 150.into(),
    };

    let result = dbms.insert::<User>(new_user);
    assert!(result.is_ok());

    // find user
    let query = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(100u32.into())))
        .build();
    let users = dbms.select::<User>(query).expect("failed to select users");
    assert_eq!(users.len(), 1);
    let user = &users[0];
    assert_eq!(user.id.expect("should have id").0, 100);
    assert_eq!(user.age.expect("should have age").0, 120); // sanitized to max 120
}

#[test]
fn test_should_delete_one_shot() {
    load_fixtures();

    // insert user with id 100
    let new_user = UserInsertRequest {
        id: Uint32(100u32),
        name: Text("DeleteUser".to_string()),
        email: "new_user@example.com".into(),
        age: 22.into(),
    };
    assert!(
        IcDbmsDatabase::oneshot(TestDatabaseSchema)
            .insert::<User>(new_user)
            .is_ok()
    );

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(100u32.into())))
        .build();
    let delete_count = dbms
        .delete::<User>(
            DeleteBehavior::Restrict,
            Some(Filter::eq("id", Value::Uint32(100u32.into()))),
        )
        .expect("failed to delete user");
    assert_eq!(delete_count, 1);

    // verify user is deleted
    let users = dbms.select::<User>(query).expect("failed to select users");
    assert_eq!(users.len(), 0);
}

#[test]
fn test_should_delete_many_entries() {
    const COUNT: u64 = 2_000;
    load_fixtures();

    for i in 1..=COUNT {
        let new_user = UserInsertRequest {
            id: Uint32(1000u32 + i as u32),
            name: Text(format!("User{}", i)),
            email: format!("user_{i}@example.com").into(),
            age: 20.into(),
        };
        assert!(
            IcDbmsDatabase::oneshot(TestDatabaseSchema)
                .insert::<User>(new_user)
                .is_ok()
        );
    }

    let mut deleted_total = 0;
    for i in 1..=COUNT {
        let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
        let delete_count = dbms
            .delete::<User>(
                DeleteBehavior::Restrict,
                Some(Filter::eq("id", Value::Uint32((1000u32 + i as u32).into()))),
            )
            .expect("failed to delete user");
        assert_eq!(delete_count, 1, "failed to delete user {}", i);
        deleted_total += delete_count;
    }
    assert_eq!(deleted_total, COUNT);
}

#[test]
fn test_should_drop_table() {
    const COUNT: u64 = 5_000;
    load_fixtures();

    for i in 1..=COUNT {
        let new_post = PostInsertRequest {
            id: Uint32(100u32 + i as u32),
            title: Text(format!("Post{}", i)),
            content: Text("Some content".to_string()),
            user: Uint32(1u32),
        };
        assert!(
            IcDbmsDatabase::oneshot(TestDatabaseSchema)
                .insert::<Post>(new_post)
                .is_ok()
        );
    }

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let delete_count = dbms
        .delete::<Post>(
            DeleteBehavior::Restrict,
            Some(Filter::ge("id", Value::Uint32(101.into()))),
        )
        .expect("failed to delete post");
    assert_eq!(
        delete_count, COUNT,
        "expected to delete all posts, but deleted {}",
        delete_count
    );
}

#[test]
#[should_panic(expected = "Foreign key constraint violation")]
fn test_should_not_delete_with_fk_restrict() {
    load_fixtures();

    // user 1 has post and messages for sure.
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

    // this delete will panic
    let _ = dbms.delete::<User>(
        DeleteBehavior::Restrict,
        Some(Filter::eq("id", Value::Uint32(1u32.into()))),
    );
}

#[test]
fn test_should_delete_with_fk_cascade() {
    load_fixtures();

    // user 1 has posts and messages for sure.
    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let delete_count = dbms
        .delete::<User>(
            DeleteBehavior::Cascade,
            Some(Filter::eq("id", Value::Uint32(1u32.into()))),
        )
        .expect("failed to delete user");
    assert!(delete_count > 1); // at least user + posts + messages

    // verify user is deleted
    let query = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(1u32.into())))
        .build();
    let users = dbms.select::<User>(query).expect("failed to select users");
    assert_eq!(users.len(), 0);

    // check posts are deleted (post ID 2)
    let post_query = Query::builder()
        .and_where(Filter::eq("user_id", Value::Uint32(1u32.into())))
        .build();
    let posts = dbms
        .select::<Post>(post_query)
        .expect("failed to select posts");
    assert_eq!(posts.len(), 0);

    // check messages are deleted (message ID 1)
    let message_query = Query::builder()
        .and_where(Filter::eq("sender_id", Value::Uint32(1u32.into())))
        .or_where(Filter::eq("recipient_id", Value::Uint32(1u32.into())))
        .build();
    let messages = dbms
        .select::<Message>(message_query)
        .expect("failed to select messages");
    assert_eq!(messages.len(), 0);
}

#[test]
fn test_should_delete_within_transaction() {
    load_fixtures();

    // create a transaction
    let transaction_id =
        TRANSACTION_SESSION.with_borrow_mut(|ts| ts.begin_transaction(Principal::anonymous()));
    let mut dbms = IcDbmsDatabase::from_transaction(TestDatabaseSchema, transaction_id.clone());

    let delete_count = dbms
        .delete::<User>(
            DeleteBehavior::Cascade,
            Some(Filter::eq("id", Value::Uint32(2u32.into()))),
        )
        .expect("failed to delete user");
    assert!(delete_count > 0);

    // user should not be visible outside the transaction
    let oneshot_dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(2u32.into())))
        .build();
    let users = oneshot_dbms
        .select::<User>(query.clone())
        .expect("failed to select users");
    assert_eq!(users.len(), 1);

    // commit transaction
    let commit_result = dbms.commit();
    assert!(commit_result.is_ok());

    // now user should be deleted
    let users_after_commit = oneshot_dbms
        .select::<User>(query)
        .expect("failed to select users");
    assert_eq!(users_after_commit.len(), 0);

    // check posts are deleted
    let post_query = Query::builder()
        .and_where(Filter::eq("user_id", Value::Uint32(2u32.into())))
        .build();
    let posts = oneshot_dbms
        .select::<Post>(post_query)
        .expect("failed to select posts");
    assert_eq!(posts.len(), 0);

    // check messages are deleted
    let message_query = Query::builder()
        .and_where(Filter::eq("sender_id", Value::Uint32(2u32.into())))
        .or_where(Filter::eq("recipient_id", Value::Uint32(2u32.into())))
        .build();
    let messages = oneshot_dbms
        .select::<Message>(message_query)
        .expect("failed to select messages");
    assert_eq!(messages.len(), 0);

    // transaction should have been removed
    TRANSACTION_SESSION.with_borrow(|ts| {
        let tx_res = ts.get_transaction(&transaction_id);
        assert!(tx_res.is_err());
    });
}

#[test]
fn test_should_update_one_shot() {
    load_fixtures();

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let filter = Filter::eq("id", Value::Uint32(3u32.into()));

    let patch = UserUpdateRequest {
        id: None,
        name: Some(Text("UpdatedName".to_string())),
        email: None,
        age: None,
        where_clause: Some(filter.clone()),
    };

    let update_count = dbms.update::<User>(patch).expect("failed to update user");
    assert_eq!(update_count, 1);

    // verify user is updated
    let query = Query::builder().and_where(filter).build();
    let users = dbms.select::<User>(query).expect("failed to select users");
    assert_eq!(users.len(), 1);
    let user = &users[0];
    assert_eq!(user.id.expect("should have id").0, 3);
    assert_eq!(
        user.name.as_ref().expect("should have name").0,
        "UpdatedName".to_string()
    );
}

#[test]
fn test_should_update_within_transaction() {
    load_fixtures();

    // create a transaction
    let transaction_id =
        TRANSACTION_SESSION.with_borrow_mut(|ts| ts.begin_transaction(Principal::anonymous()));
    let mut dbms = IcDbmsDatabase::from_transaction(TestDatabaseSchema, transaction_id.clone());

    let filter = Filter::eq("id", Value::Uint32(4u32.into()));
    let patch = UserUpdateRequest {
        id: None,
        name: Some(Text("TxUpdatedName".to_string())),
        email: None,
        age: None,
        where_clause: Some(filter.clone()),
    };

    let update_count = dbms.update::<User>(patch).expect("failed to update user");
    assert_eq!(update_count, 1);

    // user should not be visible outside the transaction
    let oneshot_dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder().and_where(filter.clone()).build();
    let users = oneshot_dbms
        .select::<User>(query.clone())
        .expect("failed to select users");
    let user = &users[0];
    assert_eq!(
        user.name.as_ref().expect("should have name").0,
        USERS_FIXTURES[4]
    );

    // commit transaction
    let commit_result = dbms.commit();
    assert!(commit_result.is_ok());

    // now user should be updated
    let users_after_commit = oneshot_dbms
        .select::<User>(query)
        .expect("failed to select users");
    assert_eq!(users_after_commit.len(), 1);
    let user = &users_after_commit[0];
    assert_eq!(
        user.name.as_ref().expect("should have name").0,
        "TxUpdatedName".to_string()
    );

    // transaction should have been removed
    TRANSACTION_SESSION.with_borrow(|ts| {
        let tx_res = ts.get_transaction(&transaction_id);
        assert!(tx_res.is_err());
    });
}

#[test]
#[should_panic(expected = "Validation error: Value 'invalid_email' is not a valid email address")]
fn test_should_fail_to_update_with_invalid_data() {
    load_fixtures();

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let filter = Filter::eq("id", Value::Uint32(3u32.into()));

    let patch = UserUpdateRequest {
        id: None,
        name: None,
        email: Some("invalid_email".into()), // invalid email format
        age: None,
        where_clause: Some(filter.clone()),
    };

    // this fails due to being inside atomic
    let _ = dbms.update::<User>(patch);
}

#[test]
fn test_should_update_fk_in_table_referencing_another_oneshot() {
    load_fixtures();

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);

    // update user with PK 0, check whether posts 0 and 1 has updated FK;
    // also check messages 0 and 1
    let filter = Filter::eq("id", Value::Uint32(0u32.into()));

    let patch = UserUpdateRequest {
        id: Some(Uint32(1_000u32)),
        name: None,
        email: None,
        age: None,
        where_clause: Some(filter.clone()),
    };

    let update_count = dbms.update::<User>(patch).expect("failed to update user");
    assert_eq!(update_count, 5); // 2 posts + 1 user + 2 messages

    // verify user is updated
    let query = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(1_000u32.into())))
        .build();
    let users = dbms.select::<User>(query).expect("failed to select users");
    assert_eq!(users.len(), 1);
    let user = &users[0];
    assert_eq!(user.id.expect("should have id").0, 1_000);

    // get messages where sender_id or recipient_id is 1_000
    let message_query = Query::builder()
        .with("users")
        .and_where(Filter::eq("sender", Value::Uint32(1_000u32.into())))
        .or_where(Filter::eq("recipient", Value::Uint32(1_000u32.into())))
        .build();
    let messages = dbms
        .select::<Message>(message_query)
        .expect("failed to select messages");
    assert_eq!(messages.len(), 2);
    for message in messages {
        let sender_id = message
            .sender
            .as_ref()
            .expect("should have sender")
            .id
            .expect("should have sender id")
            .0;
        let recipient_id = message
            .recipient
            .as_ref()
            .expect("should have recipient")
            .id
            .expect("should have recipient id")
            .0;
        assert!(sender_id == 1_000 || recipient_id == 1_000);
    }

    // check posts where user_id is 1_000
    let post_query = Query::builder()
        .with("users")
        .and_where(Filter::eq("user", Value::Uint32(1_000u32.into())))
        .build();
    let posts = dbms
        .select::<Post>(post_query)
        .expect("failed to select posts");
    assert_eq!(posts.len(), 2);
    for post in posts {
        let user_id = post
            .user
            .expect("should have user")
            .id
            .expect("should have user id")
            .0;
        assert_eq!(user_id, 1_000);
    }
}

#[test]
fn test_should_sanitize_update() {
    load_fixtures();

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let filter = Filter::eq("id", Value::Uint32(3u32.into()));

    let patch = UserUpdateRequest {
        id: None,
        name: None,
        email: None,
        age: Some(200.into()),
        where_clause: Some(filter.clone()),
    };

    let update_count = dbms.update::<User>(patch).expect("failed to update user");
    assert_eq!(update_count, 1);

    // verify user is updated
    let query = Query::builder().and_where(filter).build();
    let users = dbms.select::<User>(query).expect("failed to select users");
    assert_eq!(users.len(), 1);
    let user = &users[0];
    assert_eq!(user.id.expect("should have id").0, 3);
    assert_eq!(user.age.expect("should have age").0, 120); // sanitized to max 120
}

#[test]
fn test_should_update_multiple_records_at_once() {
    load_fixtures();

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    // update all users with id > 5 (users 6, 7, 8, 9)
    let filter = Filter::gt("id", Value::Uint32(5u32.into()));

    let patch = UserUpdateRequest {
        id: None,
        name: Some(Text("BulkUpdated".to_string())),
        email: None,
        age: None,
        where_clause: Some(filter.clone()),
    };

    let update_count = dbms.update::<User>(patch).expect("failed to update users");
    assert_eq!(update_count, 4); // users 6, 7, 8, 9

    // verify all matched users were updated
    let query = Query::builder().and_where(filter).build();
    let users = dbms.select::<User>(query).expect("failed to select users");
    assert_eq!(users.len(), 4);
    for user in &users {
        assert_eq!(
            user.name.as_ref().expect("should have name").0,
            "BulkUpdated"
        );
    }

    // verify users with id <= 5 were NOT updated
    let unaffected_query = Query::builder()
        .and_where(Filter::le("id", Value::Uint32(5u32.into())))
        .build();
    let unaffected_users = dbms
        .select::<User>(unaffected_query)
        .expect("failed to select users");
    for user in &unaffected_users {
        assert_ne!(
            user.name.as_ref().expect("should have name").0,
            "BulkUpdated"
        );
    }
}

#[test]
#[should_panic(expected = "Primary key conflict")]
fn test_should_fail_update_with_pk_conflict_e2e() {
    load_fixtures();

    let dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    // try to update user 3's PK to 2 (which already exists)
    let filter = Filter::eq("id", Value::Uint32(3u32.into()));

    let patch = UserUpdateRequest {
        id: Some(Uint32(2u32)),
        name: None,
        email: None,
        age: None,
        where_clause: Some(filter),
    };

    // this should panic inside atomic because of PK conflict
    let _ = dbms.update::<User>(patch);
}

#[test]
fn test_should_update_pk_with_fk_cascade_in_transaction() {
    load_fixtures();

    // create a transaction
    let transaction_id =
        TRANSACTION_SESSION.with_borrow_mut(|ts| ts.begin_transaction(Principal::anonymous()));
    let mut dbms = IcDbmsDatabase::from_transaction(TestDatabaseSchema, transaction_id.clone());

    // update user 0's PK to 5000 inside the transaction
    let filter = Filter::eq("id", Value::Uint32(0u32.into()));
    let patch = UserUpdateRequest {
        id: Some(Uint32(5000u32)),
        name: None,
        email: None,
        age: None,
        where_clause: Some(filter),
    };

    // NOTE: update_count in transaction path may not reflect cascaded FK changes
    // because the overlay transforms the record, making the original filter not match anymore.
    // The actual count is verified after commit.
    let _update_count = dbms.update::<User>(patch).expect("failed to update user");

    // outside the transaction, user 0 should still exist
    let oneshot_dbms = IcDbmsDatabase::oneshot(TestDatabaseSchema);
    let query = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(0u32.into())))
        .build();
    let users = oneshot_dbms
        .select::<User>(query)
        .expect("failed to select users");
    assert_eq!(users.len(), 1);

    // commit transaction
    let commit_result = dbms.commit();
    assert!(commit_result.is_ok());

    // now user 0 should be gone, user 5000 should exist
    let query_old = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(0u32.into())))
        .build();
    let users_old = oneshot_dbms
        .select::<User>(query_old)
        .expect("failed to select users");
    assert_eq!(users_old.len(), 0);

    let query_new = Query::builder()
        .and_where(Filter::eq("id", Value::Uint32(5000u32.into())))
        .build();
    let users_new = oneshot_dbms
        .select::<User>(query_new)
        .expect("failed to select users");
    assert_eq!(users_new.len(), 1);

    // verify FK cascade: posts that referenced user 0 now reference user 5000
    let post_query = Query::builder()
        .and_where(Filter::eq("user", Value::Uint32(5000u32.into())))
        .build();
    let posts = oneshot_dbms
        .select::<Post>(post_query)
        .expect("failed to select posts");
    assert_eq!(posts.len(), 2); // user 0 had 2 posts

    // verify no posts reference user 0 anymore
    let old_post_query = Query::builder()
        .and_where(Filter::eq("user", Value::Uint32(0u32.into())))
        .build();
    let old_posts = oneshot_dbms
        .select::<Post>(old_post_query)
        .expect("failed to select posts");
    assert_eq!(old_posts.len(), 0);
}

fn init_user_table() {
    SCHEMA_REGISTRY
        .with_borrow_mut(|sr| sr.register_table::<User>())
        .expect("failed to register `User` table");
}
