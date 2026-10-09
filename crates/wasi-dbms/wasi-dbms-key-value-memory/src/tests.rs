mod database;
mod support;

use support::{TestStore, fail_allocation_of};
use wasm_dbms_api::prelude::{MemoryError, MemoryResult};
use wasm_dbms_memory::MemoryProvider;
use xxhash_rust::xxh3::xxh3_64;

use crate::WasiKeyValueMemoryProvider as Provider;
use crate::provider::resolve_page_in_batch;

const P: u64 = 65_536;

fn open(store: &TestStore) -> Provider<TestStore> {
    Provider::with_store(store.clone(), "db").unwrap()
}

fn read_all(p: &mut Provider<TestStore>) -> Vec<u8> {
    let mut bytes = vec![0; p.size() as usize];
    p.read(0, &mut bytes).unwrap();
    bytes
}

fn assert_provider_error<T>(result: MemoryResult<T>, expected: &str) {
    match result {
        Err(MemoryError::ProviderError(message)) => assert_eq!(message, expected),
        Err(error) => panic!("expected provider error `{expected}`, got `{error}`"),
        Ok(_) => panic!("expected provider error `{expected}`, got success"),
    }
}

#[test]
fn cached_memory_preserves_ranges_and_zero_fills_growth() {
    let store = TestStore::default();
    let mut p = Provider::with_store(store.clone(), "db").unwrap();
    assert_eq!((p.size(), p.pages()), (0, 0));
    assert_eq!(p.grow(2).unwrap(), 0);
    assert_eq!(p.grow(0).unwrap(), 2 * P);
    store.reset_counts();
    p.write(P - 2, &[1, 2, 3, 4]).unwrap();
    let mut bytes = [9; 8];
    p.read(P - 4, &mut bytes).unwrap();
    assert_eq!(bytes, [0, 0, 1, 2, 3, 4, 0, 0]);
    p.write(P, &[8]).unwrap();
    p.read(P - 4, &mut bytes).unwrap();
    assert_eq!(bytes, [0, 0, 1, 2, 8, 4, 0, 0]);
    assert_eq!(store.host_calls(), 0);
}

#[test]
fn rejected_ranges_and_growth_do_not_change_memory() {
    let mut p = Provider::with_store(TestStore::default(), "db").unwrap();
    p.grow(1).unwrap();
    p.write(0, &[42]).unwrap();
    let before = p.export_snapshot().unwrap();
    for (offset, len) in [(P - 1, 2), (P + 1, 0), (u64::MAX, 1)] {
        assert!(matches!(
            p.write(offset, &vec![7; len]),
            Err(MemoryError::OutOfBounds)
        ));
        assert!(matches!(
            p.read(offset, &mut vec![0; len]),
            Err(MemoryError::OutOfBounds)
        ));
    }
    p.write(P, &[]).unwrap();
    p.read(P, &mut []).unwrap();
    for growth in [u64::MAX, u32::MAX as u64, 1_u64 << 40] {
        assert!(p.grow(growth).is_err());
        assert_eq!(p.export_snapshot().unwrap(), before);
    }
}

#[test]
fn snapshot_import_validates_before_changing_state() {
    let mut p = Provider::with_store(TestStore::default(), "db").unwrap();
    assert!(p.import_snapshot(&[1]).is_err());
    assert_eq!(p.pages(), 0);
    let mut bytes = vec![0; P as usize];
    bytes[31] = 71;
    p.import_snapshot(&bytes).unwrap();
    assert_eq!(p.export_snapshot().unwrap(), bytes);
    assert!(p.import_snapshot(&vec![3; P as usize]).is_err());
    assert_eq!(p.export_snapshot().unwrap(), bytes);
}

#[test]
fn flush_persists_only_dirty_pages_and_drop_does_not_publish() {
    let store = TestStore::default();
    let mut p = open(&store);
    p.grow(3).unwrap();
    p.write(0, &[1]).unwrap();
    p.flush().unwrap();
    store.reset_counts();
    p.write(P, &[2]).unwrap();
    p.write(P + 1, &[3]).unwrap();
    p.flush().unwrap();
    assert_eq!(store.page_sets(), 1);
    store.reset_counts();
    p.flush().unwrap();
    assert_eq!(store.host_calls(), 0);
    let expected = read_all(&mut p);
    p.write(0, &[9]).unwrap();
    drop(p);
    store.reverse_reads();
    assert_eq!(read_all(&mut open(&store)), expected);
}

