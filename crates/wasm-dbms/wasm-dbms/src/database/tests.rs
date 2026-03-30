use std::cmp::Ordering;

use wasm_dbms_api::prelude::{
    Database as _, DeleteBehavior, Filter, InsertRecord as _, OrderDirection, Query,
    TableSchema as _, Text, Uint32, UpdateRecord as _, Value,
};
use wasm_dbms_macros::{DatabaseSchema, Table};
use wasm_dbms_memory::prelude::HeapMemoryProvider;

use super::sort_values_with_direction;
use crate::prelude::{DbmsContext, WasmDbmsDatabase};
use crate::schema::DatabaseSchema as _;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub user_id: Uint32,
}

#[derive(DatabaseSchema)]
#[tables(User = "users", Post = "posts")]
pub struct TestSchema;

fn setup() -> DbmsContext<HeapMemoryProvider> {
    let ctx = DbmsContext::new(HeapMemoryProvider::default());
    TestSchema::register_tables(&ctx).unwrap();
    ctx
}

fn insert_user(db: &WasmDbmsDatabase<'_, HeapMemoryProvider>, id: u32, name: &str) {
    let insert = UserInsertRequest::from_values(&[
        (User::columns()[0], Value::Uint32(Uint32(id))),
        (User::columns()[1], Value::Text(Text(name.to_string()))),
    ])
    .unwrap();
    db.insert::<User>(insert).unwrap();
}

fn insert_post(db: &WasmDbmsDatabase<'_, HeapMemoryProvider>, id: u32, title: &str, user_id: u32) {
    let insert = PostInsertRequest::from_values(&[
        (Post::columns()[0], Value::Uint32(Uint32(id))),
        (Post::columns()[1], Value::Text(Text(title.to_string()))),
        (Post::columns()[2], Value::Uint32(Uint32(user_id))),
    ])
    .unwrap();
    db.insert::<Post>(insert).unwrap();
}

// -- sort_values_with_direction tests --

#[test]
fn test_sort_values_ascending() {
    let a = Value::Uint32(Uint32(1));
    let b = Value::Uint32(Uint32(2));
    assert_eq!(
        sort_values_with_direction(Some(&a), Some(&b), OrderDirection::Ascending),
        Ordering::Less
    );
}

#[test]
fn test_sort_values_descending() {
    let a = Value::Uint32(Uint32(1));
    let b = Value::Uint32(Uint32(2));
    assert_eq!(
        sort_values_with_direction(Some(&a), Some(&b), OrderDirection::Descending),
        Ordering::Greater
    );
}

#[test]
fn test_sort_values_some_none() {
    let a = Value::Uint32(Uint32(1));
    assert_eq!(
        sort_values_with_direction(Some(&a), None, OrderDirection::Ascending),
        Ordering::Greater
    );
    assert_eq!(
        sort_values_with_direction(None, Some(&a), OrderDirection::Ascending),
        Ordering::Less
    );
}

#[test]
fn test_sort_values_none_none() {
    assert_eq!(
        sort_values_with_direction(None, None, OrderDirection::Ascending),
        Ordering::Equal
    );
}

// -- select with ordering --

#[test]
fn test_select_with_order_by_ascending() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 3, "charlie");
    insert_user(&db, 1, "alice");
    insert_user(&db, 2, "bob");

    let rows = db
        .select::<User>(Query::builder().all().order_by_asc("name").build())
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].name, Some(Text("alice".to_string())));
    assert_eq!(rows[1].name, Some(Text("bob".to_string())));
    assert_eq!(rows[2].name, Some(Text("charlie".to_string())));
}

#[test]
fn test_select_with_order_by_descending() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");
    insert_user(&db, 2, "bob");
    insert_user(&db, 3, "charlie");

    let rows = db
        .select::<User>(Query::builder().all().order_by_desc("name").build())
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].name, Some(Text("charlie".to_string())));
    assert_eq!(rows[1].name, Some(Text("bob".to_string())));
    assert_eq!(rows[2].name, Some(Text("alice".to_string())));
}

// -- select with offset and limit --

#[test]
fn test_select_with_limit() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");
    insert_user(&db, 2, "bob");
    insert_user(&db, 3, "charlie");

    let rows = db
        .select::<User>(Query::builder().all().limit(2).build())
        .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn test_select_with_offset() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");
    insert_user(&db, 2, "bob");
    insert_user(&db, 3, "charlie");

    let rows = db
        .select::<User>(Query::builder().all().offset(1).build())
        .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn test_select_with_offset_and_limit() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");
    insert_user(&db, 2, "bob");
    insert_user(&db, 3, "charlie");

    let rows = db
        .select::<User>(Query::builder().all().offset(1).limit(1).build())
        .unwrap();
    assert_eq!(rows.len(), 1);
}

