use std::collections::HashMap;

use wasm_dbms_api::prelude::MemoryError;
use wasm_dbms_memory::prelude::MemoryProvider;

const PAGE_SIZE: usize = 65_536; // 64 KiB, matches WASM_PAGE_SIZE

/// Benchmark-optimized memory provider using a page-indexed HashMap.
///
/// Unlike `HeapMemoryProvider` (contiguous `Vec<u8>`), this avoids
/// reallocation overhead on grow — each page is an independent
/// heap allocation keyed by page number.
#[derive(Default)]
pub struct HashMapMemoryProvider {
    pages: HashMap<u64, Box<[u8; PAGE_SIZE]>>,
    num_pages: u64,
}

impl MemoryProvider for HashMapMemoryProvider {
    const PAGE_SIZE: u64 = PAGE_SIZE as u64;

    fn size(&self) -> u64 {
        self.num_pages * Self::PAGE_SIZE
    }

    fn pages(&self) -> u64 {
        self.num_pages
    }

    fn grow(&mut self, new_pages: u64) -> wasm_dbms_api::prelude::MemoryResult<u64> {
        let previous_size = self.size();
        for i in 0..new_pages {
            let page_num = self.num_pages + i;
            self.pages.insert(page_num, Box::new([0u8; PAGE_SIZE]));
        }
        self.num_pages += new_pages;
        Ok(previous_size)
    }

    fn read(&self, offset: u64, buf: &mut [u8]) -> wasm_dbms_api::prelude::MemoryResult<()> {
        if offset + buf.len() as u64 > self.size() {
            return Err(MemoryError::OutOfBounds);
        }

        let mut remaining = buf.len();
        let mut buf_offset = 0usize;
        let mut mem_offset = offset;

        while remaining > 0 {
            let page_num = mem_offset / Self::PAGE_SIZE;
            let page_offset = (mem_offset % Self::PAGE_SIZE) as usize;
            let page = self.pages.get(&page_num).ok_or(MemoryError::OutOfBounds)?;

            let bytes_in_page = (PAGE_SIZE - page_offset).min(remaining);
            buf[buf_offset..buf_offset + bytes_in_page]
                .copy_from_slice(&page[page_offset..page_offset + bytes_in_page]);

            buf_offset += bytes_in_page;
            mem_offset += bytes_in_page as u64;
            remaining -= bytes_in_page;
        }

        Ok(())
    }

    fn write(&mut self, offset: u64, buf: &[u8]) -> wasm_dbms_api::prelude::MemoryResult<()> {
        if offset + buf.len() as u64 > self.size() {
            return Err(MemoryError::OutOfBounds);
        }

        let mut remaining = buf.len();
        let mut buf_offset = 0usize;
        let mut mem_offset = offset;

        while remaining > 0 {
            let page_num = mem_offset / Self::PAGE_SIZE;
            let page_offset = (mem_offset % Self::PAGE_SIZE) as usize;
            let page = self
                .pages
                .get_mut(&page_num)
                .ok_or(MemoryError::OutOfBounds)?;

            let bytes_in_page = (PAGE_SIZE - page_offset).min(remaining);
            page[page_offset..page_offset + bytes_in_page]
                .copy_from_slice(&buf[buf_offset..buf_offset + bytes_in_page]);

            buf_offset += bytes_in_page;
            mem_offset += bytes_in_page as u64;
            remaining -= bytes_in_page;
        }

        Ok(())
    }
}
