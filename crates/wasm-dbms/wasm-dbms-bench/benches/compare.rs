use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use wasm_dbms::prelude::WasmDbmsDatabase;
use wasm_dbms_api::prelude::{
    Database, DeleteBehavior, Filter, Query, TableSchema, Text, Uint32, UpdateRecord, Value,
};
use wasm_dbms_bench::data::DataGenerator;
use wasm_dbms_bench::schema::{BenchDatabaseSchema, User, UserInsertRequest, UserUpdateRequest};
use wasm_dbms_bench::setup;

// -- Single-record CRUD --

fn bench_single_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_crud/insert");

    group.bench_function("wasm_dbms", |b| {
        let ctx = setup::setup_wasm_dbms();
        let mut next_id = 1u32;
        b.iter(|| {
            let db = WasmDbmsDatabase::oneshot(&ctx, BenchDatabaseSchema);
            let req = UserInsertRequest {
                id: Uint32(next_id),
                name: Text(format!("user_{next_id}")),
                email: Text(format!("user_{next_id}@example.com")),
                age: Uint32(25),
            };
            db.insert::<User>(req).expect("insert failed");
            next_id += 1;
        });
    });

    group.bench_function("rusqlite", |b| {
        let conn = setup::setup_rusqlite();
        let mut next_id = 1u32;
        b.iter(|| {
            conn.execute(
                "INSERT INTO users (id, name, email, age) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    next_id,
                    format!("user_{next_id}"),
                    format!("user_{next_id}@example.com"),
                    25
                ],
            )
            .expect("insert failed");
            next_id += 1;
        });
    });

    group.bench_function("duckdb", |b| {
        let conn = setup::setup_duckdb();
        let mut next_id = 1u32;
        b.iter(|| {
            conn.execute(
                "INSERT INTO users (id, name, email, age) VALUES (?, ?, ?, ?)",
                duckdb::params![
                    next_id,
                    format!("user_{next_id}"),
                    format!("user_{next_id}@example.com"),
                    25
                ],
            )
            .expect("insert failed");
            next_id += 1;
        });
    });

    group.finish();
}

