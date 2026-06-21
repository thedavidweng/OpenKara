use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;
use std::collections::VecDeque;
use std::fs::File;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    Arc, Condvar, Mutex,
};
use std::thread::JoinHandle;
use std::time::Duration;
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error as SymphoniaError,
    formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

use super::decode::DecodeError;

/// Configuration for low-bitrate proxy mode.
///
/// When enabled, decoded audio is downsampled to `target_sample_rate` and
/// converted to mono before being pushed into the ring buffer. This reduces
/// the data rate in the buffer by roughly `(original_rate / target_rate) * channels`,
/// allowing playback to continue on slow network connections.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Whether proxy mode is enabled.
    pub enabled: bool,
    /// Target sample rate for the proxy (e.g., 22050).
    pub target_sample_rate: u32,
    /// Target channel count (1 for mono).
    pub target_channels: usize,
}

impl ProxyConfig {
    /// Proxy mode disabled (pass-through).
    pub fn none() -> Self {
        Self {
            enabled: false,
            target_sample_rate: 0,
            target_channels: 0,
        }
    }

    /// Low-bitrate proxy: mono 22050 Hz (~4x data reduction from stereo 44100).
    pub fn low_bitrate() -> Self {
        Self {
            enabled: true,
            target_sample_rate: 22_050,
            target_channels: 1,
        }
    }
}

/// Downsample interleaved f32 samples from one rate/channels to another using
/// linear interpolation. Returns the resampled samples.
fn resample_interleaved(
    samples: &[f32],
    from_rate: u32,
    from_channels: usize,
    to_rate: u32,
    to_channels: usize,
) -> Vec<f32> {
    if from_rate == to_rate && from_channels == to_channels {
        return samples.to_vec();
    }

    let from_frames = samples.len() / from_channels;
    let ratio = from_rate as f64 / to_rate as f64;
    let to_frames = (from_frames as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(to_frames * to_channels);

    for i in 0..to_frames {
        let src_pos = i as f64 * ratio;
        let src_idx = src_pos as usize;
        let frac = src_pos - src_idx as f64;

        for ch in 0..to_channels {
            let from_ch = ch.min(from_channels - 1);
            let s0_idx = src_idx * from_channels + from_ch;
            let s1_idx = ((src_idx + 1).min(from_frames - 1)) * from_channels + from_ch;

            let s0 = samples.get(s0_idx).copied().unwrap_or(0.0);
            let s1 = samples.get(s1_idx).copied().unwrap_or(0.0);
            out.push(s0 + (s1 - s0) * frac as f32);
        }
    }

    out
}

/// Shared seek target between the consumer (output callback) and the producer
/// (decode thread). Stores the target frame position, or `NONE` when no seek
/// is pending.
pub struct SeekTarget(AtomicI64);

impl Default for SeekTarget {
    fn default() -> Self {
        Self::new()
    }
}

impl SeekTarget {
    pub const NONE: i64 = -1;

    pub fn new() -> Self {
        Self(AtomicI64::new(Self::NONE))
    }

    pub fn load(&self, order: Ordering) -> i64 {
        self.0.load(order)
    }

    pub fn store(&self, val: i64, order: Ordering) {
        self.0.store(val, order);
    }
}

impl std::fmt::Debug for SeekTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeekTarget")
            .field("target", &self.0.load(Ordering::Relaxed))
            .finish()
    }
}

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
    /// Prepend buffer for resampling lookahead. Uses `VecDeque` for efficient
    /// front insertion without allocating a new `Vec` on every `prepend_samples`.
    pending_samples: VecDeque<f32>,
    pub sample_rate: u32,
    pub channels: usize,
    /// Set by the producer when decode reaches EOF.
    is_eof: Arc<AtomicBool>,
    /// Set by the producer after a seek to signal the consumer should drain
    /// stale samples from the ring buffer before reading new ones.
    needs_flush: Arc<AtomicBool>,
    /// Shared seek target between consumer and producer.
    seek_target: Arc<SeekTarget>,
    /// Pre-allocated scratch buffer for `acknowledge_flush` to avoid heap
    /// allocation on the realtime audio thread.
    flush_scratch: Vec<f32>,
    /// Condvar notified when flush is acknowledged. Wakes the producer
    /// in `signal_flush` so it doesn't spin-wait.
    flush_done: Arc<(Mutex<()>, Condvar)>,
}

impl std::fmt::Debug for AudioConsumer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioConsumer")
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("available_samples", &self.cons.occupied_len())
            .field("is_eof", &self.is_eof.load(Ordering::Relaxed))
            .finish()
    }
}

impl AudioConsumer {
    /// Number of samples available to read right now.
    pub fn available_samples(&self) -> usize {
        self.pending_samples.len() + self.cons.occupied_len()
    }

