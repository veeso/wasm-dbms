use candid::CandidType;
use criterion::{Criterion, criterion_group, criterion_main};
use ic_dbms_api::prelude::{
    ColumnDef, Database, DeleteBehavior, Filter, InsertRecord, Query, QueryError, TableSchema,
    UpdateRecord, Value, flatten_table_columns,
};
use ic_dbms_canister::prelude::{
    DatabaseSchema, IcDbmsDatabase, InsertIntegrityValidator, MEMORY_MANAGER, SCHEMA_REGISTRY,
    Table, Text, Uint32, UpdateIntegrityValidator, get_referenced_tables,
};
use serde::Deserialize;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    id: Uint32,
    name: Text,
    email: Text,
}

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "posts"]
#[alignment = 64]
pub struct Post {
    #[primary_key]
    id: Uint32,
    title: Text,
    content: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    user: Uint32,
}

pub struct BenchDatabaseSchema;

impl DatabaseSchema for BenchDatabaseSchema {
    fn select(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &str,
        query: Query,
    ) -> ic_dbms_api::prelude::IcDbmsResult<Vec<Vec<(ColumnDef, Value)>>> {
        if table_name == User::table_name() {
            let results = dbms.select_columns::<User>(query)?;
            Ok(flatten_table_columns(results))
        } else if table_name == Post::table_name() {
            let results = dbms.select_columns::<Post>(query)?;
            Ok(flatten_table_columns(results))
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }

    fn referenced_tables(&self, table: &'static str) -> Vec<(&'static str, Vec<&'static str>)> {
        let tables = &[
            (User::table_name(), User::columns()),
            (Post::table_name(), Post::columns()),
        ];
        get_referenced_tables(table, tables)
    }

    fn insert(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> ic_dbms_api::prelude::IcDbmsResult<()> {
        if table_name == User::table_name() {
            let insert_request = UserInsertRequest::from_values(record_values)?;
            dbms.insert::<User>(insert_request)
        } else if table_name == Post::table_name() {
            let insert_request = PostInsertRequest::from_values(record_values)?;
            dbms.insert::<Post>(insert_request)
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }

    fn delete(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        delete_behavior: DeleteBehavior,
        filter: Option<Filter>,
    ) -> ic_dbms_api::prelude::IcDbmsResult<u64> {
        if table_name == User::table_name() {
            dbms.delete::<User>(delete_behavior, filter)
        } else if table_name == Post::table_name() {
            dbms.delete::<Post>(delete_behavior, filter)
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }

    fn update(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        patch_values: &[(ColumnDef, Value)],
        filter: Option<Filter>,
    ) -> ic_dbms_api::prelude::IcDbmsResult<u64> {
        if table_name == User::table_name() {
            let update_request = UserUpdateRequest::from_values(patch_values, filter);
            dbms.update::<User>(update_request)
        } else if table_name == Post::table_name() {
            let update_request = PostUpdateRequest::from_values(patch_values, filter);
            dbms.update::<Post>(update_request)
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }

    fn validate_insert(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> ic_dbms_api::prelude::IcDbmsResult<()> {
        if table_name == User::table_name() {
            InsertIntegrityValidator::<User>::new(dbms).validate(record_values)
        } else if table_name == Post::table_name() {
            InsertIntegrityValidator::<Post>::new(dbms).validate(record_values)
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }

    fn validate_update(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
        old_pk: Value,
    ) -> ic_dbms_api::prelude::IcDbmsResult<()> {
        if table_name == User::table_name() {
            UpdateIntegrityValidator::<User>::new(dbms, old_pk).validate(record_values)
        } else if table_name == Post::table_name() {
            UpdateIntegrityValidator::<Post>::new(dbms, old_pk).validate(record_values)
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }
}

/// Load test fixtures: `user_count` users and `post_count` posts (round-robin across users).
fn load_fixtures(database: &mut IcDbmsDatabase, user_count: u32, post_count: u32) {
    SCHEMA_REGISTRY.with_borrow_mut(|registry| {
        MEMORY_MANAGER.with_borrow_mut(|mm| {
            registry
                .register_table::<User>(mm)
                .expect("failed to register User table");
            registry
                .register_table::<Post>(mm)
                .expect("failed to register Post table");
        });
    });

    for id in 0..user_count {
        let user = UserInsertRequest {
            id: Uint32(id),
            name: Text(format!("User_{id}")),
            email: Text(format!("user_{id}@example.com")),
        };
        database
            .insert::<User>(user)
            .expect("failed to insert user");
    }

    for id in 0..post_count {
        let user_id = id % user_count;
        let post = PostInsertRequest {
            id: Uint32(id),
            title: Text(format!("Post_{id}")),
            content: Text(format!("Content for post {id}")),
            user: Uint32(user_id),
        };
        database
            .insert::<Post>(post)
            .expect("failed to insert post");
    }
}

fn free_tables(database: &mut IcDbmsDatabase) {
    database
        .delete::<Post>(DeleteBehavior::Restrict, None)
        .expect("failed to delete posts");
    database
        .delete::<User>(DeleteBehavior::Restrict, None)
        .expect("failed to delete users");
}

/// Simulates the old N+1 query pattern: select all posts, then individually
/// fetch each referenced user by PK.
fn naive_n_plus_1_select(database: &IcDbmsDatabase) {
    let query = Query::builder().all().build();
    let posts = database
        .select_raw(Post::table_name(), query)
        .expect("failed to select posts");

    for post_cols in &posts {
        let user_id = post_cols
            .iter()
            .find(|(col, _)| col.name == "user")
            .expect("user column missing")
            .1
            .clone();
        let user_query = Query::builder()
            .all()
            .limit(1)
            .and_where(Filter::Eq(User::primary_key().to_string(), user_id))
            .build();
        let _users = database
            .select::<User>(user_query)
            .expect("failed to select user");
    }
}

fn bench_eager_relation(c: &mut Criterion) {
    let mut group = c.benchmark_group("eager_relation");

    for post_count in &[20u32, 100, 500, 1000] {
        let user_count = 10u32;
        let mut database = IcDbmsDatabase::oneshot(BenchDatabaseSchema);
        load_fixtures(&mut database, user_count, *post_count);

        // N+1 baseline (old approach)
        let naive_label = format!("naive_n+1/{post_count}_posts_{user_count}_users");
        group.bench_with_input(&naive_label, &database, |b, database| {
            b.iter(|| naive_n_plus_1_select(database));
        });

        // Batch approach (new)
        let batch_label = format!("batch/{post_count}_posts_{user_count}_users");
        group.bench_with_input(&batch_label, &database, |b, database| {
            b.iter(|| {
                let query = Query::builder().all().with(User::table_name()).build();
                database
                    .select::<Post>(query)
                    .expect("failed to select posts with eager user");
            });
        });

        free_tables(&mut database);
    }
}

fn configure_criterion() -> Criterion {
    Criterion::default()
        .measurement_time(std::time::Duration::from_secs(10))
        .warm_up_time(std::time::Duration::from_secs(1))
        .sample_size(20)
        .noise_threshold(0.05)
}

criterion_group!(name = benches; config = configure_criterion(); targets = bench_eager_relation);
criterion_main!(benches);
