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

pub fn write_ogg_file(path: &Path, audio: &DecodedAudio) -> Result<()> {
    write_ogg_file_with_quality(path, audio, DEFAULT_VORBIS_QUALITY)
}

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

pub struct StreamingOggWriter {
    encoder: Option<vorbis_rs::VorbisEncoder<std::io::BufWriter<std::fs::File>>>,
    temp_path: PathBuf,
    final_path: PathBuf,
    source_path: Option<PathBuf>,
    stem_title: Option<String>,
    channels: usize,
    planar_staging: Vec<Vec<f32>>,
    frames_written: usize,
    finished: bool,
}

impl StreamingOggWriter {
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

    pub fn final_path(&self) -> &Path {
        &self.final_path
    }
}

impl Drop for StreamingOggWriter {
    fn drop(&mut self) {
        if !self.finished {
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}