    /// Available duration in milliseconds.
    pub fn available_ms(&self) -> u64 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0;
        }
        let frames = self.available_samples() / self.channels;
        (frames as u64 * 1000) / self.sample_rate as u64
    }

    /// Number of source frames (not interleaved samples) available to read.
    pub fn available_src_frames(&self) -> usize {
        let ch = self.channels.max(1);
        self.available_samples() / ch
    }

    /// Whether the buffer is below the low-water mark.
    /// EOF streams are never treated as underrun — the decode thread is done.
    pub fn is_below_low_water(&self) -> bool {
        !self.is_eof() && self.available_samples() < LOW_WATER_SAMPLES
    }

    /// Whether the buffer is above the high-water mark.
    /// EOF streams are always considered "ready" so playback can drain and finish.
    pub fn is_above_high_water(&self) -> bool {
        self.is_eof() || self.available_samples() >= HIGH_WATER_SAMPLES
    }

    /// Whether the producer has finished decoding all samples.
    pub fn is_eof(&self) -> bool {
        self.is_eof.load(Ordering::Relaxed)
    }

    /// Whether the producer has signaled a flush is needed after a seek.
    pub fn needs_flush(&self) -> bool {
        self.needs_flush.load(Ordering::Relaxed)
    }

    /// Acknowledge the flush — clears the flag and drains stale samples.
    pub fn acknowledge_flush(&mut self) {
        self.needs_flush.store(false, Ordering::Relaxed);
        self.pending_samples.clear();
        // Drain all stale samples that were pushed before the seek.
        // Reuse the pre-allocated scratch buffer to avoid heap allocation
        // on the realtime audio thread.
        let occupied = self.cons.occupied_len();
        self.flush_scratch.resize(occupied, 0.0);
        let _ = self.cons.pop_slice(&mut self.flush_scratch);
        // Notify the producer (waiting in `signal_flush`) that the flush
        // is complete so it can resume pushing post-seek samples.
        let (_, cvar) = &*self.flush_done;
        cvar.notify_one();
    }

    /// Get the shared seek target for setting seek positions.
    pub fn seek_target(&self) -> &SeekTarget {
        &self.seek_target
    }

    /// Pop up to `max_samples` interleaved samples into `output`.
    /// Returns the number of samples actually popped (may be less if the
    /// buffer doesn't have enough).
    pub fn pop_samples(&mut self, output: &mut [f32]) -> usize {
        let pending_count = self.pending_samples.len().min(output.len());
        if pending_count > 0 {
            for (i, sample) in self.pending_samples.drain(..pending_count).enumerate() {
                output[i] = sample;
            }
        }

        if pending_count == output.len() {
            return pending_count;
        }

        pending_count + self.cons.pop_slice(&mut output[pending_count..])
    }

    /// Put samples back at the front of the next pop. Streaming resampling
    /// reads a small lookahead window for interpolation; frames beyond the
    /// committed render position must be preserved for the next callback.
    ///
    /// Uses `VecDeque` for O(n) front insertion without allocating a new buffer.
    pub(crate) fn prepend_samples(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }

        // Prepend by extending front in reverse order.
        for &sample in samples.iter().rev() {
            self.pending_samples.push_front(sample);
        }
    }
}

/// Producer side of a streaming audio track, held by the decode thread.
pub struct AudioProducer {
    prod: ringbuf::HeapProd<f32>,
    /// Shared EOF flag — set when decode reaches end of file.
    is_eof: Arc<AtomicBool>,
    /// Shared flush-needed flag — set after a seek so the consumer drains stale data.
    needs_flush: Arc<AtomicBool>,
    /// Shared seek target — checked before each packet decode.
    seek_target: Arc<SeekTarget>,
    /// R6: Shared flush epoch — incremented on each flush so the consumer can
    /// track which flush it has acknowledged.
    flush_epoch: Arc<AtomicU64>,
    /// Condvar notified by the consumer when flush is acknowledged.
    /// Allows `signal_flush` to sleep instead of spinning with `yield_now`.
    flush_done: Arc<(Mutex<()>, Condvar)>,
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

    /// Mark this producer's stream as EOF.
    pub fn set_eof(&self) {
        self.is_eof.store(true, Ordering::Relaxed);
    }

    /// After a seek, signal the consumer to drain stale samples.
    ///
    /// R6: Increments the flush epoch and waits (with a bounded timeout) for
    /// the ring buffer to drain. Uses a condvar instead of `yield_now` spin
    /// to avoid busy-waiting while giving the realtime thread priority.
    pub fn signal_flush(&self) {
        self.flush_epoch.fetch_add(1, Ordering::Relaxed);
        self.needs_flush.store(true, Ordering::Relaxed);

        // Wait for the consumer to drain stale samples via condvar.
        // The consumer calls `acknowledge_flush` on the audio callback
        // (~10ms) and notifies this condvar after draining.
        let (lock, cvar) = &*self.flush_done;
        let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let _ = cvar.wait_timeout_while(guard, Duration::from_millis(100), |_| {
            self.prod.occupied_len() > 0
        });
    }

    /// Get the shared seek target.
    pub fn seek_target(&self) -> &SeekTarget {
        &self.seek_target
    }

