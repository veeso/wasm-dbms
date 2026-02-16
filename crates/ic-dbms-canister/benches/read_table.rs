use std::collections::HashSet;

use candid::CandidType;
use criterion::{Criterion, criterion_group, criterion_main};
use ic_dbms_api::prelude::{
    ColumnDef, Database, DeleteBehavior, Filter, InsertRecord, Query, QueryError, TableSchema,
    UpdateRecord, Value,
};
use ic_dbms_canister::prelude::{
    DatabaseSchema, IcDbmsDatabase, InsertIntegrityValidator, SCHEMA_REGISTRY, Table, Text, Uint64,
    UpdateIntegrityValidator, get_referenced_tables,
};
use serde::Deserialize;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    id: Uint64,
    name: Text,
    email: Text,
}

pub struct TestDatabaseSchema;

impl DatabaseSchema for TestDatabaseSchema {
    fn referenced_tables(&self, table: &'static str) -> Vec<(&'static str, Vec<&'static str>)> {
        let tables = &[(User::table_name(), User::columns())];
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
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }
}

/// Load test fixtures into the database.
fn load_fixtures(database: &mut IcDbmsDatabase, count: u64) {
    // register table User first
    SCHEMA_REGISTRY.with_borrow_mut(|registry| {
        registry
            .register_table::<User>()
            .expect("failed to register table");
    });
    for id in 0..count {
        let user = UserInsertRequest {
            id: Uint64(id),
            name: Text(format!("User_{id}")),
            email: Text(format!("user_{id}@example.com")),
        };
        database
            .insert::<User>(user)
            .expect("failed to insert user");
    }
}

/// Delete users with id multiple of provided numbers.
///
/// E.g. if divisors = [2, 3], users with id 0, 2, 3, 4, 6, ... will be deleted.
fn fragment_user_table(database: &mut IcDbmsDatabase, count: u64, divisors: &[u64]) {
    // calculate ids to delete
    let mut ids_to_delete = HashSet::new();
    for id in 0..count {
        if divisors.iter().any(|d| id % d == 0) {
            ids_to_delete.insert(id);
        }
    }

    let expected_deleted_records = ids_to_delete.len() as u64;

    let mut deleted_records = 0;
    for id in ids_to_delete {
        let filter = Filter::eq("id", Value::Uint64(id.into()));
        let deleted = database
            .delete::<User>(DeleteBehavior::Cascade, Some(filter))
            .expect("failed to delete users");
        assert_eq!(deleted, 1, "expected to delete one record");
        deleted_records += deleted;
    }

    assert_eq!(
        deleted_records, expected_deleted_records,
        "deleted records count mismatch"
    );
}

fn free_user_table(database: &mut IcDbmsDatabase) {
    database
        .delete::<User>(DeleteBehavior::Restrict, None)
        .expect("failed to delete users");
}

fn bench_read_table(c: &mut Criterion) {
    const COUNT: u64 = 10_000;

    let mut group = c.benchmark_group("read_table");
    for divisors in &[
        vec![2],
        vec![2, 3],
        vec![2, 3, 5, 7],
        vec![5, 7, 11],
        vec![7, 11, 13, 17],
    ] {
        // setup
        let label = format!(
            "divisors[{divisors}]",
            divisors = divisors
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut database = IcDbmsDatabase::oneshot(TestDatabaseSchema);
        load_fixtures(&mut database, COUNT);
        fragment_user_table(&mut database, COUNT, divisors);

        // run batch
        group.bench_with_input(label, &database, |b, database| {
            b.iter(|| {
                let query = Query::builder().all().build();
                database
                    .select::<User>(query)
                    .expect("failed to select user");
            });
        });

        // drop table otherwise we  get duplicate table registration error
        free_user_table(&mut database);
    }
}

fn configure_criterion() -> Criterion {
    Criterion::default()
        // avoid run too long benchmarks
        .measurement_time(std::time::Duration::from_secs(10))
        // less warmup time (heavy functions benefit little from warmup)
        .warm_up_time(std::time::Duration::from_secs(1))
        // reduces noise when each iteration is slow
        .sample_size(20)
        // for more readable reports
        .noise_threshold(0.05)
}

criterion_group!(name = benches; config = configure_criterion(); targets = bench_read_table);
criterion_main!(benches);
