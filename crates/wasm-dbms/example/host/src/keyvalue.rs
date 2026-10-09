//! Host implementation of the draft2 WASI key-value interfaces.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use wasmtime::component::{HasData, Resource};

wasmtime::component::bindgen!({
    world: "key-value-memory",
    path: "../../../../crates/wasi-dbms/wasi-dbms-key-value-memory/wit",
});

use self::wasi::keyvalue::{batch, store};

type BucketData = BTreeMap<String, Vec<u8>>;
type SharedBucket = Arc<Mutex<BucketData>>;

/// One-shot storage failures used by the component acceptance runner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureMode {
    /// Do not inject a failure.
    #[cfg(test)]
    None,
    /// Fail the next page batch before it is written.
    PageTrap,
    /// Fail the next manifest publication before writing the head.
    PublicationTrapBefore,
    /// Write the next manifest head and then report a failure.
    PublicationTrapAfter,
}

/// Shared backing state that survives component re-instantiation.
#[derive(Debug, Default)]
pub struct KeyValueBackend {
    buckets: BTreeMap<String, SharedBucket>,
    denied: BTreeSet<String>,
    fault: Option<FailureMode>,
}

impl KeyValueBackend {
    /// Creates a backend containing the named bucket.
    pub fn new(bucket: &str) -> Arc<Mutex<Self>> {
        let mut backend = Self::default();
        backend
            .buckets
            .insert(bucket.to_owned(), Arc::new(Mutex::new(BTreeMap::new())));
        Arc::new(Mutex::new(backend))
    }

    /// Denies access to `bucket` for subsequent opens.
    #[cfg(test)]
    pub fn deny_bucket(&mut self, bucket: &str) {
        self.denied.insert(bucket.to_owned());
    }

    /// Schedules a one-shot storage fault.
    #[cfg(test)]
    pub fn set_fault(&mut self, fault: FailureMode) {
        self.fault = Some(fault);
    }
}

/// Host state for one component instance.
#[derive(Debug)]
pub struct KeyValueState {
    backend: Arc<Mutex<KeyValueBackend>>,
    resources: BTreeMap<u32, SharedBucket>,
    next_resource: u32,
}

impl KeyValueState {
    /// Creates fresh resource bookkeeping over shared persistent buckets.
    pub fn new(backend: Arc<Mutex<KeyValueBackend>>) -> Self {
        Self {
            backend,
            resources: BTreeMap::new(),
            next_resource: 1,
        }
    }

    fn open_bucket(&mut self, identifier: String) -> Result<Resource<store::Bucket>, store::Error> {
        let bucket = {
            let backend = self
                .backend
                .lock()
                .map_err(|_| store::Error::Other("key-value backend lock poisoned".to_owned()))?;
            if backend.denied.contains(&identifier) {
                return Err(store::Error::AccessDenied);
            }
            backend
                .buckets
                .get(&identifier)
                .cloned()
                .ok_or(store::Error::NoSuchStore)?
        };
        let resource = self.next_resource;
        self.next_resource = self
            .next_resource
            .checked_add(1)
            .ok_or_else(|| store::Error::Other("key-value resource table exhausted".to_owned()))?;
        self.resources.insert(resource, bucket);
        Ok(Resource::new_own(resource))
    }

    fn bucket(&self, resource: Resource<store::Bucket>) -> Result<SharedBucket, store::Error> {
        self.resources
            .get(&resource.rep())
            .cloned()
            .ok_or_else(|| store::Error::Other("unknown key-value bucket resource".to_owned()))
    }

    fn apply_fault(&mut self, fault: FailureMode) -> bool {
        let Ok(mut backend) = self.backend.lock() else {
            return false;
        };
        if backend.fault == Some(fault) {
            backend.fault = None;
            true
        } else {
            false
        }
    }
}

impl store::Host for KeyValueState {
    fn open(&mut self, identifier: String) -> Result<Resource<store::Bucket>, store::Error> {
        self.open_bucket(identifier)
    }
}