#[test]
fn checkpoint_copy_allocation_failure_is_reported() {
    const CHILD_ENV: &str = "WASM_DBMS_CHECKPOINT_ALLOCATION_FAILURE_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        for size in [2, 16] {
            for occurrence in [1, 2] {
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "tests::checkpoint_copy_allocation_failure_is_reported",
                    ])
                    .env(CHILD_ENV, format!("{size}:{occurrence}"))
                    .status()
                    .unwrap();
                assert!(
                    status.success(),
                    "checkpoint {size}-byte copy {occurrence} allocation-failure child aborted"
                );
            }
        }
        return;
    }

    let store = TestStore::default();
    let mut p = open(&store);
    p.grow(2).unwrap();
    p.flush().unwrap();
    p.write(0, &[1]).unwrap();
    let failure = std::env::var(CHILD_ENV).unwrap();
    let (size, occurrence) = failure.split_once(':').unwrap();
    fail_allocation_of(size.parse().unwrap(), occurrence.parse().unwrap());
    assert!(matches!(p.flush(), Err(MemoryError::FailedToAllocatePage)));
}

#[test]
fn every_partial_page_batch_preserves_old_checkpoint_and_can_retry() {
    for applied in 0..18 {
        let store = TestStore::default();
        let mut p = open(&store);
        p.grow(2).unwrap();
        p.write(0, &[11]).unwrap();
        p.flush().unwrap();
        let old = read_all(&mut p);
        p.grow(16).unwrap();
        for page in 0..18 {
            p.write(page * P, &[21 + page as u8]).unwrap();
        }
        let next = read_all(&mut p);
        store.fail_after_page_sets(applied);
        assert!(p.flush().is_err());
        assert_eq!(read_all(&mut open(&store)), old);
        store.clear_faults();
        p.flush().unwrap();
        assert_eq!(read_all(&mut open(&store)), next);
    }
}

#[test]
fn failed_publication_requires_reopen_and_never_exposes_mixed_pages() {
    for applied in [false, true] {
        let store = TestStore::default();
        let mut p = open(&store);
        p.grow(2).unwrap();
        p.flush().unwrap();
        let old = read_all(&mut p);
        p.write(P - 1, &[7, 8]).unwrap();
        let next = read_all(&mut p);
        store.fail_head_set(applied);
        assert!(p.flush().is_err());
        assert!(p.flush().is_err());
        assert!(p.write(0, &[99]).is_err());
        assert!(p.read(0, &mut [0]).is_err());
        assert!(p.grow(1).is_err());
        assert!(p.export_snapshot().is_err());
        assert!(p.import_snapshot(&[]).is_err());
        drop(p);
        store.clear_faults();
        let mut reopened = open(&store);
        assert_eq!(read_all(&mut reopened), if applied { next } else { old });
        reopened.write(0, &[33]).unwrap();
        reopened.flush().unwrap();
        assert_eq!(read_all(&mut open(&store))[0], 33);
    }
}

#[test]
fn reopen_falls_back_when_current_checkpoint_page_is_stale() {
    let store = TestStore::default();
    let mut p = open(&store);
    p.grow(2).unwrap();
    p.write(0, &[11]).unwrap();
    p.write(P, &[12]).unwrap();
    p.flush().unwrap();
    let previous = read_all(&mut p);

    p.write(0, &[21]).unwrap();
    p.write(P, &[22]).unwrap();
    p.flush().unwrap();
    drop(p);

    store.put_raw("wasm-dbms/6462/page/0/1", vec![0; P as usize]);
    let mut reopened = open(&store);
    assert_eq!(read_all(&mut reopened), previous);
    let fallback = reopened.export_snapshot().unwrap();
    assert_provider_error(
        reopened.write(0, &[31]),
        "fallback checkpoint is read-only; reopen the provider before writing",
    );
    assert_provider_error(
        reopened.grow(1),
        "fallback checkpoint is read-only; reopen the provider before writing",
    );
    assert_provider_error(
        reopened.import_snapshot(&[]),
        "fallback checkpoint is read-only; reopen the provider before writing",
    );
    assert_eq!(reopened.export_snapshot().unwrap(), fallback);
    store.reset_counts();
    reopened.flush().unwrap();
    assert_eq!(store.host_calls(), 0);

    let mut current_page = vec![0; P as usize];
    current_page[0] = 21;
    store.put_raw("wasm-dbms/6462/page/0/1", current_page);
    assert_provider_error(
        reopened.write(0, &[31]),
        "fallback checkpoint is read-only; reopen the provider before writing",
    );
    drop(reopened);

    let mut current = open(&store);
    current.write(0, &[31]).unwrap();
    current.flush().unwrap();
    assert_eq!(read_all(&mut open(&store))[0], 31);
}