    /// Get the Arc to the shared seek target (for cloning across thread boundaries).
    pub fn seek_target_arc(&self) -> &Arc<SeekTarget> {
        &self.seek_target
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
    /// Boxed to keep the enum size reasonable — `AudioConsumer` carries
    /// pre-allocated scratch buffers and sync primitives that add up.
    FourStem {
        vocals: Box<AudioConsumer>,
        drums: Box<AudioConsumer>,
        bass: Box<AudioConsumer>,
        other: Box<AudioConsumer>,
    },
}

impl StreamingTrack {
    /// Get mutable references to all consumers in this track.
    pub fn consumers_mut(&mut self) -> Vec<&mut AudioConsumer> {
        match self {
            StreamingTrack::Single { consumer } => vec![consumer],
            StreamingTrack::TwoStem {
                vocals,
                accompaniment,
            } => vec![vocals, accompaniment],
            StreamingTrack::FourStem {
                vocals,
                drums,
                bass,
                other,
            } => vec![&mut **vocals, &mut **drums, &mut **bass, &mut **other],
        }
    }

    /// Immutable view of all consumers — used for budget/EOF checks without mixing.
    pub fn consumers(&self) -> Vec<&AudioConsumer> {
        match self {
            StreamingTrack::Single { consumer } => vec![consumer],
            StreamingTrack::TwoStem {
                vocals,
                accompaniment,
            } => vec![vocals, accompaniment],
            StreamingTrack::FourStem {
                vocals,
                drums,
                bass,
                other,
            } => vec![&**vocals, &**drums, &**bass, &**other],
        }
    }

