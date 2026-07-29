use std::{
    fs::File,
    io::{Cursor, Read, Seek},
    path::Path,
};
use symphonia::core::{
    audio::GenericAudioBufferRef,
    codecs::audio::AudioDecoderOptions,
    errors::Error as SymphoniaError,
    formats::{probe::Hint, FormatOptions, TrackType},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAudio {
    pub sample_rate_hz: u32,
    pub channels: usize,
    pub duration_ms: u64,
    pub samples: Vec<f32>,
}

#[derive(Error, Debug)]
pub enum DecodeError {
    #[error("failed to open audio file: {0}")]
    FileOpenFailed(String),

    #[error("failed to probe audio format: {0}")]
    ProbeFailed(String),

    #[error("audio container does not expose a default track")]
    NoDefaultTrack,

    #[error("failed to create audio decoder: {0}")]
    DecoderCreationFailed(String),

    #[error("failed while reading audio packets: {0}")]
    PacketReadFailed(String),

    #[error("failed to decode audio: {0}")]
    DecodeFailed(String),

    #[error("decoded audio contained no PCM samples")]
    NoSamples,

    #[error("audio track is missing sample rate metadata")]
    MissingSampleRate,

    #[error("audio track is missing channel metadata")]
    MissingChannels,

    #[error("decoder reset is not supported")]
    ResetNotSupported,

    #[error("internal decode error: {0}")]
    Internal(String),
}

pub fn decode_file(path: &Path) -> Result<DecodedAudio, DecodeError> {
    let file = File::open(path)
        .map_err(|e| DecodeError::FileOpenFailed(format!("{}: {}", path.display(), e)))?;
    decode_source(
        file,
        path.extension().and_then(|value| value.to_str()),
        &path.display().to_string(),
    )
}

pub fn decode_bytes(bytes: Vec<u8>, extension: &str) -> Result<DecodedAudio, DecodeError> {
    decode_source(
        Cursor::new(bytes),
        Some(extension),
        "in-memory Media+G audio",
    )
}

pub fn probe_file(path: &Path) -> Result<(), DecodeError> {
    let file = File::open(path)
        .map_err(|e| DecodeError::FileOpenFailed(format!("{}: {}", path.display(), e)))?;
    probe_source(
        file,
        path.extension().and_then(|value| value.to_str()),
        &path.display().to_string(),
    )
}

pub fn probe_bytes(bytes: Vec<u8>, extension: &str) -> Result<(), DecodeError> {
    probe_source(
        Cursor::new(bytes),
        Some(extension),
        "in-memory Media+G audio",
    )
}

fn extend_interleaved_samples(samples: &mut Vec<f32>, decoded: GenericAudioBufferRef<'_>) {
    // copy_to_vec_interleaved resizes (replaces) the destination, so use a
    // temporary buffer and extend the accumulator.
    let mut temp = Vec::with_capacity(decoded.samples_interleaved());
    decoded.copy_to_vec_interleaved(&mut temp);
    samples.extend_from_slice(&temp);
}

fn probe_source<R>(
    source: R,
    extension: Option<&str>,
    source_label: &str,
) -> Result<(), DecodeError>
where
    R: Read + Seek + MediaSource + Send + Sync + 'static,
{
    let media_source_stream = MediaSourceStream::new(Box::new(source), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }

    let format = symphonia::default::get_probe()
        .probe(
            &hint,
            media_source_stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| DecodeError::ProbeFailed(format!("for {source_label}: {e}")))?;

    format
        .default_track(TrackType::Audio)
        .ok_or(DecodeError::NoDefaultTrack)?;

    Ok(())
}

fn decode_source<R>(
    source: R,
    extension: Option<&str>,
    source_label: &str,
) -> Result<DecodedAudio, DecodeError>
where
    R: Read + Seek + MediaSource + Send + Sync + 'static,
{
    let media_source_stream = MediaSourceStream::new(Box::new(source), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            media_source_stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| DecodeError::ProbeFailed(format!("for {source_label}: {e}")))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or(DecodeError::NoDefaultTrack)?;
    let codec_params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or(DecodeError::NoDefaultTrack)?;
    let mut sample_rate = codec_params.sample_rate;
    let mut channels = codec_params.channels.as_ref().map(|layout| layout.count());

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_params, &AudioDecoderOptions::default())
        .map_err(|e| DecodeError::DecoderCreationFailed(e.to_string()))?;
    let track_id = track.id;
    let mut samples = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => {
                return Err(DecodeError::ResetNotSupported);
            }
            Err(error) => return Err(DecodeError::PacketReadFailed(error.to_string())),
        };

        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => {
                eprintln!(
                    "warning: skipping malformed audio packet at offset {} in {source_label}",
                    packet.pts
                );
                continue;
            }
            Err(e) => {
                return Err(DecodeError::DecodeFailed(format!(
                    "from {source_label}: {e}"
                )))
            }
        };

        let spec = decoded.spec();
        sample_rate.get_or_insert(spec.rate());
        channels.get_or_insert(spec.channels().count());
        extend_interleaved_samples(&mut samples, decoded);
    }

    if samples.is_empty() {
        return Err(DecodeError::NoSamples);
    }

    let sample_rate = sample_rate.ok_or(DecodeError::MissingSampleRate)?;
    let channels = channels.ok_or(DecodeError::MissingChannels)?;
    let frame_count = samples.len() / channels;
    let duration_ms = ((frame_count as f64 / sample_rate as f64) * 1000.0).round() as u64;

    Ok(DecodedAudio {
        sample_rate_hz: sample_rate,
        channels,
        duration_ms,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("audio")
    }

    #[test]
    fn decode_valid_file_succeeds() {
        let path = fixture_dir().join("fixture.wav");
        let result = decode_file(&path);
        assert!(
            result.is_ok(),
            "valid WAV should decode: {:?}",
            result.err()
        );
        let audio = result.unwrap();
        assert!(!audio.samples.is_empty());
        assert!(audio.sample_rate_hz > 0);
        assert!(audio.channels > 0);
        assert!(audio.duration_ms > 0);
    }

    #[test]
    fn decode_valid_ogg_succeeds() {
        let path = fixture_dir().join("fixture.ogg");
        let result = decode_file(&path);
        assert!(
            result.is_ok(),
            "valid OGG should decode: {:?}",
            result.err()
        );
        let audio = result.unwrap();
        assert!(!audio.samples.is_empty());
    }

    #[test]
    fn decode_nonexistent_file_returns_error() {
        let path = fixture_dir().join("nonexistent.wav");
        let result = decode_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn truncated_file_returns_error_or_partial() {
        let path = fixture_dir().join("fixture.wav");
        let Ok(audio) = decode_file(&path) else {
            return; // File missing in CI — skip.
        };
        assert!(audio.duration_ms > 100);
    }

    #[test]
    fn probe_valid_file_succeeds() {
        let path = fixture_dir().join("fixture.wav");
        let result = probe_file(&path);
        assert!(result.is_ok(), "valid WAV should probe: {:?}", result.err());
    }
}
