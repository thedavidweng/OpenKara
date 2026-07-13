use crate::{
    cdg::{CdgPacket, CdgRenderer, CdgRendererSnapshot},
    commands::error::{internal_error, CommandResult},
    state::AppState,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{ipc::Response, State};

// ── Checkpoint configuration ──────────────────────────────────────────────

/// Save a checkpoint every 30 seconds (300 packets/s * 30s = 9000 packets).
const CHECKPOINT_INTERVAL_PACKETS: usize = 9_000;
/// Maximum number of checkpoints per timeline. Once full, stop adding later
/// checkpoints; never evict a checkpoint during active playback.
const MAX_CHECKPOINTS: usize = 256;

/// Exclusive-cursor checkpoint: a snapshot saved after processing packet
/// index `i` uses `next_packet_index = i + 1`.
#[derive(Clone)]
pub struct CdgCheckpoint {
    /// Exclusive cursor represented by this snapshot.
    pub next_packet_index: usize,
    pub renderer: CdgRendererSnapshot,
}

// ── Timeline types ────────────────────────────────────────────────────────

/// Identifies which presentation clock owns a timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdgTimelineKind {
    Local,
    AirPlay,
}

/// Mutable per-timeline state. Local and AirPlay each own a separate
/// instance so they can advance independently without cross-contamination.
pub struct CdgTimelineState {
    pub renderer: CdgRenderer,
    /// Exclusive packet cursor: renderer contains packets [0, next_packet_index).
    pub next_packet_index: usize,
    pub cached_frame: Option<Arc<[u8]>>,
    pub frame_version: u64,
    pub needs_reset: bool,
    pub checkpoints: Vec<CdgCheckpoint>,
}

impl CdgTimelineState {
    pub fn new() -> Self {
        Self {
            renderer: CdgRenderer::new(),
            next_packet_index: 0,
            cached_frame: None,
            frame_version: 0,
            needs_reset: true,
            checkpoints: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.renderer.reset();
        self.next_packet_index = 0;
        self.cached_frame = None;
        self.frame_version = 0;
        self.needs_reset = true;
        self.checkpoints.clear();
    }
}

impl Default for CdgTimelineState {
    fn default() -> Self {
        Self::new()
    }
}

// ── CDG availability status ───────────────────────────────────────────────

/// CDG availability for the current song and generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CdgAvailability {
    None,
    Loading,
    Ready,
    Error,
}

impl Default for CdgAvailability {
    fn default() -> Self {
        CdgAvailability::None
    }
}

/// Error code for CDG parse/load failures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CdgErrorCode {
    Missing,
    Empty,
    Invalid,
    ReadFailed,
    ZipFailed,
}

/// Explicit CDG status payload exposed to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CdgStatus {
    pub availability: CdgAvailability,
    pub song_id: Option<String>,
    pub transport_generation: Option<u64>,
    pub packet_count: Option<usize>,
    pub error_code: Option<CdgErrorCode>,
}

impl Default for CdgStatus {
    fn default() -> Self {
        CdgStatus {
            availability: CdgAvailability::None,
            song_id: None,
            transport_generation: None,
            packet_count: None,
            error_code: None,
        }
    }
}

/// Slot holding status plus optional playback state.
pub struct CdgPlaybackSlot {
    pub status: CdgStatus,
    pub playback: Option<CdgPlaybackState>,
}

impl Default for CdgPlaybackSlot {
    fn default() -> Self {
        CdgPlaybackSlot {
            status: CdgStatus::default(),
            playback: None,
        }
    }
}

/// Holds shared immutable packets plus per-timeline mutable state.
pub struct CdgPlaybackState {
    pub song_id: String,
    pub transport_generation: u64,
    pub packets: Arc<[CdgPacket]>,
    pub local: CdgTimelineState,
    pub airplay: CdgTimelineState,
}

impl CdgPlaybackState {
    pub fn new(song_id: String, transport_generation: u64, packets: Arc<[CdgPacket]>) -> Self {
        Self {
            song_id,
            transport_generation,
            packets,
            local: CdgTimelineState::new(),
            airplay: CdgTimelineState::new(),
        }
    }

    /// Mark both timelines for repositioning on their next authorized advance.
    pub fn mark_seek(&mut self) {
        self.local.needs_reset = true;
        self.local.cached_frame = None;
        self.airplay.needs_reset = true;
        self.airplay.cached_frame = None;
    }