    /// True when every consumer has reached EOF and drained its ring buffer.
    pub fn all_eof_and_drained(&self) -> bool {
        self.consumers()
            .iter()
            .all(|c| c.is_eof() && c.available_samples() == 0)
    }
}

/// Create a producer-consumer pair for a single audio stream.
pub fn create_stream_pair(sample_rate: u32, channels: usize) -> (AudioProducer, AudioConsumer) {
    let capacity = ring_capacity(sample_rate, channels);
    let rb = HeapRb::<f32>::new(capacity);
    let (prod, cons) = rb.split();

    let is_eof = Arc::new(AtomicBool::new(false));
    let needs_flush = Arc::new(AtomicBool::new(false));
    let seek_target = Arc::new(SeekTarget::new());
    let flush_epoch = Arc::new(AtomicU64::new(0));
    let flush_done = Arc::new((Mutex::new(()), Condvar::new()));

    (
        AudioProducer {
            prod,
            is_eof: Arc::clone(&is_eof),
            needs_flush: Arc::clone(&needs_flush),
            seek_target: Arc::clone(&seek_target),
            flush_epoch: Arc::clone(&flush_epoch),
            flush_done: Arc::clone(&flush_done),
        },
        AudioConsumer {
            cons,
            pending_samples: VecDeque::new(),
            sample_rate,
            channels,
            is_eof,
            needs_flush,
            seek_target,
            // Pre-allocate to ring buffer capacity so the first seek's
            // acknowledge_flush does not trigger a heap allocation on the
            // realtime audio thread.
            flush_scratch: Vec::with_capacity(capacity),
            flush_done,
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
    /// Item 12: Duration is optional so playback can start immediately.
    /// When `None`, the container did not expose frame count metadata and
    /// the duration should be resolved asynchronously after playback begins.
    pub duration_ms: Option<u64>,
}

/// Probe an audio file for metadata without decoding the full PCM data.
///
/// Item 12: Returns `duration_ms: None` when the container doesn't expose
/// `n_frames` instead of blocking on a full decode. Playback can start
/// immediately and the duration can be resolved asynchronously.
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

    let mut probed = symphonia::default::get_probe()
        .format(
            &hint,
            media_source_stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| DecodeError::ProbeFailed(format!("for {label}: {e}")))?;

    let (codec_params, track_id) = {
        let track = probed
            .format
            .default_track()
            .ok_or(DecodeError::NoDefaultTrack)?;
        (track.codec_params.clone(), track.id)
    };

    let mut sample_rate = codec_params.sample_rate;
    let mut channels = codec_params.channels.map(|c| c.count());

    // Some containers don't expose sample rate / channel layout in the
    // codec params.  symphonia only populates these after decoding the
    // first packet, so try that before giving up.
    if sample_rate.is_none() || channels.is_none() {
        if let Ok(mut decoder) =
            symphonia::default::get_codecs().make(&codec_params, &DecoderOptions::default())
        {
            while let Ok(packet) = probed.format.next_packet() {
                if packet.track_id() != track_id {
                    continue;
                }
                if let Ok(decoded) = decoder.decode(&packet) {
                    let spec = *decoded.spec();
                    sample_rate.get_or_insert(spec.rate);
                    channels.get_or_insert(spec.channels.count());
                    break;
                }
            }
        }
    }

    let sample_rate = sample_rate.ok_or(DecodeError::MissingSampleRate)?;
    let channels = channels.ok_or(DecodeError::MissingChannels)?;

    // Item 12: Try to get duration from container metadata only.
    // Do NOT fall back to full decode — return None so playback starts immediately.
    let duration_ms =
        if let (Some(n_frames), Some(tb)) = (codec_params.n_frames, codec_params.time_base) {
            let time = tb.calc_time(n_frames);
            Some((time.seconds * 1000) + (time.frac * 1000.0) as u64)
        } else {
            None
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
    spawn_decode_producer_with_proxy(path, ProxyConfig::none())
}

/// Like `spawn_decode_producer`, but with optional proxy mode for low-bitrate
/// streaming. When proxy is enabled, decoded audio is downsampled before being
/// pushed into the ring buffer, and the consumer reports the proxy sample rate.
pub fn spawn_decode_producer_with_proxy(
    path: &Path,
    proxy: ProxyConfig,
) -> Result<
    (
        AudioConsumer,
        StreamMetadata,
        JoinHandle<Result<(), DecodeError>>,
    ),
    DecodeError,
> {
    let metadata = probe_stream_metadata(path)?;

    // If proxy is enabled, the ring buffer and consumer use the proxy parameters.
    let (ring_rate, ring_channels) = if proxy.enabled {
        (proxy.target_sample_rate, proxy.target_channels)
    } else {
        (metadata.sample_rate, metadata.channels)
    };

    let (mut prod, cons) = create_stream_pair(ring_rate, ring_channels);

    let path_buf = path.to_path_buf();
    let sample_rate = metadata.sample_rate;
    let channels = metadata.channels;

    let handle = std::thread::spawn(move || {
        decode_into_producer(&path_buf, &mut prod, sample_rate, channels, &proxy)
    });

    Ok((cons, metadata, handle))
}

/// Spawn a decode producer from a `RemoteMediaSource` (or any `MediaSource`).
/// The caller provides pre-probed metadata since the source may not support
/// seeking back to re-probe.
///
/// Returns `(consumer, join_handle)`.
pub fn spawn_decode_producer_from_source(
    source: Box<dyn symphonia::core::io::MediaSource>,
    extension: Option<&str>,
    metadata: &StreamMetadata,
    proxy: ProxyConfig,
) -> Result<(AudioConsumer, JoinHandle<Result<(), DecodeError>>), DecodeError> {
    let (ring_rate, ring_channels) = if proxy.enabled {
        (proxy.target_sample_rate, proxy.target_channels)
    } else {
        (metadata.sample_rate, metadata.channels)
    };

    let (mut prod, cons) = create_stream_pair(ring_rate, ring_channels);

    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }
    let label = "remote-source".to_owned();
    let sr = metadata.sample_rate;
    let ch = metadata.channels;

    let handle = std::thread::spawn(move || {
        let mss = MediaSourceStream::new(source, Default::default());
        decode_mss_into_producer(mss, hint, &label, &mut prod, sr, ch, &proxy)
    });

    Ok((cons, handle))
}

/// Result of spawning decode producers for multiple stems.
pub struct MultiStemResult {
    pub track: StreamingTrack,
    pub metadata: Vec<StreamMetadata>,
    pub decode_handles: Vec<JoinHandle<Result<(), DecodeError>>>,
}

/// Spawn decode producers for multiple stem files (e.g., vocals, drums, bass, other).
/// Returns a `StreamingTrack` with the appropriate variant based on the number of stems,
/// along with metadata and join handles for each decode thread.
pub fn spawn_multi_stem_decode_producers(
    paths: &[std::path::PathBuf],
) -> Result<MultiStemResult, DecodeError> {
    spawn_multi_stem_decode_producers_with_proxy(paths, ProxyConfig::none())
}

/// Like `spawn_multi_stem_decode_producers`, but with optional proxy mode.
pub fn spawn_multi_stem_decode_producers_with_proxy(
    paths: &[std::path::PathBuf],
    proxy: ProxyConfig,
) -> Result<MultiStemResult, DecodeError> {
    if paths.is_empty() || paths.len() > 4 {
        return Err(DecodeError::ProbeFailed(format!(
            "expected 1-4 stem paths, got {}",
            paths.len()
        )));
    }

    let mut consumers = Vec::with_capacity(paths.len());
    let mut metadata_vec = Vec::with_capacity(paths.len());
    let mut handles = Vec::with_capacity(paths.len());

    for path in paths {
        let meta = probe_stream_metadata(path)?;

        let (ring_rate, ring_channels) = if proxy.enabled {
            (proxy.target_sample_rate, proxy.target_channels)
        } else {
            (meta.sample_rate, meta.channels)
        };

        let (mut prod, cons) = create_stream_pair(ring_rate, ring_channels);

        let path_buf = path.clone();
        let sr = meta.sample_rate;
        let ch = meta.channels;
        let proxy_clone = proxy.clone();
        let handle = std::thread::spawn(move || {
            decode_into_producer(&path_buf, &mut prod, sr, ch, &proxy_clone)
        });

        consumers.push(cons);
        metadata_vec.push(meta);
        handles.push(handle);
    }

    let track = match consumers.len() {
        1 => StreamingTrack::Single {
            consumer: consumers.pop().unwrap(),
        },
        2 => {
            let mut iter = consumers.into_iter();
            StreamingTrack::TwoStem {
                vocals: iter.next().unwrap(),
                accompaniment: iter.next().unwrap(),
            }
        }
        _ => {
            let mut iter = consumers.into_iter();
            StreamingTrack::FourStem {
                vocals: Box::new(iter.next().unwrap()),
                drums: Box::new(iter.next().unwrap()),
                bass: Box::new(iter.next().unwrap()),
                other: Box::new(iter.next().unwrap()),
            }
        }
    };

    Ok(MultiStemResult {
        track,
        metadata: metadata_vec,
        decode_handles: handles,
    })
}

/// Decode a file and push samples into the producer. Runs on the decode thread.
/// Handles seek requests via the shared `seek_target` and signals EOF via `is_eof`.
fn decode_into_producer(
    path: &Path,
    prod: &mut AudioProducer,
    expected_sample_rate: u32,
    expected_channels: usize,
    proxy: &ProxyConfig,
) -> Result<(), DecodeError> {
    let file = File::open(path)
        .map_err(|e| DecodeError::FileOpenFailed(format!("{}: {}", path.display(), e)))?;
    let extension = path.extension().and_then(|v| v.to_str());
    let label = path.display().to_string();

    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }

    decode_mss_into_producer(
        mss,
        hint,
        &label,
        prod,
        expected_sample_rate,
        expected_channels,
        proxy,
    )
}

/// Decode from a `MediaSourceStream` into the producer. Used for both local
/// files and remote sources (e.g., `RemoteMediaSource`).
fn decode_mss_into_producer(
    mss: MediaSourceStream,
    hint: Hint,
    label: &str,
    prod: &mut AudioProducer,
    expected_sample_rate: u32,
    expected_channels: usize,
    proxy: &ProxyConfig,
) -> Result<(), DecodeError> {
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
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

    // Clone the Arc so we can access the seek target without borrowing prod.
    let seek_target = Arc::clone(prod.seek_target_arc());

    loop {
        // Check for pending seek before each packet.
        let target = seek_target.load(Ordering::Relaxed);
        if target != SeekTarget::NONE {
            let seconds = (target as u64) / expected_sample_rate as u64;
            let frac = ((target as u64) % expected_sample_rate as u64) as f64
                / expected_sample_rate as f64;
            match format.seek(
                symphonia::core::formats::SeekMode::Accurate,
                symphonia::core::formats::SeekTo::Time {
                    track_id: Some(track_id),
                    time: symphonia::core::units::Time { seconds, frac },
                },
            ) {
                Ok(_) => {
                    decoder.reset();
                    seek_target.store(SeekTarget::NONE, Ordering::Relaxed);
                    // Signal the consumer to drain any stale samples on its side.
                    prod.signal_flush();
                }
                Err(_) => {
                    // Seek failed — clear the target to avoid retrying forever.
                    seek_target.store(SeekTarget::NONE, Ordering::Relaxed);
                }
            }
            continue;
        }

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

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => {
                // Tolerate malformed packets — skip and continue decoding.
                eprintln!(
                    "warning: skipping malformed audio packet at offset {} in {label}",
                    packet.ts()
                );
                continue;
            }
            Err(e) => return Err(DecodeError::DecodeFailed(format!("from {label}: {e}"))),
        };

        let mut sample_buffer =
            SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        sample_buffer.copy_interleaved_ref(decoded);
        let samples = sample_buffer.samples();

        // Apply proxy resampling if enabled.
        let resampled;
        let push_samples: &[f32] = if proxy.enabled {
            resampled = resample_interleaved(
                samples,
                expected_sample_rate,
                expected_channels,
                proxy.target_sample_rate,
                proxy.target_channels,
            );
            &resampled
        } else {
            samples
        };

        // Push in chunks, yielding if the buffer is full.
        let mut offset = 0;
        while offset < push_samples.len() {
            let pushed = prod.push_samples(&push_samples[offset..]);
            if pushed == 0 {
                // Buffer full — yield to let the consumer drain.
                std::thread::yield_now();
            }
            offset += pushed;
        }
    }

