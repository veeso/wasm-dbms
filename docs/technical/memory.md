# Memory Management

- [Memory Management](#memory-management)
  - [Overview](#overview)
  - [How Internet Computer Memory Works](#how-internet-computer-memory-works)
  - [Memory Model](#memory-model)
  - [Memory Provider](#memory-provider)
  - [Memory Manager and MemoryAccess](#memory-manager-and-memoryaccess)
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
    - [Index Ledger](#index-ledger)
    - [B-Tree Structure](#b-tree-structure)
    - [Internal Node Layout](#internal-node-layout)
    - [Leaf Node Layout](#leaf-node-layout)
    - [Index Maintenance](#index-maintenance)
    - [Index Tree Walker](#index-tree-walker)

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
│ Page 4: Table "users" Index Ledger          │
├─────────────────────────────────────────────┤
│ Page 5: Table "posts" Page Ledger           │
├─────────────────────────────────────────────┤
│ Page 6: Table "posts" Free Segments Ledger  │
├─────────────────────────────────────────────┤
│ Page 7: Table "posts" Index Ledger          │
├─────────────────────────────────────────────┤
│ Page 8: Table "users" Records - Page 1      │
├─────────────────────────────────────────────┤
│ Page 9: Table "users" Records - Page 2      │
├─────────────────────────────────────────────┤
│ Page 10: B-Tree Node (index on users.id)    │
├─────────────────────────────────────────────┤
│ Page 11: B-Tree Node (index on users.email) │
├─────────────────────────────────────────────┤
│ Page 12: Table "posts" Records - Page 1     │
├─────────────────────────────────────────────┤
│ ...                                         │
└─────────────────────────────────────────────┘
```

**Layout characteristics:**

- Reserved pages (0-1) are allocated at initialization
- Each table gets a Page Ledger, Free Segments Ledger, and Index Ledger
- Record pages and B-tree node pages are allocated on demand
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
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> MemoryResult<()>;

    /// Write bytes to memory at offset
    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()>;
}
```

**Implementations:**

| Implementation | Use Case |
|----------------|----------|
| `IcMemoryProvider` | IC production (uses `ic_cdk::stable::*`) |
| `WasiMemoryProvider` | WASI production (file-backed, single flat file) |
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

    fn read(&mut self, offset: u64, buf: &mut [u8]) -> MemoryResult<()> {
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
    fn read_at<D: Encode>(&mut self, page: Page, offset: PageOffset) -> MemoryResult<D>;
    fn write_at<E: Encode>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>;
    fn zero<E: Encode>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>;
    fn read_at_raw(&mut self, page: Page, offset: PageOffset, buf: &mut [u8]) -> MemoryResult<usize>;
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
    pub pages_list_page: Page,        // Page Ledger location
    pub free_segments_page: Page,     // Free Segments Ledger location
    pub index_registry_page: Page,    // Index Ledger location
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
pub struct TableRegistry {
    free_segments_ledger: FreeSegmentsLedger,
    page_ledger: PageLedger,
    index_ledger: IndexLedger,
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

```txt
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

Each table has an `IndexLedger` that maps index definitions (column sets) to B-tree root pages.
Indexes are always B+ trees where each node occupies exactly one memory page (64 KiB).

Every table automatically gets an index on its primary key. Additional indexes can be declared
with the `#[index]` attribute (see [Schema Reference](../reference/schema.md)).

### Index Ledger

The `IndexLedger` is stored in a single page per table and maps column sets to B-tree root pages:

```rust
pub struct IndexLedger {
    ledger_page: Page,
    tables: HashMap<Vec<String>, Page>,  // column names → root page
}
```

**Serialization format:**

```txt
Offset   Size    Field
0-7      8       Number of indexes (u64)
8+       var     For each index:
                 - 8 bytes: column count (u64)
                 - For each column name:
                   - 1 byte: name length (u8)
                   - N bytes: UTF-8 column name
                 - 4 bytes: root page (u32)
```

When a table is registered via `SchemaRegistry::register_table()`, the index ledger is
initialized by allocating one root page per index definition. The ledger supports
insert, delete, update, exact-match search, and range scan operations — all delegated
to the underlying B-tree for the appropriate column set.

### B-Tree Structure

Indexes use a B+ tree where values (record pointers) are stored only in leaf nodes.
Internal nodes contain separator keys that guide traversal. Each node is a single page.

```rust
struct RecordAddress {
    page: Page,         // 4 bytes, u32
    offset: PageOffset, // 2 bytes, u16
}
```

`RecordAddress` is the pointer stored in leaf entries, pointing to the exact location
of the record in the table's data pages. It is 6 bytes when serialized.

Key characteristics:

- **Variable-size keys**: Entries are packed as many as fit in a 64 KiB page
- **Non-unique**: The same key can map to multiple `RecordAddress` values
- **Linked leaves**: Leaf nodes form a doubly-linked list for range scans
- **Node type tag**: Byte 0 distinguishes internal (0x00) from leaf (0x01) nodes

### Internal Node Layout

```txt
┌──────────────────────────────────────────────────────┐
│ Byte 0:     Node type (0x00 = INTERNAL)              │
│ Bytes 1-4:  Parent page (u32, u32::MAX if root)      │
│ Bytes 5-6:  Entry count (u16)                        │
│ Bytes 7-10: Rightmost child page (u32)               │
├──────────────────────────────────────────────────────┤
│ Entry 0:                                             │
│   Bytes 0-1: Key size (u16)                          │
│   Bytes 2+:  Key data (variable)                     │
│   Next 4:    Child page (u32)                        │
├──────────────────────────────────────────────────────┤
│ Entry 1: ...                                         │
├──────────────────────────────────────────────────────┤
│ ...                                                  │
└──────────────────────────────────────────────────────┘
```

Header size: 11 bytes. Entries are sorted by key. A search for key `K` routes to the
child page of the first entry whose key is `>= K`, or to `rightmost_child` if `K`
is greater than all entries.

### Leaf Node Layout

```txt
┌──────────────────────────────────────────────────────┐
│ Byte 0:      Node type (0x01 = LEAF)                 │
│ Bytes 1-4:   Parent page (u32, u32::MAX if root)     │
│ Bytes 5-6:   Entry count (u16)                       │
│ Bytes 7-10:  Previous leaf page (u32, u32::MAX=none) │
│ Bytes 11-14: Next leaf page (u32, u32::MAX=none)     │
├──────────────────────────────────────────────────────┤
│ Entry 0:                                             │
│   Bytes 0-1: Key size (u16)                          │
│   Bytes 2+:  Key data (variable)                     │
│   Next 6:    RecordAddress (4-byte page + 2-byte     │
│              offset)                                 │
├──────────────────────────────────────────────────────┤
│ Entry 1: ...                                         │
├──────────────────────────────────────────────────────┤
│ ...                                                  │
└──────────────────────────────────────────────────────┘
```

Header size: 15 bytes. Entries are sorted by (key, record address). The
`prev_leaf` / `next_leaf` pointers form a doubly-linked list across all leaves,
enabling efficient forward and backward range scans.

### Index Maintenance

Indexes are updated eagerly on every write operation:

- **INSERT**: After writing the record and obtaining its `RecordAddress`, the key
  is inserted into every index defined on the table.
- **DELETE**: After removing the record, the key-pointer pair is removed from every
  index.
- **UPDATE**: If indexed columns changed, those indexes are updated (delete old
  key + insert new key). If the record moved (size change), all indexes are updated
  with the new `RecordAddress`.

When a leaf node overflows during insertion, it splits at its midpoint. The first
key of the new right sibling is promoted to the parent internal node. If the parent
also overflows, the split propagates upward. When the root splits, a new root is
created and the tree height increases by one.

When a leaf becomes empty after deletion (and is not the root), it is unlinked from
the leaf chain and its parent is updated.

### Index Tree Walker

Range scans use an `IndexTreeWalker` that iterates through leaf entries across
linked leaf pages:

```rust
pub struct IndexTreeWalker<K: Encode + Ord> {
    entries: Vec<LeafEntry<K>>,   // Current leaf's entries
    cursor: usize,                // Position within current leaf
    next_leaf: Option<Page>,      // Next leaf page for continuation
    end_key: Option<K>,           // Optional upper bound (inclusive)
}
```

The walker starts at the first leaf entry `>= start_key` and advances through the
linked-leaf chain until it reaches an entry `> end_key` (or exhausts all leaves).
This provides efficient iteration for range queries without revisiting internal
nodes.
