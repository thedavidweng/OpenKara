//! Output-format descriptor published by the CPAL output worker.
//!
//! The output worker publishes an `OutputFormatSnapshot` once a stream is
//! successfully constructed. The preload scheduler captures the descriptor,
//! decodes and normalizes the next track to the captured format, then sends
//! the captured descriptor with the ready payload. The coordinator rejects a
//! prepared payload when the current output descriptor differs by generation,
//! rate or channels.

use std::sync::{Arc, RwLock};

/// Copyable output descriptor published by the output worker.
/// `generation` increments whenever a new CPAL stream is successfully
/// constructed so stale preparations can be rejected after device restart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFormatSnapshot {
    pub generation: u64,
    pub sample_rate: u32,
    pub channels: u16,
}

impl OutputFormatSnapshot {
    pub fn new(generation: u64, sample_rate: u32, channels: u16) -> Self {
        Self {
            generation,
            sample_rate,
            channels,
        }
    }
}

/// Shared, copyable output-format state. The output worker writes (under the
/// RwLock write guard) before reporting itself ready; the preload scheduler
/// and coordinator read it (under the read guard) without holding the playback
/// mutex.
pub type OutputFormatState = Arc<RwLock<Option<OutputFormatSnapshot>>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_snapshot_is_copy_and_eq() {
        let a = OutputFormatSnapshot::new(1, 44_100, 2);
        let b = a;
        assert_eq!(a, b);
        let c = OutputFormatSnapshot::new(2, 44_100, 2);
        assert_ne!(a, c);
    }

    #[test]
    fn create_state_starts_none() {
        let state: OutputFormatState = Arc::new(RwLock::new(None));
        assert!(state.read().ok().and_then(|guard| *guard).is_none());
    }

    #[test]
    fn publish_and_read() {
        let state: OutputFormatState = Arc::new(RwLock::new(None));
        if let Ok(mut guard) = state.write() {
            *guard = Some(OutputFormatSnapshot::new(1, 48_000, 2));
        }
        let s = state
            .read()
            .ok()
            .and_then(|guard| *guard)
            .expect("should be published");
        assert_eq!(s.generation, 1);
        assert_eq!(s.sample_rate, 48_000);
        assert_eq!(s.channels, 2);
    }

    #[test]
    fn publish_overwrites_previous() {
        let state: OutputFormatState = Arc::new(RwLock::new(None));
        if let Ok(mut guard) = state.write() {
            *guard = Some(OutputFormatSnapshot::new(1, 44_100, 2));
        }
        if let Ok(mut guard) = state.write() {
            *guard = Some(OutputFormatSnapshot::new(2, 48_000, 2));
        }
        let s = state
            .read()
            .ok()
            .and_then(|guard| *guard)
            .expect("should exist");
        assert_eq!(s.generation, 2);
        assert_eq!(s.sample_rate, 48_000);
    }
}
