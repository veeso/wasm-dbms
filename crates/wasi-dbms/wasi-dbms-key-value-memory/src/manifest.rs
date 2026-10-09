//! Checkpoint manifest encoding and storage-key helpers.

use std::fmt::Write as _;

use wasm_dbms_api::prelude::{MemoryError, MemoryResult};

const MAGIC: &[u8; 8] = b"WDBMSKV2";
const COUNT_SIZE: usize = std::mem::size_of::<u64>();
const HEADER_SIZE: usize = MAGIC.len() + COUNT_SIZE * 2;
const ENTRY_SIZE: usize = std::mem::size_of::<u8>() + std::mem::size_of::<u64>();
const NO_PREVIOUS_CHECKPOINT: u64 = u64::MAX;
const MAX_PAGES: u64 = u32::MAX as u64;
const PAGE_SIZE: u64 = 65_536;

#[derive(Debug)]
pub(crate) struct Checkpoint {
    pub(crate) slots: Vec<u8>,
    pub(crate) hashes: Vec<u64>,
}

impl Checkpoint {
    pub(crate) fn new(slots: Vec<u8>, hashes: Vec<u64>) -> MemoryResult<Self> {
        if slots.len() != hashes.len() {
            return Err(invalid_manifest(
                "checkpoint slot and hash counts do not match",
            ));
        }
        let pages = u64::try_from(slots.len()).map_err(|_| MemoryError::FailedToAllocatePage)?;
        validate_page_count(pages)?;
        if slots.iter().any(|slot| *slot > 1) {
            return Err(invalid_manifest("manifest contains an invalid slot"));
        }
        Ok(Self { slots, hashes })
    }

    pub(crate) fn empty() -> Self {
        Self {
            slots: Vec::new(),
            hashes: Vec::new(),
        }
    }

    pub(crate) fn pages(&self) -> u64 {
        self.slots.len() as u64
    }

    pub(crate) fn try_copy(&self) -> MemoryResult<Self> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(self.slots.len())
            .map_err(|_| MemoryError::FailedToAllocatePage)?;
        slots.extend_from_slice(&self.slots);
        let mut hashes = Vec::new();
        hashes
            .try_reserve_exact(self.hashes.len())
            .map_err(|_| MemoryError::FailedToAllocatePage)?;
        hashes.extend_from_slice(&self.hashes);
        Ok(Self { slots, hashes })
    }
}

#[derive(Debug)]
pub(crate) struct Manifest {
    pub(crate) current: Checkpoint,
    pub(crate) previous: Option<Checkpoint>,
}

impl Manifest {
    pub(crate) fn new(current: Checkpoint, previous: Option<Checkpoint>) -> MemoryResult<Self> {
        validate_page_count(current.pages())?;
        if let Some(previous) = &previous {
            validate_page_count(previous.pages())?;
        }
        Ok(Self { current, previous })
    }

    pub(crate) fn empty() -> Self {
        Self {
            current: Checkpoint::empty(),
            previous: None,
        }
    }

    pub(crate) fn encode(&self) -> MemoryResult<Vec<u8>> {
        let current_count = self.current.pages();
        let previous_count = self
            .previous
            .as_ref()
            .map_or(NO_PREVIOUS_CHECKPOINT, Checkpoint::pages);
        let entries = self
            .current
            .slots
            .len()
            .checked_add(
                self.previous
                    .as_ref()
                    .map_or(0, |checkpoint| checkpoint.slots.len()),
            )
            .ok_or(MemoryError::FailedToAllocatePage)?;
        let length = HEADER_SIZE
            .checked_add(
                entries
                    .checked_mul(ENTRY_SIZE)
                    .ok_or(MemoryError::FailedToAllocatePage)?,
            )
            .ok_or(MemoryError::FailedToAllocatePage)?;
        let mut encoded = Vec::new();
        encoded
            .try_reserve_exact(length)
            .map_err(|_| MemoryError::FailedToAllocatePage)?;
        encoded.extend_from_slice(MAGIC);
        encoded.extend_from_slice(&current_count.to_le_bytes());
        encoded.extend_from_slice(&previous_count.to_le_bytes());
        encode_checkpoint(&mut encoded, &self.current);
        if let Some(previous) = &self.previous {
            encode_checkpoint(&mut encoded, previous);
        }
        Ok(encoded)
    }

