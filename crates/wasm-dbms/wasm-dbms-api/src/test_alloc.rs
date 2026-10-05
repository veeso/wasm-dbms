//! Allocation-recording global allocator for unit tests.
//!
//! Every allocation is forwarded unchanged to [`System`]; the allocator only
//! records the largest request made by each thread, so tests running in
//! parallel do not observe each other's allocations.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Global allocator that records the largest allocation requested by the
/// current thread, then forwards to the system allocator.
struct RecordingAllocator;

thread_local! {
    static LARGEST_ALLOCATION: Cell<usize> = const { Cell::new(0) };
}

fn record(size: usize) {
    // `try_with` avoids panicking while the thread-local is being torn down.
    let _ = LARGEST_ALLOCATION.try_with(|largest| largest.set(largest.get().max(size)));
}

// SAFETY: every call is forwarded unchanged to `System`; recording a size has
// no effect on the returned memory.
unsafe impl GlobalAlloc for RecordingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: the caller upholds the `GlobalAlloc::alloc` contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller upholds the `GlobalAlloc::dealloc` contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: the caller upholds the `GlobalAlloc::alloc_zeroed` contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(new_size);
        // SAFETY: the caller upholds the `GlobalAlloc::realloc` contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: RecordingAllocator = RecordingAllocator;

/// Runs `f` and returns its result with the largest allocation, in bytes,
/// that the current thread requested while running it.
pub fn largest_allocation_during<T, F>(f: F) -> (T, usize)
where
    F: FnOnce() -> T,
{
    LARGEST_ALLOCATION.with(|largest| largest.set(0));
    let result = f();
    (result, LARGEST_ALLOCATION.with(Cell::get))
}
