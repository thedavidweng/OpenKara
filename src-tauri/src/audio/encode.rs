use crate::audio::decode::DecodedAudio;
use crate::metadata;
use anyhow::{Context, Result};
use std::num::{NonZeroU32, NonZeroU8};
use std::path::{Path, PathBuf};
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

/// Default Vorbis quality setting (~320 kbps for stereo at 44100 Hz).
const DEFAULT_VORBIS_QUALITY: f32 = 0.9;

/// Recommended chunk size (in frames) when feeding audio to the Vorbis encoder.
/// 1024 is the value recommended by the libvorbis documentation.
const ENCODE_CHUNK_FRAMES: usize = 1024;

/// Write audio data as an OGG/Vorbis file.
///
/// OpenKara stores generated stems as OGG on purpose: the library keeps the
/// original source media separately, so the cache format is optimized for
/// space efficiency instead of lossless archival quality.
pub fn write_ogg_file(path: &Path, audio: &DecodedAudio) -> Result<()> {
    write_ogg_file_with_quality(path, audio, DEFAULT_VORBIS_QUALITY)
}

/// Write audio data as an OGG/Vorbis file with configurable quality.
///
/// Quality ranges from -0.1 (lowest, ~45 kbps) to 1.0 (highest, ~500 kbps).
/// Recommended values: 0.4 (~128 kbps), 0.5 (~160 kbps), 0.6 (~192 kbps).
pub fn write_ogg_file_with_quality(path: &Path, audio: &DecodedAudio, quality: f32) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let channels = audio.channels;
    let sample_rate =
        NonZeroU32::new(audio.sample_rate_hz).context("sample rate must be non-zero")?;
    let channel_count =
        NonZeroU8::try_from(u8::try_from(channels).context("channel count exceeds u8 range")?)
            .context("channel count must be non-zero")?;

    let out_file = std::fs::File::create(path)
        .with_context(|| format!("failed to create OGG file at {}", path.display()))?;
    let writer = std::io::BufWriter::new(out_file);

    let mut encoder = VorbisEncoderBuilder::new(sample_rate, channel_count, writer)
        .context("failed to create Vorbis encoder builder")?
        .bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
            target_quality: quality,
        })
        .build()
        .context("failed to build Vorbis encoder")?;

    let total_frames = audio.samples.len() / channels;
    let mut offset = 0;
    let mut planar = (0..channels)
        .map(|_| Vec::with_capacity(ENCODE_CHUNK_FRAMES))
        .collect::<Vec<_>>();

    while offset < total_frames {
        let chunk_frames = ENCODE_CHUNK_FRAMES.min(total_frames - offset);

        for channel_samples in &mut planar {
            channel_samples.clear();
        }
        for frame in 0..chunk_frames {
            let frame_offset = (offset + frame) * channels;
            for (channel_index, channel_samples) in planar.iter_mut().enumerate().take(channels) {
                channel_samples.push(audio.samples[frame_offset + channel_index]);
            }
        }

        encoder
            .encode_audio_block(&planar)
            .context("failed to encode audio block")?;

        offset += chunk_frames;
    }

    encoder
        .finish()
        .context("failed to finish Vorbis encoding")?;

    Ok(())
}

/// Streaming OGG/Vorbis writer that accepts PCM frames incrementally and
/// promotes the output atomically on success.
///
/// The encoder writes to a temporary file. On `finish`, the encoder is closed,
/// metadata is copied from the source song, and the temp file is atomically
/// renamed to the final path. If the writer is dropped without finishing, the
/// temp file is deleted so a crash or cancellation never leaves a partial
/// cache entry visible to the playback path.
pub struct StreamingOggWriter {
    encoder: Option<vorbis_rs::VorbisEncoder<std::io::BufWriter<std::fs::File>>>,
    temp_path: PathBuf,
    final_path: PathBuf,
    source_path: Option<PathBuf>,
    stem_title: Option<String>,
    channels: usize,
    /// Planar staging buffer reused across `accept_frames` calls to avoid
    /// per-call allocation.
    planar_staging: Vec<Vec<f32>>,
    frames_written: usize,
    finished: bool,
}

