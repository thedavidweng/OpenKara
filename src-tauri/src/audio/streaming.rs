use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;
use std::collections::VecDeque;
use std::fs::File;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::Duration;
use symphonia::core::{
    codecs::audio::AudioDecoderOptions,
    errors::Error as SymphoniaError,
    formats::{probe::Hint, FormatOptions, TrackType},
    io::MediaSourceStream,
    meta::MetadataOptions,
    units::Timestamp,
};

use super::decode::DecodeError;

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub enabled: bool,
    pub target_sample_rate_hz: u32,
    pub target_channels: usize,
}

impl ProxyConfig {
    pub fn none() -> Self {
        Self {
            enabled: false,
            target_sample_rate_hz: 0,
            target_channels: 0,
        }
    }

    pub fn low_bitrate() -> Self {
        Self {
            enabled: true,
            target_sample_rate_hz: 22_050,
            target_channels: 1,
        }
    }
}

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

const BUFFER_SECONDS: u32 = 2;

const LOW_WATER_SAMPLES: usize = 4_410; // ~100ms at 44.1 kHz

const HIGH_WATER_SAMPLES: usize = 88_200; // ~2s at 44.1 kHz (== BUFFER_SECONDS)

pub fn ring_capacity(sample_rate: u32, channels: usize) -> usize {
    BUFFER_SECONDS as usize * sample_rate as usize * channels
}

pub struct AudioConsumer {
    cons: ringbuf::HeapCons<f32>,
    pending_samples: VecDeque<f32>,
    pub sample_rate_hz: u32,
    pub channels: usize,
    is_eof: Arc<AtomicBool>,
    needs_flush: Arc<AtomicBool>,
    seek_target: Arc<SeekTarget>,
    flush_scratch: Vec<f32>,
    flush_done: Arc<AtomicBool>,
}

impl std::fmt::Debug for AudioConsumer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioConsumer")
            .field("sample_rate", &self.sample_rate_hz)
            .field("channels", &self.channels)
            .field("available_samples", &self.cons.occupied_len())
            .field("is_eof", &self.is_eof.load(Ordering::Relaxed))
            .finish()
    }
}

impl AudioConsumer {
    pub fn available_samples(&self) -> usize {
        self.pending_samples.len() + self.cons.occupied_len()
    }

    pub fn available_ms(&self) -> u64 {
        if self.sample_rate_hz == 0 || self.channels == 0 {
            return 0;
        }
        let frames = self.available_samples() / self.channels;
        (frames as u64 * 1000) / self.sample_rate_hz as u64
    }

    pub fn available_src_frames(&self) -> usize {
        let ch = self.channels.max(1);
        self.available_samples() / ch
    }

    pub fn is_below_low_water(&self) -> bool {
        !self.is_eof() && self.available_samples() < LOW_WATER_SAMPLES
    }

    pub fn is_above_high_water(&self) -> bool {
        self.is_eof() || self.available_samples() >= HIGH_WATER_SAMPLES
    }

    pub fn is_eof(&self) -> bool {
        self.is_eof.load(Ordering::Relaxed)
    }

    /// Uses Acquire to pair with the producer's Release store in signal_flush,
    /// ensuring the consumer observes flush_done = false before acting on
    /// needs_flush = true on weakly-ordered hardware (ARM).
    pub fn needs_flush(&self) -> bool {
        self.needs_flush.load(Ordering::Acquire)
    }

    pub fn acknowledge_flush(&mut self) {
        self.needs_flush.store(false, Ordering::Relaxed);
        self.pending_samples.clear();
        let occupied = self.cons.occupied_len();
        self.flush_scratch.resize(occupied, 0.0);
        let _ = self.cons.pop_slice(&mut self.flush_scratch);
        self.flush_done.store(true, Ordering::Release);
    }

    pub fn seek_target(&self) -> &SeekTarget {
        &self.seek_target
    }

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
}

pub struct AudioProducer {
    prod: ringbuf::HeapProd<f32>,
    is_eof: Arc<AtomicBool>,
    needs_flush: Arc<AtomicBool>,
    seek_target: Arc<SeekTarget>,
    flush_epoch: Arc<AtomicU64>,
    flush_done: Arc<AtomicBool>,
}

