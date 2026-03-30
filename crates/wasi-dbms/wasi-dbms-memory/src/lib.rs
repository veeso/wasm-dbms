// Rust guideline compliant 2026-03-30

//! WASI file-backed [`MemoryProvider`] implementation for `wasm-dbms`.
//!
//! This crate provides [`WasiMemoryProvider`], a persistent storage backend
//! that uses a single flat file on the filesystem. It enables `wasm-dbms`
//! to run on any WASI-compliant runtime (Wasmer, Wasmtime, WasmEdge, etc.)
//! with durable data persistence.
//!
//! The backing file is byte-for-byte equivalent to IC stable memory:
//! a contiguous sequence of 64 KiB pages, zero-filled on allocation.

use std::fs::{File, OpenOptions};
use std::io::{Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};

use wasm_dbms_api::memory::{MemoryError, MemoryResult};
use wasm_dbms_memory::MemoryProvider;

/// Size of a single memory page in bytes (64 KiB).
const PAGE_SIZE: u64 = 65536;

/// File-backed [`MemoryProvider`] for WASI runtimes.
///
/// Persists database pages as a single flat file on the filesystem.
/// Each page is 64 KiB, matching the WASM memory page size. The file
/// layout is byte-for-byte equivalent to IC stable memory, enabling
/// portable database snapshots.
///
/// Single-writer access is the caller's responsibility. WASM is
/// single-threaded by default and WASI lock support varies across runtimes.
///
/// # Examples
///
/// ```no_run
/// use wasi_dbms_memory::WasiMemoryProvider;
/// use wasm_dbms_memory::MemoryProvider;
///
/// let mut provider = WasiMemoryProvider::new("./data/mydb.bin").unwrap();
/// provider.grow(1).unwrap(); // allocate 1 page (64 KiB)
///
/// let data = b"hello";
/// provider.write(0, data).unwrap();
///
/// let mut buf = vec![0u8; 5];
/// provider.read(0, &mut buf).unwrap();
/// assert_eq!(&buf, data);
/// ```
#[derive(Debug)]
pub struct WasiMemoryProvider {
    file: File,
    path: PathBuf,
    pages: u64,
}

