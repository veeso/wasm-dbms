use candid::CandidType;
use criterion::{Criterion, criterion_group, criterion_main};
use ic_dbms_api::prelude::{Database, DeleteBehavior, Filter, Query, TableSchema};
use ic_dbms_canister::prelude::{
    DBMS_CONTEXT, DatabaseSchema, Table, Text, Uint32, WasmDbmsDatabase,
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

#[derive(DatabaseSchema)]
#[tables(User = "users", Post = "posts")]
pub struct BenchDatabaseSchema;

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
