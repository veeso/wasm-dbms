// Rust guideline compliant 2026-02-28

//! File-backed implementation of [`MemoryProvider`].
//!
//! [`FileMemoryProvider`] stores pages in a regular file on disk,
//! making it suitable for native host runtimes that need persistent
//! storage outside of WASM stable memory.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use wasm_dbms_api::prelude::{MemoryError, MemoryResult};
use wasm_dbms_memory::prelude::{MemoryProvider, WASM_PAGE_SIZE};

/// A [`MemoryProvider`] backed by a file on disk.
///
/// Pages are persisted across process restarts, which makes this
/// provider useful for testing and for native host applications
/// that embed the wasm-dbms engine.
#[derive(Debug)]
pub struct FileMemoryProvider {
    file: File,
    path: PathBuf,
    size: u64,
}

impl FileMemoryProvider {
    /// Opens or creates a file-backed memory provider at the given path.
    ///
    /// If the file already exists its current length is used as the
    /// initial memory size.
    pub fn new(path: impl Into<PathBuf>) -> MemoryResult<Self> {
        let path = path.into();
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| MemoryError::StableMemoryError(e.to_string()))?;

        let size = file
            .metadata()
            .map_err(|e| MemoryError::StableMemoryError(e.to_string()))?
            .len();

        Ok(Self { file, path, size })
    }

    /// Returns the file path backing this provider.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl MemoryProvider for FileMemoryProvider {
    const PAGE_SIZE: u64 = WASM_PAGE_SIZE;

    fn size(&self) -> u64 {
        self.size
    }

    fn pages(&self) -> u64 {
        self.size / Self::PAGE_SIZE
    }

    fn grow(&mut self, new_pages: u64) -> MemoryResult<u64> {
        let previous_size = self.size;
        let new_size = previous_size + new_pages * Self::PAGE_SIZE;
        self.file
            .set_len(new_size)
            .map_err(|e| MemoryError::StableMemoryError(e.to_string()))?;
        self.size = new_size;
        Ok(previous_size)
    }

    fn read(&self, offset: u64, buf: &mut [u8]) -> MemoryResult<()> {
        if offset + buf.len() as u64 > self.size {
            return Err(MemoryError::OutOfBounds);
        }

        // `File` implements `Read` for `&File`, so shared access is
        // sufficient here.
        let mut reader = &self.file;
        reader
            .seek(SeekFrom::Start(offset))
            .map_err(|e| MemoryError::StableMemoryError(e.to_string()))?;
        reader
            .read_exact(buf)
            .map_err(|e| MemoryError::StableMemoryError(e.to_string()))?;
        Ok(())
    }

    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()> {
        if offset + buf.len() as u64 > self.size {
            return Err(MemoryError::OutOfBounds);
        }

        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|e| MemoryError::StableMemoryError(e.to_string()))?;
        self.file
            .write_all(buf)
            .map_err(|e| MemoryError::StableMemoryError(e.to_string()))?;
        self.file
            .flush()
            .map_err(|e| MemoryError::StableMemoryError(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    /// Creates a temporary file path for testing.
    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wasm_dbms_test_{name}_{}", std::process::id()))
    }

    /// Removes a test file if it exists.
    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_initial_state() {
        let path = temp_path("initial_state");
        cleanup(&path);

        let provider = FileMemoryProvider::new(&path).unwrap();
        assert_eq!(provider.size(), 0);
        assert_eq!(provider.pages(), 0);

        cleanup(&path);
    }

    #[test]
    fn test_grow_and_size() {
        let path = temp_path("grow_and_size");
        cleanup(&path);

        let mut provider = FileMemoryProvider::new(&path).unwrap();

        let prev = provider.grow(2).unwrap();
        assert_eq!(prev, 0);
        assert_eq!(provider.size(), 2 * WASM_PAGE_SIZE);
        assert_eq!(provider.pages(), 2);

        let prev = provider.grow(1).unwrap();
        assert_eq!(prev, 2 * WASM_PAGE_SIZE);
        assert_eq!(provider.size(), 3 * WASM_PAGE_SIZE);
        assert_eq!(provider.pages(), 3);

        cleanup(&path);
    }

    #[test]
    fn test_write_and_read() {
        let path = temp_path("write_and_read");
        cleanup(&path);

        let mut provider = FileMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        let data = vec![10, 20, 30, 40, 50];
        provider.write(0, &data).unwrap();

        let mut buf = vec![0u8; 5];
        provider.read(0, &mut buf).unwrap();
        assert_eq!(buf, data);

        cleanup(&path);
    }

    #[test]
    fn test_read_out_of_bounds() {
        let path = temp_path("read_oob");
        cleanup(&path);

        let mut provider = FileMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        let mut buf = vec![0u8; 10];
        let result = provider.read(WASM_PAGE_SIZE - 5, &mut buf);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MemoryError::OutOfBounds));

        cleanup(&path);
    }

    #[test]
    fn test_write_out_of_bounds() {
        let path = temp_path("write_oob");
        cleanup(&path);

        let mut provider = FileMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        let data = vec![1, 2, 3, 4, 5];
        let result = provider.write(WASM_PAGE_SIZE - 3, &data);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MemoryError::OutOfBounds));

        cleanup(&path);
    }

    #[test]
    fn test_persistence_across_reopen() {
        let path = temp_path("persistence");
        cleanup(&path);

        // Write data with the first provider instance.
        {
            let mut provider = FileMemoryProvider::new(&path).unwrap();
            provider.grow(1).unwrap();
            provider.write(100, &[0xAA, 0xBB, 0xCC]).unwrap();
        }

        // Reopen and verify the data survives.
        {
            let provider = FileMemoryProvider::new(&path).unwrap();
            assert_eq!(provider.size(), WASM_PAGE_SIZE);
            assert_eq!(provider.pages(), 1);

            let mut buf = vec![0u8; 3];
            provider.read(100, &mut buf).unwrap();
            assert_eq!(buf, vec![0xAA, 0xBB, 0xCC]);
        }

        cleanup(&path);
    }
}
