// Rust guideline compliant 2026-02-28

use super::free_segments_ledger::FreeSegmentTicket;
use wasm_dbms_api::prelude::{Page, PageOffset};

/// Indicates where to write a record
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WriteAt {
    /// Write at a previously allocated segment
    ReusedSegment(FreeSegmentTicket),
    /// Write at the end of the table
    End(Page, PageOffset),
}

impl WriteAt {
    /// Gets the page where to write the record
    pub fn page(&self) -> Page {
        match self {
            WriteAt::ReusedSegment(segment) => segment.segment.page,
            WriteAt::End(page, _) => *page,
        }
    }

    /// Gets the offset where to write the record
    pub fn offset(&self) -> PageOffset {
        match self {
            WriteAt::ReusedSegment(segment) => segment.segment.offset,
            WriteAt::End(_, offset) => *offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::free_segments_ledger::FreeSegment;

    #[test]
    fn test_write_at_free_segment() {
        let reused_segment = FreeSegmentTicket {
            table: 1,
            segment: FreeSegment {
                page: 1,
                offset: 100,
                size: 50,
            },
        };
        let write_at_reused = WriteAt::ReusedSegment(reused_segment);
        assert_eq!(write_at_reused.page(), 1);
        assert_eq!(write_at_reused.offset(), 100);
    }

    #[test]
    fn test_write_at_end() {
        let write_at_end = WriteAt::End(2, 200);
        assert_eq!(write_at_end.page(), 2);
        assert_eq!(write_at_end.offset(), 200);
    }
}