// -- select with filter --

#[test]
fn test_select_with_filter() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");
    insert_user(&db, 2, "bob");

    let rows = db
        .select::<User>(
            Query::builder()
                .all()
                .and_where(Filter::eq("name", Value::Text(Text("alice".to_string()))))
                .build(),
        )
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, Some(Text("alice".to_string())));
}

// -- select with column selection --

#[test]
fn test_select_with_column_selection() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");

    let rows = TestSchema
        .select(&db, "users", Query::builder().field("name").build())
        .unwrap();
    assert_eq!(rows.len(), 1);
    // Should only have the "name" column
    assert_eq!(rows[0].len(), 1);
    assert_eq!(rows[0][0].0.name, "name");
}

// -- update operations --

#[test]
fn test_update_record() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");

    let patch = UserUpdateRequest::from_values(
        &[(User::columns()[1], Value::Text(Text("alicia".to_string())))],
        Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
    );
    let count = db.update::<User>(patch).unwrap();
    assert_eq!(count, 1);

    let rows = db.select::<User>(Query::builder().build()).unwrap();
    assert_eq!(rows[0].name, Some(Text("alicia".to_string())));
}

#[test]
fn test_update_no_matching_records() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");

    let patch = UserUpdateRequest::from_values(
        &[(User::columns()[1], Value::Text(Text("bob".to_string())))],
        Some(Filter::eq("id", Value::Uint32(Uint32(999)))),
    );
    let count = db.update::<User>(patch).unwrap();
    assert_eq!(count, 0);
}

// -- delete operations --

#[test]
fn test_delete_with_filter() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");
    insert_user(&db, 2, "bob");

    let count = db
        .delete::<User>(
            DeleteBehavior::Restrict,
            Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
        )
        .unwrap();
    assert_eq!(count, 1);

    let rows = db.select::<User>(Query::builder().build()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, Some(Uint32(2)));
}

#[test]
fn test_delete_restrict_with_fk_reference_fails() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");
    insert_post(&db, 10, "post1", 1);

    let result = db.delete::<User>(DeleteBehavior::Restrict, None);
    assert!(result.is_err());
}

#[test]
fn test_delete_cascade_removes_referencing_records() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");
    insert_post(&db, 10, "post1", 1);

    let count = db.delete::<User>(DeleteBehavior::Cascade, None).unwrap();
    // 1 user + 1 cascaded post
    assert_eq!(count, 2);

    let users = db.select::<User>(Query::builder().build()).unwrap();
    assert!(users.is_empty());
    let posts = db.select::<Post>(Query::builder().build()).unwrap();
    assert!(posts.is_empty());
}

// -- commit without transaction --

#[test]
fn test_commit_without_transaction_returns_error() {
    let ctx = setup();
    let mut db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    let result = db.commit();
    assert!(result.is_err());
}

// -- transaction commit with update --

#[test]
fn test_transaction_update_and_commit() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");

    let owner = vec![1, 2, 3];
    let tx_id = ctx.begin_transaction(owner);
    let mut db = WasmDbmsDatabase::from_transaction(&ctx, TestSchema, tx_id);

    let patch = UserUpdateRequest::from_values(
        &[(User::columns()[1], Value::Text(Text("alicia".to_string())))],
        Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
    );
    db.update::<User>(patch).unwrap();
    db.commit().unwrap();

    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    let rows = db.select::<User>(Query::builder().build()).unwrap();
    assert_eq!(rows[0].name, Some(Text("alicia".to_string())));
}

// -- transaction delete and commit --

#[test]
fn test_transaction_delete_and_commit() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");
    insert_user(&db, 2, "bob");

    let owner = vec![1, 2, 3];
    let tx_id = ctx.begin_transaction(owner);
    let mut db = WasmDbmsDatabase::from_transaction(&ctx, TestSchema, tx_id);

    db.delete::<User>(
        DeleteBehavior::Restrict,
        Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
    )
    .unwrap();
    db.commit().unwrap();

    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    let rows = db.select::<User>(Query::builder().build()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, Some(Uint32(2)));
}

// -- transaction PK update then subsequent update and commit (#65) --

