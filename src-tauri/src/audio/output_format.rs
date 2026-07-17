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

/// Create a fresh `OutputFormatState` initialized to `None`.
pub fn create_output_format_state() -> OutputFormatState {
    Arc::new(RwLock::new(None))
}

/// Publish a new output-format snapshot. Called by the output worker after a
/// stream is successfully constructed, before reporting readiness.
pub fn publish(state: &OutputFormatState, generation: u64, sample_rate: u32, channels: u16) {
    if let Ok(mut guard) = state.write() {
        *guard = Some(OutputFormatSnapshot::new(generation, sample_rate, channels));
    }
}

/// Read the current output-format snapshot. Returns `None` if no stream has
/// been constructed yet.
pub fn snapshot(state: &OutputFormatState) -> Option<OutputFormatSnapshot> {
    state.read().ok().and_then(|guard| *guard)
}

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
        let state = create_output_format_state();
        assert!(snapshot(&state).is_none());
    }

    #[test]
    fn publish_and_read() {
        let state = create_output_format_state();
        publish(&state, 1, 48_000, 2);
        let s = snapshot(&state).expect("should be published");
        assert_eq!(s.generation, 1);
        assert_eq!(s.sample_rate, 48_000);
        assert_eq!(s.channels, 2);
    }

    #[test]
    fn publish_overwrites_previous() {
        let state = create_output_format_state();
        publish(&state, 1, 44_100, 2);
        publish(&state, 2, 48_000, 2);
        let s = snapshot(&state).expect("should exist");
        assert_eq!(s.generation, 2);
        assert_eq!(s.sample_rate, 48_000);
    }
}