fn bench_single_read_by_pk(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_crud/read_by_pk");

    group.bench_function("wasm_dbms", |b| {
        let ctx = setup::setup_wasm_dbms_with_users(1_000);
        b.iter(|| {
            let db = WasmDbmsDatabase::oneshot(&ctx, BenchDatabaseSchema);
            let query = Query::builder()
                .all()
                .filter(Some(Filter::eq("id", Value::Uint32(Uint32(500)))))
                .build();
            db.select::<User>(query).expect("select failed");
        });
    });

    group.bench_function("rusqlite", |b| {
        let conn = setup::setup_rusqlite_with_users(1_000);
        b.iter(|| {
            let mut stmt = conn
                .prepare_cached("SELECT id, name, email, age FROM users WHERE id = ?1")
                .expect("prepare failed");
            let _rows: Vec<(i32, String, String, i32)> = stmt
                .query_map(rusqlite::params![500], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .expect("query failed")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect failed");
        });
    });

    group.bench_function("duckdb", |b| {
        let conn = setup::setup_duckdb_with_users(1_000);
        b.iter(|| {
            let mut stmt = conn
                .prepare_cached("SELECT id, name, email, age FROM users WHERE id = ?")
                .expect("prepare failed");
            let _rows: Vec<(i32, String, String, i32)> = stmt
                .query_map(duckdb::params![500], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .expect("query failed")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect failed");
        });
    });

    group.finish();
}

fn bench_single_update(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_crud/update");

    group.bench_function("wasm_dbms", |b| {
        let ctx = setup::setup_wasm_dbms_with_users(1_000);
        let mut counter = 0u32;
        b.iter(|| {
            let db = WasmDbmsDatabase::oneshot(&ctx, BenchDatabaseSchema);
            let patch = UserUpdateRequest::from_values(
                &[(
                    User::columns()[1],
                    Value::Text(Text(format!("updated_{counter}"))),
                )],
                Some(Filter::eq("id", Value::Uint32(Uint32(500)))),
            );
            db.update::<User>(patch).expect("update failed");
            counter += 1;
        });
    });

    group.bench_function("rusqlite", |b| {
        let conn = setup::setup_rusqlite_with_users(1_000);
        let mut counter = 0u32;
        b.iter(|| {
            conn.execute(
                "UPDATE users SET name = ?1 WHERE id = ?2",
                rusqlite::params![format!("updated_{counter}"), 500],
            )
            .expect("update failed");
            counter += 1;
        });
    });

    group.bench_function("duckdb", |b| {
        let conn = setup::setup_duckdb_with_users(1_000);
        let mut counter = 0u32;
        b.iter(|| {
            conn.execute(
                "UPDATE users SET name = ? WHERE id = ?",
                duckdb::params![format!("updated_{counter}"), 500],
            )
            .expect("update failed");
            counter += 1;
        });
    });

    group.finish();
}

fn bench_single_delete(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_crud/delete");

    // For delete benchmarks we use iter_batched to re-create state each iteration
    group.bench_function("wasm_dbms", |b| {
        b.iter_batched(
            || setup::setup_wasm_dbms_with_users(1_000),
            |ctx| {
                let db = WasmDbmsDatabase::oneshot(&ctx, BenchDatabaseSchema);
                let filter = Filter::eq("id", Value::Uint32(Uint32(500)));
                db.delete::<User>(DeleteBehavior::Restrict, Some(filter))
                    .expect("delete failed");
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("rusqlite", |b| {
        b.iter_batched(
            || setup::setup_rusqlite_with_users(1_000),
            |conn| {
                conn.execute("DELETE FROM users WHERE id = ?1", rusqlite::params![500])
                    .expect("delete failed");
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("duckdb", |b| {
        b.iter_batched(
            || setup::setup_duckdb_with_users(1_000),
            |conn| {
                conn.execute("DELETE FROM users WHERE id = ?", duckdb::params![500])
                    .expect("delete failed");
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ── Bulk insert ──

fn bench_bulk_insert(c: &mut Criterion) {
    for count in [100u32, 1_000, 10_000] {
        let mut group = c.benchmark_group(format!("bulk_insert/{count}"));

        group.bench_function("wasm_dbms", |b| {
            b.iter_batched(
                || {
                    let ctx = setup::setup_wasm_dbms();
                    let users = DataGenerator::new().users(count);
                    (ctx, users)
                },
                |(ctx, users)| {
                    setup::populate_wasm_dbms_users(&ctx, &users);
                },
                criterion::BatchSize::SmallInput,
            );
        });

        group.bench_function("rusqlite", |b| {
            b.iter_batched(
                || {
                    let conn = setup::setup_rusqlite();
                    let users = DataGenerator::new().users(count);
                    (conn, users)
                },
                |(conn, users)| {
                    setup::populate_rusqlite_users(&conn, &users);
                },
                criterion::BatchSize::SmallInput,
            );
        });

        group.bench_function("duckdb", |b| {
            b.iter_batched(
                || {
                    let conn = setup::setup_duckdb();
                    let users = DataGenerator::new().users(count);
                    (conn, users)
                },
                |(conn, users)| {
                    setup::populate_duckdb_users(&conn, &users);
                },
                criterion::BatchSize::SmallInput,
            );
        });

        group.finish();
    }
}

// ── Query operations ──

fn bench_query_filtered(c: &mut Criterion) {
    let mut group = c.benchmark_group("query/filtered");

    group.bench_function("wasm_dbms", |b| {
        let ctx = setup::setup_wasm_dbms_with_users(10_000);
        b.iter(|| {
            let db = WasmDbmsDatabase::oneshot(&ctx, BenchDatabaseSchema);
            let query = Query::builder()
                .all()
                .filter(Some(Filter::gt("age", Value::Uint32(Uint32(50)))))
                .build();
            db.select::<User>(query).expect("select failed");
        });
    });

    group.bench_function("rusqlite", |b| {
        let conn = setup::setup_rusqlite_with_users(10_000);
        b.iter(|| {
            let mut stmt = conn
                .prepare_cached("SELECT id, name, email, age FROM users WHERE age > ?1")
                .expect("prepare failed");
            let _rows: Vec<(i32, String, String, i32)> = stmt
                .query_map(rusqlite::params![50], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .expect("query failed")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect failed");
        });
    });

    group.bench_function("duckdb", |b| {
        let conn = setup::setup_duckdb_with_users(10_000);
        b.iter(|| {
            let mut stmt = conn
                .prepare_cached("SELECT id, name, email, age FROM users WHERE age > ?")
                .expect("prepare failed");
            let _rows: Vec<(i32, String, String, i32)> = stmt
                .query_map(duckdb::params![50], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .expect("query failed")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect failed");
        });
    });

    group.finish();
}

fn bench_query_ordered(c: &mut Criterion) {
    let mut group = c.benchmark_group("query/ordered");

    group.bench_function("wasm_dbms", |b| {
        let ctx = setup::setup_wasm_dbms_with_users(10_000);
        b.iter(|| {
            let db = WasmDbmsDatabase::oneshot(&ctx, BenchDatabaseSchema);
            let query = Query::builder().all().order_by_asc("name").build();
            db.select::<User>(query).expect("select failed");
        });
    });

    group.bench_function("rusqlite", |b| {
        let conn = setup::setup_rusqlite_with_users(10_000);
        b.iter(|| {
            let mut stmt = conn
                .prepare_cached("SELECT id, name, email, age FROM users ORDER BY name ASC")
                .expect("prepare failed");
            let _rows: Vec<(i32, String, String, i32)> = stmt
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .expect("query failed")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect failed");
        });
    });

    group.bench_function("duckdb", |b| {
        let conn = setup::setup_duckdb_with_users(10_000);
        b.iter(|| {
            let mut stmt = conn
                .prepare_cached("SELECT id, name, email, age FROM users ORDER BY name ASC")
                .expect("prepare failed");
            let _rows: Vec<(i32, String, String, i32)> = stmt
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .expect("query failed")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect failed");
        });
    });

    group.finish();
}

fn bench_query_join(c: &mut Criterion) {
    let mut group = c.benchmark_group("query/join");

    group.bench_function("wasm_dbms", |b| {
        let ctx = setup::setup_wasm_dbms_with_users_and_posts(100, 10_000);
        b.iter(|| {
            let db = WasmDbmsDatabase::oneshot(&ctx, BenchDatabaseSchema);
            let query = Query::builder()
                .all()
                .inner_join("posts", "id", "user_id")
                .build();
            db.select_join("users", query).expect("join failed");
        });
    });

    group.bench_function("rusqlite", |b| {
        let conn = setup::setup_rusqlite_with_users_and_posts(100, 10_000);
        b.iter(|| {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT u.id, u.name, u.email, u.age, p.id, p.title, p.body, p.user_id \
                     FROM users u INNER JOIN posts p ON u.id = p.user_id",
                )
                .expect("prepare failed");
            let _rows: Vec<(i32, String, String, i32, i32, String, String, i32)> = stmt
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                })
                .expect("query failed")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect failed");
        });
    });

    group.bench_function("duckdb", |b| {
        let conn = setup::setup_duckdb_with_users_and_posts(100, 10_000);
        b.iter(|| {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT u.id, u.name, u.email, u.age, p.id, p.title, p.body, p.user_id \
                     FROM users u INNER JOIN posts p ON u.id = p.user_id",
                )
                .expect("prepare failed");
            let _rows: Vec<(i32, String, String, i32, i32, String, String, i32)> = stmt
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                })
                .expect("query failed")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect failed");
        });
    });

    group.finish();
}

// ── Transactions ──

fn bench_transaction_commit(c: &mut Criterion) {
    let mut group = c.benchmark_group("transaction/commit");

    group.bench_function("wasm_dbms", |b| {
        b.iter_batched(
            setup::setup_wasm_dbms,
            |ctx| {
                let tx_id = ctx.begin_transaction(vec![1, 2, 3]);
                let mut db = WasmDbmsDatabase::from_transaction(&ctx, BenchDatabaseSchema, tx_id);
                for id in 1..=100u32 {
                    let req = UserInsertRequest {
                        id: Uint32(id),
                        name: Text(format!("user_{id}")),
                        email: Text(format!("user_{id}@example.com")),
                        age: Uint32(25),
                    };
                    db.insert::<User>(req).expect("insert failed");
                }
                db.commit().expect("commit failed");
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("rusqlite", |b| {
        b.iter_batched(
            setup::setup_rusqlite,
            |conn| {
                let tx = conn.unchecked_transaction().expect("begin failed");
                {
                    let mut stmt = tx
                        .prepare("INSERT INTO users (id, name, email, age) VALUES (?1, ?2, ?3, ?4)")
                        .expect("prepare failed");
                    for id in 1..=100u32 {
                        stmt.execute(rusqlite::params![
                            id,
                            format!("user_{id}"),
                            format!("user_{id}@example.com"),
                            25
                        ])
                        .expect("insert failed");
                    }
                }
                tx.commit().expect("commit failed");
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("duckdb", |b| {
        b.iter_batched(
            setup::setup_duckdb,
            |conn| {
                let tx = conn.unchecked_transaction().expect("begin failed");
                {
                    let mut stmt = tx
                        .prepare("INSERT INTO users (id, name, email, age) VALUES (?, ?, ?, ?)")
                        .expect("prepare failed");
                    for id in 1..=100u32 {
                        stmt.execute(duckdb::params![
                            id,
                            format!("user_{id}"),
                            format!("user_{id}@example.com"),
                            25
                        ])
                        .expect("insert failed");
                    }
                }
                tx.commit().expect("commit failed");
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_transaction_rollback(c: &mut Criterion) {
    let mut group = c.benchmark_group("transaction/rollback");

    group.bench_function("wasm_dbms", |b| {
        b.iter_batched(
            setup::setup_wasm_dbms,
            |ctx| {
                let tx_id = ctx.begin_transaction(vec![1, 2, 3]);
                let mut db = WasmDbmsDatabase::from_transaction(&ctx, BenchDatabaseSchema, tx_id);
                for id in 1..=100u32 {
                    let req = UserInsertRequest {
                        id: Uint32(id),
                        name: Text(format!("user_{id}")),
                        email: Text(format!("user_{id}@example.com")),
                        age: Uint32(25),
                    };
                    db.insert::<User>(req).expect("insert failed");
                }
                db.rollback().expect("rollback failed");
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("rusqlite", |b| {
        b.iter_batched(
            setup::setup_rusqlite,
            |conn| {
                let tx = conn.unchecked_transaction().expect("begin failed");
                {
                    let mut stmt = tx
                        .prepare("INSERT INTO users (id, name, email, age) VALUES (?1, ?2, ?3, ?4)")
                        .expect("prepare failed");
                    for id in 1..=100u32 {
                        stmt.execute(rusqlite::params![
                            id,
                            format!("user_{id}"),
                            format!("user_{id}@example.com"),
                            25
                        ])
                        .expect("insert failed");
                    }
                }
                tx.rollback().expect("rollback failed");
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("duckdb", |b| {
        b.iter_batched(
            setup::setup_duckdb,
            |conn| {
                let tx = conn.unchecked_transaction().expect("begin failed");
                {
                    let mut stmt = tx
                        .prepare("INSERT INTO users (id, name, email, age) VALUES (?, ?, ?, ?)")
                        .expect("prepare failed");
                    for id in 1..=100u32 {
                        stmt.execute(duckdb::params![
                            id,
                            format!("user_{id}"),
                            format!("user_{id}@example.com"),
                            25
                        ])
                        .expect("insert failed");
                    }
                }
                tx.rollback().expect("rollback failed");
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn configure_criterion() -> Criterion {
    Criterion::default()
        .measurement_time(Duration::from_secs(10))
        .warm_up_time(Duration::from_secs(1))
        .sample_size(20)
        .noise_threshold(0.05)
}

criterion_group!(
    name = single_crud;
    config = configure_criterion();
    targets =
        bench_single_insert,
        bench_single_read_by_pk,
        bench_single_update,
        bench_single_delete
);

criterion_group!(
    name = bulk;
    config = configure_criterion();
    targets = bench_bulk_insert
);

criterion_group!(
    name = queries;
    config = configure_criterion();
    targets =
        bench_query_filtered,
        bench_query_ordered,
        bench_query_join
);

criterion_group!(
    name = transactions;
    config = configure_criterion();
    targets =
        bench_transaction_commit,
        bench_transaction_rollback
);

criterion_main!(single_crud, bulk, queries, transactions);