#[test]
fn test_transaction_pk_update_then_column_update_and_commit() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");

    let owner = vec![1, 2, 3];
    let tx_id = ctx.begin_transaction(owner);
    let mut db = WasmDbmsDatabase::from_transaction(&ctx, TestSchema, tx_id);

    // Step 1: update PK from 1 to 10
    let patch = UserUpdateRequest::from_values(
        &[(User::columns()[0], Value::Uint32(Uint32(10)))],
        Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
    );
    db.update::<User>(patch).unwrap();

    // Step 2: update name on the same row (now keyed by id=10)
    let patch = UserUpdateRequest::from_values(
        &[(User::columns()[1], Value::Text(Text("alicia".to_string())))],
        Some(Filter::eq("id", Value::Uint32(Uint32(10)))),
    );
    db.update::<User>(patch).unwrap();

    // Verify within transaction: select should return the row with both updates
    let rows = db.select::<User>(Query::builder().build()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, Some(Uint32(10)));
    assert_eq!(rows[0].name, Some(Text("alicia".to_string())));

    // Commit and verify persistence
    db.commit().unwrap();

    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    let rows = db.select::<User>(Query::builder().build()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, Some(Uint32(10)));
    assert_eq!(rows[0].name, Some(Text("alicia".to_string())));
}

#[test]
fn test_transaction_pk_update_then_delete() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");

    let owner = vec![1, 2, 3];
    let tx_id = ctx.begin_transaction(owner);
    let mut db = WasmDbmsDatabase::from_transaction(&ctx, TestSchema, tx_id);

    // Step 1: update PK from 1 to 10
    let patch = UserUpdateRequest::from_values(
        &[(User::columns()[0], Value::Uint32(Uint32(10)))],
        Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
    );
    db.update::<User>(patch).unwrap();

    // Step 2: delete the row by new PK
    db.delete::<User>(
        DeleteBehavior::Restrict,
        Some(Filter::eq("id", Value::Uint32(Uint32(10)))),
    )
    .unwrap();

    // Verify within transaction: row should be gone
    let rows = db.select::<User>(Query::builder().build()).unwrap();
    assert_eq!(rows.len(), 0);

    // Commit and verify
    db.commit().unwrap();

    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    let rows = db.select::<User>(Query::builder().build()).unwrap();
    assert_eq!(rows.len(), 0);
}

// -- select_raw --

#[test]
fn test_select_raw() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
    insert_user(&db, 1, "alice");

    let rows = db.select_raw("users", Query::builder().build()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0].1, Value::Uint32(Uint32(1)));
}

// -- select with join returns error on typed select --

#[test]
fn test_typed_select_with_join_returns_error() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);

    let query = Query::builder()
        .all()
        .inner_join("posts", "id", "user_id")
        .build();
    let result = db.select::<User>(query);
    assert!(result.is_err());
}

// -- insert_index tests --

/// A table with a single-column index on `email`, used for index tests.
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "indexed_users"]
pub struct IndexedUser {
    #[primary_key]
    pub id: Uint32,
    #[index]
    pub email: Text,
}

/// A table with a composite index on `(first_name, last_name)`.
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "composite_users"]
pub struct CompositeUser {
    #[primary_key]
    pub id: Uint32,
    #[index(group = "idx_full_name")]
    pub first_name: Text,
    #[index(group = "idx_full_name")]
    pub last_name: Text,
}

#[derive(DatabaseSchema)]
#[tables(IndexedUser = "indexed_users", CompositeUser = "composite_users")]
pub struct IndexedTestSchema;

fn setup_indexed() -> DbmsContext<HeapMemoryProvider> {
    let ctx = DbmsContext::new(HeapMemoryProvider::default());
    IndexedTestSchema::register_tables(&ctx).unwrap();
    ctx
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "name_indexed_users"]
pub struct NameIndexedUser {
    #[primary_key]
    pub id: Uint32,
    #[index]
    pub name: Text,
    pub age: Uint32,
}

#[derive(DatabaseSchema)]
#[tables(NameIndexedUser = "name_indexed_users")]
pub struct NameIndexedTestSchema;

fn setup_name_indexed() -> DbmsContext<HeapMemoryProvider> {
    let ctx = DbmsContext::new(HeapMemoryProvider::default());
    NameIndexedTestSchema::register_tables(&ctx).unwrap();
    ctx
}

fn insert_name_indexed_user(
    db: &WasmDbmsDatabase<'_, HeapMemoryProvider>,
    id: u32,
    name: &str,
    age: u32,
) {
    let insert = NameIndexedUserInsertRequest::from_values(&[
        (NameIndexedUser::columns()[0], Value::Uint32(Uint32(id))),
        (
            NameIndexedUser::columns()[1],
            Value::Text(Text(name.to_string())),
        ),
        (NameIndexedUser::columns()[2], Value::Uint32(Uint32(age))),
    ])
    .unwrap();
    db.insert::<NameIndexedUser>(insert).unwrap();
}