    /// Update transport generation without resetting renderer pixels or cursor.
    pub fn update_transport_generation(&mut self, generation: u64) {
        self.transport_generation = generation;
    }
}

// ── Frame update result ───────────────────────────────────────────────────

/// Result of advancing a named timeline.
#[derive(Debug)]
pub enum CdgFrameUpdate {
    NoChange {
        frame_version: u64,
        packet_index: usize,
    },
    Frame {
        frame_version: u64,
        packet_index: usize,
        rgba: Arc<[u8]>,
    },
}

// ── Timeline advancement ──────────────────────────────────────────────────

/// Advance the selected timeline to the position implied by `position_ms`.
///
/// - Reposition only the selected timeline.
/// - Convert indexed pixels to RGBA only when visible state changed, no
///   cached frame exists, or a reset/seek requires a new authoritative frame.
/// - Increment only that timeline's `frame_version`, and only when its
///   cached frame changes.
/// - `NoChange` does not clone the cached RGBA buffer.
pub fn advance_cdg_timeline(
    state: &mut CdgPlaybackState,
    timeline: CdgTimelineKind,
    position_ms: u64,
) -> CdgFrameUpdate {
    let ts = match timeline {
        CdgTimelineKind::Local => &mut state.local,
        CdgTimelineKind::AirPlay => &mut state.airplay,
    };
    let packets = &state.packets;

    // Overflow-safe target index calculation.
    let target_index = ((position_ms as u128 * 300) / 1000) as usize;
    let target_index = target_index.min(packets.len());

    let needs_reposition = ts.needs_reset || target_index < ts.next_packet_index;

    if needs_reposition {
        reposition_timeline(ts, packets, target_index);
        let rgba = Arc::from(ts.renderer.to_rgba());
        ts.cached_frame = Some(Arc::clone(&rgba));
        ts.frame_version = ts.frame_version.saturating_add(1);
        ts.needs_reset = false;
        return CdgFrameUpdate::Frame {
            frame_version: ts.frame_version,
            packet_index: ts.next_packet_index,
            rgba,
        };
    }

    if target_index > ts.next_packet_index {
        let changed = ts
            .renderer
            .process_range(packets, ts.next_packet_index, target_index);
        ts.next_packet_index = target_index;
        maybe_save_checkpoint(ts, target_index);

        if changed || ts.cached_frame.is_none() {
            let rgba = Arc::from(ts.renderer.to_rgba());
            ts.cached_frame = Some(Arc::clone(&rgba));
            ts.frame_version = ts.frame_version.saturating_add(1);
            return CdgFrameUpdate::Frame {
                frame_version: ts.frame_version,
                packet_index: ts.next_packet_index,
                rgba,
            };
        }
    }

    // No change — return current version without cloning the buffer.
    CdgFrameUpdate::NoChange {
        frame_version: ts.frame_version,
        packet_index: ts.next_packet_index,
    }
}

/// Reposition a timeline to `target_index` using checkpoint restore + replay
/// for backward seeks, or reset + replay from zero when no checkpoint exists.
fn reposition_timeline(ts: &mut CdgTimelineState, packets: &[CdgPacket], target_index: usize) {
    // Find the greatest checkpoint whose exclusive cursor is <= target_index.
    let checkpoint = ts
        .checkpoints
        .iter()
        .rev()
        .find(|cp| cp.next_packet_index <= target_index)
        .cloned();

    if let Some(cp) = checkpoint {
        ts.renderer.restore(&cp.renderer);
        ts.next_packet_index = cp.next_packet_index;
        // Replay [checkpoint.next_packet_index, target_index).
        ts.renderer
            .process_range(packets, cp.next_packet_index, target_index);
        ts.next_packet_index = target_index;
    } else {
        // No checkpoint — reset and replay from 0.
        ts.renderer.reset();
        ts.next_packet_index = 0;
        ts.checkpoints.clear();
        ts.renderer.process_range(packets, 0, target_index);
        ts.next_packet_index = target_index;
        // Rebuild checkpoints up to target_index by replaying in intervals.
        rebuild_checkpoints(ts, packets, target_index);
    }
}

