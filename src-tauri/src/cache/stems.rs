use crate::library_root::LibraryRoot;
use crate::separator::{
    inference::{self, SeparationResult},
    mix,
};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const STEMS_CACHE_DIRECTORY: &str = "stems";
const ACCOMPANIMENT_FILENAME: &str = "accompaniment.wav";
const VOCALS_FILENAME: &str = "vocals.wav";

#[derive(Debug, Clone, PartialEq)]
pub struct StemCacheEntry {
    pub song_hash: String,
    pub vocals_path: String,
    pub accomp_path: String,
    pub separated_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StemCacheResult {
    pub entry: StemCacheEntry,
    pub cache_hit: bool,
    pub stem_directory: PathBuf,
}

pub fn stem_cache_root(stems_base: &Path) -> PathBuf {
    stems_base.to_path_buf()
}

pub fn stem_directory(stems_base: &Path, song_hash: &str) -> PathBuf {
    stems_base.join(song_hash)
}

pub fn get_or_create_stem_cache<F>(
    connection: &Connection,
    stems_base: &Path,
    library_root: &LibraryRoot,
    song_hash: &str,
    generate: F,
) -> Result<StemCacheResult>
where
    F: FnOnce() -> Result<SeparationResult>,
{
    ensure_song_exists(connection, song_hash)?;

    if let Some(existing) = get_valid_cached_stem_entry(connection, library_root, song_hash)? {
        return Ok(existing);
    }

    let separation = generate().context("failed to generate stems for cache population")?;
    store_generated_stem_cache(connection, stems_base, song_hash, &separation)
}

pub fn get_cached_stem_entry(
    connection: &Connection,
    song_hash: &str,
) -> rusqlite::Result<Option<StemCacheEntry>> {
    let mut statement = connection.prepare(
        "SELECT song_hash, vocals_path, accomp_path, separated_at
        FROM stems
        WHERE song_hash = ?1
        LIMIT 1",
    )?;

    let mut rows = statement.query([song_hash])?;
    match rows.next()? {
        Some(row) => Ok(Some(StemCacheEntry {
            song_hash: row.get(0)?,
            vocals_path: row.get(1)?,
            accomp_path: row.get(2)?,
            separated_at: row.get(3)?,
        })),
        None => Ok(None),
    }
}

pub fn get_valid_cached_stem_entry(
    connection: &Connection,
    library_root: &LibraryRoot,
    song_hash: &str,
) -> Result<Option<StemCacheResult>> {
    let stem_directory = stem_directory(&library_root.stems_dir(), song_hash);

    if let Some(entry) = get_cached_stem_entry(connection, song_hash)? {
        if cache_entry_files_exist(library_root, &entry) {
            return Ok(Some(StemCacheResult {
                entry,
                cache_hit: true,
                stem_directory,
            }));
        }
    }

    Ok(None)
}

pub fn store_generated_stem_cache(
    connection: &Connection,
    stems_base: &Path,
    song_hash: &str,
    separation: &SeparationResult,
) -> Result<StemCacheResult> {
    ensure_song_exists(connection, song_hash)?;
    let stem_directory = stem_directory(stems_base, song_hash);

    if stem_directory.exists() {
        fs::remove_dir_all(&stem_directory).with_context(|| {
            format!(
                "failed to clear stale stem cache directory at {}",
                stem_directory.display()
            )
        })?;
    }
    fs::create_dir_all(&stem_directory).with_context(|| {
        format!(
            "failed to create stem cache directory at {}",
            stem_directory.display()
        )
    })?;

    inference::write_stems_to_directory(separation, &stem_directory)
        .context("failed to write stem wav files into cache")?;

    let accompaniment =
        mix::mix_accompaniment(separation).context("failed to mix accompaniment for stem cache")?;
    let accompaniment_path = stem_directory.join(ACCOMPANIMENT_FILENAME);
    mix::write_accompaniment_wav(&accompaniment, &accompaniment_path)
        .context("failed to write accompaniment wav into cache")?;

    let entry = StemCacheEntry {
        song_hash: song_hash.to_owned(),
        vocals_path: format!("{STEMS_CACHE_DIRECTORY}/{song_hash}/{VOCALS_FILENAME}"),
        accomp_path: format!("{STEMS_CACHE_DIRECTORY}/{song_hash}/{ACCOMPANIMENT_FILENAME}"),
        separated_at: unix_timestamp(),
    };
    upsert_stem_cache_entry(connection, &entry).context("failed to persist stem cache entry")?;

    Ok(StemCacheResult {
        entry,
        cache_hit: false,
        stem_directory,
    })
}

fn upsert_stem_cache_entry(
    connection: &Connection,
    entry: &StemCacheEntry,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO stems (
            song_hash,
            vocals_path,
            accomp_path,
            separated_at
        ) VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(song_hash) DO UPDATE SET
            vocals_path = excluded.vocals_path,
            accomp_path = excluded.accomp_path,
            separated_at = excluded.separated_at",
        params![
            entry.song_hash,
            entry.vocals_path,
            entry.accomp_path,
            entry.separated_at,
        ],
    )?;

    Ok(())
}

fn cache_entry_files_exist(library_root: &LibraryRoot, entry: &StemCacheEntry) -> bool {
    library_root.resolve(&entry.vocals_path).exists()
        && library_root.resolve(&entry.accomp_path).exists()
}

fn ensure_song_exists(connection: &Connection, song_hash: &str) -> Result<()> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM songs WHERE hash = ?1)",
            [song_hash],
            |row| row.get(0),
        )
        .with_context(|| format!("failed to look up song {song_hash} before caching stems"))?;

    if !exists {
        anyhow::bail!("cannot cache stems for missing song {song_hash}");
    }

    Ok(())
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_secs() as i64
}