impl store::HostBucket for KeyValueState {
    fn get(
        &mut self,
        resource: Resource<store::Bucket>,
        key: String,
    ) -> Result<Option<Vec<u8>>, store::Error> {
        let bucket = self.bucket(resource)?;
        bucket
            .lock()
            .map_err(|_| store::Error::Other("key-value bucket lock poisoned".to_owned()))
            .map(|bucket| bucket.get(&key).cloned())
    }

    fn set(
        &mut self,
        resource: Resource<store::Bucket>,
        key: String,
        value: Vec<u8>,
    ) -> Result<(), store::Error> {
        let bucket = self.bucket(resource)?;
        if key.ends_with("/head") {
            if self.apply_fault(FailureMode::PublicationTrapBefore) {
                return Err(store::Error::Other(
                    "publication fault before head write".to_owned(),
                ));
            }
            bucket
                .lock()
                .map_err(|_| store::Error::Other("key-value bucket lock poisoned".to_owned()))?
                .insert(key, value);
            if self.apply_fault(FailureMode::PublicationTrapAfter) {
                return Err(store::Error::Other(
                    "publication fault after head write".to_owned(),
                ));
            }
            return Ok(());
        }
        bucket
            .lock()
            .map_err(|_| store::Error::Other("key-value bucket lock poisoned".to_owned()))?
            .insert(key, value);
        Ok(())
    }

    fn delete(
        &mut self,
        resource: Resource<store::Bucket>,
        key: String,
    ) -> Result<(), store::Error> {
        let bucket = self.bucket(resource)?;
        bucket
            .lock()
            .map_err(|_| store::Error::Other("key-value bucket lock poisoned".to_owned()))?
            .remove(&key);
        Ok(())
    }

    fn exists(
        &mut self,
        resource: Resource<store::Bucket>,
        key: String,
    ) -> Result<bool, store::Error> {
        let bucket = self.bucket(resource)?;
        bucket
            .lock()
            .map_err(|_| store::Error::Other("key-value bucket lock poisoned".to_owned()))
            .map(|bucket| bucket.contains_key(&key))
    }

    fn list_keys(
        &mut self,
        resource: Resource<store::Bucket>,
        cursor: Option<String>,
    ) -> Result<store::KeyResponse, store::Error> {
        let bucket = self.bucket(resource)?;
        let bucket = bucket
            .lock()
            .map_err(|_| store::Error::Other("key-value bucket lock poisoned".to_owned()))?;
        let keys = bucket
            .keys()
            .filter(|key| cursor.as_ref().is_none_or(|cursor| *key > cursor))
            .cloned()
            .collect();
        Ok(store::KeyResponse { keys, cursor: None })
    }

    fn drop(&mut self, resource: Resource<store::Bucket>) -> wasmtime::Result<()> {
        self.resources.remove(&resource.rep());
        Ok(())
    }
}

impl batch::Host for KeyValueState {
    fn get_many(
        &mut self,
        resource: Resource<store::Bucket>,
        keys: Vec<String>,
    ) -> Result<Vec<(String, Option<Vec<u8>>)>, store::Error> {
        let bucket = self.bucket(resource)?;
        let bucket = bucket
            .lock()
            .map_err(|_| store::Error::Other("key-value bucket lock poisoned".to_owned()))?;
        let mut entries: Vec<_> = keys
            .into_iter()
            .map(|key| {
                let value = bucket.get(&key).cloned();
                (key, value)
            })
            .collect();
        entries.reverse();
        Ok(entries)
    }

