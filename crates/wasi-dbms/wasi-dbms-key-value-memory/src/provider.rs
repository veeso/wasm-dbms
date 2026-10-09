//! Cached key-value memory provider implementation.

use std::fmt::{Debug, Formatter};
use std::ops::Range;

use wasm_dbms_api::prelude::{MemoryError, MemoryResult};
use wasm_dbms_memory::MemoryProvider;
use xxhash_rust::xxh3::xxh3_64;

use crate::manifest::{Checkpoint, Manifest, head_key, page_key};
use crate::store::{KeyValueStore, WasiKeyValueStore};

/// Size of one logical database page in bytes.
const PAGE_SIZE: u64 = 65_536;
const MAX_BATCH_PAGES: usize = 16;

/// A cached memory provider backed by a key-value store.
pub struct WasiKeyValueMemoryProvider<S = WasiKeyValueStore>
where
    S: KeyValueStore,
{
    store: S,
    namespace: String,
    memory: Vec<u8>,
    pages: u64,
    published: Checkpoint,
    dirty_pages: Vec<bool>,
    fallback_checkpoint: bool,
    poisoned: bool,
}

impl<S> Debug for WasiKeyValueMemoryProvider<S>
where
    S: KeyValueStore,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WasiKeyValueMemoryProvider")
            .field("namespace", &self.namespace)
            .field("pages", &self.pages)
            .field("memory_bytes", &self.memory.len())
            .field("fallback_checkpoint", &self.fallback_checkpoint)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}

