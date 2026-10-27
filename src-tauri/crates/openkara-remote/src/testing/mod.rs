pub mod conformance;
mod memory;

pub use conformance::ConformanceFixture;
pub use memory::MemoryStorage;

/// A failure scripted into a storage. Each fault fires once, on the first
/// operation it matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The next `write_new` or `conditional_replace` of `path` fails with
    /// `RemoteConflict` and stores nothing, whatever the real precondition.
    FailPrecondition { path: String },
    /// The next `read` or `read_range` of `path` delivers at most
    /// `after_bytes` bytes of its window, then fails with `NetworkUnavailable`.
    InterruptTransfer { path: String, after_bytes: u64 },
    /// The next `write_new` or `conditional_replace` of `path` that passes its
    /// precondition stores only the first `stored_bytes` bytes, then fails
    /// with `NetworkUnavailable`.
    PartialWrite { path: String, stored_bytes: u64 },
    /// The next `list` returns the objects as they were when this fault was
    /// scripted.
    StaleListing,
    /// Another device replaces `path` with `bytes` right after the next
    /// successful `stat`, `read` or `read_range` of `path`.
    MoveRevision { path: String, bytes: Vec<u8> },
}