    fn set_many(
        &mut self,
        resource: Resource<store::Bucket>,
        key_values: Vec<(String, Vec<u8>)>,
    ) -> Result<(), store::Error> {
        let partial_fault = self.apply_fault(FailureMode::PageTrap);
        let bucket = self.bucket(resource)?;
        let mut bucket = bucket
            .lock()
            .map_err(|_| store::Error::Other("key-value bucket lock poisoned".to_owned()))?;
        if partial_fault {
            if let Some(entry) = key_values.into_iter().next() {
                bucket.insert(entry.0, entry.1);
            }
            return Err(store::Error::Other("page write fault".to_owned()));
        }
        bucket.extend(key_values);
        Ok(())
    }

    fn delete_many(
        &mut self,
        resource: Resource<store::Bucket>,
        keys: Vec<String>,
    ) -> Result<(), store::Error> {
        let bucket = self.bucket(resource)?;
        let mut bucket = bucket
            .lock()
            .map_err(|_| store::Error::Other("key-value bucket lock poisoned".to_owned()))?;
        for key in keys {
            bucket.remove(&key);
        }
        Ok(())
    }
}

struct KeyValueHost;

impl HasData for KeyValueHost {
    type Data<'a> = &'a mut KeyValueState;
}

/// Adds the draft2 key-value imports to a host linker.
pub fn add_to_linker<T>(
    linker: &mut wasmtime::component::Linker<T>,
    host_getter: fn(&mut T) -> &mut KeyValueState,
) -> wasmtime::Result<()>
where
    T: 'static,
{
    KeyValueMemory::add_to_linker::<T, KeyValueHost>(linker, host_getter)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use wasmtime::component::{Component, Linker, ResourceTable};
    use wasmtime::{Config, Engine, Store};
    use wasmtime_wasi::WasiCtxBuilder;
    use wasmtime_wasi::p2::add_to_linker_sync;

    use super::*;
    use crate::wasm_dbms::dbms::types::{ColumnValue, Value};

    const BUCKET: &str = "default";
    fn instantiate(
        path: &Path,
        backend: Arc<Mutex<KeyValueBackend>>,
    ) -> wasmtime::Result<(Engine, Store<crate::HostState>, crate::Dbms)> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config)?;
        let mut linker: Linker<crate::HostState> = Linker::new(&engine);
        add_to_linker_sync(&mut linker)?;
        super::add_to_linker(&mut linker, |state| &mut state.keyvalue)?;
        let component = Component::from_file(&engine, path)?;
        let state = crate::HostState {
            wasi_ctx: WasiCtxBuilder::new().build(),
            resource_table: ResourceTable::new(),
            keyvalue: KeyValueState::new(backend),
        };
        let mut store = Store::new(&engine, state);
        let bindings = crate::Dbms::instantiate(&mut store, &component, &linker)?;
        Ok((engine, store, bindings))
    }

    fn insert_user(
        store: &mut Store<crate::HostState>,
        db: &crate::exports::wasm_dbms::dbms::database::Guest,
        id: u32,
        name: &str,
        tx: Option<u64>,
    ) -> wasmtime::Result<()> {
        let row = vec![
            ColumnValue {
                name: "id".to_owned(),
                value: Value::U32Val(id),
            },
            ColumnValue {
                name: "name".to_owned(),
                value: Value::TextVal(name.to_owned()),
            },
            ColumnValue {
                name: "email".to_owned(),
                value: Value::TextVal(format!("{name}@example.com")),
            },
        ];
        db.call_insert(store, "users", &row, tx)?
            .map_err(|error| wasmtime::Error::msg(format!("guest insert failed: {error:?}")))
    }

    fn user_names(
        store: &mut Store<crate::HostState>,
        db: &crate::exports::wasm_dbms::dbms::database::Guest,
    ) -> wasmtime::Result<Vec<String>> {
        let rows = db
            .call_select(store, "users", &crate::select_all_asc("name"))?
            .map_err(|error| wasmtime::Error::msg(format!("guest select failed: {error:?}")))?;
        let mut names = rows
            .into_iter()
            .filter_map(|row| {
                row.into_iter()
                    .find_map(|column| match (column.name.as_str(), column.value) {
                        ("name", Value::TextVal(name)) => Some(name),
                        _ => None,
                    })
            })
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }

    fn component() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../.artifact/wasm-dbms-example-guest-key-value.wasm")
    }

    /// Runs one component checkpoint/restart scenario against shared storage.
    pub fn run_checkpoint_scenario(path: &Path, mode: FailureMode) -> wasmtime::Result<Report> {
        assert!(path.is_file(), "key-value guest component must exist");
        let backend = KeyValueBackend::new(BUCKET);
        let (_engine, mut store, bindings) = instantiate(path, Arc::clone(&backend))?;
        let db = bindings.wasm_dbms_dbms_database();
        insert_user(&mut store, db, 1, "Alice", None)?;
        let before = user_names(&mut store, db)?;
        let tx = db
            .call_begin_transaction(&mut store)?
            .map_err(|error| wasmtime::Error::msg(format!("begin failed: {error:?}")))?;
        insert_user(&mut store, db, 2, "Bob", Some(tx))?;
        if mode != FailureMode::None {
            backend
                .lock()
                .map_err(|_| wasmtime::Error::msg("backend lock poisoned"))?
                .set_fault(mode);
        }
        let write_failed = db.call_commit(&mut store, tx)?.is_err();
        drop(store);

        let (_engine, mut store, bindings) = instantiate(path, backend)?;
        let after = user_names(&mut store, bindings.wasm_dbms_dbms_database())?;
        Ok(Report {
            before,
            after,
            write_failed,
        })
    }

    /// Executes the component against an inaccessible or missing bucket.
    pub fn open_with_denied_bucket(path: &Path, missing: bool) -> wasmtime::Result<String> {
        assert!(path.is_file(), "key-value guest component must exist");
        let backend = if missing {
            KeyValueBackend::new("other")
        } else {
            let backend = KeyValueBackend::new(BUCKET);
            backend
                .lock()
                .map_err(|_| wasmtime::Error::msg("backend lock poisoned"))?
                .deny_bucket(BUCKET);
            backend
        };
        let (_engine, mut store, bindings) = instantiate(path, backend)?;
        let result = bindings.wasm_dbms_dbms_database().call_select(
            &mut store,
            "users",
            &crate::empty_query(),
        )?;
        match result {
            Ok(_) => Err(wasmtime::Error::msg("bucket open unexpectedly succeeded")),
            Err(error) => Ok(format!("{error:?}")),
        }
    }

    /// Results collected from one restart scenario.
    #[derive(Debug, Eq, PartialEq)]
    pub struct Report {
        pub before: Vec<String>,
        pub after: Vec<String>,
        pub write_failed: bool,
    }

    #[test]
    fn keyvalue_components_restart_with_only_store_and_batch() -> wasmtime::Result<()> {
        let report = run_checkpoint_scenario(&component(), FailureMode::None)?;
        assert_eq!(report.before, vec!["Alice"]);
        assert_eq!(report.after, vec!["Alice", "Bob"]);
        assert!(!report.write_failed);
        Ok(())
    }

    #[test]
    fn keyvalue_components_recover_from_host_traps() -> wasmtime::Result<()> {
        for fault in [FailureMode::PageTrap, FailureMode::PublicationTrapBefore] {
            let report = run_checkpoint_scenario(&component(), fault)?;
            assert!(report.write_failed);
            assert_eq!(report.after, vec!["Alice"]);
        }
        let report = run_checkpoint_scenario(&component(), FailureMode::PublicationTrapAfter)?;
        assert!(report.write_failed);
        assert_eq!(report.after, vec!["Alice", "Bob"]);
        Ok(())
    }

    #[test]
    fn keyvalue_components_report_bucket_open_errors() -> wasmtime::Result<()> {
        let path = component();
        assert!(open_with_denied_bucket(&path, false)?.contains("access"));
        assert!(open_with_denied_bucket(&path, true)?.contains("store"));
        Ok(())
    }
}
