//! Provider selection for the component guest example.

#[cfg(not(feature = "key-value"))]
use std::path::Path;

#[cfg(feature = "key-value")]
use wasi_dbms_key_value_memory::WasiKeyValueMemoryProvider;
use wasm_dbms_api::prelude::MemoryResult;

#[cfg(not(feature = "key-value"))]
use crate::file_provider::FileMemoryProvider;

/// The provider used by the guest component.
#[cfg(feature = "key-value")]
pub type SelectedProvider = WasiKeyValueMemoryProvider;

/// The provider used by the guest component.
#[cfg(not(feature = "key-value"))]
pub type SelectedProvider = FileMemoryProvider;

/// The key-value bucket used by the component example.
#[cfg(feature = "key-value")]
const KEY_VALUE_BUCKET: &str = "default";

/// The database namespace used by the component example.
#[cfg(feature = "key-value")]
const KEY_VALUE_DATABASE: &str = "example";

/// Opens the provider selected by the `key-value` feature.
pub fn open_provider() -> MemoryResult<SelectedProvider> {
    #[cfg(feature = "key-value")]
    {
        WasiKeyValueMemoryProvider::new(KEY_VALUE_BUCKET, KEY_VALUE_DATABASE)
    }

    #[cfg(not(feature = "key-value"))]
    {
        open_provider_at(Path::new("wasm-dbms.db"))
    }
}

/// Opens the file-backed provider at `path` for native example tests.
#[cfg(not(feature = "key-value"))]
pub fn open_provider_at(path: &Path) -> MemoryResult<SelectedProvider> {
    FileMemoryProvider::new(path)
}
