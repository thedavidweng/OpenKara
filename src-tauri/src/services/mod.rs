pub mod cdg;
pub mod next_track;
pub mod playback;
pub mod separation;
pub mod track_load;
pub mod waveform;

/// Decode helpers shared with `perf` and the gapless preload path. The rest of
/// [`track_load::source`] is reachable only from inside `track_load`.
pub(crate) mod playback_source {
    pub(crate) use super::track_load::source::{
        load_playback_source, load_song_audio, probe_song_audio,
    };
}

/// `remote::fault_injection` drives `run_reconnect` directly against injected
/// closures; production reconnect goes through `track_load`.
#[cfg(test)]
pub(crate) use track_load::reconnect;
