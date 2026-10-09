use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use wasm_dbms_api::prelude::{MemoryError, MemoryResult};

use crate::KeyValueStore;

struct FailingAllocator;

thread_local! {
    static FAILED_ALLOCATION: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

// SAFETY: allocations are forwarded unchanged to `System` unless the current
// test thread armed one matching failure, in which case returning null follows
// the `GlobalAlloc` contract.
unsafe impl GlobalAlloc for FailingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if should_fail(layout.size()) {
            std::ptr::null_mut()
        } else {
            // SAFETY: the caller upholds the `GlobalAlloc::alloc` contract.
            unsafe { System.alloc(layout) }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller upholds the `GlobalAlloc::dealloc` contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if should_fail(layout.size()) {
            std::ptr::null_mut()
        } else {
            // SAFETY: the caller upholds the `GlobalAlloc::alloc_zeroed` contract.
            unsafe { System.alloc_zeroed(layout) }
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if should_fail(new_size) {
            std::ptr::null_mut()
        } else {
            // SAFETY: the caller upholds the `GlobalAlloc::realloc` contract.
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }
}

#[global_allocator]
static ALLOCATOR: FailingAllocator = FailingAllocator;

pub fn fail_allocation_of(size: usize, occurrence: usize) {
    FAILED_ALLOCATION.with(|failure| failure.set(Some((size, occurrence))));
}

fn should_fail(size: usize) -> bool {
    FAILED_ALLOCATION
        .try_with(|failure| match failure.get() {
            Some((failed_size, 1)) if failed_size == size => {
                failure.set(None);
                true
            }
            Some((failed_size, occurrence)) if failed_size == size => {
                failure.set(Some((failed_size, occurrence - 1)));
                false
            }
            Some(_) | None => false,
        })
        .unwrap_or(false)
}

#[derive(Clone, Debug, Default)]
pub struct TestStore {
    state: Rc<RefCell<TestStoreState>>,
}

#[derive(Debug, Default)]
struct TestStoreState {
    values: BTreeMap<String, Vec<u8>>,
    page_sets: usize,
    host_calls: usize,
    fail_after_page_sets: Option<usize>,
    fail_head_set: Option<bool>,
    reverse_reads: bool,
    omit_next_read: bool,
}

impl TestStore {
    pub fn fail_after_page_sets(&self, count: usize) {
        self.state.borrow_mut().fail_after_page_sets = Some(count);
    }

    pub fn fail_head_set(&self, apply_before_failure: bool) {
        self.state.borrow_mut().fail_head_set = Some(apply_before_failure);
    }

    pub fn clear_faults(&self) {
        let mut state = self.state.borrow_mut();
        state.fail_after_page_sets = None;
        state.fail_head_set = None;
        state.omit_next_read = false;
    }

    pub fn reset_counts(&self) {
        let mut state = self.state.borrow_mut();
        state.page_sets = 0;
        state.host_calls = 0;
    }

    pub fn page_sets(&self) -> usize {
        self.state.borrow().page_sets
    }

    pub fn host_calls(&self) -> usize {
        self.state.borrow().host_calls
    }

    pub fn put_raw(&self, key: &str, value: Vec<u8>) {
        self.state.borrow_mut().values.insert(key.to_owned(), value);
    }

    pub fn remove_raw(&self, key: &str) {
        self.state.borrow_mut().values.remove(key);
    }

    pub fn reverse_reads(&self) {
        self.state.borrow_mut().reverse_reads = true;
    }

    pub fn omit_next_read(&self) {
        self.state.borrow_mut().omit_next_read = true;
    }

    fn is_page_key(key: &str) -> bool {
        key.contains("/page/")
    }
}

impl KeyValueStore for TestStore {
    fn get(&mut self, key: &str) -> MemoryResult<Option<Vec<u8>>> {
        let mut state = self.state.borrow_mut();
        state.host_calls += 1;
        Ok(state.values.get(key).cloned())
    }

    fn set(&mut self, key: &str, value: &[u8]) -> MemoryResult<()> {
        let mut state = self.state.borrow_mut();
        state.host_calls += 1;
        if key.ends_with("/head")
            && let Some(apply_before_failure) = state.fail_head_set.take()
        {
            if apply_before_failure {
                state.values.insert(key.to_owned(), value.to_vec());
            }
            return Err(MemoryError::ProviderError("head set failed".to_owned()));
        }
        state.values.insert(key.to_owned(), value.to_vec());
        Ok(())
    }

    fn get_many(&mut self, keys: &[String]) -> MemoryResult<Vec<(String, Option<Vec<u8>>)>> {
        let mut state = self.state.borrow_mut();
        state.host_calls += 1;
        let mut values: Vec<_> = keys
            .iter()
            .map(|key| (key.clone(), state.values.get(key).cloned()))
            .collect();
        if state.reverse_reads {
            values.reverse();
        }
        if state.omit_next_read {
            state.omit_next_read = false;
            values.pop();
        }
        Ok(values)
    }

    fn set_many(&mut self, entries: &[(String, Vec<u8>)]) -> MemoryResult<()> {
        let mut state = self.state.borrow_mut();
        state.host_calls += 1;
        for (key, value) in entries {
            if Self::is_page_key(key)
                && let Some(limit) = state.fail_after_page_sets
                && state.page_sets >= limit
            {
                state.fail_after_page_sets = None;
                return Err(MemoryError::ProviderError("page set failed".to_owned()));
            }
            state.values.insert(key.clone(), value.clone());
            if Self::is_page_key(key) {
                state.page_sets += 1;
            }
        }
        Ok(())
    }
}
