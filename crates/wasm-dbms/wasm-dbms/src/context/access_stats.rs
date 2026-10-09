//! Storage access counters used to prove which query path ran.

/// The kind of storage access counted by [`AccessStats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessKind {
    /// A record decoded by address after an index lookup.
    RecordFetch,
    /// A row yielded by a full-table scan, including transaction rows.
    ScannedRow,
    /// An index entry returned by an index lookup or cursor.
    IndexEntry,
}

/// Storage accesses counted since the last reset.
///
/// Available with the `access-stats` feature. The counters separate record
/// fetches by address, full-table scan rows, and index entries, so tests and
/// benchmarks can prove that an index path avoided record pages. Schema and
/// ledger metadata reads are not counted.
#[cfg(any(test, feature = "access-stats"))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AccessStats {
    /// Records decoded by address after an index lookup.
    pub record_fetches: u64,
    /// Rows yielded by full-table scans, including transaction rows.
    pub scanned_rows: u64,
    /// Index entries returned by index lookups and cursors.
    pub index_entries: u64,
}

#[cfg(any(test, feature = "access-stats"))]
impl AccessStats {
    /// Adds `count` accesses of `kind`.
    pub(crate) fn add(&mut self, kind: AccessKind, count: u64) {
        let counter = match kind {
            AccessKind::RecordFetch => &mut self.record_fetches,
            AccessKind::ScannedRow => &mut self.scanned_rows,
            AccessKind::IndexEntry => &mut self.index_entries,
        };
        *counter = counter.saturating_add(count);
    }
}