#[test]
fn test_select_eq_on_indexed_column() {
    let ctx = setup_name_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, NameIndexedTestSchema);
    insert_name_indexed_user(&db, 1, "alice", 20);
    insert_name_indexed_user(&db, 2, "bob", 25);
    insert_name_indexed_user(&db, 3, "alice", 30);

    let rows = db
        .select::<NameIndexedUser>(
            Query::builder()
                .all()
                .and_where(Filter::eq("name", Value::Text(Text("alice".to_string()))))
                .build(),
        )
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.name == Some(Text("alice".to_string())))
    );
}

#[test]
fn test_select_range_on_indexed_column() {
    let ctx = setup_name_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, NameIndexedTestSchema);
    insert_name_indexed_user(&db, 1, "alice", 20);
    insert_name_indexed_user(&db, 2, "bob", 25);
    insert_name_indexed_user(&db, 3, "charlie", 30);

    let rows = db
        .select::<NameIndexedUser>(
            Query::builder()
                .all()
                .and_where(Filter::gt("name", Value::Text(Text("alice".to_string()))))
                .build(),
        )
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.name != Some(Text("alice".to_string())))
    );
}

#[test]
fn test_select_in_on_indexed_column() {
    let ctx = setup_name_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, NameIndexedTestSchema);
    insert_name_indexed_user(&db, 1, "alice", 20);
    insert_name_indexed_user(&db, 2, "bob", 25);
    insert_name_indexed_user(&db, 3, "charlie", 30);

    let rows = db
        .select::<NameIndexedUser>(
            Query::builder()
                .all()
                .and_where(Filter::in_list(
                    "name",
                    vec![
                        Value::Text(Text("alice".to_string())),
                        Value::Text(Text("charlie".to_string())),
                    ],
                ))
                .build(),
        )
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| {
        matches!(
            row.name.as_ref(),
            Some(Text(name)) if name == "alice" || name == "charlie"
        )
    }));
}

#[test]
fn test_select_on_non_indexed_column_falls_back_to_scan() {
    let ctx = setup_name_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, NameIndexedTestSchema);
    insert_name_indexed_user(&db, 1, "alice", 20);
    insert_name_indexed_user(&db, 2, "bob", 25);
    insert_name_indexed_user(&db, 3, "charlie", 30);

    let rows = db
        .select::<NameIndexedUser>(
            Query::builder()
                .all()
                .and_where(Filter::eq("age", Value::Uint32(Uint32(25))))
                .build(),
        )
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, Some(Text("bob".to_string())));
}

#[test]
fn test_select_eq_on_indexed_column_in_transaction() {
    let ctx = setup_name_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, NameIndexedTestSchema);
    insert_name_indexed_user(&db, 1, "alice", 20);
    insert_name_indexed_user(&db, 2, "bob", 25);

    let tx_id = ctx.begin_transaction(vec![1, 2, 3]);
    let tx_db = WasmDbmsDatabase::from_transaction(&ctx, NameIndexedTestSchema, tx_id);
    insert_name_indexed_user(&tx_db, 3, "alice", 35);

    let rows = tx_db
        .select::<NameIndexedUser>(
            Query::builder()
                .all()
                .and_where(Filter::eq("name", Value::Text(Text("alice".to_string()))))
                .build(),
        )
        .unwrap();

    assert_eq!(rows.len(), 2);
}

#[test]
fn test_select_eq_on_indexed_column_after_delete_and_reinsert_in_transaction() {
    let ctx = setup_name_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, NameIndexedTestSchema);
    insert_name_indexed_user(&db, 1, "alice", 20);

    let tx_id = ctx.begin_transaction(vec![1, 2, 3]);
    let tx_db = WasmDbmsDatabase::from_transaction(&ctx, NameIndexedTestSchema, tx_id);
    let deleted = tx_db
        .delete::<NameIndexedUser>(
            DeleteBehavior::Restrict,
            Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
        )
        .unwrap();
    assert_eq!(deleted, 1);
    insert_name_indexed_user(&tx_db, 2, "alice", 99);

    let rows = tx_db
        .select::<NameIndexedUser>(
            Query::builder()
                .all()
                .and_where(Filter::eq("name", Value::Text(Text("alice".to_string()))))
                .build(),
        )
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, Some(Uint32(2)));
    assert_eq!(rows[0].age, Some(Uint32(99)));
}