    pub(crate) fn decode(bytes: &[u8]) -> MemoryResult<Self> {
        if bytes.len() < HEADER_SIZE || &bytes[..MAGIC.len()] != MAGIC {
            return Err(invalid_manifest("manifest header is invalid"));
        }
        let current_count = decode_count(bytes, MAGIC.len())?;
        let previous_count = decode_count(bytes, MAGIC.len() + COUNT_SIZE)?;
        validate_page_count(current_count)?;
        if previous_count != NO_PREVIOUS_CHECKPOINT {
            validate_page_count(previous_count)?;
        }
        let current_count =
            usize::try_from(current_count).map_err(|_| MemoryError::FailedToAllocatePage)?;
        let previous_count = if previous_count == NO_PREVIOUS_CHECKPOINT {
            None
        } else {
            Some(usize::try_from(previous_count).map_err(|_| MemoryError::FailedToAllocatePage)?)
        };
        let entries = current_count
            .checked_add(previous_count.unwrap_or(0))
            .ok_or(MemoryError::FailedToAllocatePage)?;
        let expected_length = HEADER_SIZE
            .checked_add(
                entries
                    .checked_mul(ENTRY_SIZE)
                    .ok_or(MemoryError::FailedToAllocatePage)?,
            )
            .ok_or(MemoryError::FailedToAllocatePage)?;
        if bytes.len() != expected_length {
            return Err(invalid_manifest(
                "manifest length does not match page count",
            ));
        }
        let (current, offset) = decode_checkpoint(bytes, HEADER_SIZE, current_count)?;
        let previous = previous_count
            .map(|count| decode_checkpoint(bytes, offset, count).map(|(checkpoint, _)| checkpoint))
            .transpose()?;
        Self::new(current, previous)
    }
}

fn encode_checkpoint(encoded: &mut Vec<u8>, checkpoint: &Checkpoint) {
    for (&slot, &hash) in checkpoint.slots.iter().zip(&checkpoint.hashes) {
        encoded.push(slot);
        encoded.extend_from_slice(&hash.to_le_bytes());
    }
}

fn decode_count(bytes: &[u8], offset: usize) -> MemoryResult<u64> {
    let end = offset
        .checked_add(COUNT_SIZE)
        .ok_or(MemoryError::FailedToAllocatePage)?;
    Ok(u64::from_le_bytes(bytes[offset..end].try_into().map_err(
        |_| invalid_manifest("manifest page count is invalid"),
    )?))
}

fn decode_checkpoint(
    bytes: &[u8],
    offset: usize,
    pages: usize,
) -> MemoryResult<(Checkpoint, usize)> {
    let mut slots = Vec::new();
    let mut hashes = Vec::new();
    slots
        .try_reserve_exact(pages)
        .map_err(|_| MemoryError::FailedToAllocatePage)?;
    hashes
        .try_reserve_exact(pages)
        .map_err(|_| MemoryError::FailedToAllocatePage)?;
    let mut cursor = offset;
    for _ in 0..pages {
        let hash_start = cursor
            .checked_add(1)
            .ok_or(MemoryError::FailedToAllocatePage)?;
        let end = hash_start
            .checked_add(std::mem::size_of::<u64>())
            .ok_or(MemoryError::FailedToAllocatePage)?;
        slots.push(bytes[cursor]);
        hashes.push(u64::from_le_bytes(
            bytes[hash_start..end]
                .try_into()
                .map_err(|_| invalid_manifest("manifest page hash is invalid"))?,
        ));
        cursor = end;
    }
    Checkpoint::new(slots, hashes).map(|checkpoint| (checkpoint, cursor))
}

pub(crate) fn head_key(namespace: &str) -> MemoryResult<String> {
    append_key(namespace, "/head")
}

pub(crate) fn page_key(namespace: &str, page: u64, slot: u8) -> MemoryResult<String> {
    if slot > 1 || page > MAX_PAGES {
        return Err(invalid_manifest("page key contains an invalid address"));
    }
    let mut key = String::new();
    let capacity = namespace
        .len()
        .checked_add(32)
        .ok_or(MemoryError::FailedToAllocatePage)?;
    key.try_reserve_exact(capacity)
        .map_err(|_| MemoryError::FailedToAllocatePage)?;
    write!(&mut key, "{namespace}/page/{page}/{slot}")
        .map_err(|_| MemoryError::ProviderError("failed to build page key".to_owned()))?;
    Ok(key)
}

fn append_key(namespace: &str, suffix: &str) -> MemoryResult<String> {
    let capacity = namespace
        .len()
        .checked_add(suffix.len())
        .ok_or(MemoryError::FailedToAllocatePage)?;
    let mut key = String::new();
    key.try_reserve_exact(capacity)
        .map_err(|_| MemoryError::FailedToAllocatePage)?;
    key.push_str(namespace);
    key.push_str(suffix);
    Ok(key)
}

fn validate_page_count(pages: u64) -> MemoryResult<()> {
    if pages > MAX_PAGES {
        return Err(invalid_manifest("page count exceeds the engine limit"));
    }
    let bytes = pages
        .checked_mul(PAGE_SIZE)
        .and_then(|size| usize::try_from(size).ok())
        .filter(|size| *size <= isize::MAX as usize);
    if bytes.is_none() {
        return Err(MemoryError::FailedToAllocatePage);
    }
    Ok(())
}

fn invalid_manifest(message: &str) -> MemoryError {
    MemoryError::ProviderError(message.to_owned())
}