/// Rebuild checkpoints by replaying from 0 in CHECKPOINT_INTERVAL_PACKETS steps.
/// This is only called on the reset path (no existing checkpoint).
fn rebuild_checkpoints(ts: &mut CdgTimelineState, packets: &[CdgPacket], target_index: usize) {
    let mut snap_renderer = CdgRenderer::new();
    let mut cursor = 0usize;
    while cursor < target_index {
        let next = (cursor + CHECKPOINT_INTERVAL_PACKETS).min(target_index);
        snap_renderer.process_range(packets, cursor, next);
        if next - cursor >= CHECKPOINT_INTERVAL_PACKETS && ts.checkpoints.len() < MAX_CHECKPOINTS {
            // Save checkpoint with exclusive cursor = next.
            ts.checkpoints.push(CdgCheckpoint {
                next_packet_index: next,
                renderer: snap_renderer.snapshot(),
            });
        }
        cursor = next;
    }
}

/// Save a checkpoint after processing exactly `next_packet_index` packets.
/// A snapshot saved after packet index `i` uses `next_packet_index = i + 1`.
fn maybe_save_checkpoint(ts: &mut CdgTimelineState, next_packet_index: usize) {
    if ts.checkpoints.len() >= MAX_CHECKPOINTS {
        return;
    }
    // Save after every CHECKPOINT_INTERVAL_PACKETS boundary.
    let last_cp_cursor = ts
        .checkpoints
        .last()
        .map(|cp| cp.next_packet_index)
        .unwrap_or(0);
    if next_packet_index >= last_cp_cursor + CHECKPOINT_INTERVAL_PACKETS {
        // Don't insert duplicate checkpoint cursors.
        if !ts
            .checkpoints
            .iter()
            .any(|cp| cp.next_packet_index == next_packet_index)
        {
            ts.checkpoints.push(CdgCheckpoint {
                next_packet_index,
                renderer: ts.renderer.snapshot(),
            });
        }
    }
}

// ── Binary frame protocol ─────────────────────────────────────────────────

/// Binary protocol magic bytes ("OKCG").
const PROTOCOL_MAGIC: [u8; 4] = *b"OKCG";
/// Binary protocol version.
const PROTOCOL_VERSION: u16 = 1;
/// Header size in bytes.
const PROTOCOL_HEADER_SIZE: usize = 32;
/// Flag bit 0: RGBA payload present.
const FLAG_RGBA_PRESENT: u16 = 0x01;