    prod.set_eof();
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
        let duration = metadata
            .duration_ms
            .expect("WAV should have duration from container");
        assert!((999..=1_001).contains(&duration));

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

    #[test]
    fn eof_consumer_is_not_below_low_water() {
        let (prod, consumer) = super::create_stream_pair(44_100, 2);
        prod.set_eof();
        assert!(!consumer.is_below_low_water());
        assert!(consumer.is_above_high_water());
    }

    #[test]
    fn all_eof_and_drained_detects_natural_end() {
        let (prod, consumer) = super::create_stream_pair(44_100, 2);
        prod.set_eof();
        let track = super::StreamingTrack::Single { consumer };
        assert!(track.all_eof_and_drained());
    }

    #[test]
    fn consumer_reports_eof_when_producer_finishes() {
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("audio")
            .join("fixture.wav");

        let (consumer, _metadata, handle) =
            spawn_decode_producer(&path).expect("spawn_decode_producer should succeed");

        // Before decode finishes, EOF should be false
        assert!(!consumer.is_eof());

        handle
            .join()
            .expect("decode thread should not panic")
            .expect("decode should succeed");

        // After decode finishes, EOF should be true
        assert!(consumer.is_eof());
    }

    #[test]
    fn probe_stream_metadata_returns_valid_duration_for_m4a() {
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("metadata")
            .join("fixture.m4a");

        let metadata = probe_stream_metadata(&path).expect("probe should succeed for m4a");
        eprintln!(
            "m4a metadata: rate={}, ch={}, dur={:?}ms",
            metadata.sample_rate, metadata.channels, metadata.duration_ms
        );
        assert!(metadata.sample_rate > 0);
        assert!(metadata.channels > 0);
        let duration = metadata
            .duration_ms
            .expect("m4a should have duration from container");
        assert!(duration > 0, "duration_ms must be > 0, got {}", duration);
    }