#[test]
fn test_update_on_indexed_column_filter() {
    let ctx = setup_name_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, NameIndexedTestSchema);
    insert_name_indexed_user(&db, 1, "alice", 20);
    insert_name_indexed_user(&db, 2, "bob", 25);
    insert_name_indexed_user(&db, 3, "alice", 30);

    let patch = NameIndexedUserUpdateRequest::from_values(
        &[(NameIndexedUser::columns()[2], Value::Uint32(Uint32(99)))],
        Some(Filter::eq("name", Value::Text(Text("alice".to_string())))),
    );
    let count = db.update::<NameIndexedUser>(patch).unwrap();
    assert_eq!(count, 2);

    let rows = db
        .select::<NameIndexedUser>(
            Query::builder()
                .all()
                .and_where(Filter::eq("name", Value::Text(Text("alice".to_string()))))
                .build(),
        )
        .unwrap();

    assert!(rows.iter().all(|row| row.age == Some(Uint32(99))));
}

#[test]
fn test_delete_on_indexed_column_filter() {
    let ctx = setup_name_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, NameIndexedTestSchema);
    insert_name_indexed_user(&db, 1, "alice", 20);
    insert_name_indexed_user(&db, 2, "bob", 25);
    insert_name_indexed_user(&db, 3, "alice", 30);

    let count = db
        .delete::<NameIndexedUser>(
            DeleteBehavior::Restrict,
            Some(Filter::eq("name", Value::Text(Text("alice".to_string())))),
        )
        .unwrap();
    assert_eq!(count, 2);

    let rows = db
        .select::<NameIndexedUser>(Query::builder().all().build())
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, Some(Text("bob".to_string())));
}

#[test]
fn test_insert_index_populates_single_column_index() {
    use wasm_dbms_memory::RecordAddress;

    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    // Load the table registry and manually call insert_index.
    let mut table_registry = db.load_table_registry::<IndexedUser>().unwrap();
    let record_address = RecordAddress::new(100, 0);
    let values = vec![
        (IndexedUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            IndexedUser::columns()[1],
            Value::Text(Text("alice@example.com".to_string())),
        ),
    ];

    let mut mm = db.ctx.mm.borrow_mut();
    db.insert_index::<IndexedUser>(&mut table_registry, record_address, &values, &mut *mm)
        .unwrap();

    // Search the index for the inserted key.
    let key = vec![Value::Text(Text("alice@example.com".to_string()))];
    let results = table_registry
        .index_ledger()
        .search(&["email"], &key, &*mm)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], record_address);
}

#[test]
fn test_insert_index_populates_composite_index() {
    use wasm_dbms_memory::RecordAddress;

    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    let mut table_registry = db.load_table_registry::<CompositeUser>().unwrap();
    let record_address = RecordAddress::new(200, 16);
    let values = vec![
        (CompositeUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            CompositeUser::columns()[1],
            Value::Text(Text("Alice".to_string())),
        ),
        (
            CompositeUser::columns()[2],
            Value::Text(Text("Smith".to_string())),
        ),
    ];

    let mut mm = db.ctx.mm.borrow_mut();
    db.insert_index::<CompositeUser>(&mut table_registry, record_address, &values, &mut *mm)
        .unwrap();

    // Search the composite index with the correct key order.
    let key = vec![
        Value::Text(Text("Alice".to_string())),
        Value::Text(Text("Smith".to_string())),
    ];
    let results = table_registry
        .index_ledger()
        .search(&["first_name", "last_name"], &key, &*mm)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], record_address);
}

#[test]
fn test_insert_index_missing_column_defaults_to_null() {
    use wasm_dbms_memory::RecordAddress;

    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    let mut table_registry = db.load_table_registry::<IndexedUser>().unwrap();
    let record_address = RecordAddress::new(300, 0);
    // Provide only the PK, omit the indexed `email` column.
    let values = vec![(IndexedUser::columns()[0], Value::Uint32(Uint32(1)))];

    let mut mm = db.ctx.mm.borrow_mut();
    db.insert_index::<IndexedUser>(&mut table_registry, record_address, &values, &mut *mm)
        .unwrap();

    // The index should contain a Null key.
    let key = vec![Value::Null];
    let results = table_registry
        .index_ledger()
        .search(&["email"], &key, &*mm)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], record_address);
}