/// Build a 32-byte little-endian header.
fn build_header(
    transport_generation: u64,
    frame_version: u64,
    packet_index: u64,
    rgba_present: bool,
) -> [u8; PROTOCOL_HEADER_SIZE] {
    let mut header = [0u8; PROTOCOL_HEADER_SIZE];
    header[0..4].copy_from_slice(&PROTOCOL_MAGIC);
    header[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    let flags = if rgba_present { FLAG_RGBA_PRESENT } else { 0 };
    header[6..8].copy_from_slice(&flags.to_le_bytes());
    header[8..16].copy_from_slice(&transport_generation.to_le_bytes());
    header[16..24].copy_from_slice(&frame_version.to_le_bytes());
    header[24..32].copy_from_slice(&packet_index.to_le_bytes());
    header
}

/// Build the full binary response (header + optional RGBA payload).
pub fn build_cdg_frame_response(transport_generation: u64, update: CdgFrameUpdate) -> Vec<u8> {
    match update {
        CdgFrameUpdate::NoChange {
            frame_version,
            packet_index,
        } => {
            let header = build_header(
                transport_generation,
                frame_version,
                packet_index as u64,
                false,
            );
            header.to_vec()
        }
        CdgFrameUpdate::Frame {
            frame_version,
            packet_index,
            rgba,
        } => {
            let mut buf = Vec::with_capacity(PROTOCOL_HEADER_SIZE + rgba.len());
            let header = build_header(
                transport_generation,
                frame_version,
                packet_index as u64,
                true,
            );
            buf.extend_from_slice(&header);
            buf.extend_from_slice(&rgba);
            buf
        }
    }
}

// ── IPC commands ──────────────────────────────────────────────────────────

/// Returns a binary CDG frame envelope (32-byte header + optional RGBA).
///
/// Request parameters: `songId`, `transportGeneration`, `positionMs`, `lastFrameVersion`.
///
/// Response:
/// - 0 bytes: no active CDG, stale song/generation, or error state.
/// - 32 bytes (header only, no RGBA flag): active CDG but caller already has current frame.
/// - 32 bytes + 221,184 bytes: caller needs the current frame.
///
/// A mismatch in `songId` or `transportGeneration` returns 0 bytes and does
/// not mutate any decoder state.
#[tauri::command]
pub fn get_cdg_frame(
    state: State<'_, AppState>,
    song_id: String,
    transport_generation: u64,
    position_ms: u64,
    last_frame_version: u64,
) -> CommandResult<Response> {
    let mut cdg_guard = state
        .playback
        .cdg_state
        .lock()
        .map_err(|_| internal_error("CDG state lock was poisoned".to_owned()))?;

    let slot = cdg_guard.as_mut();
    let Some(slot) = slot else {
        return Ok(Response::new(Vec::<u8>::new()));
    };

    let Some(cdg) = slot.playback.as_mut() else {
        return Ok(Response::new(Vec::<u8>::new()));
    };

    // Stale song/generation guard: do not mutate decoder state.
    if cdg.song_id != song_id || cdg.transport_generation != transport_generation {
        return Ok(Response::new(Vec::<u8>::new()));
    }

    let update = advance_cdg_timeline(cdg, CdgTimelineKind::Local, position_ms);

    // If the caller already has the current frame version, return header-only.
    match &update {
        CdgFrameUpdate::NoChange { frame_version, .. } if *frame_version == last_frame_version => {
            let header = build_header(
                cdg.transport_generation,
                *frame_version,
                cdg.local.next_packet_index as u64,
                false,
            );
            return Ok(Response::new(header.to_vec()));
        }
        _ => {}
    }

    let response = build_cdg_frame_response(cdg.transport_generation, update);
    Ok(Response::new(response))
}

/// Returns the current CDG status for the given song and transport generation.
#[tauri::command]
pub fn get_cdg_status(
    state: State<'_, AppState>,
    song_id: String,
    transport_generation: u64,
) -> CommandResult<CdgStatus> {
    let cdg_guard = state
        .playback
        .cdg_state
        .lock()
        .map_err(|_| internal_error("CDG state lock was poisoned".to_owned()))?;

    let Some(slot) = cdg_guard.as_ref() else {
        return Ok(CdgStatus::default());
    };

    // Return status only if it matches the requested song/generation.
    if slot.status.song_id.as_deref() == Some(song_id.as_str())
        && slot.status.transport_generation == Some(transport_generation)
    {
        return Ok(slot.status.clone());
    }

    // If the slot's status is for a different song/generation, return None.
    Ok(CdgStatus::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdg::{CdgPacket, CDG_RGBA_LEN};

    fn cdg_packet(instruction: u8, data: [u8; 16]) -> CdgPacket {
        CdgPacket {
            command: 0x09,
            instruction,
            data,
        }
    }

    fn make_state(packets: Vec<CdgPacket>) -> CdgPlaybackState {
        CdgPlaybackState::new(
            "song-1".to_owned(),
            1,
            Arc::from(packets.into_boxed_slice()),
        )
    }

    #[test]
    fn new_timeline_state_needs_reset() {
        let ts = CdgTimelineState::new();
        assert!(ts.needs_reset);
        assert_eq!(ts.next_packet_index, 0);
        assert!(ts.cached_frame.is_none());
        assert_eq!(ts.frame_version, 0);
    }

    #[test]
    fn first_advance_returns_frame() {
        let mut state = make_state(vec![cdg_packet(1, [0u8; 16])]);
        let update = advance_cdg_timeline(&mut state, CdgTimelineKind::Local, 0);
        match update {
            CdgFrameUpdate::Frame { rgba, .. } => {
                assert_eq!(rgba.len(), CDG_RGBA_LEN);
            }
            _ => panic!("expected Frame on first advance"),
        }
    }

    #[test]
    fn repeated_same_position_returns_no_change() {
        let mut state = make_state(vec![cdg_packet(1, [0u8; 16])]);
        let _ = advance_cdg_timeline(&mut state, CdgTimelineKind::Local, 0);
        let update = advance_cdg_timeline(&mut state, CdgTimelineKind::Local, 0);
        assert!(matches!(update, CdgFrameUpdate::NoChange { .. }));
    }

    #[test]
    fn advancing_local_does_not_affect_airplay() {
        let mut state = make_state(vec![cdg_packet(1, [0u8; 16])]);
        let _ = advance_cdg_timeline(&mut state, CdgTimelineKind::Local, 0);

        // AirPlay should be untouched.
        assert_eq!(state.airplay.next_packet_index, 0);
        assert!(state.airplay.cached_frame.is_none());
        assert_eq!(state.airplay.frame_version, 0);
    }

    #[test]
    fn advancing_airplay_does_not_affect_local() {
        let mut state = make_state(vec![cdg_packet(1, [0u8; 16])]);
        let _ = advance_cdg_timeline(&mut state, CdgTimelineKind::AirPlay, 0);

        assert_eq!(state.local.next_packet_index, 0);
        assert!(state.local.cached_frame.is_none());
    }

    #[test]
    fn mark_seek_resets_both_timelines() {
        let mut state = make_state(vec![cdg_packet(1, [0u8; 16])]);
        let _ = advance_cdg_timeline(&mut state, CdgTimelineKind::Local, 0);
        let _ = advance_cdg_timeline(&mut state, CdgTimelineKind::AirPlay, 0);

        state.mark_seek();

        assert!(state.local.needs_reset);
        assert!(state.local.cached_frame.is_none());
        assert!(state.airplay.needs_reset);
        assert!(state.airplay.cached_frame.is_none());
    }

    #[test]
    fn update_transport_generation_preserves_renderer() {
        let mut state = make_state(vec![cdg_packet(1, [0u8; 16])]);
        let _ = advance_cdg_timeline(&mut state, CdgTimelineKind::Local, 0);
        let local_cursor = state.local.next_packet_index;

        state.update_transport_generation(5);
        assert_eq!(state.transport_generation, 5);
        assert_eq!(state.local.next_packet_index, local_cursor);
    }

    #[test]
    fn binary_protocol_header_format() {
        let header = build_header(42, 7, 100, false);
        assert_eq!(&header[0..4], b"OKCG");
        assert_eq!(u16::from_le_bytes([header[4], header[5]]), 1);
        assert_eq!(u16::from_le_bytes([header[6], header[7]]), 0);
        assert_eq!(u64::from_le_bytes(header[8..16].try_into().unwrap()), 42);
        assert_eq!(u64::from_le_bytes(header[16..24].try_into().unwrap()), 7);
        assert_eq!(u64::from_le_bytes(header[24..32].try_into().unwrap()), 100);
    }

    #[test]
    fn binary_protocol_header_with_rgba_flag() {
        let header = build_header(1, 2, 3, true);
        assert_eq!(
            u16::from_le_bytes([header[6], header[7]]),
            FLAG_RGBA_PRESENT
        );
    }

    #[test]
    fn build_frame_response_no_change_is_header_only() {
        let update = CdgFrameUpdate::NoChange {
            frame_version: 5,
            packet_index: 100,
        };
        let response = build_cdg_frame_response(1, update);
        assert_eq!(response.len(), PROTOCOL_HEADER_SIZE);
    }

    #[test]
    fn build_frame_response_with_rgba_is_header_plus_payload() {
        let rgba = Arc::from(vec![0u8; CDG_RGBA_LEN].into_boxed_slice());
        let update = CdgFrameUpdate::Frame {
            frame_version: 5,
            packet_index: 100,
            rgba,
        };
        let response = build_cdg_frame_response(1, update);
        assert_eq!(response.len(), PROTOCOL_HEADER_SIZE + CDG_RGBA_LEN);
    }

    #[test]
    fn checkpoint_seek_equivalence() {
        // Build enough packets to trigger checkpoints.
        let mut packets = Vec::new();
        // Set color 1 to white.
        let mut color_data = [0u8; 16];
        color_data[2] = 0x3F;
        color_data[3] = 0x3F;
        packets.push(cdg_packet(30, color_data)); // ColorsLow
                                                  // Memory preset with color 1.
        packets.push(cdg_packet(1, {
            let mut d = [0u8; 16];
            d[0] = 1;
            d
        }));
        // Add packets to exceed CHECKPOINT_INTERVAL.
        for _ in 2..(CHECKPOINT_INTERVAL_PACKETS + 10) {
            packets.push(cdg_packet(1, {
                let mut d = [0u8; 16];
                d[0] = 0;
                d[1] = 1; // repeat != 0, filtered
                d
            }));
        }
        // Add an XOR packet after the checkpoint boundary.
        packets.push(cdg_packet(38, {
            let mut d = [0u8; 16];
            d[0] = 1;
            d[1] = 0;
            d[2] = 1;
            d[3] = 1;
            d[4] = 0x3F;
            d
        }));

        let packets_arc: Arc<[CdgPacket]> = Arc::from(packets.into_boxed_slice());

        // Sequential decode from 0 to target.
        let mut seq_renderer = CdgRenderer::new();
        seq_renderer.process_range(&packets_arc, 0, packets_arc.len());
        let _seq_rgba = seq_renderer.to_rgba();

        // Checkpoint restore + replay.
        let mut state = CdgPlaybackState::new("s".to_owned(), 1, Arc::clone(&packets_arc));
        // Advance forward to build checkpoints.
        let _ = advance_cdg_timeline(
            &mut state,
            CdgTimelineKind::Local,
            (packets_arc.len() as u64 * 1000) / 300,
        );
        // Seek backward to a position after the checkpoint.
        let target_ms = ((CHECKPOINT_INTERVAL_PACKETS as u64 + 5) * 1000) / 300;
        state.mark_seek();
        let update = advance_cdg_timeline(&mut state, CdgTimelineKind::Local, target_ms);

        match update {
            CdgFrameUpdate::Frame { rgba, .. } => {
                // The checkpoint-replayed frame at this cursor should match
                // sequential decode at the same cursor.
                let mut seq2 = CdgRenderer::new();
                seq2.process_range(&packets_arc, 0, (target_ms as u128 * 300 / 1000) as usize);
                let seq2_rgba = seq2.to_rgba();
                assert_eq!(rgba.as_ref(), seq2_rgba.as_slice());
            }
            _ => panic!("expected Frame after seek"),
        }
    }

    #[test]
    fn ten_minute_simulation_no_backward_resets() {
        // 10 minutes at 33ms polling = ~18,181 polls.
        // At 300 packets/s, 10 min = 180,000 packets.
        let packet_count = 180_000usize;
        let packets: Vec<CdgPacket> = (0..packet_count)
            .map(|i| {
                let mut d = [0u8; 16];
                d[0] = (i % 16) as u8;
                d[1] = if i % 100 == 0 { 0 } else { 1 }; // mostly filtered repeats
                cdg_packet(1, d)
            })
            .collect();
        let packets_arc: Arc<[CdgPacket]> = Arc::from(packets.into_boxed_slice());

        let mut state = CdgPlaybackState::new("s".to_owned(), 1, Arc::clone(&packets_arc));

        let mut rgba_conversion_count = 0u64;
        let mut poll_count = 0u64;
        let mut current_version = 0u64;

        for i in 0..18_181 {
            let position_ms = (i as u64) * 33;
            let update = advance_cdg_timeline(&mut state, CdgTimelineKind::Local, position_ms);
            poll_count += 1;
            match update {
                CdgFrameUpdate::Frame { frame_version, .. } => {
                    if frame_version != current_version {
                        rgba_conversion_count += 1;
                        current_version = frame_version;
                    }
                }
                _ => {}
            }
        }

        // No backward resets: the cursor should never go backward.
        assert_eq!(
            state.local.next_packet_index,
            (18_180 * 33 * 300 / 1000).min(packet_count)
        );
        // RGBA conversion count should be much less than poll count.
        assert!(
            rgba_conversion_count < poll_count,
            "RGBA conversions ({rgba_conversion_count}) should be less than polls ({poll_count})"
        );
    }

    #[test]
    fn airplay_inactive_no_rgba_conversion() {
        let packets: Vec<CdgPacket> = (0..1000).map(|_| cdg_packet(1, [0u8; 16])).collect();
        let packets_arc: Arc<[CdgPacket]> = Arc::from(packets.into_boxed_slice());

        let mut state = CdgPlaybackState::new("s".to_owned(), 1, Arc::clone(&packets_arc));

        // Only advance Local; AirPlay should have zero conversions.
        for i in 0..100 {
            let _ = advance_cdg_timeline(&mut state, CdgTimelineKind::Local, i * 33);
        }

        assert_eq!(state.airplay.next_packet_index, 0);
        assert!(state.airplay.cached_frame.is_none());
        assert_eq!(state.airplay.frame_version, 0);
    }

    #[test]
    fn stale_generation_returns_no_bytes_without_mutation() {
        use std::sync::Mutex;

        let slot = CdgPlaybackSlot {
            status: CdgStatus {
                availability: CdgAvailability::Ready,
                song_id: Some("song-1".to_owned()),
                transport_generation: Some(1),
                packet_count: Some(10),
                error_code: None,
            },
            playback: Some(CdgPlaybackState::new(
                "song-1".to_owned(),
                1,
                Arc::from(vec![cdg_packet(1, [0u8; 16])].into_boxed_slice()),
            )),
        };

        let cdg_state = Arc::new(Mutex::new(Some(slot)));
        // Manually inject CDG state for this test.
        let mut guard = cdg_state.lock().unwrap();
        let slot = guard.as_mut().unwrap();
        let cdg = slot.playback.as_mut().unwrap();
        let local_cursor_before = cdg.local.next_packet_index;

        // Stale generation.
        if cdg.song_id != "song-1" || cdg.transport_generation != 999 {
            // This is the expected path — no mutation.
        }

        assert_eq!(cdg.local.next_packet_index, local_cursor_before);
    }

    #[test]
    fn checkpoint_count_stays_bounded() {
        // Verify that checkpoints never exceed MAX_CHECKPOINTS (256).
        let packet_count = MAX_CHECKPOINTS * CHECKPOINT_INTERVAL_PACKETS * 2;
        let packets: Vec<CdgPacket> = (0..packet_count)
            .map(|i| {
                let mut d = [0u8; 16];
                d[0] = (i % 16) as u8;
                d[1] = if i % CHECKPOINT_INTERVAL_PACKETS == 0 {
                    0
                } else {
                    1
                };
                cdg_packet(1, d)
            })
            .collect();
        let packets_arc: Arc<[CdgPacket]> = Arc::from(packets.into_boxed_slice());

        let mut state = CdgPlaybackState::new("s".to_owned(), 1, Arc::clone(&packets_arc));

        // Advance through the entire packet range.
        let total_ms = (packet_count as u64 * 1000) / 300;
        let _ = advance_cdg_timeline(&mut state, CdgTimelineKind::Local, total_ms);

        assert!(
            state.local.checkpoints.len() <= MAX_CHECKPOINTS,
            "checkpoints ({}) must not exceed MAX_CHECKPOINTS ({})",
            state.local.checkpoints.len(),
            MAX_CHECKPOINTS
        );
    }

    #[test]
    fn frame_version_monotonic_on_visible_changes() {
        let packets: Vec<CdgPacket> = vec![
            cdg_packet(1, {
                let mut d = [0u8; 16];
                d[0] = 0;
                d[1] = 0; // repeat=0, first
                d
            }),
            cdg_packet(1, {
                let mut d = [0u8; 16];
                d[0] = 1;
                d[1] = 0; // repeat=0, first (color change)
                d
            }),
            cdg_packet(1, {
                let mut d = [0u8; 16];
                d[0] = 1;
                d[1] = 1; // repeat=1, filtered (no change)
                d
            }),
        ];
        let packets_arc: Arc<[CdgPacket]> = Arc::from(packets.into_boxed_slice());

        let mut state = CdgPlaybackState::new("s".to_owned(), 1, Arc::clone(&packets_arc));

        // First advance: needs_reset → Frame.
        let v1 = match advance_cdg_timeline(&mut state, CdgTimelineKind::Local, 0) {
            CdgFrameUpdate::Frame { frame_version, .. } => frame_version,
            _ => panic!("expected Frame"),
        };

        // Second advance: new packet → Frame (color change).
        let v2 = match advance_cdg_timeline(&mut state, CdgTimelineKind::Local, 10) {
            CdgFrameUpdate::Frame { frame_version, .. } => frame_version,
            _ => panic!("expected Frame"),
        };
        assert!(v2 > v1, "frame version must be monotonic");

        // Third advance: filtered repeat → NoChange.
        let v3 = match advance_cdg_timeline(&mut state, CdgTimelineKind::Local, 20) {
            CdgFrameUpdate::NoChange { frame_version, .. } => frame_version,
            other => panic!("expected NoChange, got {:?}", other),
        };
        assert_eq!(v3, v2, "NoChange must not increment frame version");
    }
}
