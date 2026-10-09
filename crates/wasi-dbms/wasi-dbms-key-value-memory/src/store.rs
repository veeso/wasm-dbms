//! Storage seams for the key-value memory provider.

use wasm_dbms_api::prelude::{MemoryError, MemoryResult};

#[cfg(target_os = "wasi")]
use crate::bindings::wasi::keyvalue::{batch, store};

/// Narrow storage interface used by the cached provider.
pub trait KeyValueStore {
    /// Reads one value from `key`.
    fn get(&mut self, key: &str) -> MemoryResult<Option<Vec<u8>>>;

    /// Replaces the value at `key`.
    fn set(&mut self, key: &str, value: &[u8]) -> MemoryResult<()>;

    /// Reads several keys and returns their key/value pairs.
    fn get_many(&mut self, keys: &[String]) -> MemoryResult<Vec<(String, Option<Vec<u8>>)>>;

    /// Replaces several values.
    fn set_many(&mut self, entries: &[(String, Vec<u8>)]) -> MemoryResult<()>;
}

/// Default WASI key-value store adapter.
///
/// Native users can inject a [`KeyValueStore`] implementation with
/// [`crate::WasiKeyValueMemoryProvider::with_store`].
#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Default)]
pub struct WasiKeyValueStore;

#[cfg(target_os = "wasi")]
/// Default WASI key-value store adapter.
pub struct WasiKeyValueStore {
    bucket: store::Bucket,
}

#[cfg(target_os = "wasi")]
impl std::fmt::Debug for WasiKeyValueStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WasiKeyValueStore")
            .field("bucket", &self.bucket)
            .finish()
    }
}

impl WasiKeyValueStore {
    /// Opens a WASI key-value bucket identified by `identifier`.
    pub fn open(identifier: &str) -> MemoryResult<Self> {
        open_store(identifier)
    }
}

#[cfg(not(target_os = "wasi"))]
fn open_store(_identifier: &str) -> MemoryResult<WasiKeyValueStore> {
    Err(unsupported())
}

#[cfg(target_os = "wasi")]
fn open_store(identifier: &str) -> MemoryResult<WasiKeyValueStore> {
    store::open(identifier)
        .map(|bucket| WasiKeyValueStore { bucket })
        .map_err(|error| map_error("open", error))
}

#[cfg(not(target_os = "wasi"))]
impl KeyValueStore for WasiKeyValueStore {
    fn get(&mut self, _key: &str) -> MemoryResult<Option<Vec<u8>>> {
        Err(unsupported())
    }

    fn set(&mut self, _key: &str, _value: &[u8]) -> MemoryResult<()> {
        Err(unsupported())
    }

    fn get_many(&mut self, _keys: &[String]) -> MemoryResult<Vec<(String, Option<Vec<u8>>)>> {
        Err(unsupported())
    }

    fn set_many(&mut self, _entries: &[(String, Vec<u8>)]) -> MemoryResult<()> {
        Err(unsupported())
    }
}

#[cfg(target_os = "wasi")]
impl KeyValueStore for WasiKeyValueStore {
    fn get(&mut self, key: &str) -> MemoryResult<Option<Vec<u8>>> {
        self.bucket
            .get(key)
            .map_err(|error| map_error("get", error))
    }

    fn set(&mut self, key: &str, value: &[u8]) -> MemoryResult<()> {
        self.bucket
            .set(key, value)
            .map_err(|error| map_error("set", error))
    }

    fn get_many(&mut self, keys: &[String]) -> MemoryResult<Vec<(String, Option<Vec<u8>>)>> {
        batch::get_many(&self.bucket, keys).map_err(|error| map_error("get-many", error))
    }

    fn set_many(&mut self, entries: &[(String, Vec<u8>)]) -> MemoryResult<()> {
        batch::set_many(&self.bucket, entries).map_err(|error| map_error("set-many", error))
    }
}

#[cfg(not(target_os = "wasi"))]
fn unsupported() -> MemoryError {
    MemoryError::ProviderError("WASI key-value storage is unavailable on this target".to_owned())
}

#[cfg(target_os = "wasi")]
fn map_error(operation: &str, error: store::Error) -> MemoryError {
    let detail = match error {
        store::Error::NoSuchStore => "no such store".to_owned(),
        store::Error::AccessDenied => "access denied".to_owned(),
        store::Error::Other(message) => message,
    };
    MemoryError::ProviderError(format!(
        "WASI key-value operation `{operation}` failed: {detail}"
    ))
}