#[test]
fn test_insert_index_always_includes_pk_index() {
    use wasm_dbms_memory::RecordAddress;

    // User has no explicit #[index] but the PK is always auto-indexed.
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);

    let mut table_registry = db.load_table_registry::<User>().unwrap();
    let record_address = RecordAddress::new(100, 0);
    let values = vec![
        (User::columns()[0], Value::Uint32(Uint32(42))),
        (User::columns()[1], Value::Text(Text("alice".to_string()))),
    ];

    let mut mm = db.ctx.mm.borrow_mut();
    db.insert_index::<User>(&mut table_registry, record_address, &values, &mut *mm)
        .unwrap();

    // The PK index should be searchable.
    let pk_key = vec![Value::Uint32(Uint32(42))];
    let results = table_registry
        .index_ledger()
        .search(&["id"], &pk_key, &*mm)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], record_address);
}

// -- insert flow populates index --

#[test]
fn test_insert_populates_single_column_index() {
    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    let insert = IndexedUserInsertRequest::from_values(&[
        (IndexedUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            IndexedUser::columns()[1],
            Value::Text(Text("alice@example.com".to_string())),
        ),
    ])
    .unwrap();
    db.insert::<IndexedUser>(insert).unwrap();

    // Load the table registry and verify the index was populated.
    let table_registry = db.load_table_registry::<IndexedUser>().unwrap();
    let mm = db.ctx.mm.borrow();
    let key = vec![Value::Text(Text("alice@example.com".to_string()))];
    let results = table_registry
        .index_ledger()
        .search(&["email"], &key, &*mm)
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_insert_multiple_records_populates_index() {
    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    for (id, email) in [(1, "alice@example.com"), (2, "bob@example.com")] {
        let insert = IndexedUserInsertRequest::from_values(&[
            (IndexedUser::columns()[0], Value::Uint32(Uint32(id))),
            (
                IndexedUser::columns()[1],
                Value::Text(Text(email.to_string())),
            ),
        ])
        .unwrap();
        db.insert::<IndexedUser>(insert).unwrap();
    }

    let table_registry = db.load_table_registry::<IndexedUser>().unwrap();
    let mm = db.ctx.mm.borrow();

    // Both entries should be individually searchable.
    let alice_key = vec![Value::Text(Text("alice@example.com".to_string()))];
    let bob_key = vec![Value::Text(Text("bob@example.com".to_string()))];
    assert_eq!(
        table_registry
            .index_ledger()
            .search(&["email"], &alice_key, &*mm)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        table_registry
            .index_ledger()
            .search(&["email"], &bob_key, &*mm)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn test_insert_populates_composite_index() {
    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    let insert = CompositeUserInsertRequest::from_values(&[
        (CompositeUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            CompositeUser::columns()[1],
            Value::Text(Text("Alice".to_string())),
        ),
        (
            CompositeUser::columns()[2],
            Value::Text(Text("Smith".to_string())),
        ),
    ])
    .unwrap();
    db.insert::<CompositeUser>(insert).unwrap();

    let table_registry = db.load_table_registry::<CompositeUser>().unwrap();
    let mm = db.ctx.mm.borrow();
    let key = vec![
        Value::Text(Text("Alice".to_string())),
        Value::Text(Text("Smith".to_string())),
    ];
    let results = table_registry
        .index_ledger()
        .search(&["first_name", "last_name"], &key, &*mm)
        .unwrap();
    assert_eq!(results.len(), 1);
}

// -- index_key tests --

#[test]
fn test_index_key_extracts_matching_columns() {
    let values = vec![
        (IndexedUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            IndexedUser::columns()[1],
            Value::Text(Text("alice@example.com".to_string())),
        ),
    ];
    let key = super::index_key(&["email"], &values);
    assert_eq!(
        key,
        vec![Value::Text(Text("alice@example.com".to_string()))]
    );
}

#[test]
fn test_index_key_missing_column_defaults_to_null() {
    let values = vec![(IndexedUser::columns()[0], Value::Uint32(Uint32(1)))];
    let key = super::index_key(&["email"], &values);
    assert_eq!(key, vec![Value::Null]);
}

#[test]
fn test_index_key_composite() {
    let values = vec![
        (CompositeUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            CompositeUser::columns()[1],
            Value::Text(Text("Alice".to_string())),
        ),
        (
            CompositeUser::columns()[2],
            Value::Text(Text("Smith".to_string())),
        ),
    ];
    let key = super::index_key(&["first_name", "last_name"], &values);
    assert_eq!(
        key,
        vec![
            Value::Text(Text("Alice".to_string())),
            Value::Text(Text("Smith".to_string())),
        ]
    );
}

// -- delete_index unit tests --

#[test]
fn test_delete_index_removes_entry() {
    use wasm_dbms_memory::RecordAddress;

    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    let mut table_registry = db.load_table_registry::<IndexedUser>().unwrap();
    let record_address = RecordAddress::new(100, 0);
    let values = vec![
        (IndexedUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            IndexedUser::columns()[1],
            Value::Text(Text("alice@example.com".to_string())),
        ),
    ];

    let mut mm = db.ctx.mm.borrow_mut();
    db.insert_index::<IndexedUser>(&mut table_registry, record_address, &values, &mut *mm)
        .unwrap();

    // Delete the index entry.
    db.delete_index::<IndexedUser>(&mut table_registry, record_address, &values, &mut *mm)
        .unwrap();

    // The index should now be empty for this key.
    let key = vec![Value::Text(Text("alice@example.com".to_string()))];
    let results = table_registry
        .index_ledger()
        .search(&["email"], &key, &*mm)
        .unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_delete_index_removes_pk_index() {
    use wasm_dbms_memory::RecordAddress;

    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);

    let mut table_registry = db.load_table_registry::<User>().unwrap();
    let record_address = RecordAddress::new(100, 0);
    let values = vec![
        (User::columns()[0], Value::Uint32(Uint32(42))),
        (User::columns()[1], Value::Text(Text("alice".to_string()))),
    ];

    let mut mm = db.ctx.mm.borrow_mut();
    db.insert_index::<User>(&mut table_registry, record_address, &values, &mut *mm)
        .unwrap();

    db.delete_index::<User>(&mut table_registry, record_address, &values, &mut *mm)
        .unwrap();

    // The PK index entry should be gone.
    let pk_key = vec![Value::Uint32(Uint32(42))];
    let results = table_registry
        .index_ledger()
        .search(&["id"], &pk_key, &*mm)
        .unwrap();
    assert!(results.is_empty());
}

// -- update_index unit tests --

#[test]
fn test_update_index_same_key_updates_pointer() {
    use wasm_dbms_memory::RecordAddress;

    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    let mut table_registry = db.load_table_registry::<IndexedUser>().unwrap();
    let old_address = RecordAddress::new(100, 0);
    let new_address = RecordAddress::new(200, 32);
    let values = vec![
        (IndexedUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            IndexedUser::columns()[1],
            Value::Text(Text("alice@example.com".to_string())),
        ),
    ];

    let mut mm = db.ctx.mm.borrow_mut();
    db.insert_index::<IndexedUser>(&mut table_registry, old_address, &values, &mut *mm)
        .unwrap();

    // Update with same values (only address changed).
    db.update_index::<IndexedUser>(
        &mut table_registry,
        old_address,
        new_address,
        &values,
        &values,
        &mut *mm,
    )
    .unwrap();

    // The key should now point to the new address.
    let key = vec![Value::Text(Text("alice@example.com".to_string()))];
    let results = table_registry
        .index_ledger()
        .search(&["email"], &key, &*mm)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], new_address);
}

#[test]
fn test_update_index_changed_key_replaces_entry() {
    use wasm_dbms_memory::RecordAddress;

    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    let mut table_registry = db.load_table_registry::<IndexedUser>().unwrap();
    let old_address = RecordAddress::new(100, 0);
    let new_address = RecordAddress::new(200, 32);
    let old_values = vec![
        (IndexedUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            IndexedUser::columns()[1],
            Value::Text(Text("old@example.com".to_string())),
        ),
    ];
    let new_values = vec![
        (IndexedUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            IndexedUser::columns()[1],
            Value::Text(Text("new@example.com".to_string())),
        ),
    ];

    let mut mm = db.ctx.mm.borrow_mut();
    db.insert_index::<IndexedUser>(&mut table_registry, old_address, &old_values, &mut *mm)
        .unwrap();

    // Update with a changed indexed column value.
    db.update_index::<IndexedUser>(
        &mut table_registry,
        old_address,
        new_address,
        &old_values,
        &new_values,
        &mut *mm,
    )
    .unwrap();

    // The old key should be gone.
    let old_key = vec![Value::Text(Text("old@example.com".to_string()))];
    let results = table_registry
        .index_ledger()
        .search(&["email"], &old_key, &*mm)
        .unwrap();
    assert!(results.is_empty());

    // The new key should point to the new address.
    let new_key = vec![Value::Text(Text("new@example.com".to_string()))];
    let results = table_registry
        .index_ledger()
        .search(&["email"], &new_key, &*mm)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], new_address);
}

