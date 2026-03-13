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

pub fn stem_cache_root(base_cache_dir: &Path) -> PathBuf {
    base_cache_dir.join(STEMS_CACHE_DIRECTORY)
}

pub fn stem_directory(base_cache_dir: &Path, song_hash: &str) -> PathBuf {
    stem_cache_root(base_cache_dir).join(song_hash)
}

pub fn get_or_create_stem_cache<F>(
    connection: &Connection,
    base_cache_dir: &Path,
    song_hash: &str,
    generate: F,
) -> Result<StemCacheResult>
where
    F: FnOnce() -> Result<SeparationResult>,
{
    ensure_song_exists(connection, song_hash)?;
    let stem_directory = stem_directory(base_cache_dir, song_hash);

    if let Some(entry) = get_cached_stem_entry(connection, song_hash)? {
        if cache_entry_files_exist(&entry) {
            return Ok(StemCacheResult {
                entry,
                cache_hit: true,
                stem_directory,
            });
        }
    }

    let separation = generate().context("failed to generate stems for cache population")?;
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

    inference::write_stems_to_directory(&separation, &stem_directory)
        .context("failed to write stem wav files into cache")?;

    let accompaniment = mix::mix_accompaniment(&separation)
        .context("failed to mix accompaniment for stem cache")?;
    let accompaniment_path = stem_directory.join(ACCOMPANIMENT_FILENAME);
    mix::write_accompaniment_wav(&accompaniment, &accompaniment_path)
        .context("failed to write accompaniment wav into cache")?;

    let entry = StemCacheEntry {
        song_hash: song_hash.to_owned(),
        vocals_path: stem_directory.join(VOCALS_FILENAME).display().to_string(),
        accomp_path: accompaniment_path.display().to_string(),
        separated_at: unix_timestamp(),
    };
    upsert_stem_cache_entry(connection, &entry).context("failed to persist stem cache entry")?;

    Ok(StemCacheResult {
        entry,
        cache_hit: false,
        stem_directory,
    })
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

fn cache_entry_files_exist(entry: &StemCacheEntry) -> bool {
    Path::new(&entry.vocals_path).exists() && Path::new(&entry.accomp_path).exists()
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
