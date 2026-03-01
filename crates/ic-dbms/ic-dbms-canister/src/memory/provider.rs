#[cfg(target_family = "wasm")]
use ic_dbms_api::prelude::{MemoryError, MemoryResult};
#[cfg(target_family = "wasm")]
use wasm_dbms_memory::{MemoryProvider, WASM_PAGE_SIZE};

/// An implementation of [`MemoryProvider`] that uses the Internet Computer's stable memory.
#[cfg(target_family = "wasm")]
#[derive(Default)]
pub struct IcMemoryProvider;

#[cfg(target_family = "wasm")]
impl MemoryProvider for IcMemoryProvider {
    const PAGE_SIZE: u64 = WASM_PAGE_SIZE;

    fn grow(&mut self, new_pages: u64) -> MemoryResult<u64> {
        ic_cdk::stable::stable_grow(new_pages)
            .map_err(|e| MemoryError::ProviderError(e.to_string()))
    }

    fn size(&self) -> u64 {
        self.pages() * Self::PAGE_SIZE
    }

    fn pages(&self) -> u64 {
        ic_cdk::stable::stable_size()
    }

    fn read(&self, offset: u64, buf: &mut [u8]) -> MemoryResult<()> {
        // check if the read is within bounds
        if offset + buf.len() as u64 > self.size() {
            return Err(MemoryError::OutOfBounds);
        }

        ic_cdk::stable::stable_read(offset, buf);
        Ok(())
    }

    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()> {
        // check if the write is within bounds
        if offset + buf.len() as u64 > self.size() {
            return Err(MemoryError::OutOfBounds);
        }

        ic_cdk::stable::stable_write(offset, buf);
        Ok(())
    }
}
