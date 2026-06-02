use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;
use std::fs::File;
use std::path::Path;
use std::thread::JoinHandle;
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error as SymphoniaError,
    formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

use super::decode::DecodeError;

/// Target buffer duration in seconds. 2 seconds provides enough headroom for
/// most decode latencies while keeping memory bounded.
const BUFFER_SECONDS: u32 = 2;

/// Minimum water mark in samples — below this the consumer should report
/// underrun and the producer should prioritise refilling.
const LOW_WATER_SAMPLES: usize = 4_410; // ~100ms at 44.1 kHz

/// High water mark — when the producer has filled this many samples it should
/// yield to avoid overrunning the consumer.
const HIGH_WATER_SAMPLES: usize = 88_200; // ~2s at 44.1 kHz (== BUFFER_SECONDS)

/// Capacity of the ring buffer in samples (interleaved f32).
/// `BUFFER_SECONDS × sample_rate × channels`.
pub fn ring_capacity(sample_rate: u32, channels: usize) -> usize {
    BUFFER_SECONDS as usize * sample_rate as usize * channels
}

/// Consumer side of a streaming audio track, held by the cpal callback.
pub struct AudioConsumer {
    cons: ringbuf::HeapCons<f32>,
    pub sample_rate: u32,
    pub channels: usize,
}

impl std::fmt::Debug for AudioConsumer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioConsumer")
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("available_samples", &self.cons.occupied_len())
            .finish()
    }
}

impl AudioConsumer {
    /// Number of samples available to read right now.
    pub fn available_samples(&self) -> usize {
        self.cons.occupied_len()
    }

    /// Available duration in milliseconds.
    pub fn available_ms(&self) -> u64 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0;
        }
        let frames = self.available_samples() / self.channels;
        (frames as u64 * 1000) / self.sample_rate as u64
    }

    /// Whether the buffer is below the low-water mark.
    pub fn is_below_low_water(&self) -> bool {
        self.cons.occupied_len() < LOW_WATER_SAMPLES
    }

    /// Pop up to `max_samples` interleaved samples into `output`.
    /// Returns the number of samples actually popped (may be less if the
    /// buffer doesn't have enough).
    pub fn pop_samples(&mut self, output: &mut [f32]) -> usize {
        self.cons.pop_slice(output)
    }
}

/// Producer side of a streaming audio track, held by the decode thread.
pub struct AudioProducer {
    prod: ringbuf::HeapProd<f32>,
}

impl AudioProducer {
    /// Push interleaved samples into the ring buffer.
    /// Returns the number of samples actually pushed.
    pub fn push_samples(&mut self, samples: &[f32]) -> usize {
        self.prod.push_slice(samples)
    }

    /// Number of samples that can be written before the buffer is full.
    pub fn vacant_samples(&self) -> usize {
        self.prod.vacant_len()
    }

    /// Whether the buffer is above the high-water mark (producer should yield).
    pub fn is_above_high_water(&self) -> bool {
        self.prod.vacant_len() < (ring_capacity_from_prod(&self.prod) - HIGH_WATER_SAMPLES)
    }
}

/// Helper to compute capacity from a producer (via the underlying ring buffer).
fn ring_capacity_from_prod(prod: &ringbuf::HeapProd<f32>) -> usize {
    // vacant_len + occupied_len == capacity
    prod.vacant_len() + prod.occupied_len()
}

/// Streaming variants for multi-track playback.
#[derive(Debug)]
pub enum StreamingTrack {
    /// Single audio track (no stems).
    Single { consumer: AudioConsumer },
    /// Two-stem mode: vocals + accompaniment.
    TwoStem {
        vocals: AudioConsumer,
        accompaniment: AudioConsumer,
    },
    /// Four-stem mode: vocals, drums, bass, other.
    FourStem {
        vocals: AudioConsumer,
        drums: AudioConsumer,
        bass: AudioConsumer,
        other: AudioConsumer,
    },
}

/// Create a producer-consumer pair for a single audio stream.
pub fn create_stream_pair(sample_rate: u32, channels: usize) -> (AudioProducer, AudioConsumer) {
    let capacity = ring_capacity(sample_rate, channels);
    let rb = HeapRb::<f32>::new(capacity);
    let (prod, cons) = rb.split();
    (
        AudioProducer { prod },
        AudioConsumer {
            cons,
            sample_rate,
            channels,
        },
    )
}

/// How many frames (not interleaved samples) correspond to `ms` at `sample_rate`.
pub fn ms_to_frames(ms: u64, sample_rate: u32) -> u64 {
    ms.saturating_mul(sample_rate as u64) / 1000
}