impl<S> WasiKeyValueMemoryProvider<S>
where
    S: KeyValueStore,
{
    /// Opens a provider using an injected store.
    pub fn with_store(mut store: S, name: &str) -> MemoryResult<Self> {
        let namespace = namespace(name)?;
        let manifest = match store.get(&head_key(&namespace)?)? {
            Some(bytes) => Manifest::decode(&bytes)?,
            None => Manifest::empty(),
        };
        let (memory, published, fallback_checkpoint) =
            match load_memory(&mut store, &namespace, &manifest.current) {
                Ok(memory) => (memory, manifest.current, false),
                Err(current_error) => match manifest.previous {
                    Some(previous) if previous.pages() > 0 => {
                        match load_memory(&mut store, &namespace, &previous) {
                            Ok(memory) => (memory, previous, true),
                            Err(_) => return Err(current_error),
                        }
                    }
                    Some(_) | None => return Err(current_error),
                },
            };
        let pages = published.pages();
        let page_count = usize::try_from(pages).map_err(|_| MemoryError::FailedToAllocatePage)?;
        let mut dirty_pages = Vec::new();
        dirty_pages
            .try_reserve_exact(page_count)
            .map_err(|_| MemoryError::FailedToAllocatePage)?;
        dirty_pages.resize(page_count, false);
        Ok(Self {
            store,
            namespace,
            memory,
            pages,
            published,
            dirty_pages,
            fallback_checkpoint,
            poisoned: false,
        })
    }

    /// Exports the current logical memory, including unflushed changes.
    pub fn export_snapshot(&self) -> MemoryResult<Vec<u8>> {
        self.check_poisoned()?;
        copy_bytes(&self.memory)
    }

    /// Imports a page-aligned snapshot into an empty provider.
    pub fn import_snapshot(&mut self, bytes: &[u8]) -> MemoryResult<()> {
        self.check_writable()?;
        if self.pages != 0 || !self.memory.is_empty() {
            return Err(MemoryError::ProviderError(
                "cannot import a snapshot into a non-empty provider".to_owned(),
            ));
        }

        if !bytes.len().is_multiple_of(PAGE_SIZE as usize) {
            return Err(MemoryError::ProviderError(
                "snapshot length is not page-aligned".to_owned(),
            ));
        }

        let pages = bytes.len() / PAGE_SIZE as usize;
        let pages = u64::try_from(pages).map_err(|_| MemoryError::FailedToAllocatePage)?;
        validate_page_count(pages)?;
        let page_count = usize::try_from(pages).map_err(|_| MemoryError::FailedToAllocatePage)?;
        let memory = copy_bytes(bytes)?;
        let mut dirty_pages = Vec::new();
        dirty_pages
            .try_reserve_exact(page_count)
            .map_err(|_| MemoryError::FailedToAllocatePage)?;
        dirty_pages.resize(page_count, true);
        self.memory = memory;
        self.pages = pages;
        self.published = Checkpoint::empty();
        self.dirty_pages = dirty_pages;
        Ok(())
    }

    fn checked_range(&self, offset: u64, len: usize) -> MemoryResult<Range<usize>> {
        let len = u64::try_from(len).map_err(|_| MemoryError::OutOfBounds)?;
        let end = offset
            .checked_add(len)
            .filter(|end| *end <= self.size())
            .ok_or(MemoryError::OutOfBounds)?;
        let start = usize::try_from(offset).map_err(|_| MemoryError::OutOfBounds)?;
        let end = usize::try_from(end).map_err(|_| MemoryError::OutOfBounds)?;
        Ok(start..end)
    }

    fn check_poisoned(&self) -> MemoryResult<()> {
        if self.poisoned {
            return Err(MemoryError::ProviderError(
                "key-value memory provider is poisoned; reopen it".to_owned(),
            ));
        }
        Ok(())
    }

    fn check_writable(&self) -> MemoryResult<()> {
        self.check_poisoned()?;
        if self.fallback_checkpoint {
            return Err(MemoryError::ProviderError(
                "fallback checkpoint is read-only; reopen the provider before writing".to_owned(),
            ));
        }
        Ok(())
    }

    fn mark_dirty(&mut self, range: Range<usize>) {
        range.for_each(|page| self.dirty_pages[page] = true);
    }

    fn dirty_page_numbers(&self) -> MemoryResult<Vec<usize>> {
        let count = self.dirty_pages.iter().filter(|dirty| **dirty).count();
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(count)
            .map_err(|_| MemoryError::FailedToAllocatePage)?;
        pages.extend(
            self.dirty_pages
                .iter()
                .enumerate()
                .filter_map(|(page, dirty)| dirty.then_some(page)),
        );
        Ok(pages)
    }

    fn flush_inner(&mut self) -> MemoryResult<()> {
        self.check_poisoned()?;
        let dirty_pages = self.dirty_page_numbers()?;
        if dirty_pages.is_empty() {
            return Ok(());
        }
        self.check_writable()?;

        let page_count =
            usize::try_from(self.pages).map_err(|_| MemoryError::FailedToAllocatePage)?;
        let mut candidate = self.published.try_copy()?;
        candidate
            .slots
            .try_reserve_exact(page_count.saturating_sub(candidate.slots.len()))
            .map_err(|_| MemoryError::FailedToAllocatePage)?;
        candidate
            .hashes
            .try_reserve_exact(page_count.saturating_sub(candidate.hashes.len()))
            .map_err(|_| MemoryError::FailedToAllocatePage)?;
        candidate.slots.resize(page_count, 0);
        candidate.hashes.resize(page_count, 0);
        for &page in &dirty_pages {
            candidate.slots[page] = if page < self.published.slots.len() {
                1 - self.published.slots[page]
            } else {
                0
            };
            let start = page * PAGE_SIZE as usize;
            let end = start + PAGE_SIZE as usize;
            candidate.hashes[page] = xxh3_64(&self.memory[start..end]);
        }
        let manifest = Manifest::new(candidate, Some(self.published.try_copy()?))?;
        let encoded_manifest = manifest.encode()?;

        for batch in dirty_pages.chunks(MAX_BATCH_PAGES) {
            let mut entries = Vec::new();
            entries
                .try_reserve_exact(batch.len())
                .map_err(|_| MemoryError::FailedToAllocatePage)?;
            for &page in batch {
                let slot = manifest.current.slots[page];
                let key = page_key(&self.namespace, page as u64, slot)?;
                let start = page * PAGE_SIZE as usize;
                let end = start + PAGE_SIZE as usize;
                let value = copy_bytes(&self.memory[start..end])?;
                entries.push((key, value));
            }
            self.store.set_many(&entries)?;
        }

        let head = head_key(&self.namespace)?;
        if let Err(error) = self.store.set(&head, &encoded_manifest) {
            self.poisoned = true;
            return Err(error);
        }
        self.published = manifest.current;
        self.dirty_pages.fill(false);
        self.fallback_checkpoint = false;
        Ok(())
    }
}

