use crate::{
    cdg::{parse_cdg_bytes_with_diagnostics, CdgParseResult},
    commands::cdg::{CdgAvailability, CdgErrorCode, CdgPlaybackSlot, CdgPlaybackState, CdgStatus},
    library_root::LibraryRoot,
    media_g::{self, MEDIA_G_ZIP},
};
use std::path::Path;
use std::sync::Arc;

/// Removes both status and playback state so the previous frame cannot
/// survive while decode/import work continues.
pub fn clear_cdg_for_transport_change(cdg_state: &mut Option<CdgPlaybackSlot>) {
    *cdg_state = None;
}

/// Clears the old slot immediately so the previous frame cannot survive
/// while decode/import work continues.
pub fn mark_cdg_loading(
    cdg_state: &mut Option<CdgPlaybackSlot>,
    song_id: &str,
    transport_generation: u64,
) {
    *cdg_state = Some(CdgPlaybackSlot {
        status: CdgStatus {
            availability: CdgAvailability::Loading,
            song_id: Some(song_id.to_owned()),
            transport_generation: Some(transport_generation),
            packet_count: None,
            error_code: None,
        },
        playback: None,
    });
}

pub fn attach_cdg_for_song(
    cdg_state: &mut Option<CdgPlaybackSlot>,
    song_id: &str,
    transport_generation: u64,
    packets: Arc<[crate::cdg::CdgPacket]>,
) {
    let packet_count = packets.len();
    let has_cdg_commands = packets.iter().any(|p| p.is_cdg_command());

    if packet_count == 0 {
        // Empty file or fewer than 24 bytes.
        *cdg_state = Some(CdgPlaybackSlot {
            status: CdgStatus {
                availability: CdgAvailability::Error,
                song_id: Some(song_id.to_owned()),
                transport_generation: Some(transport_generation),
                packet_count: Some(0),
                error_code: Some(CdgErrorCode::Empty),
            },
            playback: None,
        });
        return;
    }

    if !has_cdg_commands {
        // At least one complete packet but zero valid CDG command packets.
        *cdg_state = Some(CdgPlaybackSlot {
            status: CdgStatus {
                availability: CdgAvailability::Error,
                song_id: Some(song_id.to_owned()),
                transport_generation: Some(transport_generation),
                packet_count: Some(packet_count),
                error_code: Some(CdgErrorCode::Invalid),
            },
            playback: None,
        });
        return;
    }

    *cdg_state = Some(CdgPlaybackSlot {
        status: CdgStatus {
            availability: CdgAvailability::Ready,
            song_id: Some(song_id.to_owned()),
            transport_generation: Some(transport_generation),
            packet_count: Some(packet_count),
            error_code: None,
        },
        playback: Some(CdgPlaybackState::new(
            song_id.to_owned(),
            transport_generation,
            packets,
        )),
    });
}

pub fn mark_cdg_error(
    cdg_state: &mut Option<CdgPlaybackSlot>,
    song_id: &str,
    transport_generation: u64,
    error_code: CdgErrorCode,
) {
    *cdg_state = Some(CdgPlaybackSlot {
        status: CdgStatus {
            availability: CdgAvailability::Error,
            song_id: Some(song_id.to_owned()),
            transport_generation: Some(transport_generation),
            packet_count: None,
            error_code: Some(error_code),
        },
        playback: None,
    });
}

pub fn update_cdg_transport_generation(
    cdg_state: &mut Option<CdgPlaybackSlot>,
    transport_generation: u64,
) {
    if let Some(slot) = cdg_state.as_mut() {
        slot.status.transport_generation = Some(transport_generation);
        if let Some(cdg) = slot.playback.as_mut() {
            cdg.update_transport_generation(transport_generation);
        }
    }
}

pub fn mark_cdg_seek(cdg_state: &mut Option<CdgPlaybackSlot>, transport_generation: u64) {
    if let Some(slot) = cdg_state.as_mut() {
        slot.status.transport_generation = Some(transport_generation);
        if let Some(cdg) = slot.playback.as_mut() {
            // Update the playback state's own generation counter so that
            // get_cdg_frame's stale song/generation guard (which compares
            // cdg.transport_generation) accepts the post-seek caller.
            // Without this, the guard rejects the caller and the CDG
            // graphics freeze after any seek.
            cdg.update_transport_generation(transport_generation);
            cdg.mark_seek();
        }
    }
}

pub fn load_cdg_packets_for_song(
    library_root: &LibraryRoot,
    song: &crate::library::Song,
) -> CdgLoadResult {
    let absolute_path = match song
        .file_path
        .as_deref()
        .map(|path| library_root.resolve(path))
    {
        Some(path) => path,
        None => return CdgLoadResult::Missing,
    };

    match song.media_g_container.as_deref() {
        Some(MEDIA_G_ZIP) => load_cdg_packets_from_zip(&absolute_path),
        _ => {
            if let Some(cdg_path) = song.cdg_path.as_deref() {
                load_cdg_packets_from_explicit_path(&library_root.resolve(cdg_path))
            } else {
                load_cdg_packets_from_sidecar(&absolute_path)
            }
        }
    }
}