/// How many milliseconds correspond to `frames` at `sample_rate`.
pub fn frames_to_ms(frames: u64, sample_rate: u32) -> u64 {
    if sample_rate == 0 {
        return 0;
    }
    frames.saturating_mul(1000) / sample_rate as u64
}

/// Metadata about a streaming audio source, returned from the probe step.
pub struct StreamMetadata {
    pub sample_rate: u32,
    pub channels: usize,
    pub duration_ms: u64,
}

/// Probe an audio file for metadata without decoding the full PCM data.
/// Falls back to a full decode if the container doesn't expose `n_frames`.
pub fn probe_stream_metadata(path: &Path) -> Result<StreamMetadata, DecodeError> {
    let file = File::open(path)
        .map_err(|e| DecodeError::FileOpenFailed(format!("{}: {}", path.display(), e)))?;
    let extension = path.extension().and_then(|v| v.to_str());
    let label = path.display().to_string();

    let media_source_stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            media_source_stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| DecodeError::ProbeFailed(format!("for {label}: {e}")))?;

    let track = probed
        .format
        .default_track()
        .ok_or(DecodeError::NoDefaultTrack)?;
    let codec_params = &track.codec_params;

    let sample_rate = codec_params
        .sample_rate
        .ok_or(DecodeError::MissingSampleRate)?;
    let channels = codec_params
        .channels
        .map(|c| c.count())
        .ok_or(DecodeError::MissingChannels)?;

    // Try to get duration from container metadata.
    let duration_ms =
        if let (Some(n_frames), Some(tb)) = (codec_params.n_frames, codec_params.time_base) {
            let time = tb.calc_time(n_frames);
            (time.seconds * 1000) + (time.frac * 1000.0) as u64
        } else {
            // Fallback: full decode to compute duration.
            // Re-open the file since the MediaSourceStream was consumed.
            let decoded = super::decode::decode_file(path)?;
            decoded.duration_ms
        };

    Ok(StreamMetadata {
        sample_rate,
        channels,
        duration_ms,
    })
}

/// Spawn a decode producer thread that reads from `path` and pushes
/// interleaved f32 samples into the ring buffer.
///
/// Returns `(consumer, metadata, join_handle)`.
/// The consumer is ready to be used by the cpal callback.
/// The join handle can be used to wait for the decode thread to finish.
pub fn spawn_decode_producer(
    path: &Path,
) -> Result<
    (
        AudioConsumer,
        StreamMetadata,
        JoinHandle<Result<(), DecodeError>>,
    ),
    DecodeError,
> {
    let metadata = probe_stream_metadata(path)?;
    let (mut prod, cons) = create_stream_pair(metadata.sample_rate, metadata.channels);

    let path_buf = path.to_path_buf();
    let sample_rate = metadata.sample_rate;
    let channels = metadata.channels;

    let handle = std::thread::spawn(move || {
        decode_into_producer(&path_buf, &mut prod, sample_rate, channels)
    });

    Ok((cons, metadata, handle))
}