impl WasiKeyValueMemoryProvider<WasiKeyValueStore> {
    /// Opens a provider backed by the named WASI key-value bucket.
    pub fn new(bucket: &str, name: &str) -> MemoryResult<Self> {
        Self::with_store(WasiKeyValueStore::open(bucket)?, name)
    }
}

impl<S> MemoryProvider for WasiKeyValueMemoryProvider<S>
where
    S: KeyValueStore,
{
    const PAGE_SIZE: u64 = PAGE_SIZE;

    fn size(&self) -> u64 {
        self.pages * Self::PAGE_SIZE
    }

    fn pages(&self) -> u64 {
        self.pages
    }

    fn grow(&mut self, new_pages: u64) -> MemoryResult<u64> {
        self.check_writable()?;
        let previous_size = self.size();
        let pages = self
            .pages
            .checked_add(new_pages)
            .ok_or(MemoryError::FailedToAllocatePage)?;
        validate_page_count(pages)?;
        let new_size = pages
            .checked_mul(Self::PAGE_SIZE)
            .and_then(|size| usize::try_from(size).ok())
            .filter(|size| *size <= isize::MAX as usize)
            .ok_or(MemoryError::FailedToAllocatePage)?;
        let additional = new_size
            .checked_sub(self.memory.len())
            .ok_or(MemoryError::FailedToAllocatePage)?;
        let additional_pages =
            usize::try_from(new_pages).map_err(|_| MemoryError::FailedToAllocatePage)?;
        self.memory
            .try_reserve_exact(additional)
            .map_err(|_| MemoryError::FailedToAllocatePage)?;
        self.dirty_pages
            .try_reserve_exact(additional_pages)
            .map_err(|_| MemoryError::FailedToAllocatePage)?;
        self.memory.resize(new_size, 0);
        self.dirty_pages
            .resize(new_size / PAGE_SIZE as usize, false);
        self.dirty_pages[self.pages as usize..].fill(true);
        self.pages = pages;
        Ok(previous_size)
    }

    fn read(&mut self, offset: u64, buf: &mut [u8]) -> MemoryResult<()> {
        self.check_poisoned()?;
        let range = self.checked_range(offset, buf.len())?;
        buf.copy_from_slice(&self.memory[range]);
        Ok(())
    }

    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()> {
        self.check_writable()?;
        let range = self.checked_range(offset, buf.len())?;
        let first_page = range.start / PAGE_SIZE as usize;
        let last_page = range.end.saturating_sub(1) / PAGE_SIZE as usize;
        self.memory[range].copy_from_slice(buf);
        if !buf.is_empty() {
            self.mark_dirty(first_page..last_page + 1);
        }
        Ok(())
    }

    fn flush(&mut self) -> MemoryResult<()> {
        self.flush_inner()
    }
}