pub enum CdgLoadResult {
    Missing,
    Loaded(CdgParseResult),
    ReadFailed,
    ZipFailed,
}

fn load_cdg_packets_from_sidecar(audio_path: &Path) -> CdgLoadResult {
    let sidecar_path = audio_path.with_extension("cdg");
    if !sidecar_path.is_file() {
        return CdgLoadResult::Missing;
    }

    match std::fs::read(&sidecar_path) {
        Ok(bytes) => CdgLoadResult::Loaded(parse_cdg_bytes_with_diagnostics(&bytes)),
        Err(error) => {
            eprintln!(
                "warning: failed to read CDG sidecar at {}: {}",
                sidecar_path.display(),
                error
            );
            CdgLoadResult::ReadFailed
        }
    }
}

fn load_cdg_packets_from_explicit_path(cdg_path: &Path) -> CdgLoadResult {
    if !cdg_path.is_file() {
        return CdgLoadResult::Missing;
    }

    match std::fs::read(cdg_path) {
        Ok(bytes) => CdgLoadResult::Loaded(parse_cdg_bytes_with_diagnostics(&bytes)),
        Err(error) => {
            eprintln!(
                "warning: failed to read CDG sidecar at {}: {}",
                cdg_path.display(),
                error
            );
            CdgLoadResult::ReadFailed
        }
    }
}

fn load_cdg_packets_from_zip(zip_path: &Path) -> CdgLoadResult {
    match media_g::inspect_zip_for_media_g(zip_path) {
        Ok(asset) => CdgLoadResult::Loaded(parse_cdg_bytes_with_diagnostics(&asset.cdg_bytes)),
        Err(error) => {
            eprintln!(
                "warning: failed to read CDG packets from Media+G ZIP at {}: {}",
                zip_path.display(),
                error
            );
            CdgLoadResult::ZipFailed
        }
    }
}

