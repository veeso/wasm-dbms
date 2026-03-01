# Memory Management

- [Memory Management](#memory-management)
  - [Overview](#overview)
  - [How Internet Computer Memory Works](#how-internet-computer-memory-works)
  - [Memory Model](#memory-model)
  - [Memory Provider](#memory-provider)
  - [Memory Manager](#memory-manager)
  - [Encode Trait](#encode-trait)
  - [Schema Registry](#schema-registry)
  - [ACL Storage](#acl-storage)
  - [Table Registry](#table-registry)
    - [Page Ledger](#page-ledger)
    - [Free Segments Ledger](#free-segments-ledger)
  - [Record Storage](#record-storage)
    - [Record Encoding](#record-encoding)
    - [Record Alignment](#record-alignment)
    - [Table Reader](#table-reader)
  - [Index Registry](#index-registry)

---

## Overview

This document provides the technical details of memory management in wasm-dbms, also known as Layer 0 (the Memory Layer). Understanding this layer is useful for:

- Performance optimization
- Debugging memory issues
- Contributing to wasm-dbms
- Understanding storage costs

---

## How Internet Computer Memory Works

On the Internet Computer, canisters have access to stable memory that persists across upgrades. Key characteristics:

- **Page-based**: Memory is divided into 64 KiB (65,536 bytes) pages
- **Growable**: Canisters start small and can allocate additional pages
- **Persistent**: Survives canister upgrades
- **Limited**: Subject to subnet memory limits

wasm-dbms uses stable memory directly (not the heap) to ensure data persistence.

---

## Memory Model

```txt
┌─────────────────────────────────────────────┐
│ Page 0: Schema Registry (65 KiB)            │
│   - Table fingerprints → Page ledger pages  │
├─────────────────────────────────────────────┤
│ Page 1: ACL Table (65 KiB)                  │
│   - List of allowed principals              │
├─────────────────────────────────────────────┤
│ Page 2: Table "users" Page Ledger           │
├─────────────────────────────────────────────┤
│ Page 3: Table "users" Free Segments Ledger  │
├─────────────────────────────────────────────┤
│ Page 4: Table "posts" Page Ledger           │
├─────────────────────────────────────────────┤
│ Page 5: Table "posts" Free Segments Ledger  │
├─────────────────────────────────────────────┤
│ Page 6: Table "users" Records - Page 1      │
├─────────────────────────────────────────────┤
│ Page 7: Table "users" Records - Page 2      │
├─────────────────────────────────────────────┤
│ Page 8: Table "posts" Records - Page 1      │
├─────────────────────────────────────────────┤
│ ...                                         │
└─────────────────────────────────────────────┘
```

**Layout characteristics:**

- Reserved pages (0-1) are allocated at initialization
- Each table gets its own Page Ledger and Free Segments Ledger
- Record pages are allocated on demand
- Pages can be interleaved between tables

---

## Memory Provider

The `MemoryProvider` trait abstracts memory access:

```rust
pub trait MemoryProvider {
    /// Size of a memory page in bytes (64 KiB for IC)
    const PAGE_SIZE: u64;

    /// Current memory size in bytes
    fn size(&self) -> u64;

    /// Number of allocated pages
    fn pages(&self) -> u64;

    /// Grow memory by new_pages
    /// Returns previous size on success
    fn grow(&mut self, new_pages: u64) -> MemoryResult<u64>;

    /// Read bytes from memory at offset
    fn read(&self, offset: u64, buf: &mut [u8]) -> MemoryResult<()>;

    /// Write bytes to memory at offset
    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()>;
}
```

**Implementations:**

| Implementation | Use Case |
|----------------|----------|
| `IcMemoryProvider` | Production (uses `ic_cdk::stable::*`) |
| `HeapMemoryProvider` | Testing (uses `Vec<u8>`) |

```rust
// Production: Uses IC stable memory APIs
pub struct IcMemoryProvider;

#[cfg(target_family = "wasm")]
impl MemoryProvider for IcMemoryProvider {
    const PAGE_SIZE: u64 = ic_cdk::stable::WASM_PAGE_SIZE_IN_BYTES;

    fn grow(&mut self, new_pages: u64) -> MemoryResult<u64> {
        ic_cdk::stable::stable_grow(new_pages)
            .map_err(MemoryError::ProviderError)
    }

    fn read(&self, offset: u64, buf: &mut [u8]) -> MemoryResult<()> {
        ic_cdk::stable::stable_read(offset, buf);
        Ok(())
    }

    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()> {
        ic_cdk::stable::stable_write(offset, buf);
        Ok(())
    }
}

// Testing: Uses heap memory
pub struct HeapMemoryProvider {
    memory: Vec<u8>,
}
```

---

## Memory Manager and MemoryAccess

The `MemoryManager` builds on `MemoryProvider` to handle page allocation. Its page-level
read/write operations are exposed through the `MemoryAccess` trait, which allows the DBMS layer
to substitute a journaled writer for atomic transactions (see [Atomicity](./atomicity.md)).

```rust
/// Abstracts page-level read/write operations.
///
/// `MemoryManager` implements this trait directly. The DBMS layer provides
/// `JournaledWriter`, which wraps a `MemoryManager` and records original
/// bytes before each write for rollback support.
pub trait MemoryAccess {
    fn page_size(&self) -> u64;
    fn allocate_page(&mut self) -> MemoryResult<Page>;
    fn read_at<D: Encode>(&self, page: Page, offset: PageOffset) -> MemoryResult<D>;
    fn write_at<E: Encode>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>;
    fn zero<E: Encode>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>;
    fn read_at_raw(&self, page: Page, offset: PageOffset, buf: &mut [u8]) -> MemoryResult<usize>;
}
```

```rust
pub struct MemoryManager<P: MemoryProvider> {
    provider: P,
}

// Global instance (thread-local for IC)
// All state is consolidated in a single DbmsContext:
thread_local! {
    pub static DBMS_CONTEXT: DbmsContext<IcMemoryProvider> =
        DbmsContext::new(IcMemoryProvider::default());
}

impl<P: MemoryProvider> MemoryManager<P> {
    /// Initialize and allocate reserved pages
    fn init(provider: P) -> Self;

    /// ACL page number (always 1)
    pub const fn acl_page(&self) -> Page;

    /// Schema registry page (always 0)
    pub const fn schema_page(&self) -> Page;
}

// MemoryAccess is implemented for MemoryManager<P>,
// delegating directly to the underlying MemoryProvider.
impl<P: MemoryProvider> MemoryAccess for MemoryManager<P> { /* ... */ }
```

All table-registry and ledger functions are generic over `impl MemoryAccess` rather than
taking `&[mut] MemoryManager` directly. This makes it possible to intercept writes at the
DBMS layer without modifying any memory-crate code.

---

## Encode Trait

All data stored in memory implements the `Encode` trait:

```rust
pub trait Encode {
    /// Size characteristic: Fixed or Dynamic
    const SIZE: DataSize;

    /// Memory alignment in bytes
    /// - For Fixed: must equal size
    /// - For Dynamic: minimum 8, default 32
    const ALIGNMENT: PageOffset;

    /// Encode to bytes
    fn encode(&'_ self) -> Cow<'_, [u8]>;

    /// Decode from bytes
    fn decode(data: Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized;

    /// Size of encoded data
    fn size(&self) -> MSize;
}

pub enum DataSize {
    /// Fixed size in bytes (e.g., integers)
    Fixed(MSize),
    /// Variable size (e.g., strings, blobs)
    Dynamic,
}
```

**Examples:**

| Type | SIZE | ALIGNMENT |
|------|------|-----------|
| `Uint32` | `Fixed(4)` | 4 |
| `Int64` | `Fixed(8)` | 8 |
| `Text` | `Dynamic` | 32 (default) |
| `Blob` | `Dynamic` | 32 (default) |
| User-defined record | `Dynamic` | Configurable (default 32) |

---

## Schema Registry

The Schema Registry maps tables to their storage pages:

```rust
/// Information about a table's storage pages
pub struct TableRegistryPage {
    pub pages_list_page: Page,      // Page Ledger location
    pub free_segments_page: Page,   // Free Segments Ledger location
}

/// Maps table fingerprints to storage locations
pub struct SchemaRegistry {
    tables: HashMap<TableFingerprint, TableRegistryPage>,
}
```

**Table Fingerprint:**
- Unique identifier derived from table schema
- Used to detect schema changes on upgrade
- Enables multiple tables in one canister

---

## ACL Storage

The Access Control List is stored in Page 1. Access control is abstracted
behind the `AccessControl` trait, which allows different runtimes to use
different identity types (e.g., `Principal` on IC, `Vec<u8>` for generic use).

```rust
pub trait AccessControl: Default {
    type Id;

    fn load<M>(mm: &MemoryManager<M>) -> MemoryResult<Self>
    where
        M: MemoryProvider,
        Self: Sized;

    fn is_allowed(&self, identity: &Self::Id) -> bool;
    fn allowed_identities(&self) -> Vec<Self::Id>;
    fn add_identity<M>(&mut self, identity: Self::Id, mm: &mut MemoryManager<M>) -> MemoryResult<()>
    where
        M: MemoryProvider;
    fn remove_identity<M>(&mut self, identity: &Self::Id, mm: &mut MemoryManager<M>) -> MemoryResult<()>
    where
        M: MemoryProvider;
}
```

The default implementation `AccessControlList` uses `Vec<u8>` as its identity
type. `NoAccessControl` is a no-op implementation (with `type Id = ()`) for
runtimes that don't require ACL. The IC layer provides `IcAccessControlList`
which wraps `AccessControlList` and uses `Principal` as its identity type.

---

## Table Registry

Each table has a `TableRegistry` managing its records:

```rust
pub struct TableRegistry<E: Encode> {
    _marker: PhantomData<E>,
    free_segments_ledger: FreeSegmentsLedger,
    page_ledger: PageLedger,
}
```

### Page Ledger

Tracks which pages contain records for this table:

```rust
pub struct PageLedger {
    ledger_page: Page,      // Where this ledger is stored
    pages: PageTable,       // List of data pages with free space info
}

impl PageLedger {
    /// Load from memory
    pub fn load(page: Page) -> MemoryResult<Self>;

    /// Get page for writing a record
    /// Returns existing page with space or allocates new
    pub fn get_page_for_record<R: Encode>(&mut self, record: &R) -> MemoryResult<Page>;

    /// Commit allocation (update free space tracking)
    pub fn commit<R: Encode>(&mut self, page: Page, record: &R) -> MemoryResult<()>;
}
```

### Free Segments Ledger

Tracks free space from deleted/moved records:

```rust
pub struct FreeSegmentsLedger {
    free_segments_page: Page,
    tables: PagesTable,  // Pages containing FreeSegmentsTables
}

pub struct FreeSegment {
    pub page: Page,
    pub offset: PageOffset,
    pub size: MSize,
}

impl FreeSegmentsLedger {
    /// Insert a free segment (when record is deleted)
    pub fn insert_free_segment<E: Encode>(
        &mut self,
        page: Page,
        offset: PageOffset,
        record: &E,
    ) -> MemoryResult<()>;

    /// Find reusable space for a record
    pub fn find_reusable_segment<E: Encode>(
        &self,
        record: &E,
    ) -> MemoryResult<Option<FreeSegmentTicket>>;

    /// Commit reused space
    pub fn commit_reused_space<E: Encode>(
        &mut self,
        record: &E,
        segment: FreeSegmentTicket,
    ) -> MemoryResult<()>;
}
```

**Space reuse logic:**

1. When a record is deleted, its space is added to free segments
2. When inserting, check for suitable free segment first
3. If found, reuse the space; remaining space becomes new free segment
4. Adjacent free segments are merged to reduce fragmentation

---

## Record Storage

### Record Encoding

Records are wrapped in `RawRecord` with a length header:

```
┌─────────────────────────────────────────┐
│  2 bytes: Data length (little-endian)   │
├─────────────────────────────────────────┤
│  N bytes: Encoded data                  │
├─────────────────────────────────────────┤
│  Padding to alignment boundary          │
└─────────────────────────────────────────┘
```

**Dynamic size example (alignment=32, data=24 bytes):**

```txt
Bytes 0-1:   Data length (24)
Bytes 2-25:  Data (24 bytes)
Bytes 26-31: Padding (6 bytes)
Total: 32 bytes (aligned)
```

**Fixed size example (size=14 bytes):**

```txt
Bytes 0-1:   Data length (14)
Bytes 2-15:  Data (14 bytes)
Total: 16 bytes (no padding for fixed)
```

### Record Alignment

Alignment ensures efficient memory access:

```rust
impl<E: Encode> Alignment for E {
    fn alignment() -> usize {
        match E::SIZE {
            DataSize::Fixed(size) => size as usize,
            DataSize::Dynamic => E::ALIGNMENT as usize,
        }
    }
}

fn align_up<E: Encode>(size: usize) -> usize {
    let align = E::alignment();
    (size + align - 1) / align * align
}
```

**Configuring alignment:**

```rust
#[derive(Table, ...)]
#[table = "large_records"]
#[alignment = 64]  // Custom alignment for this table
pub struct LargeRecord {
    // ...
}
```

### Table Reader

Reading records from a table:

```rust
impl<E: Encode> TableRegistry<E> {
    pub fn read_all(&self) -> MemoryResult<Vec<E>> {
        let mut records = Vec::new();

        for page in self.page_ledger.pages() {
            let mut offset = 0;

            while offset < PAGE_SIZE {
                // Read length header
                let len = read_u16_le(page, offset);

                if len == 0 {
                    // Skip empty slot
                    offset += E::alignment();
                    continue;
                }

                // Read and decode record
                let data = read_bytes(page, offset + 2, len);
                let record = E::decode(data)?;
                records.push(record);

                // Move to next aligned position
                offset += align_up::<E>(len + 2);
            }
        }

        Ok(records)
    }
}
```

**Read process:**

1. Read 2 bytes at offset for data length
2. If length is 0, skip to next aligned position
3. Read `length` bytes of data
4. Decode data into record
5. Move to next aligned position
6. Repeat until end of page

---

## Index Registry

> **Note:** Index Registry is reserved for future use (RFU).

Planned features:

- Secondary indexes for faster queries
- B-tree or hash-based indexes
- Automatic index updates on insert/update/delete