    #[test]
    fn streaming_decode_works_for_m4a_files() {
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("metadata")
            .join("fixture.m4a");

        // Probe should succeed and return valid metadata
        let metadata =
            probe_stream_metadata(&path).expect("probe_stream_metadata should succeed for m4a");
        assert!(metadata.sample_rate > 0, "sample_rate should be positive");
        assert!(metadata.channels > 0, "channels should be positive");
        let duration = metadata.duration_ms.expect("m4a should have duration");
        assert!(
            duration > 0,
            "duration_ms should be positive, got {}",
            duration
        );

        // Spawn decode producer and verify it fills the ring buffer
        let (mut consumer, _, handle) =
            spawn_decode_producer(&path).expect("spawn_decode_producer should succeed for m4a");

        // Wait for decode to finish
        handle
            .join()
            .expect("decode thread should not panic")
            .expect("decode should succeed for m4a");

        // Should have decoded audio samples
        assert!(
            consumer.available_samples() > 0,
            "ring buffer should have samples after decoding m4a"
        );

        // Pop and verify non-zero content
        let mut output = vec![0.0f32; consumer.available_samples()];
        let popped = consumer.pop_samples(&mut output);
        assert!(popped > 0, "should pop > 0 samples");
        assert!(
            output.iter().any(|s| *s != 0.0),
            "decoded m4a samples should not all be zero"
        );
    }

    #[test]
    fn streaming_decode_with_render_output_buffer_works_for_m4a() {
        use crate::audio::output::render_output_buffer;
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("metadata")
            .join("fixture.m4a");

        let (consumer, metadata, handle) =
            spawn_decode_producer(&path).expect("spawn_decode_producer should succeed for m4a");

        let mut controller = crate::audio::playback::PlaybackController::default();
        controller.start_track_streaming(
            "test-m4a".to_owned(),
            metadata.sample_rate,
            metadata.channels,
            metadata.duration_ms.unwrap_or(0),
            StreamingTrack::Single { consumer },
            0,
        );

        // Wait for decode to fill the buffer
        handle
            .join()
            .expect("decode thread should not panic")
            .expect("decode should succeed");

        // Render audio — the ring buffer should have data now
        let device_rate = metadata.sample_rate;
        let device_channels = 2;
        let buffer_frames = 512;
        let mut output = vec![0.0f32; buffer_frames * device_channels];
        let mut rc = crate::audio::output::ResamplerCache::new();
        let rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            device_rate,
            device_channels,
            &mut rc,
        );

