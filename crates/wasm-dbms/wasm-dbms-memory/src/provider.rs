// Rust guideline compliant 2026-02-28

use wasm_dbms_api::prelude::{MemoryError, MemoryResult};

/// The size of a WASM memory page in bytes (64 KiB).
pub const WASM_PAGE_SIZE: u64 = 65536;

/// Memory Provider trait defines the interface for interacting with the underlying memory.
///
/// Abstracting memory access allows different implementations for production
/// (e.g. stable memory) and testing (heap-based).
pub trait MemoryProvider {
    /// The size of a memory page in bytes.
    const PAGE_SIZE: u64;

    /// Gets the current size of the memory in bytes.
    fn size(&self) -> u64;

    /// Gets the amount of pages currently allocated.
    fn pages(&self) -> u64;

    /// Attempts to grow the memory by `new_pages` (added pages).
    ///
    /// Returns an error if it wasn't possible. Otherwise, returns the previous size that was reserved.
    ///
    /// Actual reserved size after the growth will be `previous_size + (new_pages * PAGE_SIZE)`.
    fn grow(&mut self, new_pages: u64) -> MemoryResult<u64>;

    /// Reads data from memory starting at `offset` into the provided buffer `buf`.
    ///
    /// Returns an error if `offset + buf.len()` exceeds the current memory size.
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> MemoryResult<()>;

    /// Writes data from the provided buffer `buf` into memory starting at `offset`.
    ///
    /// Returns an error if `offset + buf.len()` exceeds the current memory size.
    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()>;
}

/// An implementation of [`MemoryProvider`] that uses heap memory for testing purposes.
#[derive(Debug, Default)]
pub struct HeapMemoryProvider {
    memory: Vec<u8>,
}

impl MemoryProvider for HeapMemoryProvider {
    const PAGE_SIZE: u64 = WASM_PAGE_SIZE; // 64 KiB

    fn grow(&mut self, new_pages: u64) -> MemoryResult<u64> {
        let previous_size = self.size();
        let additional_size = (new_pages * Self::PAGE_SIZE) as usize;
        self.memory
            .resize(previous_size as usize + additional_size, 0);
        Ok(previous_size)
    }

    fn size(&self) -> u64 {
        self.memory.len() as u64
    }

    fn pages(&self) -> u64 {
        self.size() / Self::PAGE_SIZE
    }

    fn read(&mut self, offset: u64, buf: &mut [u8]) -> MemoryResult<()> {
        // check if the read is within bounds
        if offset + buf.len() as u64 > self.size() {
            return Err(MemoryError::OutOfBounds);
        }

        buf.copy_from_slice(&self.memory[offset as usize..(offset as usize + buf.len())]);
        Ok(())
    }

    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()> {
        // check if the write is within bounds
        if offset + buf.len() as u64 > self.size() {
            return Err(MemoryError::OutOfBounds);
        }

        self.memory[offset as usize..(offset as usize + buf.len())].copy_from_slice(buf);
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_should_grow_heap_memory() {
        let mut provider = HeapMemoryProvider::default();
        assert_eq!(provider.size(), 0);

        let previous_size = provider.grow(2).unwrap();
        assert_eq!(previous_size, 0);
        assert_eq!(provider.size(), 2 * HeapMemoryProvider::PAGE_SIZE);

        let previous_size = provider.grow(1).unwrap();
        assert_eq!(previous_size, 2 * HeapMemoryProvider::PAGE_SIZE);
        assert_eq!(provider.size(), 3 * HeapMemoryProvider::PAGE_SIZE);
    }

    #[test]
    fn test_should_read_and_write_heap_memory() {
        let mut provider = HeapMemoryProvider::default();
        provider.grow(1).unwrap(); // grow by 1 page (64 KiB)
        let data_to_write = vec![1, 2, 3, 4, 5];
        provider.write(0, &data_to_write).unwrap();
        let mut buffer = vec![0; 5];
        provider.read(0, &mut buffer).unwrap();
        assert_eq!(buffer, data_to_write);
    }

    #[test]
    fn test_should_not_read_out_of_bounds_heap_memory() {
        let mut provider = HeapMemoryProvider::default();
        provider.grow(1).unwrap(); // grow by 1 page (64 KiB)
        let mut buffer = vec![0; 10];
        let result = provider.read(HeapMemoryProvider::PAGE_SIZE - 5, &mut buffer);
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), MemoryError::OutOfBounds));
    }

    #[test]
    fn test_should_not_write_out_of_bounds_heap_memory() {
        let mut provider = HeapMemoryProvider::default();
        provider.grow(1).unwrap(); // grow by 1 page (64 KiB)
        let data_to_write = vec![1, 2, 3, 4, 5];
        let result = provider.write(HeapMemoryProvider::PAGE_SIZE - 3, &data_to_write);
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), MemoryError::OutOfBounds));
    }

    #[test]
    fn test_should_get_amount_of_pages_heap_memory() {
        let mut provider = HeapMemoryProvider::default();
        assert_eq!(provider.pages(), 0);

        provider.grow(3).unwrap(); // grow by 3 pages
        assert_eq!(provider.pages(), 3);

        provider.grow(2).unwrap(); // grow by 2 more pages
        assert_eq!(provider.pages(), 5);
    }
}