/// Kept for backward compatibility with existing call sites that don't need
/// the intermediate `CdgLoadResult`.
pub fn load_and_attach_cdg(
    cdg_state: &mut Option<CdgPlaybackSlot>,
    library_root: &LibraryRoot,
    song: &crate::library::Song,
    song_id: &str,
    transport_generation: u64,
) {
    match load_cdg_packets_for_song(library_root, song) {
        CdgLoadResult::Missing => {
            clear_cdg_for_transport_change(cdg_state);
        }
        CdgLoadResult::Loaded(result) => {
            if let Some(diag) = &result.diagnostic {
                eprintln!("warning: CDG parse diagnostic for {}: {:?}", song_id, diag);
            }
            let packets: Arc<[crate::cdg::CdgPacket]> =
                Arc::from(result.packets.into_boxed_slice());
            attach_cdg_for_song(cdg_state, song_id, transport_generation, packets);
        }
        CdgLoadResult::ReadFailed => {
            mark_cdg_error(
                cdg_state,
                song_id,
                transport_generation,
                CdgErrorCode::ReadFailed,
            );
        }
        CdgLoadResult::ZipFailed => {
            mark_cdg_error(
                cdg_state,
                song_id,
                transport_generation,
                CdgErrorCode::ZipFailed,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdg::CdgPacket;

    fn make_packet(instruction: u8) -> CdgPacket {
        CdgPacket {
            command: 0x09,
            instruction,
            data: [0u8; 16],
        }
    }

    #[test]
    fn clear_cdg_removes_slot() {
        let mut cdg_state = Some(CdgPlaybackSlot::default());
        clear_cdg_for_transport_change(&mut cdg_state);
        assert!(cdg_state.is_none());
    }

    #[test]
    fn mark_loading_sets_loading_status() {
        let mut cdg_state = None;
        mark_cdg_loading(&mut cdg_state, "song-1", 1);
        let slot = cdg_state.as_ref().unwrap();
        assert_eq!(slot.status.availability, CdgAvailability::Loading);
        assert_eq!(slot.status.song_id.as_deref(), Some("song-1"));
        assert_eq!(slot.status.transport_generation, Some(1));
        assert!(slot.playback.is_none());
    }

    #[test]
    fn attach_cdg_with_valid_packets_sets_ready() {
        let mut cdg_state = None;
        let packets: Arc<[CdgPacket]> = Arc::from(vec![make_packet(1)].into_boxed_slice());
        attach_cdg_for_song(&mut cdg_state, "song-1", 1, packets);
        let slot = cdg_state.as_ref().unwrap();
        assert_eq!(slot.status.availability, CdgAvailability::Ready);
        assert!(slot.playback.is_some());
    }

    #[test]
    fn attach_cdg_with_empty_packets_sets_error() {
        let mut cdg_state = None;
        let packets: Arc<[CdgPacket]> = Arc::from(Vec::new().into_boxed_slice());
        attach_cdg_for_song(&mut cdg_state, "song-1", 1, packets);
        let slot = cdg_state.as_ref().unwrap();
        assert_eq!(slot.status.availability, CdgAvailability::Error);
        assert_eq!(slot.status.error_code, Some(CdgErrorCode::Empty));
        assert!(slot.playback.is_none());
    }

    #[test]
    fn attach_cdg_with_no_cdg_commands_sets_error() {
        let mut cdg_state = None;
        let packets: Arc<[CdgPacket]> = Arc::from(
            vec![CdgPacket {
                command: 0x09,
                instruction: 0x99, // unrecognized instruction
                data: [0u8; 16],
            }]
            .into_boxed_slice(),
        );
        attach_cdg_for_song(&mut cdg_state, "song-1", 1, packets);
        let slot = cdg_state.as_ref().unwrap();
        assert_eq!(slot.status.availability, CdgAvailability::Error);
        assert_eq!(slot.status.error_code, Some(CdgErrorCode::Invalid));
    }

    #[test]
    fn mark_cdg_error_sets_error_status() {
        let mut cdg_state = None;
        mark_cdg_error(&mut cdg_state, "song-1", 1, CdgErrorCode::ReadFailed);
        let slot = cdg_state.as_ref().unwrap();
        assert_eq!(slot.status.availability, CdgAvailability::Error);
        assert_eq!(slot.status.error_code, Some(CdgErrorCode::ReadFailed));
    }

    #[test]
    fn update_transport_generation_preserves_renderer() {
        let mut cdg_state = None;
        let packets: Arc<[CdgPacket]> = Arc::from(vec![make_packet(1)].into_boxed_slice());
        attach_cdg_for_song(&mut cdg_state, "song-1", 1, packets);
        let cursor = cdg_state
            .as_ref()
            .unwrap()
            .playback
            .as_ref()
            .unwrap()
            .local
            .next_packet_index;

        update_cdg_transport_generation(&mut cdg_state, 5);
        let slot = cdg_state.as_ref().unwrap();
        assert_eq!(slot.status.transport_generation, Some(5));
        assert_eq!(
            slot.playback.as_ref().unwrap().local.next_packet_index,
            cursor
        );
    }

    #[test]
    fn mark_cdg_seek_resets_both_timelines() {
        let mut cdg_state = None;
        let packets: Arc<[CdgPacket]> = Arc::from(vec![make_packet(1)].into_boxed_slice());
        attach_cdg_for_song(&mut cdg_state, "song-1", 1, packets);

        mark_cdg_seek(&mut cdg_state, 2);
        let slot = cdg_state.as_ref().unwrap();
        assert_eq!(slot.status.transport_generation, Some(2));
        let cdg = slot.playback.as_ref().unwrap();
        // The playback state's own generation counter must be updated so
        // get_cdg_frame's stale song/generation guard accepts post-seek
        // callers. Without this, CDG graphics freeze after any seek.
        assert_eq!(cdg.transport_generation, 2);
        assert!(cdg.local.needs_reset);
        assert!(cdg.airplay.needs_reset);
    }

    #[test]
    fn loads_same_basename_cdg_sidecar_when_present() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let audio_path = dir.path().join("track.mp3");
        std::fs::write(&audio_path, b"audio").expect("audio fixture should be written");

        let mut packet = [0u8; 24];
        packet[0] = 0x09;
        packet[1] = 0x01;
        packet[4] = 0x07;
        std::fs::write(audio_path.with_extension("cdg"), packet)
            .expect("cdg fixture should be written");

        let result = load_cdg_packets_from_sidecar(&audio_path);
        match result {
            CdgLoadResult::Loaded(parse_result) => {
                assert_eq!(parse_result.packets.len(), 1);
            }
            _ => panic!("expected Loaded result"),
        }
    }

    #[test]
    fn truncated_cdg_sidecar_yields_empty_packets_without_failing() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let audio_path = dir.path().join("track.mp3");
        std::fs::write(&audio_path, b"audio").expect("audio fixture should be written");
        std::fs::write(audio_path.with_extension("cdg"), [0x09, 0x01, 0x07])
            .expect("broken cdg fixture should be written");

        let result = load_cdg_packets_from_sidecar(&audio_path);
        match result {
            CdgLoadResult::Loaded(parse_result) => {
                assert!(parse_result.packets.is_empty());
            }
            _ => panic!("expected Loaded result with empty packets"),
        }
    }
}