impl WasiMemoryProvider {
    /// Opens or creates a file-backed memory provider at `path`.
    ///
    /// If the file exists, the page count is inferred from the file size.
    /// If the file does not exist, it is created empty (0 pages).
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::ProviderError`] if:
    /// - The file cannot be opened or created.
    /// - The existing file size is not a multiple of the page size (64 KiB).
    pub fn new(path: impl AsRef<Path>) -> MemoryResult<Self> {
        let path = path.as_ref();

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| MemoryError::ProviderError(e.to_string()))?;

        let file_size = file
            .metadata()
            .map_err(|e| MemoryError::ProviderError(e.to_string()))?
            .len();

        // reject files whose size isn't page-aligned
        if file_size % PAGE_SIZE != 0 {
            return Err(MemoryError::ProviderError(format!(
                "file size {file_size} is not a multiple of page size {PAGE_SIZE}"
            )));
        }

        let pages = file_size / PAGE_SIZE;

        Ok(Self {
            file,
            path: path.to_path_buf(),
            pages,
        })
    }

    /// Returns the path to the backing file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Seeks the file handle to `offset`.
    fn seek_to(&mut self, offset: u64) -> MemoryResult<()> {
        self.file
            .seek(std::io::SeekFrom::Start(offset))
            .map_err(|e| MemoryError::ProviderError(e.to_string()))?;
        Ok(())
    }
}

impl TryFrom<&Path> for WasiMemoryProvider {
    type Error = MemoryError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        Self::new(path)
    }
}

impl TryFrom<PathBuf> for WasiMemoryProvider {
    type Error = MemoryError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        Self::new(path.as_path())
    }
}

impl MemoryProvider for WasiMemoryProvider {
    const PAGE_SIZE: u64 = PAGE_SIZE;

    fn size(&self) -> u64 {
        self.pages * Self::PAGE_SIZE
    }

    fn pages(&self) -> u64 {
        self.pages
    }

    fn grow(&mut self, new_pages: u64) -> MemoryResult<u64> {
        let previous_pages = self.pages;
        let new_size = self.size() + new_pages * Self::PAGE_SIZE;

        // extend with zeros via set_len
        self.file
            .set_len(new_size)
            .map_err(|e| MemoryError::ProviderError(e.to_string()))?;

        self.pages += new_pages;
        Ok(previous_pages)
    }

    fn read(&mut self, offset: u64, buf: &mut [u8]) -> MemoryResult<()> {
        if offset + buf.len() as u64 > self.size() {
            return Err(MemoryError::OutOfBounds);
        }

        self.seek_to(offset)?;
        self.file
            .read_exact(buf)
            .map_err(|e| MemoryError::ProviderError(e.to_string()))
    }

    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()> {
        if offset + buf.len() as u64 > self.size() {
            return Err(MemoryError::OutOfBounds);
        }

        self.seek_to(offset)?;
        self.file
            .write_all(buf)
            .map_err(|e| MemoryError::ProviderError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {

    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// Atomic counter to generate unique temp file paths across tests.
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Creates a unique temporary database file path.
    ///
    /// Returns the parent directory path and the file path. The caller is
    /// responsible for cleanup (tests run in a temp dir so OS handles it).
    fn temp_db_path() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join("wasi_dbms_tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("test_{id}.db"))
    }

    /// Removes the temp file if it exists (best-effort cleanup).
    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_should_create_new_empty_file() {
        let path = temp_db_path();
        let provider = WasiMemoryProvider::new(&path).unwrap();
        assert_eq!(provider.pages(), 0);
        assert_eq!(provider.size(), 0);
        assert!(path.exists());
        cleanup(&path);
    }

    #[test]
    fn test_should_open_existing_file_with_correct_page_count() {
        let path = temp_db_path();

        // create a file with exactly 2 pages
        {
            let mut f = File::create(&path).unwrap();
            f.write_all(&vec![0u8; PAGE_SIZE as usize * 2]).unwrap();
        }

        let provider = WasiMemoryProvider::new(&path).unwrap();
        assert_eq!(provider.pages(), 2);
        assert_eq!(provider.size(), PAGE_SIZE * 2);
        cleanup(&path);
    }

    #[test]
    fn test_should_reject_non_page_aligned_file() {
        let path = temp_db_path();

        {
            let mut f = File::create(&path).unwrap();
            f.write_all(&vec![0u8; PAGE_SIZE as usize + 100]).unwrap();
        }

        let result = WasiMemoryProvider::new(&path);
        assert!(result.is_err());
        assert!(matches!(
            result.err().unwrap(),
            MemoryError::ProviderError(_)
        ));
        cleanup(&path);
    }

    #[test]
    fn test_should_grow_memory() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();
        assert_eq!(provider.pages(), 0);

        let previous = provider.grow(2).unwrap();
        assert_eq!(previous, 0);
        assert_eq!(provider.pages(), 2);
        assert_eq!(provider.size(), PAGE_SIZE * 2);

        let previous = provider.grow(1).unwrap();
        assert_eq!(previous, 2);
        assert_eq!(provider.pages(), 3);
        assert_eq!(provider.size(), PAGE_SIZE * 3);
        cleanup(&path);
    }

    #[test]
    fn test_should_read_and_write() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        let data = vec![1, 2, 3, 4, 5];
        provider.write(0, &data).unwrap();

        let mut buf = vec![0u8; 5];
        provider.read(0, &mut buf).unwrap();
        assert_eq!(buf, data);
        cleanup(&path);
    }

    #[test]
    fn test_should_write_at_arbitrary_offset() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        let data = vec![0xAA, 0xBB, 0xCC];
        provider.write(100, &data).unwrap();

        // verify zeroed region before
        let mut before = vec![0xFFu8; 100];
        provider.read(0, &mut before).unwrap();
        assert!(before.iter().all(|&b| b == 0));

        // verify written data
        let mut buf = vec![0u8; 3];
        provider.read(100, &mut buf).unwrap();
        assert_eq!(buf, data);
        cleanup(&path);
    }

    #[test]
    fn test_should_overwrite_existing_data() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        provider.write(0, &[1, 2, 3]).unwrap();
        provider.write(0, &[4, 5, 6]).unwrap();

        let mut buf = vec![0u8; 3];
        provider.read(0, &mut buf).unwrap();
        assert_eq!(buf, vec![4, 5, 6]);
        cleanup(&path);
    }

    #[test]
    fn test_should_read_and_write_across_page_boundary() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();
        provider.grow(2).unwrap();

        // write data spanning two pages
        let offset = PAGE_SIZE - 3;
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];
        provider.write(offset, &data).unwrap();

        let mut buf = vec![0u8; 6];
        provider.read(offset, &mut buf).unwrap();
        assert_eq!(buf, data);
        cleanup(&path);
    }

    #[test]
    fn test_should_not_read_out_of_bounds() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        let mut buf = vec![0u8; 10];
        let result = provider.read(PAGE_SIZE - 5, &mut buf);
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), MemoryError::OutOfBounds));
        cleanup(&path);
    }

    #[test]
    fn test_should_not_write_out_of_bounds() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        let data = vec![1, 2, 3, 4, 5];
        let result = provider.write(PAGE_SIZE - 3, &data);
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), MemoryError::OutOfBounds));
        cleanup(&path);
    }

    #[test]
    fn test_should_not_read_from_empty_provider() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();

        let mut buf = vec![0u8; 1];
        let result = provider.read(0, &mut buf);
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), MemoryError::OutOfBounds));
        cleanup(&path);
    }

    #[test]
    fn test_should_not_write_to_empty_provider() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();

        let result = provider.write(0, &[1]);
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), MemoryError::OutOfBounds));
        cleanup(&path);
    }

    #[test]
    fn test_should_persist_data_across_reopen() {
        let path = temp_db_path();

        // write data
        {
            let mut provider = WasiMemoryProvider::new(&path).unwrap();
            provider.grow(1).unwrap();
            provider.write(10, &[42, 43, 44]).unwrap();
        }

        // reopen and verify
        {
            let mut provider = WasiMemoryProvider::new(&path).unwrap();
            assert_eq!(provider.pages(), 1);

            let mut buf = vec![0u8; 3];
            provider.read(10, &mut buf).unwrap();
            assert_eq!(buf, vec![42, 43, 44]);
        }
        cleanup(&path);
    }

    #[test]
    fn test_should_grow_zero_pages() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();

        let previous = provider.grow(0).unwrap();
        assert_eq!(previous, 0);
        assert_eq!(provider.pages(), 0);
        assert_eq!(provider.size(), 0);
        cleanup(&path);
    }

    #[test]
    fn test_should_read_and_write_exact_page_boundary() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        // write at the last byte of the page
        provider.write(PAGE_SIZE - 1, &[0xFF]).unwrap();

        let mut buf = vec![0u8; 1];
        provider.read(PAGE_SIZE - 1, &mut buf).unwrap();
        assert_eq!(buf, vec![0xFF]);
        cleanup(&path);
    }

    #[test]
    fn test_should_read_and_write_empty_buffer() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        // empty reads/writes should succeed
        provider.write(0, &[]).unwrap();
        let mut buf = vec![];
        provider.read(0, &mut buf).unwrap();
        cleanup(&path);
    }

    #[test]
    fn test_should_return_path() {
        let path = temp_db_path();
        let provider = WasiMemoryProvider::new(&path).unwrap();
        assert_eq!(provider.path(), path);
        cleanup(&path);
    }

    #[test]
    fn test_should_convert_from_path_ref() {
        let path = temp_db_path();
        let provider = WasiMemoryProvider::try_from(path.as_path()).unwrap();
        assert_eq!(provider.pages(), 0);
        cleanup(&path);
    }

    #[test]
    fn test_should_convert_from_pathbuf() {
        let path = temp_db_path();
        let provider = WasiMemoryProvider::try_from(path.clone()).unwrap();
        assert_eq!(provider.pages(), 0);
        cleanup(&path);
    }

    #[test]
    fn test_should_write_full_page() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        let data = vec![0xAB; PAGE_SIZE as usize];
        provider.write(0, &data).unwrap();

        let mut buf = vec![0u8; PAGE_SIZE as usize];
        provider.read(0, &mut buf).unwrap();
        assert_eq!(buf, data);
        cleanup(&path);
    }

    #[test]
    fn test_should_grow_preserves_existing_data() {
        let path = temp_db_path();
        let mut provider = WasiMemoryProvider::new(&path).unwrap();
        provider.grow(1).unwrap();

        let data = vec![1, 2, 3, 4, 5];
        provider.write(0, &data).unwrap();

        provider.grow(1).unwrap();

        // original data intact
        let mut buf = vec![0u8; 5];
        provider.read(0, &mut buf).unwrap();
        assert_eq!(buf, data);

        // new page is zeroed
        let mut new_page = vec![0xFFu8; PAGE_SIZE as usize];
        provider.read(PAGE_SIZE, &mut new_page).unwrap();
        assert!(new_page.iter().all(|&b| b == 0));
        cleanup(&path);
    }
}