impl StreamingOggWriter {
    /// Create a new streaming writer. The parent directory of `final_path`
    /// is created if it does not exist. The temp file is created in the same
    /// directory so the final rename is atomic on all supported filesystems.
    pub fn new(
        final_path: &Path,
        sample_rate: u32,
        channels: usize,
        source_path: Option<&Path>,
        stem_title: Option<&str>,
    ) -> Result<Self> {
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }

        let temp_path = final_path.with_extension("ogg.tmp");
        let out_file = std::fs::File::create(&temp_path).with_context(|| {
            format!("failed to create temp OGG file at {}", temp_path.display())
        })?;
        let writer = std::io::BufWriter::new(out_file);

        let nz_sample_rate =
            NonZeroU32::new(sample_rate).context("sample rate must be non-zero")?;
        let nz_channels =
            NonZeroU8::try_from(u8::try_from(channels).context("channel count exceeds u8 range")?)
                .context("channel count must be non-zero")?;

        let encoder = VorbisEncoderBuilder::new(nz_sample_rate, nz_channels, writer)
            .context("failed to create Vorbis encoder builder")?
            .bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
                target_quality: DEFAULT_VORBIS_QUALITY,
            })
            .build()
            .context("failed to build Vorbis encoder")?;

        let planar_staging = (0..channels)
            .map(|_| Vec::with_capacity(ENCODE_CHUNK_FRAMES))
            .collect();

        Ok(Self {
            encoder: Some(encoder),
            temp_path,
            final_path: final_path.to_path_buf(),
            source_path: source_path.map(|p| p.to_path_buf()),
            stem_title: stem_title.map(|s| s.to_owned()),
            channels,
            planar_staging,
            frames_written: 0,
            finished: false,
        })
    }

    /// Accept interleaved PCM frames and encode them incrementally.
    /// `samples` must contain `frames * channels` interleaved samples.
    pub fn accept_frames(&mut self, samples: &[f32]) -> Result<()> {
        let encoder = self
            .encoder
            .as_mut()
            .context("streaming OGG writer was already finished")?;
        let channels = self.channels;
        let total_frames = samples.len() / channels;
        let mut offset = 0;

        while offset < total_frames {
            let chunk_frames = ENCODE_CHUNK_FRAMES.min(total_frames - offset);

            for channel_samples in &mut self.planar_staging {
                channel_samples.clear();
            }
            for frame in 0..chunk_frames {
                let frame_offset = (offset + frame) * channels;
                for (channel_index, channel_samples) in
                    self.planar_staging.iter_mut().enumerate().take(channels)
                {
                    channel_samples.push(samples[frame_offset + channel_index]);
                }
            }

            encoder
                .encode_audio_block(&self.planar_staging)
                .context("failed to encode audio block")?;

            offset += chunk_frames;
        }

        self.frames_written += total_frames;
        Ok(())
    }

    pub fn frames_written(&self) -> usize {
        self.frames_written
    }

    /// Close the encoder, write metadata, and atomically promote the temp
    /// file to the final path. After this call the writer is consumed and
    /// no further frames can be accepted.
    pub fn finish(mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }

        let encoder = self
            .encoder
            .take()
            .context("streaming OGG writer encoder was already consumed")?;
        encoder
            .finish()
            .context("failed to finish Vorbis encoding")?;

        // Preserve source metadata (artist, album, cover art, etc.) and set
        // the stem-specific title. This mirrors the non-streaming path so
        // cache metadata and file format stay identical.
        if let (Some(source), Some(title)) =
            (self.source_path.as_deref(), self.stem_title.as_deref())
        {
            metadata::write_ogg_with_preserved_metadata(source, &self.temp_path, title)
                .context("failed to write ogg metadata on streaming output")?;
        }

        std::fs::rename(&self.temp_path, &self.final_path).with_context(|| {
            format!(
                "failed to promote temp OGG file from {} to {}",
                self.temp_path.display(),
                self.final_path.display()
            )
        })?;

        self.finished = true;
        Ok(())
    }

    /// The final destination path.
    pub fn final_path(&self) -> &Path {
        &self.final_path
    }
}

impl Drop for StreamingOggWriter {
    fn drop(&mut self) {
        if !self.finished {
            // Best-effort cleanup: remove the temp file so a crash or
            // cancellation does not leave a partial OGG visible to the
            // playback path.
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}
