use candid::CandidType;
use criterion::{Criterion, criterion_group, criterion_main};
use ic_dbms_api::prelude::{
    ColumnDef, Database, DeleteBehavior, Filter, InsertRecord, Query, QueryError, TableSchema,
    UpdateRecord, Value, flatten_table_columns,
};
use ic_dbms_canister::prelude::{
    AccessControl, DBMS_CONTEXT, DatabaseSchema, InsertIntegrityValidator, MemoryProvider, Table,
    Text, Uint32, UpdateIntegrityValidator, WasmDbmsDatabase, get_referenced_tables,
};
use serde::Deserialize;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
#[table = "users"]
pub struct User {
    #[primary_key]
    id: Uint32,
    name: Text,
    email: Text,
}

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
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

impl<M, A> DatabaseSchema<M, A> for BenchDatabaseSchema
where
    M: MemoryProvider,
    A: AccessControl,
{
    fn select(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &str,
        query: Query,
    ) -> ic_dbms_api::prelude::IcDbmsResult<Vec<Vec<(ColumnDef, Value)>>> {
        match table_name {
            name if name == User::table_name() => {
                let results = dbms.select_columns::<User>(query)?;
                Ok(flatten_table_columns(results))
            }
            name if name == Post::table_name() => {
                let results = dbms.select_columns::<Post>(query)?;
                Ok(flatten_table_columns(results))
            }
            _ => Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            )),
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
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> ic_dbms_api::prelude::IcDbmsResult<()> {
        match table_name {
            name if name == User::table_name() => {
                let insert_request = UserInsertRequest::from_values(record_values)?;
                dbms.insert::<User>(insert_request)
            }
            name if name == Post::table_name() => {
                let insert_request = PostInsertRequest::from_values(record_values)?;
                dbms.insert::<Post>(insert_request)
            }
            _ => Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            )),
        }
    }

    fn delete(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &'static str,
        delete_behavior: DeleteBehavior,
        filter: Option<Filter>,
    ) -> ic_dbms_api::prelude::IcDbmsResult<u64> {
        match table_name {
            name if name == User::table_name() => dbms.delete::<User>(delete_behavior, filter),
            name if name == Post::table_name() => dbms.delete::<Post>(delete_behavior, filter),
            _ => Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            )),
        }
    }

    fn update(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &'static str,
        patch_values: &[(ColumnDef, Value)],
        filter: Option<Filter>,
    ) -> ic_dbms_api::prelude::IcDbmsResult<u64> {
        match table_name {
            name if name == User::table_name() => {
                let update_request = UserUpdateRequest::from_values(patch_values, filter);
                dbms.update::<User>(update_request)
            }
            name if name == Post::table_name() => {
                let update_request = PostUpdateRequest::from_values(patch_values, filter);
                dbms.update::<Post>(update_request)
            }
            _ => Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            )),
        }
    }

    fn validate_insert(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> ic_dbms_api::prelude::IcDbmsResult<()> {
        match table_name {
            name if name == User::table_name() => {
                InsertIntegrityValidator::<User, M, A>::new(dbms).validate(record_values)
            }
            name if name == Post::table_name() => {
                InsertIntegrityValidator::<Post, M, A>::new(dbms).validate(record_values)
            }
            _ => Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            )),
        }
    }

    fn validate_update(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
        old_pk: Value,
    ) -> ic_dbms_api::prelude::IcDbmsResult<()> {
        match table_name {
            name if name == User::table_name() => {
                UpdateIntegrityValidator::<User, M, A>::new(dbms, old_pk).validate(record_values)
            }
            name if name == Post::table_name() => {
                UpdateIntegrityValidator::<Post, M, A>::new(dbms, old_pk).validate(record_values)
            }
            _ => Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            )),
        }
    }
}

/// Load test fixtures: `user_count` users and `post_count` posts (round-robin across users).
fn load_fixtures(user_count: u32, post_count: u32) {
    DBMS_CONTEXT.with(|ctx| {
        ctx.register_table::<User>()
            .expect("failed to register User table");
        ctx.register_table::<Post>()
            .expect("failed to register Post table");

        let db = WasmDbmsDatabase::oneshot(ctx, BenchDatabaseSchema);

        for id in 0..user_count {
            let user = UserInsertRequest {
                id: Uint32(id),
                name: Text(format!("User_{id}")),
                email: Text(format!("user_{id}@example.com")),
            };
            db.insert::<User>(user).expect("failed to insert user");
        }

        for id in 0..post_count {
            let user_id = id % user_count;
            let post = PostInsertRequest {
                id: Uint32(id),
                title: Text(format!("Post_{id}")),
                content: Text(format!("Content for post {id}")),
                user: Uint32(user_id),
            };
            db.insert::<Post>(post).expect("failed to insert post");
        }
    });
}

fn free_tables() {
    DBMS_CONTEXT.with(|ctx| {
        let db = WasmDbmsDatabase::oneshot(ctx, BenchDatabaseSchema);
        db.delete::<Post>(DeleteBehavior::Restrict, None)
            .expect("failed to delete posts");
        db.delete::<User>(DeleteBehavior::Restrict, None)
            .expect("failed to delete users");
    });
}

/// Simulates the old N+1 query pattern: select all posts, then individually
/// fetch each referenced user by PK.
fn naive_n_plus_1_select() {
    DBMS_CONTEXT.with(|ctx| {
        let db = WasmDbmsDatabase::oneshot(ctx, BenchDatabaseSchema);
        let query = Query::builder().all().build();
        let posts = db
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
            let _users = db
                .select::<User>(user_query)
                .expect("failed to select user");
        }
    });
}

fn bench_eager_relation(c: &mut Criterion) {
    let mut group = c.benchmark_group("eager_relation");

    for post_count in &[20u32, 100, 500, 1000] {
        let user_count = 10u32;
        load_fixtures(user_count, *post_count);

        // N+1 baseline (old approach)
        let naive_label = format!("naive_n+1/{post_count}_posts_{user_count}_users");
        group.bench_function(&naive_label, |b| {
            b.iter(naive_n_plus_1_select);
        });

        // Batch approach (new)
        let batch_label = format!("batch/{post_count}_posts_{user_count}_users");
        group.bench_function(&batch_label, |b| {
            b.iter(|| {
                DBMS_CONTEXT.with(|ctx| {
                    let db = WasmDbmsDatabase::oneshot(ctx, BenchDatabaseSchema);
                    let query = Query::builder().all().with(User::table_name()).build();
                    db.select::<Post>(query)
                        .expect("failed to select posts with eager user");
                });
            });
        });

        free_tables();
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