        assert!(rendered > 0, "should render audio from m4a streaming");
        assert!(
            output.iter().any(|s| *s != 0.0),
            "rendered m4a audio should contain non-zero samples"
        );
    }

    #[test]
    fn streaming_playback_loop_consumes_entire_track() {
        use crate::audio::output::render_output_buffer;
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("metadata")
            .join("fixture.m4a");

        let (consumer, metadata, handle) =
            spawn_decode_producer(&path).expect("spawn_decode_producer should succeed");

        let mut controller = crate::audio::playback::PlaybackController::default();
        controller.start_track_streaming(
            "test-m4a-loop".to_owned(),
            metadata.sample_rate,
            metadata.channels,
            metadata.duration_ms.unwrap_or(0),
            StreamingTrack::Single { consumer },
            0,
        );

        // Wait for full decode
        handle.join().expect("thread join").expect("decode ok");

        let device_rate = metadata.sample_rate;
        let device_channels = 2;
        let buffer_frames = 512;
        let mut total_rendered = 0u64;
        let mut callbacks_with_audio = 0u32;

        // Simulate cpal callback loop until track finishes
        let mut rc = crate::audio::output::ResamplerCache::new();
        for _ in 0..10_000 {
            let snapshot = controller.snapshot();
            if !snapshot.is_playing {
                break;
            }
            let mut output = vec![0.0f32; buffer_frames * device_channels];
            let rendered = render_output_buffer(
                &mut controller,
                &mut output,
                &mut Vec::new(),
                device_rate,
                device_channels,
                &mut rc,
            );
            if rendered > 0 {
                total_rendered += rendered as u64;
                callbacks_with_audio += 1;
            }
        }

        assert!(
            callbacks_with_audio > 0,
            "should have rendered audio in at least one callback"
        );
        // fixture.m4a is ~1s of audio at 44.1kHz stereo = ~88200 interleaved samples
        assert!(total_rendered > 0, "total rendered samples should be > 0");

        // The position should have advanced
        let final_snapshot = controller.snapshot();
        assert!(
            final_snapshot.position_ms > 0,
            "position should have advanced from 0"
        );
    }

    #[test]
    fn multi_stem_decode_producers_all_fill_their_buffers() {
        use std::path::PathBuf;

        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("audio")
            .join("fixture.wav");

        // Simulate two-stem mode: same file for both (in real use, different files)
        let result = spawn_multi_stem_decode_producers(&[fixture.clone(), fixture.clone()])
            .expect("spawn_multi_stem_decode_producers should succeed");

        let StreamingTrack::TwoStem {
            vocals,
            accompaniment,
        } = result.track
        else {
            panic!("expected TwoStem variant");
        };

        // Both metadata entries should match the fixture
        assert_eq!(result.metadata.len(), 2);
        for meta in &result.metadata {
            assert_eq!(meta.sample_rate, 44_100);
            assert_eq!(meta.channels, 2);
        }

        // Wait for all decode threads
        for handle in result.decode_handles {
            handle
                .join()
                .expect("decode thread should not panic")
                .expect("decode should succeed");
        }

        // Both consumers should have the full 1-second audio
        assert_eq!(vocals.available_samples(), 88_200);
        assert_eq!(accompaniment.available_samples(), 88_200);
        assert!(vocals.is_eof());
        assert!(accompaniment.is_eof());
    }

    #[test]
    fn seek_target_can_be_shared_between_producer_and_consumer() {
        use std::path::PathBuf;
        use std::sync::atomic::Ordering;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("audio")
            .join("fixture.wav");

        let (consumer, metadata, handle) =
            spawn_decode_producer(&path).expect("spawn_decode_producer should succeed");

        // Wait for decode to fill the buffer
        handle
            .join()
            .expect("decode thread should not panic")
            .expect("decode should succeed");

        assert_eq!(consumer.available_samples(), 88_200);

        // No seek pending
        let seek_target = consumer.seek_target();
        assert_eq!(seek_target.load(Ordering::Relaxed), SeekTarget::NONE);

        // Setting a seek target should be visible to the consumer
        seek_target.store(
            ms_to_frames(500, metadata.sample_rate) as i64,
            Ordering::Relaxed,
        );
        assert_ne!(seek_target.load(Ordering::Relaxed), SeekTarget::NONE);
    }

    #[test]
    fn resample_same_rate_passthrough() {
        let input = vec![1.0f32, 2.0, 3.0, 4.0]; // 2 stereo frames
        let output = resample_interleaved(&input, 44_100, 2, 44_100, 2);
        assert_eq!(output, input);
    }

    #[test]
    fn resample_stereo_to_mono() {
        // 2 stereo frames: [L0, R0, L1, R1]
        let input = vec![1.0f32, 0.5, 0.8, 0.2];
        let output = resample_interleaved(&input, 44_100, 2, 44_100, 1);
        // Each mono frame takes the first channel (L).
        assert_eq!(output.len(), 2); // 2 mono frames
        assert_eq!(output[0], 1.0); // L0
        assert_eq!(output[1], 0.8); // L1
    }

    #[test]
    fn resample_halves_sample_rate() {
        // 4 mono frames at 44100 Hz → 2 mono frames at 22050 Hz
        let input = vec![1.0f32, 2.0, 3.0, 4.0];
        let output = resample_interleaved(&input, 44_100, 1, 22_050, 1);
        assert_eq!(output.len(), 2);
        // Frame 0: src_pos=0.0 → 1.0
        assert_eq!(output[0], 1.0);
        // Frame 1: src_pos=2.0 → 3.0
        assert_eq!(output[1], 3.0);
    }

    #[test]
    fn resample_stereo_to_mono_halved_rate() {
        // 4 stereo frames at 44100 Hz → 2 mono frames at 22050 Hz
        let input = vec![
            1.0f32, 0.1, // frame 0: L=1.0, R=0.1
            2.0, 0.2, // frame 1: L=2.0, R=0.2
            3.0, 0.3, // frame 2: L=3.0, R=0.3
            4.0, 0.4, // frame 3: L=4.0, R=0.4
        ];
        let output = resample_interleaved(&input, 44_100, 2, 22_050, 1);
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], 1.0); // src_pos=0.0, ch0 (L) of frame 0
        assert_eq!(output[1], 3.0); // src_pos=2.0, ch0 (L) of frame 2
    }

    #[test]
    fn proxy_config_low_bitrate_values() {
        let proxy = ProxyConfig::low_bitrate();
        assert!(proxy.enabled);
        assert_eq!(proxy.target_sample_rate, 22_050);
        assert_eq!(proxy.target_channels, 1);
    }

    #[test]
    fn proxy_config_none_is_disabled() {
        let proxy = ProxyConfig::none();
        assert!(!proxy.enabled);
    }

    #[test]
    fn proxy_decode_producer_downsamples() {
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("audio")
            .join("fixture.wav");

        let proxy = ProxyConfig::low_bitrate();
        let (consumer, metadata, handle) =
            spawn_decode_producer_with_proxy(&path, proxy).expect("spawn should succeed");

        // Source metadata is unchanged.
        assert_eq!(metadata.sample_rate, 44_100);
        assert_eq!(metadata.channels, 2);

        handle
            .join()
            .expect("thread should not panic")
            .expect("decode should succeed");

        // Consumer reports proxy rate.
        assert_eq!(consumer.sample_rate, 22_050);
        assert_eq!(consumer.channels, 1);

        // fixture.wav is 1s: 44100 stereo → 22050 mono = 22050 samples.
        assert_eq!(consumer.available_samples(), 22_050);
        assert_eq!(consumer.available_ms(), 1000);
    }

    #[test]
    fn source_decode_preserves_audio_frames() {
        use std::fs::File;
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("audio")
            .join("fixture.wav");
        let file = File::open(path).expect("fixture should open");
        let metadata = StreamMetadata {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: Some(1_000),
        };

        let (consumer, handle) = spawn_decode_producer_from_source(
            Box::new(file),
            Some("wav"),
            &metadata,
            ProxyConfig::none(),
        )
        .expect("spawn should succeed");

        handle
            .join()
            .expect("thread should not panic")
            .expect("decode should succeed");

        assert_eq!(consumer.available_samples(), 88_200);
        assert_eq!(consumer.available_ms(), 1_000);
    }

    /// R6: Verify that post-seek samples are not drained by the flush
    /// that clears stale pre-seek data. The producer's signal_flush waits
    /// for the ring buffer to drain before the decode thread pushes new
    /// samples, so the consumer only drains stale data.
    #[test]
    fn seek_flush_preserves_post_seek_samples() {
        let (mut prod, mut cons) = create_stream_pair(44_100, 2);

        // Push pre-seek samples.
        let pre_seek: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        assert_eq!(prod.push_samples(&pre_seek), 1000);
        assert_eq!(cons.available_samples(), 1000);

        // Signal flush from a background thread (simulates decode thread).
        let flush_handle = std::thread::spawn(move || {
            prod.signal_flush();
            // After flush returns, the ring buffer should be empty.
            // Push post-seek samples.
            let post_seek: Vec<f32> = (2000..2500).map(|i| i as f32).collect();
            prod.push_samples(&post_seek);
            prod
        });

        // Give the flush thread time to start waiting.
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Consumer acknowledges flush (drains stale samples).
        // This should unblock the producer's signal_flush.
        cons.acknowledge_flush();
        assert!(!cons.needs_flush());

        // Wait for the producer to finish pushing post-seek samples.
        let _prod = flush_handle.join().expect("flush thread should finish");

        // The consumer should have only the post-seek samples.
        let available = cons.available_samples();
        assert_eq!(
            available, 500,
            "should have exactly 500 post-seek samples, got {available}"
        );

        let mut output = vec![0.0f32; 500];
        let popped = cons.pop_samples(&mut output);
        assert_eq!(popped, 500);
        assert_eq!(output[0], 2000.0, "first post-seek sample should be 2000");
        assert_eq!(output[499], 2499.0, "last post-seek sample should be 2499");
    }

    /// Item 12: Verify that probe_stream_metadata returns duration as Option.
    /// WAV files have container-level duration, so it should be Some.
    #[test]
    fn probe_returns_some_duration_when_container_has_metadata() {
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("audio")
            .join("fixture.wav");

        let metadata = probe_stream_metadata(&path).expect("probe should succeed");
        // WAV container exposes n_frames, so duration should be available.
        assert!(
            metadata.duration_ms.is_some(),
            "WAV files should have duration from container metadata"
        );
        let dur = metadata.duration_ms.unwrap();
        assert!(dur > 0, "duration should be positive");
    }

    /// Item 12: Verify that StreamMetadata.duration_ms is Option to allow
    /// immediate playback start when duration is unknown.
    #[test]
    fn stream_metadata_duration_is_optional() {
        let metadata = StreamMetadata {
            sample_rate: 44100,
            channels: 2,
            duration_ms: None,
        };
        // Playback should be able to start with None duration.
        assert!(metadata.duration_ms.is_none());
    }
}