#[test]
fn reopen_falls_back_after_a_later_batch_omits_a_new_page() {
    let store = TestStore::default();
    let mut p = open(&store);
    p.grow(2).unwrap();
    p.write(0, &[11]).unwrap();
    p.write(P, &[12]).unwrap();
    p.flush().unwrap();
    let previous = read_all(&mut p);

    p.grow(16).unwrap();
    p.write(0, &[21]).unwrap();
    p.write(17 * P, &[22]).unwrap();
    p.flush().unwrap();
    drop(p);

    store.remove_raw("wasm-dbms/6462/page/17/0");
    store.reverse_reads();
    let mut reopened = open(&store);
    assert_eq!(reopened.pages(), 2);
    assert_eq!(read_all(&mut reopened), previous);
}

#[test]
fn batch_key_resolution_is_relative_to_the_current_batch() {
    let keys = vec![
        "wasm-dbms/db/page/32/0".to_owned(),
        "wasm-dbms/db/page/33/1".to_owned(),
    ];

    assert_eq!(
        resolve_page_in_batch(&keys, "wasm-dbms/db/page/33/1", 32).unwrap(),
        33
    );
    assert_provider_error(
        resolve_page_in_batch(&keys, "wasm-dbms/db/page/31/1", 32),
        "key-value page batch returned an unexpected key",
    );
}

#[test]
fn interrupted_first_flush_and_namespace_separation_are_safe() {
    let store = TestStore::default();
    assert!(Provider::with_store(store.clone(), "").is_err());
    let mut p = open(&store);
    p.grow(2).unwrap();
    store.fail_after_page_sets(1);
    assert!(p.flush().is_err());
    drop(p);
    store.clear_faults();
    assert_eq!(open(&store).pages(), 0);
    for (name, value) in [("a/b", 1), ("a", 2), ("61", 3)] {
        let mut p = Provider::with_store(store.clone(), name).unwrap();
        p.grow(1).unwrap();
        p.write(0, &[value]).unwrap();
        p.flush().unwrap();
    }
    for (name, value) in [("a/b", 1), ("a", 2), ("61", 3)] {
        let mut p = Provider::with_store(store.clone(), name).unwrap();
        assert_eq!(read_all(&mut p)[0], value);
    }
}

#[test]
fn invalid_heads_and_referenced_pages_are_errors() {
    const HEAD: &str = "wasm-dbms/6462/head";
    let mut valid = b"WDBMSKV2".to_vec();
    valid.extend_from_slice(&1_u64.to_le_bytes());
    valid.extend_from_slice(&u64::MAX.to_le_bytes());
    valid.push(0);
    valid.extend_from_slice(&xxh3_64(&vec![0; P as usize]).to_le_bytes());
    let mut bad_slot = valid.clone();
    bad_slot[24] = 2;
    let mut bad_count = b"WDBMSKV2".to_vec();
    bad_count.extend_from_slice(&u64::MAX.to_le_bytes());
    bad_count.extend_from_slice(&u64::MAX.to_le_bytes());
    let mut trailing = valid.clone();
    trailing.push(0);
    for (head, expected) in [
        (vec![], "manifest header is invalid"),
        (b"WDBMSKV1".to_vec(), "manifest header is invalid"),
        (bad_slot, "manifest contains an invalid slot"),
        (bad_count, "page count exceeds the engine limit"),
        (trailing, "manifest length does not match page count"),
    ] {
        let store = TestStore::default();
        store.put_raw(HEAD, head);
        assert_provider_error(Provider::with_store(store, "db"), expected);
    }
    for (page, expected) in [
        (None, "key-value page value is missing"),
        (Some(vec![0; 1]), "key-value page has an invalid length"),
        (
            Some(vec![0; P as usize + 1]),
            "key-value page has an invalid length",
        ),
    ] {
        let store = TestStore::default();
        store.put_raw(HEAD, valid.clone());
        if let Some(page) = page {
            store.put_raw("wasm-dbms/6462/page/0/0", page);
        }
        assert_provider_error(Provider::with_store(store, "db"), expected);
    }
    let store = TestStore::default();
    let mut p = open(&store);
    p.grow(1).unwrap();
    p.flush().unwrap();
    p.write(0, &[1]).unwrap();
    p.flush().unwrap();
    store.remove_raw("wasm-dbms/6462/page/0/0");
    store.omit_next_read();
    assert_provider_error(
        Provider::with_store(store, "db"),
        "key-value page batch omitted a key",
    );
}

#[test]
fn invalid_first_checkpoint_does_not_fall_back_to_its_empty_predecessor() {
    let store = TestStore::default();
    let mut p = open(&store);
    p.grow(1).unwrap();
    p.write(0, &[1]).unwrap();
    p.flush().unwrap();
    drop(p);

    store.put_raw("wasm-dbms/6462/page/0/0", vec![0; P as usize]);
    assert_provider_error(
        Provider::with_store(store, "db"),
        "key-value page hash does not match checkpoint",
    );
}
