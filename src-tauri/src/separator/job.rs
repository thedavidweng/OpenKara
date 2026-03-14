use crate::{
    audio::decode,
    cache,
    library_root::LibraryRoot,
    separator::{inference, model},
};
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

const CACHE_HIT_PROGRESS: u8 = 100;
const LOOKUP_PROGRESS: u8 = 10;
const DECODE_PROGRESS: u8 = 25;
const MODEL_LOAD_PROGRESS: u8 = 45;
const INFERENCE_PROGRESS: u8 = 70;
const CACHE_WRITE_PROGRESS: u8 = 90;
const COMPLETE_PROGRESS: u8 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeparationArtifacts {
    pub vocals_path: String,
    pub accomp_path: String,
    pub cache_hit: bool,
}

pub fn separate_song_into_cache(
    connection: &Connection,
    library_root: &LibraryRoot,
    model_path: &Path,
    song_hash: &str,
    mut report_progress: impl FnMut(u8),
) -> Result<SeparationArtifacts> {
    if let Some(cached) =
        cache::stems::get_valid_cached_stem_entry(connection, library_root, song_hash)?
    {
        report_progress(CACHE_HIT_PROGRESS);
        return Ok(artifacts_from_cache_entry(cached.entry, true));
    }

    report_progress(LOOKUP_PROGRESS);
    let song = cache::get_song_by_hash(connection, song_hash)
        .context("failed to load song before stem separation")?
        .with_context(|| format!("song with hash {song_hash} was not found in the library"))?;

    report_progress(DECODE_PROGRESS);
    let absolute_path = library_root.resolve(&song.file_path);
    let decoded_audio = decode::decode_file(&absolute_path)
        .with_context(|| format!("failed to decode audio for {}", song.file_path))?;

    report_progress(MODEL_LOAD_PROGRESS);
    let mut loaded_model = model::load_from_path(model_path)
        .with_context(|| format!("failed to load Demucs model from {}", model_path.display()))?;

    report_progress(INFERENCE_PROGRESS);
    let separation = inference::separate_audio(&mut loaded_model, &decoded_audio)
        .with_context(|| format!("failed to separate stems for song {song_hash}"))?;

    report_progress(CACHE_WRITE_PROGRESS);
    let stems_base = library_root.stems_dir();
    let cached = cache::stems::store_generated_stem_cache(
        connection,
        &stems_base,
        song_hash,
        &separation,
    )
    .with_context(|| format!("failed to cache generated stems for song {song_hash}"))?;

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
    }
}