impl AudioProducer {
    pub fn push_samples(&mut self, samples: &[f32]) -> usize {
        self.prod.push_slice(samples)
    }

    pub fn vacant_samples(&self) -> usize {
        self.prod.vacant_len()
    }

    pub fn is_above_high_water(&self) -> bool {
        self.prod.vacant_len() < (ring_capacity_from_prod(&self.prod) - HIGH_WATER_SAMPLES)
    }

    pub fn set_eof(&self) {
        self.is_eof.store(true, Ordering::Relaxed);
    }

    pub fn signal_flush(&self) {
        self.flush_epoch.fetch_add(1, Ordering::Relaxed);
        // Clear flush_done before publishing needs_flush. If needs_flush is set
        // first, a concurrent audio callback can see it, run acknowledge_flush
        // (setting flush_done = true), and return before we reset flush_done to
        // false here — the producer would then poll a flag that never flips,
        // time out, and push post-seek samples before the ring buffer drains.
        // Release ordering on needs_flush guarantees the consumer observes
        // flush_done = false before it acts on needs_flush = true.
        self.flush_done.store(false, Ordering::Release);
        self.needs_flush.store(true, Ordering::Release);

        let deadline = std::time::Instant::now() + Duration::from_millis(100);
        while std::time::Instant::now() < deadline {
            if self.flush_done.load(Ordering::Acquire) && self.prod.occupied_len() == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn seek_target(&self) -> &SeekTarget {
        &self.seek_target
    }

    pub fn seek_target_arc(&self) -> &Arc<SeekTarget> {
        &self.seek_target
    }
}

fn ring_capacity_from_prod(prod: &ringbuf::HeapProd<f32>) -> usize {
    prod.vacant_len() + prod.occupied_len()
}

#[derive(Debug)]
pub enum StreamingTrack {
    Single {
        consumer: AudioConsumer,
    },
    TwoStem {
        vocals: AudioConsumer,
        accompaniment: AudioConsumer,
    },
    FourStem {
        vocals: Box<AudioConsumer>,
        drums: Box<AudioConsumer>,
        bass: Box<AudioConsumer>,
        other: Box<AudioConsumer>,
    },
}

impl StreamingTrack {
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

    pub fn all_eof_and_drained(&self) -> bool {
        self.consumers()
            .iter()
            .all(|c| c.is_eof() && c.available_samples() == 0)
    }

    pub fn acknowledge_flush_if_needed(&mut self) {
        match self {
            StreamingTrack::Single { consumer } => {
                if consumer.needs_flush() {
                    consumer.acknowledge_flush();
                }
            }
            StreamingTrack::TwoStem {
                vocals,
                accompaniment,
            } => {
                if vocals.needs_flush() {
                    vocals.acknowledge_flush();
                }
                if accompaniment.needs_flush() {
                    accompaniment.acknowledge_flush();
                }
            }
            StreamingTrack::FourStem {
                vocals,
                drums,
                bass,
                other,
            } => {
                if vocals.needs_flush() {
                    vocals.acknowledge_flush();
                }
                if drums.needs_flush() {
                    drums.acknowledge_flush();
                }
                if bass.needs_flush() {
                    bass.acknowledge_flush();
                }
                if other.needs_flush() {
                    other.acknowledge_flush();
                }
            }
        }
    }

    pub fn any_consumer_below_low_water(&self) -> bool {
        match self {
            StreamingTrack::Single { consumer } => consumer.is_below_low_water(),
            StreamingTrack::TwoStem {
                vocals,
                accompaniment,
            } => vocals.is_below_low_water() || accompaniment.is_below_low_water(),
            StreamingTrack::FourStem {
                vocals,
                drums,
                bass,
                other,
            } => {
                vocals.is_below_low_water()
                    || drums.is_below_low_water()
                    || bass.is_below_low_water()
                    || other.is_below_low_water()
            }
        }
    }

    pub fn all_consumers_above_high_water(&self) -> bool {
        match self {
            StreamingTrack::Single { consumer } => consumer.is_above_high_water(),
            StreamingTrack::TwoStem {
                vocals,
                accompaniment,
            } => vocals.is_above_high_water() && accompaniment.is_above_high_water(),
            StreamingTrack::FourStem {
                vocals,
                drums,
                bass,
                other,
            } => {
                vocals.is_above_high_water()
                    && drums.is_above_high_water()
                    && bass.is_above_high_water()
                    && other.is_above_high_water()
            }
        }
    }
}

pub fn create_stream_pair(sample_rate: u32, channels: usize) -> (AudioProducer, AudioConsumer) {
    let capacity = ring_capacity(sample_rate, channels);
    let rb = HeapRb::<f32>::new(capacity);
    let (prod, cons) = rb.split();

    let is_eof = Arc::new(AtomicBool::new(false));
    let needs_flush = Arc::new(AtomicBool::new(false));
    let seek_target = Arc::new(SeekTarget::new());
    let flush_epoch = Arc::new(AtomicU64::new(0));
    let flush_done = Arc::new(AtomicBool::new(false));

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
            sample_rate_hz: sample_rate,
            channels,
            is_eof,
            needs_flush,
            seek_target,
            flush_scratch: Vec::with_capacity(capacity),
            flush_done,
        },
    )
}

