// Rust guideline compliant 2026-10-07
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Reads of variable-length values that copy only the bytes they span.
//!
//! [`MemoryAccess::read_at`] cannot know how long a [`DataSize::Dynamic`]
//! value is, so it copies everything from the offset to the end of the page.
//! The table registry ledgers are stored at the start of their own page and
//! are usually a few hundred bytes long, so [`read_sized`] reads a short
//! prefix first, works out the encoded length from it and then reads exactly
//! that many bytes.
//!
//! [`DataSize::Dynamic`]: wasm_dbms_api::prelude::DataSize::Dynamic

use std::borrow::Cow;

use wasm_dbms_api::prelude::{DecodeError, Encode, MemoryError, MemoryResult, Page};

use crate::MemoryAccess;

/// Number of bytes read on the first attempt.
///
/// Large enough to hold a typical ledger whole, so most loads take a single
/// read.
const INITIAL_READ_SIZE: usize = 1_024;

/// Reads the value stored at the start of `page`, copying only its encoded bytes.
///
/// `encoded_len` receives a prefix of the page and returns the total encoded
/// length of the value, or `None` when the prefix is too short to tell. The
/// prefix grows until the length is known.
///
/// # Errors
///
/// Returns [`DecodeError::TooShort`] when the encoded length does not fit in
/// the page, and propagates any read or decode error.
pub(super) fn read_sized<D, MA>(
    page: Page,
    mm: &mut MA,
    encoded_len: fn(&[u8]) -> Option<usize>,
) -> MemoryResult<D>
where
    D: Encode,
    MA: MemoryAccess,
{
    let page_size = mm.page_size() as usize;
    let mut buf = vec![0u8; INITIAL_READ_SIZE.min(page_size)];

    loop {
        mm.read_at_raw(page, 0, &mut buf)?;
        match encoded_len(&buf) {
            Some(len) if len <= buf.len() => {
                buf.truncate(len);
                return D::decode(Cow::Owned(buf));
            }
            Some(len) if len <= page_size => buf.resize(len, 0),
            None if buf.len() < page_size => buf.resize((buf.len() * 4).min(page_size), 0),
            Some(_) | None => return Err(MemoryError::DecodeError(DecodeError::TooShort)),
        }
    }
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{DEFAULT_ALIGNMENT, DataSize, MSize, PageOffset};

    use super::*;
    use crate::{HeapMemoryProvider, MemoryManager};

    /// Length-prefixed byte string used to drive [`read_sized`].
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Blob(Vec<u8>);

    impl Blob {
        fn encoded_len(prefix: &[u8]) -> Option<usize> {
            let header = prefix.get(..2)?;
            Some(2 + u16::from_le_bytes([header[0], header[1]]) as usize)
        }
    }

    impl Encode for Blob {
        const SIZE: DataSize = DataSize::Dynamic;
        const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

        fn encode(&'_ self) -> Cow<'_, [u8]> {
            let mut bytes = (self.0.len() as u16).to_le_bytes().to_vec();
            bytes.extend_from_slice(&self.0);
            Cow::Owned(bytes)
        }

        fn decode(data: Cow<[u8]>) -> MemoryResult<Self> {
            // The buffer must hold exactly the encoded value, nothing more.
            let len = u16::from_le_bytes([data[0], data[1]]) as usize;
            assert_eq!(data.len(), 2 + len, "decode got extra bytes");
            Ok(Self(data[2..].to_vec()))
        }

        fn size(&self) -> MSize {
            2 + self.0.len() as MSize
        }
    }

    fn page_with(blob: &Blob) -> (MemoryManager<HeapMemoryProvider>, Page) {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let page = mm.claim_page().expect("claim page");
        mm.write_at_raw(page, 0, &blob.encode())
            .expect("write blob");
        (mm, page)
    }

    #[test]
    fn test_reads_value_shorter_than_first_read() {
        let blob = Blob(vec![7; 10]);
        let (mut mm, page) = page_with(&blob);

        let read: Blob = read_sized(page, &mut mm, Blob::encoded_len).expect("read");
        assert_eq!(read, blob);
    }

    #[test]
    fn test_reads_value_longer_than_first_read() {
        let blob = Blob(vec![9; 40_000]);
        let (mut mm, page) = page_with(&blob);

        let read: Blob = read_sized(page, &mut mm, Blob::encoded_len).expect("read");
        assert_eq!(read, blob);
    }

    #[test]
    fn test_grows_prefix_until_length_is_known() {
        // Length stored at the end of a 5_000-byte prefix: needs two growths.
        fn late_len(prefix: &[u8]) -> Option<usize> {
            prefix.get(4_999).map(|_| 5_002)
        }
        let blob = Blob(vec![1; 5_000]);
        let (mut mm, page) = page_with(&blob);

        let read: Blob = read_sized(page, &mut mm, late_len).expect("read");
        assert_eq!(read, blob);
    }

    #[test]
    fn test_rejects_length_past_page_end() {
        let (mut mm, page) = page_with(&Blob(vec![]));

        let result: MemoryResult<Blob> = read_sized(page, &mut mm, |_| Some(70_000));
        assert!(matches!(
            result,
            Err(MemoryError::DecodeError(DecodeError::TooShort))
        ));
    }

    #[test]
    fn test_rejects_length_never_known() {
        let (mut mm, page) = page_with(&Blob(vec![]));

        let result: MemoryResult<Blob> = read_sized(page, &mut mm, |_| None);
        assert!(matches!(
            result,
            Err(MemoryError::DecodeError(DecodeError::TooShort))
        ));
    }
}
