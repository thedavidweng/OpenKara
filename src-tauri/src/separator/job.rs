use crate::{
    audio::decode,
    audio::encode::StreamingOggWriter,
    cache,
    config::{ExecutionProviderPreference, StemMode},
    library_root::LibraryRoot,
    separator::{
        inference::{self, StemWriters},
        model,
        model_cache::ModelCache,
        preprocess,
        workspace::SeparationWorkspace,
    },
};
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc, LazyLock, Mutex},
};

/// Global lock that serializes audio decoding for separation jobs.
/// Prevents N concurrent full-song PCM decodes from accumulating in memory.
static DECODE_SERIALIZE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

const CACHE_HIT_PROGRESS: u8 = 100;
const LOOKUP_PROGRESS: u8 = 2;
const DECODE_PROGRESS: u8 = 5;
const MODEL_LOAD_PROGRESS: u8 = 10;
const CACHE_WRITE_PROGRESS: u8 = 95;
const COMPLETE_PROGRESS: u8 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeparationArtifacts {
    pub vocals_path: String,
    pub accomp_path: String,
    pub cache_hit: bool,
    pub drums_path: Option<String>,
    pub bass_path: Option<String>,
    pub other_path: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn separate_song_into_cache(
    connection: &Connection,
    library_root: &LibraryRoot,
    model_cache: &Arc<Mutex<ModelCache<model::LoadedModel>>>,
    model_path: &Path,
    song_hash: &str,
    stem_mode: StemMode,
    model_variant: &str,
    ep_preference: ExecutionProviderPreference,
    cancel: &AtomicBool,
    mut report_progress: impl FnMut(u8),
) -> Result<SeparationArtifacts> {
    if let Some(cached) =
        cache::stems::get_valid_cached_stem_entry(connection, library_root, song_hash)?
    {
        let variant_matches = cached.entry.model_variant == model_variant;
        let mode_matches = match stem_mode {
            StemMode::TwoStem => true,
            StemMode::FourStem => cached.entry.has_individual_stems(),
        };
        if mode_matches && variant_matches {
            report_progress(CACHE_HIT_PROGRESS);
            return Ok(artifacts_from_cache_entry(cached.entry, true));
        }
    }

    report_progress(LOOKUP_PROGRESS);
    let song = cache::get_song_by_hash(connection, song_hash)
        .context("failed to load song before stem separation")?
        .with_context(|| format!("song with hash {song_hash} was not found in the library"))?;

    report_progress(DECODE_PROGRESS);
    let Some(song_path) = song.file_path.as_deref() else {
        return Err(anyhow::anyhow!(
            "song {} does not have a local file path",
            song_hash
        ));
    };
    let absolute_path = library_root.resolve(song_path);

    // Serialize decode (full-song PCM) across jobs; release before model load.
    let _decode_guard = DECODE_SERIALIZE_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("separation decode lock was poisoned"))?;
    let decoded_audio = decode::decode_file(&absolute_path)
        .map_err(|e| anyhow::anyhow!("failed to decode audio for {}: {}", song_path, e))?;
    drop(_decode_guard);

    report_progress(MODEL_LOAD_PROGRESS);
    // Hold the model-cache lock only for get_or_load.
    let loaded_model = {
        let mut model_cache = model_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("separator model cache lock was poisoned"))?;
        let runtime_metadata =
            model::read_model_runtime_metadata(model_path).with_context(|| {
                format!(
                    "failed to inspect model metadata for {}",
                    model_path.display()
                )
            })?;
        let cache_key = model::session_cache_key(model_path, ep_preference, &runtime_metadata);
        model_cache.get_or_load_with_key(cache_key, || {
            model::load_from_path(model_path, ep_preference).with_context(|| {
                format!("failed to load Demucs model from {}", model_path.display())
            })
        })?
    };

    // Takes ownership to avoid two full-song PCM copies.
    let normalized_audio = preprocess::normalize_audio_for_model(decoded_audio)
        .context("failed to normalize audio for model")?;

    let channels = normalized_audio.channels;
    let input_frame_count = normalized_audio.samples.len() / channels;
    let chunk_size = preprocess::target_frame_count(&loaded_model, input_frame_count)?;
    let hop_size = chunk_size / 2;

    // Interrupted runs restart from chunk 0.
    let stems_base = library_root.stems_dir();
    let stem_directory = cache::stems::prepare_stem_directory(&stems_base, song_hash)
        .context("failed to prepare stem cache directory")?;

    // Writers promote temp → final on finish so a crash never publishes partial stems.
    let source_path_for_metadata = &absolute_path;
    let vocals_title = format!(
        "{} (Acapella)",
        song.title.as_deref().unwrap_or_else(|| {
            source_path_for_metadata
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
        })
    );
    let accomp_title = format!(
        "{} (Instrumental)",
        song.title.as_deref().unwrap_or_else(|| {
            source_path_for_metadata
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
        })
    );
    let drums_title = format!(
        "{} (Drums)",
        song.title.as_deref().unwrap_or_else(|| {
            source_path_for_metadata
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
        })
    );
    let bass_title = format!(
        "{} (Bass)",
        song.title.as_deref().unwrap_or_else(|| {
            source_path_for_metadata
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
        })
    );
    let other_title = format!(
        "{} (Other)",
        song.title.as_deref().unwrap_or_else(|| {
            source_path_for_metadata
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
        })
    );

    let sample_rate = normalized_audio.sample_rate_hz;
    let vocals_path = stem_directory.join("vocals.ogg");
    let accomp_path = stem_directory.join("accompaniment.ogg");
    let drums_path = stem_directory.join("drums.ogg");
    let bass_path = stem_directory.join("bass.ogg");
    let other_path = stem_directory.join("other.ogg");

    let mut writers = match stem_mode {
        StemMode::TwoStem => StemWriters {
            mode: StemMode::TwoStem,
            vocals: StreamingOggWriter::new(
                &vocals_path,
                sample_rate,
                channels,
                Some(source_path_for_metadata.as_path()),
                Some(&vocals_title),
            )
            .context("failed to create vocals streaming writer")?,
            accompaniment: Some(
                StreamingOggWriter::new(
                    &accomp_path,
                    sample_rate,
                    channels,
                    Some(source_path_for_metadata.as_path()),
                    Some(&accomp_title),
                )
                .context("failed to create accompaniment streaming writer")?,
            ),
            drums: None,
            bass: None,
            other: None,
        },
        StemMode::FourStem => StemWriters {
            mode: StemMode::FourStem,
            vocals: StreamingOggWriter::new(
                &vocals_path,
                sample_rate,
                channels,
                Some(source_path_for_metadata.as_path()),
                Some(&vocals_title),
            )
            .context("failed to create vocals streaming writer")?,
            accompaniment: None,
            drums: Some(
                StreamingOggWriter::new(
                    &drums_path,
                    sample_rate,
                    channels,
                    Some(source_path_for_metadata.as_path()),
                    Some(&drums_title),
                )
                .context("failed to create drums streaming writer")?,
            ),
            bass: Some(
                StreamingOggWriter::new(
                    &bass_path,
                    sample_rate,
                    channels,
                    Some(source_path_for_metadata.as_path()),
                    Some(&bass_title),
                )
                .context("failed to create bass streaming writer")?,
            ),
            other: Some(
                StreamingOggWriter::new(
                    &other_path,
                    sample_rate,
                    channels,
                    Some(source_path_for_metadata.as_path()),
                    Some(&other_title),
                )
                .context("failed to create other streaming writer")?,
            ),
        },
    };

    let mut workspace =
        SeparationWorkspace::new(stem_mode, channels, chunk_size, hop_size, input_frame_count);

    let inference_progress = |completed: usize, total: usize| {
        if total > 0 {
            let fraction = completed as f64 / total as f64;
            let percent = MODEL_LOAD_PROGRESS as f64
                + fraction * (CACHE_WRITE_PROGRESS as f64 - MODEL_LOAD_PROGRESS as f64);
            report_progress(percent.round() as u8);
        }
    };

    let _outcome = inference::separate_streaming(
        &loaded_model,
        &normalized_audio,
        stem_mode,
        &mut writers,
        &mut workspace,
        cancel,
        inference_progress,
    )
    .with_context(|| format!("failed to separate stems for song {song_hash}"))?;

    report_progress(CACHE_WRITE_PROGRESS);

    writers
        .finish_all()
        .context("failed to finalize streaming OGG writers")?;

    let cached = cache::stems::register_streamed_stem_cache(
        connection,
        &stems_base,
        song_hash,
        stem_mode,
        model_variant,
    )
    .with_context(|| format!("failed to register streamed stem cache for song {song_hash}"))?;

    report_progress(COMPLETE_PROGRESS);
    Ok(artifacts_from_cache_entry(cached.entry, false))
}

