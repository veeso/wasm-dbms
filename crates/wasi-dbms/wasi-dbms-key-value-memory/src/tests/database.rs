use wasi_dbms_memory::WasiMemoryProvider;
use wasm_dbms::prelude::{DbmsContext, WasmDbmsDatabase};
use wasm_dbms_api::prelude::{Database as _, Query, Text, Uint32};
use wasm_dbms_macros::{DatabaseSchema, Table};
use wasm_dbms_memory::prelude::MemoryProvider;

use super::open;
use super::support::TestStore;

#[derive(Debug, Clone, PartialEq, Eq, Table)]
#[table = "items"]
pub struct Item {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
}

#[derive(DatabaseSchema)]
#[tables(Item = "items")]
pub struct Schema;

fn insert<M>(ctx: &DbmsContext<M>, id: u32, name: &str)
where
    M: MemoryProvider,
{
    WasmDbmsDatabase::oneshot(ctx, Schema)
        .insert::<Item>(ItemInsertRequest {
            id: id.into(),
            name: name.into(),
        })
        .unwrap();
}

fn names<M>(ctx: &DbmsContext<M>) -> Vec<String>
where
    M: MemoryProvider,
{
    let rows = WasmDbmsDatabase::oneshot(ctx, Schema)
        .select::<Item>(Query::builder().all().build())
        .unwrap();
    let mut result: Vec<_> = rows.into_iter().map(|row| row.name.unwrap().0).collect();
    result.sort();
    result
}

#[test]
fn flush_persists_committed_rows_but_not_active_or_rolled_back_inserts() {
    let store = TestStore::default();
    let ctx = DbmsContext::new(open(&store));
    Schema::register_tables(&ctx).unwrap();
    insert(&ctx, 1, "base");
    ctx.flush().unwrap();
    {
        let tx = ctx.begin_transaction();
        let mut db = WasmDbmsDatabase::from_transaction(&ctx, Schema, tx);
        db.insert::<Item>(ItemInsertRequest {
            id: 2.into(),
            name: "pending".into(),
        })
        .unwrap();
        ctx.flush().unwrap();
        assert_eq!(names(&DbmsContext::new(open(&store))), vec!["base"]);
        db.rollback().unwrap();
    }
    ctx.flush().unwrap();
    assert_eq!(names(&DbmsContext::new(open(&store))), vec!["base"]);
    {
        let tx = ctx.begin_transaction();
        let mut db = WasmDbmsDatabase::from_transaction(&ctx, Schema, tx);
        db.insert::<Item>(ItemInsertRequest {
            id: 3.into(),
            name: "committed".into(),
        })
        .unwrap();
        db.commit().unwrap();
    }
    ctx.flush().unwrap();
    drop(ctx);
    let reopened = DbmsContext::new(open(&store));
    assert!(reopened.has_table("items"));
    assert_eq!(names(&reopened), vec!["base", "committed"]);
}

#[test]
fn file_database_round_trips_through_key_value_pages() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "wasm-dbms-kv-{pid}-{stamp}",
        pid = std::process::id()
    ));
    std::fs::create_dir(&dir).unwrap();
    let source = dir.join("source.db");
    let result = dir.join("result.db");
    let file_ctx = DbmsContext::new(WasiMemoryProvider::new(&source).unwrap());
    Schema::register_tables(&file_ctx).unwrap();
    insert(&file_ctx, 1, "from-file");
    drop(file_ctx);
    let original = std::fs::read(&source).unwrap();
    let store = TestStore::default();
    let mut p = open(&store);
    p.import_snapshot(&original).unwrap();
    assert_eq!(p.export_snapshot().unwrap(), original);
    p.flush().unwrap();
    drop(p);
    let kv_ctx = DbmsContext::new(open(&store));
    assert_eq!(names(&kv_ctx), vec!["from-file"]);
    insert(&kv_ctx, 2, "from-kv");
    kv_ctx.flush().unwrap();
    drop(kv_ctx);
    std::fs::write(&result, open(&store).export_snapshot().unwrap()).unwrap();
    let restored = DbmsContext::new(WasiMemoryProvider::new(&result).unwrap());
    assert_eq!(names(&restored), vec!["from-file", "from-kv"]);
    drop(restored);
    std::fs::remove_dir_all(dir).unwrap();
}