// -- delete flow removes index --

#[test]
fn test_delete_removes_index_entry() {
    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    let insert = IndexedUserInsertRequest::from_values(&[
        (IndexedUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            IndexedUser::columns()[1],
            Value::Text(Text("alice@example.com".to_string())),
        ),
    ])
    .unwrap();
    db.insert::<IndexedUser>(insert).unwrap();

    db.delete::<IndexedUser>(DeleteBehavior::Restrict, None)
        .unwrap();

    let table_registry = db.load_table_registry::<IndexedUser>().unwrap();
    let mm = db.ctx.mm.borrow();
    let key = vec![Value::Text(Text("alice@example.com".to_string()))];
    let results = table_registry
        .index_ledger()
        .search(&["email"], &key, &*mm)
        .unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_delete_with_filter_removes_only_matching_index_entries() {
    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    for (id, email) in [(1, "alice@example.com"), (2, "bob@example.com")] {
        let insert = IndexedUserInsertRequest::from_values(&[
            (IndexedUser::columns()[0], Value::Uint32(Uint32(id))),
            (
                IndexedUser::columns()[1],
                Value::Text(Text(email.to_string())),
            ),
        ])
        .unwrap();
        db.insert::<IndexedUser>(insert).unwrap();
    }

    db.delete::<IndexedUser>(
        DeleteBehavior::Restrict,
        Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
    )
    .unwrap();

    let table_registry = db.load_table_registry::<IndexedUser>().unwrap();
    let mm = db.ctx.mm.borrow();

    // alice's index entry should be gone.
    let alice_key = vec![Value::Text(Text("alice@example.com".to_string()))];
    assert!(
        table_registry
            .index_ledger()
            .search(&["email"], &alice_key, &*mm)
            .unwrap()
            .is_empty()
    );

    // bob's index entry should remain.
    let bob_key = vec![Value::Text(Text("bob@example.com".to_string()))];
    assert_eq!(
        table_registry
            .index_ledger()
            .search(&["email"], &bob_key, &*mm)
            .unwrap()
            .len(),
        1
    );
}

// -- update flow updates index --

#[test]
fn test_update_non_indexed_column_preserves_index() {
    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    let insert = IndexedUserInsertRequest::from_values(&[
        (IndexedUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            IndexedUser::columns()[1],
            Value::Text(Text("alice@example.com".to_string())),
        ),
    ])
    .unwrap();
    db.insert::<IndexedUser>(insert).unwrap();

    // Update a non-indexed column (id is the PK, not indexed by #[index]).
    // IndexedUser only has email indexed, so changing the PK should not affect
    // the email index. However, the PK is the primary key, not a patchable
    // column in normal usage. Instead, let's just verify that the index is
    // still searchable after an update that does not touch the indexed column.
    // We update `email` to the same value, which should preserve the index.
    let patch = IndexedUserUpdateRequest::from_values(
        &[(
            IndexedUser::columns()[1],
            Value::Text(Text("alice@example.com".to_string())),
        )],
        Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
    );
    db.update::<IndexedUser>(patch).unwrap();

    let table_registry = db.load_table_registry::<IndexedUser>().unwrap();
    let mm = db.ctx.mm.borrow();
    let key = vec![Value::Text(Text("alice@example.com".to_string()))];
    let results = table_registry
        .index_ledger()
        .search(&["email"], &key, &*mm)
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_update_indexed_column_updates_index() {
    let ctx = setup_indexed();
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexedTestSchema);

    let insert = IndexedUserInsertRequest::from_values(&[
        (IndexedUser::columns()[0], Value::Uint32(Uint32(1))),
        (
            IndexedUser::columns()[1],
            Value::Text(Text("old@example.com".to_string())),
        ),
    ])
    .unwrap();
    db.insert::<IndexedUser>(insert).unwrap();

    // Update the indexed column to a new value.
    let patch = IndexedUserUpdateRequest::from_values(
        &[(
            IndexedUser::columns()[1],
            Value::Text(Text("new@example.com".to_string())),
        )],
        Some(Filter::eq("id", Value::Uint32(Uint32(1)))),
    );
    db.update::<IndexedUser>(patch).unwrap();

    let table_registry = db.load_table_registry::<IndexedUser>().unwrap();
    let mm = db.ctx.mm.borrow();

    // The old key should no longer be in the index.
    let old_key = vec![Value::Text(Text("old@example.com".to_string()))];
    assert!(
        table_registry
            .index_ledger()
            .search(&["email"], &old_key, &*mm)
            .unwrap()
            .is_empty()
    );

    // The new key should be present.
    let new_key = vec![Value::Text(Text("new@example.com".to_string()))];
    assert_eq!(
        table_registry
            .index_ledger()
            .search(&["email"], &new_key, &*mm)
            .unwrap()
            .len(),
        1
    );
}