pub fn ms_to_frames(ms: u64, sample_rate: u32) -> u64 {
    ms.saturating_mul(sample_rate as u64) / 1000
}

pub fn frames_to_ms(frames: u64, sample_rate: u32) -> u64 {
    if sample_rate == 0 {
        return 0;
    }
    frames.saturating_mul(1000) / sample_rate as u64
}

pub struct StreamMetadata {
    pub sample_rate_hz: u32,
    pub channels: usize,
    pub duration_ms: Option<u64>,
}

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
        .probe(
            &hint,
            media_source_stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| DecodeError::ProbeFailed(format!("for {label}: {e}")))?;

    let (codec_params, track_id, n_frames, time_base) = {
        let track = probed
            .default_track(TrackType::Audio)
            .ok_or(DecodeError::NoDefaultTrack)?;
        let audio_params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or(DecodeError::NoDefaultTrack)?
            .clone();
        (audio_params, track.id, track.num_frames, track.time_base)
    };

    let mut sample_rate = codec_params.sample_rate;
    let mut channels = codec_params.channels.as_ref().map(|c| c.count());

    // Some containers don't expose sample rate / channel layout in the
    // codec params.  symphonia only populates these after decoding the
    // first packet, so try that before giving up.
    if sample_rate.is_none() || channels.is_none() {
        if let Ok(mut decoder) = symphonia::default::get_codecs()
            .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
        {
            while let Ok(Some(packet)) = probed.next_packet() {
                if packet.track_id != track_id {
                    continue;
                }
                if let Ok(decoded) = decoder.decode(&packet) {
                    let spec = decoded.spec();
                    sample_rate.get_or_insert(spec.rate());
                    channels.get_or_insert(spec.channels().count());
                    break;
                }
            }
        }
    }

    let sample_rate = sample_rate.ok_or(DecodeError::MissingSampleRate)?;
    let channels = channels.ok_or(DecodeError::MissingChannels)?;

    let duration_ms = if let (Some(n_frames), Some(tb)) = (n_frames, time_base) {
        let time = tb.calc_time(Timestamp::new(n_frames as i64));
        time.map(|t| t.as_millis() as u64)
    } else {
        None
    };

    Ok(StreamMetadata {
        sample_rate_hz: sample_rate,
        channels,
        duration_ms,
    })
}

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

    let (ring_rate, ring_channels) = if proxy.enabled {
        (proxy.target_sample_rate_hz, proxy.target_channels)
    } else {
        (metadata.sample_rate_hz, metadata.channels)
    };

    let (mut prod, cons) = create_stream_pair(ring_rate, ring_channels);

    let path_buf = path.to_path_buf();
    let sample_rate = metadata.sample_rate_hz;
    let channels = metadata.channels;

    let handle = std::thread::spawn(move || {
        decode_into_producer(&path_buf, &mut prod, sample_rate, channels, &proxy)
    });

    Ok((cons, metadata, handle))
}