fn artifacts_from_cache_entry(
    entry: cache::stems::StemCacheEntry,
    cache_hit: bool,
) -> SeparationArtifacts {
    SeparationArtifacts {
        vocals_path: entry.vocals_path,
        accomp_path: entry.accomp_path,
        cache_hit,
        drums_path: entry.drums_path,
        bass_path: entry.bass_path,
        other_path: entry.other_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn decode_serialize_lock_serializes_concurrent_tasks() {
        let (tx, rx) = mpsc::channel();
        let lock_guard = DECODE_SERIALIZE_LOCK
            .lock()
            .expect("should acquire decode lock");

        let worker_tx = tx.clone();
        let handle = std::thread::spawn(move || {
            worker_tx.send("worker_started").unwrap();
            let _guard = DECODE_SERIALIZE_LOCK
                .lock()
                .expect("worker should acquire lock");
            worker_tx.send("worker_acquired").unwrap();
        });

        assert_eq!(
            rx.recv_timeout(Duration::from_millis(100)).unwrap(),
            "worker_started"
        );

        std::thread::sleep(Duration::from_millis(50));
        assert!(
            rx.try_recv().is_err(),
            "worker should not have acquired the lock yet"
        );

        drop(lock_guard);

        assert_eq!(
            rx.recv_timeout(Duration::from_millis(100)).unwrap(),
            "worker_acquired"
        );

        handle.join().expect("worker thread should finish");
    }

    #[test]
    fn decode_serialize_lock_prevents_concurrent_execution() {
        let active_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let max_concurrent = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..4 {
            let active = Arc::clone(&active_count);
            let max = Arc::clone(&max_concurrent);
            handles.push(std::thread::spawn(move || {
                let _guard = DECODE_SERIALIZE_LOCK.lock().expect("should acquire lock");
                let current = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                max.fetch_max(current, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
                active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }));
        }

        for h in handles {
            h.join().expect("thread should finish");
        }

        assert_eq!(
            max_concurrent.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "at most 1 thread should hold the decode lock at a time"
        );
    }

    #[test]
    fn model_cache_lock_released_before_inference() {
        use crate::separator::model_cache::ModelCache;
        use std::sync::{Arc, Mutex};

        let cache: Arc<Mutex<ModelCache<i32>>> = Arc::new(Mutex::new(ModelCache::default()));

        let loaded_model = {
            let mut guard = cache.lock().unwrap();
            guard
                .get_or_load_with_key("test-model", || Ok::<_, anyhow::Error>(42))
                .unwrap()
        };

        let cache_clone = Arc::clone(&cache);
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let _guard = cache_clone.lock().unwrap();
            tx.send("lock_acquired").unwrap();
        });

        assert_eq!(
            rx.recv_timeout(Duration::from_millis(100)).unwrap(),
            "lock_acquired",
            "model cache lock should be available while inference runs"
        );
        handle.join().unwrap();

        assert_eq!(*loaded_model, 42);
    }
}