fn load_memory<S>(store: &mut S, namespace: &str, checkpoint: &Checkpoint) -> MemoryResult<Vec<u8>>
where
    S: KeyValueStore,
{
    let length = checkpoint
        .pages()
        .checked_mul(PAGE_SIZE)
        .and_then(|size| usize::try_from(size).ok())
        .filter(|size| *size <= isize::MAX as usize)
        .ok_or(MemoryError::FailedToAllocatePage)?;
    let mut memory = Vec::new();
    memory
        .try_reserve_exact(length)
        .map_err(|_| MemoryError::FailedToAllocatePage)?;
    memory.resize(length, 0);
    let key_count =
        usize::try_from(checkpoint.pages()).map_err(|_| MemoryError::FailedToAllocatePage)?;
    let mut keys = Vec::new();
    keys.try_reserve_exact(key_count)
        .map_err(|_| MemoryError::FailedToAllocatePage)?;
    for (page, slot) in checkpoint.slots.iter().enumerate() {
        keys.push(page_key(namespace, page as u64, *slot)?);
    }
    for (batch_number, batch) in keys.chunks(MAX_BATCH_PAGES).enumerate() {
        let batch_start = batch_number * MAX_BATCH_PAGES;
        let values = store.get_many(batch)?;
        if values.len() != batch.len() {
            return Err(MemoryError::ProviderError(
                "key-value page batch omitted a key".to_owned(),
            ));
        }
        let mut seen = Vec::new();
        seen.try_reserve_exact(batch.len())
            .map_err(|_| MemoryError::FailedToAllocatePage)?;
        for (key, value) in values {
            let page = resolve_page_in_batch(batch, &key, batch_start)?;
            if seen.contains(&page) {
                return Err(MemoryError::ProviderError(
                    "key-value page batch returned a duplicate key".to_owned(),
                ));
            }
            seen.push(page);
            let value = value.ok_or_else(|| {
                MemoryError::ProviderError("key-value page value is missing".to_owned())
            })?;
            if value.len() != PAGE_SIZE as usize {
                return Err(MemoryError::ProviderError(
                    "key-value page has an invalid length".to_owned(),
                ));
            }
            if xxh3_64(&value) != checkpoint.hashes[page] {
                return Err(MemoryError::ProviderError(
                    "key-value page hash does not match checkpoint".to_owned(),
                ));
            }
            let start = page * PAGE_SIZE as usize;
            memory[start..start + PAGE_SIZE as usize].copy_from_slice(&value);
        }
        if seen.len() != batch.len()
            || (batch_start..batch_start + batch.len()).any(|page| !seen.contains(&page))
        {
            return Err(MemoryError::ProviderError(
                "key-value page batch omitted a key".to_owned(),
            ));
        }
    }
    Ok(memory)
}

pub(crate) fn resolve_page_in_batch(
    batch: &[String],
    key: &str,
    batch_start: usize,
) -> MemoryResult<usize> {
    batch
        .iter()
        .position(|expected| expected == key)
        .and_then(|page| batch_start.checked_add(page))
        .ok_or_else(|| {
            MemoryError::ProviderError("key-value page batch returned an unexpected key".to_owned())
        })
}

fn copy_bytes(bytes: &[u8]) -> MemoryResult<Vec<u8>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len())
        .map_err(|_| MemoryError::FailedToAllocatePage)?;
    copy.extend_from_slice(bytes);
    Ok(copy)
}

fn validate_page_count(pages: u64) -> MemoryResult<()> {
    if pages > u32::MAX as u64 {
        return Err(MemoryError::ProviderError(
            "page count exceeds the engine limit".to_owned(),
        ));
    }
    let size = pages
        .checked_mul(PAGE_SIZE)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value <= isize::MAX as usize);
    if size.is_none() {
        return Err(MemoryError::FailedToAllocatePage);
    }
    Ok(())
}

fn namespace(name: &str) -> MemoryResult<String> {
    if name.is_empty() {
        return Err(MemoryError::ProviderError(
            "database name cannot be empty".to_owned(),
        ));
    }
    let capacity = name
        .len()
        .checked_mul(2)
        .and_then(|length| length.checked_add("wasm-dbms/".len()))
        .ok_or(MemoryError::FailedToAllocatePage)?;
    let mut result = String::new();
    result
        .try_reserve_exact(capacity)
        .map_err(|_| MemoryError::FailedToAllocatePage)?;
    result.push_str("wasm-dbms/");
    name.bytes().for_each(|byte| {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    });
    Ok(result)
}