pub fn spawn_decode_producer_from_source(
    source: Box<dyn symphonia::core::io::MediaSource>,
    extension: Option<&str>,
    metadata: &StreamMetadata,
    proxy: ProxyConfig,
) -> Result<(AudioConsumer, JoinHandle<Result<(), DecodeError>>), DecodeError> {
    let (ring_rate, ring_channels) = if proxy.enabled {
        (proxy.target_sample_rate_hz, proxy.target_channels)
    } else {
        (metadata.sample_rate_hz, metadata.channels)
    };

    let (mut prod, cons) = create_stream_pair(ring_rate, ring_channels);

    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }
    let label = "remote-source".to_owned();
    let sr = metadata.sample_rate_hz;
    let ch = metadata.channels;

    let handle = std::thread::spawn(move || {
        let mss = MediaSourceStream::new(source, Default::default());
        decode_mss_into_producer(mss, hint, &label, &mut prod, sr, ch, &proxy)
    });

    Ok((cons, handle))
}

pub struct MultiStemResult {
    pub track: StreamingTrack,
    pub metadata: Vec<StreamMetadata>,
    pub decode_handles: Vec<JoinHandle<Result<(), DecodeError>>>,
}

pub fn spawn_multi_stem_decode_producers(
    paths: &[std::path::PathBuf],
) -> Result<MultiStemResult, DecodeError> {
    spawn_multi_stem_decode_producers_with_proxy(paths, ProxyConfig::none())
}

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

    let mut metadata_vec = Vec::with_capacity(paths.len());
    for path in paths {
        let meta = probe_stream_metadata(path)?;
        metadata_vec.push(meta);
    }

    // Validate stem timeline consistency. The source-domain mix bus
    // (issue #143) pops the same source-frame range from every stem, so
    // mismatched sample_rate or channels would cause one stem to exhaust
    // early and stall the transport. Duration is checked when available —
    // a mismatch signals different source material even though the exact
    // frame count is only known after full decode.
    if metadata_vec.len() > 1 {
        let first = &metadata_vec[0];
        for (i, meta) in metadata_vec.iter().enumerate().skip(1) {
            if meta.sample_rate_hz != first.sample_rate_hz {
                return Err(DecodeError::ProbeFailed(format!(
                    "stem timeline mismatch: stem 0 sample_rate {} != stem {i} sample_rate {}",
                    first.sample_rate_hz, meta.sample_rate_hz
                )));
            }
            if meta.channels != first.channels {
                return Err(DecodeError::ProbeFailed(format!(
                    "stem timeline mismatch: stem 0 channels {} != stem {i} channels {}",
                    first.channels, meta.channels
                )));
            }
            if let (Some(d0), Some(di)) = (first.duration_ms, meta.duration_ms) {
                if d0 != di {
                    return Err(DecodeError::ProbeFailed(format!(
                    "stem timeline mismatch: stem 0 duration_ms {d0} != stem {i} duration_ms {di}"
                )));
                }
            }
        }
    }

    let mut consumers = Vec::with_capacity(paths.len());
    let mut handles = Vec::with_capacity(paths.len());
    for (path, meta) in paths.iter().zip(metadata_vec.iter()) {
        let (ring_rate, ring_channels) = if proxy.enabled {
            (proxy.target_sample_rate_hz, proxy.target_channels)
        } else {
            (meta.sample_rate_hz, meta.channels)
        };

        let (mut prod, cons) = create_stream_pair(ring_rate, ring_channels);

        let path_buf = path.clone();
        let sr = meta.sample_rate_hz;
        let ch = meta.channels;
        let proxy_clone = proxy.clone();
        let handle = std::thread::spawn(move || {
            decode_into_producer(&path_buf, &mut prod, sr, ch, &proxy_clone)
        });

        consumers.push(cons);
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

fn decode_mss_into_producer(
    mss: MediaSourceStream,
    hint: Hint,
    label: &str,
    prod: &mut AudioProducer,
    expected_sample_rate: u32,
    expected_channels: usize,
    proxy: &ProxyConfig,
) -> Result<(), DecodeError> {
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| DecodeError::ProbeFailed(format!("for {label}: {e}")))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or(DecodeError::NoDefaultTrack)?;
    let codec_params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or(DecodeError::NoDefaultTrack)?;

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_params, &AudioDecoderOptions::default())
        .map_err(|e| DecodeError::DecoderCreationFailed(e.to_string()))?;
    let track_id = track.id;

    let seek_target = Arc::clone(prod.seek_target_arc());

    loop {
        let target = seek_target.load(Ordering::Relaxed);
        if target != SeekTarget::NONE {
            let seconds = (target as u64) / expected_sample_rate as u64;
            let frac = ((target as u64) % expected_sample_rate as u64) as f64
                / expected_sample_rate as f64;
            let total_secs = seconds as f64 + frac;
            let time =
                symphonia::core::units::Time::try_from_secs_f64(total_secs).unwrap_or_default();
            match format.seek(
                symphonia::core::formats::SeekMode::Accurate,
                symphonia::core::formats::SeekTo::Time {
                    track_id: Some(track_id),
                    time,
                },
            ) {
                Ok(_) => {
                    decoder.reset();
                    seek_target.store(SeekTarget::NONE, Ordering::Relaxed);
                    prod.signal_flush();
                }
                Err(_) => {
                    seek_target.store(SeekTarget::NONE, Ordering::Relaxed);
                }
            }
            continue;
        }

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
                    "warning: skipping malformed audio packet at offset {} in {label}",
                    packet.pts
                );
                continue;
            }
            Err(e) => return Err(DecodeError::DecodeFailed(format!("from {label}: {e}"))),
        };

        let mut samples_vec: Vec<f32> = Vec::with_capacity(decoded.samples_interleaved());
        decoded.copy_to_vec_interleaved(&mut samples_vec);
        let samples = samples_vec.as_slice();

        let resampled;
        let push_samples: &[f32] = if proxy.enabled {
            resampled = resample_interleaved(
                samples,
                expected_sample_rate,
                expected_channels,
                proxy.target_sample_rate_hz,
                proxy.target_channels,
            );
            &resampled
        } else {
            samples
        };

        let mut offset = 0;
        while offset < push_samples.len() {
            let pushed = prod.push_samples(&push_samples[offset..]);
            if pushed == 0 {
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
        assert_eq!(ring_capacity(44_100, 2), 176_400);
        assert_eq!(ring_capacity(48_000, 1), 96_000);
    }

    #[test]
    fn push_and_pop_samples_through_ring_buffer() {
        let (mut prod, mut cons) = create_stream_pair(44_100, 2);

        assert_eq!(cons.available_samples(), 0);
        assert!(cons.is_below_low_water());

        let input: Vec<f32> = (0..1024).map(|i| i as f32).collect();
        let pushed = prod.push_samples(&input);
        assert_eq!(pushed, 1024);
        assert_eq!(cons.available_samples(), 1024);

        let mut output = vec![0.0f32; 1024];
        let popped = cons.pop_samples(&mut output);
        assert_eq!(popped, 1024);
        assert_eq!(output, input);
        assert_eq!(cons.available_samples(), 0);
    }

    #[test]
    fn available_ms_reflects_buffer_fill_level() {
        let (mut prod, cons) = create_stream_pair(44_100, 2);

        let samples_500ms = 44_100usize;
        let input = vec![0.0f32; samples_500ms];
        prod.push_samples(&input);

        assert_eq!(cons.available_ms(), 500);
    }

    #[test]
    fn high_water_mark_works() {
        let (mut prod, mut cons) = create_stream_pair(44_100, 2);

        let fill_amount = HIGH_WATER_SAMPLES - 100;
        let input = vec![0.0f32; fill_amount];
        prod.push_samples(&input);

        assert!(!prod.is_above_high_water());

        let more = vec![0.0f32; 200];
        prod.push_samples(&more);
        assert!(prod.is_above_high_water());

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
        assert_eq!(frames_to_ms(100, 0), 0);
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

        assert_eq!(metadata.sample_rate_hz, 44_100);
        assert_eq!(metadata.channels, 2);
        let duration = metadata
            .duration_ms
            .expect("WAV should have duration from container");
        assert!((999..=1_001).contains(&duration));

        handle
            .join()
            .expect("decode thread should not panic")
            .expect("decode should succeed");

        assert_eq!(consumer.available_samples(), 88_200);
        assert_eq!(consumer.available_ms(), 1000);

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

        assert!(!consumer.is_eof());

        handle
            .join()
            .expect("decode thread should not panic")
            .expect("decode should succeed");

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
            metadata.sample_rate_hz, metadata.channels, metadata.duration_ms
        );
        assert!(metadata.sample_rate_hz > 0);
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

        let metadata =
            probe_stream_metadata(&path).expect("probe_stream_metadata should succeed for m4a");
        assert!(
            metadata.sample_rate_hz > 0,
            "sample_rate should be positive"
        );
        assert!(metadata.channels > 0, "channels should be positive");
        let duration = metadata.duration_ms.expect("m4a should have duration");
        assert!(
            duration > 0,
            "duration_ms should be positive, got {}",
            duration
        );

        let (mut consumer, _, handle) =
            spawn_decode_producer(&path).expect("spawn_decode_producer should succeed for m4a");

        handle
            .join()
            .expect("decode thread should not panic")
            .expect("decode should succeed for m4a");

        assert!(
            consumer.available_samples() > 0,
            "ring buffer should have samples after decoding m4a"
        );

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
            metadata.sample_rate_hz,
            metadata.channels,
            metadata.duration_ms.unwrap_or(0),
            StreamingTrack::Single { consumer },
            0,
        );

        handle
            .join()
            .expect("decode thread should not panic")
            .expect("decode should succeed");

        let device_rate = metadata.sample_rate_hz;
        let device_channels = 2;
        let buffer_frames = 512;
        let mut output = vec![0.0f32; buffer_frames * device_channels];
        let mut rc = crate::audio::output::ResamplerCache::new();
        let mut rc_in = crate::audio::output::ResamplerCache::new();
        let mut crossfade_scratch =
            vec![0.0f32; crate::audio::crossfade::CROSSFADE_SCRATCH_FRAMES * device_channels];
        let mut eq = crate::audio::eq::EqProcessor::new(device_rate, device_channels);
        let peak_ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
        let rendered = render_output_buffer(
            &mut controller,
            &mut output,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut crossfade_scratch,
            device_rate,
            device_channels,
            &mut rc,
            &mut rc_in,
            &mut eq,
            &mut peak_acc,
            &peak_ring,
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
            metadata.sample_rate_hz,
            metadata.channels,
            metadata.duration_ms.unwrap_or(0),
            StreamingTrack::Single { consumer },
            0,
        );

        handle.join().expect("thread join").expect("decode ok");

        let device_rate = metadata.sample_rate_hz;
        let device_channels = 2;
        let buffer_frames = 512;
        let mut total_rendered = 0u64;
        let mut callbacks_with_audio = 0u32;

        let mut rc = crate::audio::output::ResamplerCache::new();
        let mut rc_in = crate::audio::output::ResamplerCache::new();
        let mut crossfade_scratch =
            vec![0.0f32; crate::audio::crossfade::CROSSFADE_SCRATCH_FRAMES * device_channels];
        let mut eq = crate::audio::eq::EqProcessor::new(device_rate, device_channels);
        let peak_ring = crate::audio::peaks::PeakRing::new();
        let mut peak_acc = crate::audio::peaks::PeakAccumulator::new();
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
                &mut Vec::new(),
                &mut crossfade_scratch,
                device_rate,
                device_channels,
                &mut rc,
                &mut rc_in,
                &mut eq,
                &mut peak_acc,
                &peak_ring,
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
        assert!(total_rendered > 0, "total rendered samples should be > 0");

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

        let result = spawn_multi_stem_decode_producers(&[fixture.clone(), fixture.clone()])
            .expect("spawn_multi_stem_decode_producers should succeed");

        let StreamingTrack::TwoStem {
            vocals,
            accompaniment,
        } = result.track
        else {
            panic!("expected TwoStem variant");
        };

        assert_eq!(result.metadata.len(), 2);
        for meta in &result.metadata {
            assert_eq!(meta.sample_rate_hz, 44_100);
            assert_eq!(meta.channels, 2);
        }

        for handle in result.decode_handles {
            handle
                .join()
                .expect("decode thread should not panic")
                .expect("decode should succeed");
        }

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

        handle
            .join()
            .expect("decode thread should not panic")
            .expect("decode should succeed");

        assert_eq!(consumer.available_samples(), 88_200);

        let seek_target = consumer.seek_target();
        assert_eq!(seek_target.load(Ordering::Relaxed), SeekTarget::NONE);

        seek_target.store(
            ms_to_frames(500, metadata.sample_rate_hz) as i64,
            Ordering::Relaxed,
        );
        assert_ne!(seek_target.load(Ordering::Relaxed), SeekTarget::NONE);
    }

    #[test]
    fn resample_same_rate_passthrough() {
        let input = vec![1.0f32, 2.0, 3.0, 4.0];
        let output = resample_interleaved(&input, 44_100, 2, 44_100, 2);
        assert_eq!(output, input);
    }

    #[test]
    fn resample_stereo_to_mono() {
        let input = vec![1.0f32, 0.5, 0.8, 0.2];
        let output = resample_interleaved(&input, 44_100, 2, 44_100, 1);
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], 1.0);
        assert_eq!(output[1], 0.8);
    }

    #[test]
    fn resample_halves_sample_rate() {
        let input = vec![1.0f32, 2.0, 3.0, 4.0];
        let output = resample_interleaved(&input, 44_100, 1, 22_050, 1);
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], 1.0);
        assert_eq!(output[1], 3.0);
    }

    #[test]
    fn resample_stereo_to_mono_halved_rate() {
        let input = vec![1.0f32, 0.1, 2.0, 0.2, 3.0, 0.3, 4.0, 0.4];
        let output = resample_interleaved(&input, 44_100, 2, 22_050, 1);
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], 1.0);
        assert_eq!(output[1], 3.0);
    }

    #[test]
    fn proxy_config_low_bitrate_values() {
        let proxy = ProxyConfig::low_bitrate();
        assert!(proxy.enabled);
        assert_eq!(proxy.target_sample_rate_hz, 22_050);
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

        assert_eq!(metadata.sample_rate_hz, 44_100);
        assert_eq!(metadata.channels, 2);

        handle
            .join()
            .expect("thread should not panic")
            .expect("decode should succeed");

        assert_eq!(consumer.sample_rate_hz, 22_050);
        assert_eq!(consumer.channels, 1);

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
            sample_rate_hz: 44_100,
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

    #[test]
    fn seek_flush_preserves_post_seek_samples() {
        let (mut prod, mut cons) = create_stream_pair(44_100, 2);

        let pre_seek: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        assert_eq!(prod.push_samples(&pre_seek), 1000);
        assert_eq!(cons.available_samples(), 1000);

        let flush_handle = std::thread::spawn(move || {
            prod.signal_flush();
            let post_seek: Vec<f32> = (2000..2500).map(|i| i as f32).collect();
            prod.push_samples(&post_seek);
            prod
        });

        std::thread::sleep(std::time::Duration::from_millis(10));

        cons.acknowledge_flush();
        assert!(!cons.needs_flush());

        let _prod = flush_handle.join().expect("flush thread should finish");

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

    #[test]
    fn probe_returns_some_duration_when_container_has_metadata() {
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("audio")
            .join("fixture.wav");

        let metadata = probe_stream_metadata(&path).expect("probe should succeed");
        assert!(
            metadata.duration_ms.is_some(),
            "WAV files should have duration from container metadata"
        );
        let dur = metadata.duration_ms.unwrap();
        assert!(dur > 0, "duration should be positive");
    }

    #[test]
    fn stream_metadata_duration_is_optional() {
        let metadata = StreamMetadata {
            sample_rate_hz: 44100,
            channels: 2,
            duration_ms: None,
        };
        assert!(metadata.duration_ms.is_none());
    }
}