/// Decode a file and push samples into the producer. Runs on the decode thread.
fn decode_into_producer(
    path: &Path,
    prod: &mut AudioProducer,
    _expected_sample_rate: u32,
    _expected_channels: usize,
) -> Result<(), DecodeError> {
    let file = File::open(path)
        .map_err(|e| DecodeError::FileOpenFailed(format!("{}: {}", path.display(), e)))?;
    let extension = path.extension().and_then(|v| v.to_str());
    let label = path.display().to_string();

    let media_source_stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            media_source_stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| DecodeError::ProbeFailed(format!("for {label}: {e}")))?;
    let mut format = probed.format;

    let track = format.default_track().ok_or(DecodeError::NoDefaultTrack)?;
    let codec_params = &track.codec_params;

    let mut decoder = symphonia::default::get_codecs()
        .make(codec_params, &DecoderOptions::default())
        .map_err(|e| DecodeError::DecoderCreationFailed(e.to_string()))?;
    let track_id = track.id;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                return Err(DecodeError::ResetNotSupported);
            }
            Err(error) => return Err(DecodeError::PacketReadFailed(error.to_string())),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder
            .decode(&packet)
            .map_err(|e| DecodeError::DecodeFailed(format!("from {label}: {e}")))?;

        let mut sample_buffer =
            SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        sample_buffer.copy_interleaved_ref(decoded);
        let samples = sample_buffer.samples();

        // Push in chunks, yielding if the buffer is full.
        let mut offset = 0;
        while offset < samples.len() {
            let pushed = prod.push_samples(&samples[offset..]);
            if pushed == 0 {
                // Buffer full — yield to let the consumer drain.
                std::thread::yield_now();
            }
            offset += pushed;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_capacity_matches_target_duration() {
        // 44.1 kHz stereo, 2 seconds = 2 * 44100 * 2 = 176_400 samples
        assert_eq!(ring_capacity(44_100, 2), 176_400);
        // 48 kHz mono, 2 seconds = 2 * 48000 * 1 = 96_000
        assert_eq!(ring_capacity(48_000, 1), 96_000);
    }

    #[test]
    fn push_and_pop_samples_through_ring_buffer() {
        let (mut prod, mut cons) = create_stream_pair(44_100, 2);

        // Buffer starts empty
        assert_eq!(cons.available_samples(), 0);
        assert!(cons.is_below_low_water());

        // Push 1024 samples
        let input: Vec<f32> = (0..1024).map(|i| i as f32).collect();
        let pushed = prod.push_samples(&input);
        assert_eq!(pushed, 1024);
        assert_eq!(cons.available_samples(), 1024);

        // Pop them back
        let mut output = vec![0.0f32; 1024];
        let popped = cons.pop_samples(&mut output);
        assert_eq!(popped, 1024);
        assert_eq!(output, input);
        assert_eq!(cons.available_samples(), 0);
    }

    #[test]
    fn available_ms_reflects_buffer_fill_level() {
        let (mut prod, cons) = create_stream_pair(44_100, 2);

        // 44100 samples = 44100 frames / 2 channels = 22050 frames
        // 22050 frames at 44100 Hz = 500ms
        let samples_500ms = 44_100usize; // 1 second of interleaved stereo
        let input = vec![0.0f32; samples_500ms];
        prod.push_samples(&input);

        // 44100 interleaved stereo samples = 22050 frames = 500ms
        assert_eq!(cons.available_ms(), 500);
    }

    #[test]
    fn high_water_mark_works() {
        let (mut prod, mut cons) = create_stream_pair(44_100, 2);

        // Fill to just below high water
        let fill_amount = HIGH_WATER_SAMPLES - 100;
        let input = vec![0.0f32; fill_amount];
        prod.push_samples(&input);

        // Not above high water yet (lots of vacant space)
        assert!(!prod.is_above_high_water());

        // Fill past high water
        let more = vec![0.0f32; 200];
        prod.push_samples(&more);
        assert!(prod.is_above_high_water());

        // Drain below low water
        let mut drain = vec![0.0f32; cons.available_samples() - LOW_WATER_SAMPLES + 1];
        cons.pop_samples(&mut drain);
        assert!(cons.is_below_low_water());
    }

    #[test]
    fn ms_to_frames_conversion() {
        assert_eq!(ms_to_frames(1000, 44_100), 44_100);
        assert_eq!(ms_to_frames(500, 44_100), 22_050);
        assert_eq!(ms_to_frames(0, 44_100), 0);
    }

    #[test]
    fn frames_to_ms_conversion() {
        assert_eq!(frames_to_ms(44_100, 44_100), 1000);
        assert_eq!(frames_to_ms(22_050, 44_100), 500);
        assert_eq!(frames_to_ms(0, 44_100), 0);
        assert_eq!(frames_to_ms(100, 0), 0); // division by zero guard
    }

    #[test]
    fn streaming_decode_pushes_samples_into_ring_buffer() {
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("audio")
            .join("fixture.wav");

        let (mut consumer, metadata, handle) =
            spawn_decode_producer(&path).expect("spawn_decode_producer should succeed");

        // Metadata should match the fixture
        assert_eq!(metadata.sample_rate, 44_100);
        assert_eq!(metadata.channels, 2);
        assert!(metadata.duration_ms >= 999 && metadata.duration_ms <= 1_001);

        // Wait for decode to finish
        handle
            .join()
            .expect("decode thread should not panic")
            .expect("decode should succeed");

        // All samples should be in the ring buffer now
        // fixture.wav is 1 second of 44.1 kHz stereo = 88200 interleaved samples
        assert_eq!(consumer.available_samples(), 88_200);
        assert_eq!(consumer.available_ms(), 1000);

        // Pop and verify non-zero content
        let mut output = vec![0.0f32; 88_200];
        let popped = consumer.pop_samples(&mut output);
        assert_eq!(popped, 88_200);
        assert!(
            output.iter().any(|s| *s != 0.0),
            "decoded samples should not all be zero"
        );
    }
}
