use super::expand::match_cdg_source;
use crate::{
    cache,
    commands::error::current_unix_timestamp,
    hash,
    library::{artwork, Song},
    library_root::LibraryRoot,
    lyrics::fetch::read_embedded_lyrics,
    media_g::{self, MEDIA_G_PAIRED, MEDIA_G_ZIP},
    metadata,
};
use anyhow::{Context, Result};
use lofty::{file::TaggedFileExt, tag::ItemKey};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::lyrics::fetch::LyricsSource;

pub(super) struct SongBuildResult {
    pub song: Song,
    pub consumed_cdg_source: Option<PathBuf>,
}

pub(super) fn try_generate_artwork_derivatives(
    library: &LibraryRoot,
    cover_art: &Option<Vec<u8>>,
) -> (Option<String>, Option<String>) {
    let Some(bytes) = cover_art.as_deref() else {
        return (None, None);
    };
    match artwork::generate_artwork_derivatives(library, bytes) {
        Ok(derivatives) => (Some(derivatives.thumb_path), Some(derivatives.preview_path)),
        Err(e) => {
            tracing::warn!("artwork derivative generation failed: {e}");
            (None, None)
        }
    }
}

pub(super) fn try_generate_artwork_derivatives_for_song(
    song: &Song,
    library: &LibraryRoot,
) -> (Option<String>, Option<String>) {
    try_generate_artwork_derivatives(library, &song.cover_art)
}

pub(super) fn build_and_store_song(
    source: &Path,
    library: &LibraryRoot,
    selected_cdg_by_stem: &HashMap<String, Vec<PathBuf>>,
    explicit_cdg_by_audio_path: &HashMap<String, String>,
    skip_cdg_for_audio_paths: &[String],
    consumed_cdg_paths: &mut HashSet<PathBuf>,
) -> Result<SongBuildResult> {
    if let Some(cdg_source) = match_cdg_source(
        source,
        selected_cdg_by_stem,
        explicit_cdg_by_audio_path,
        skip_cdg_for_audio_paths,
    ) {
        consumed_cdg_paths.insert(cdg_source.clone());
        let song = build_and_store_media_g_pair(source, &cdg_source, library)?;
        return Ok(SongBuildResult {
            song,
            consumed_cdg_source: Some(cdg_source),
        });
    }

    let metadata = metadata::read_from_path(source)?;
    let hash = sha256_for_file(source)?;
    let imported_at = current_unix_timestamp()?;

    let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("bin");

    let dest = library.media_path(&hash, ext);
    import_media_file(source, &dest)?;

    let relative_path = format!("media/{}.{}", hash, ext);
    let title = metadata.title.or_else(|| {
        source
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    });

    Ok(SongBuildResult {
        song: Song {
            hash,
            file_path: Some(relative_path),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title,
            artist: metadata.artist,
            album: metadata.album,
            duration_ms: metadata.duration_ms,
            has_cover_art: metadata.cover_art.is_some(),
            artwork_thumb_path: None,
            cover_art: metadata.cover_art,
            imported_at,
            original_ext: Some(ext.to_owned()),
        },
        consumed_cdg_source: None,
    })
}

pub(super) fn build_and_store_media_g_pair(
    source: &Path,
    cdg_source: &Path,
    library: &LibraryRoot,
) -> Result<Song> {
    let metadata = metadata::read_from_path(source)?;
    let audio_bytes = fs::read(source)
        .with_context(|| format!("failed to read audio file at {}", source.display()))?;
    let cdg_bytes = fs::read(cdg_source)
        .with_context(|| format!("failed to read CDG file at {}", cdg_source.display()))?;
    let hash = media_g::media_g_hash(&audio_bytes, &cdg_bytes);
    let imported_at = current_unix_timestamp()?;
    let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("bin");

    let audio_dest = library.media_g_audio_path(&hash, ext);
    import_media_file(source, &audio_dest)?;
    let cdg_dest = library.media_g_cdg_path(&hash);
    import_media_file(cdg_source, &cdg_dest)?;

    let title = metadata.title.or_else(|| {
        source
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    });

    Ok(Song {
        hash: hash.clone(),
        file_path: Some(format!("media-g/{}.{}", hash, ext)),
        cdg_path: Some(format!("media-g/{}.cdg", hash)),
        media_g_container: Some(MEDIA_G_PAIRED.to_owned()),
        instrumental: false,
        language: None,
        audio_source_kind: "original".to_owned(),
        title,
        artist: metadata.artist,
        album: metadata.album,
        duration_ms: metadata.duration_ms,
        has_cover_art: metadata.cover_art.is_some(),
        artwork_thumb_path: None,
        cover_art: metadata.cover_art,
        imported_at,
        original_ext: Some(ext.to_owned()),
    })
}

pub(super) fn build_and_store_media_g_zip(source: &Path, library: &LibraryRoot) -> Result<Song> {
    let asset = media_g::inspect_zip_for_media_g(source)?;
    let metadata = metadata::read_from_bytes(&asset.audio_bytes, &asset.audio_extension)?;
    let hash = media_g::media_g_hash(&asset.audio_bytes, &asset.cdg_bytes);
    let imported_at = current_unix_timestamp()?;
    let dest = library.media_g_zip_path(&hash);
    import_media_file(source, &dest)?;

    let title = metadata.title.or(Some(asset.display_stem));

    Ok(Song {
        hash: hash.clone(),
        file_path: Some(format!("media-g/{}.zip", hash)),
        cdg_path: None,
        media_g_container: Some(MEDIA_G_ZIP.to_owned()),
        instrumental: false,
        language: None,
        audio_source_kind: "original".to_owned(),
        title,
        artist: metadata.artist,
        album: metadata.album,
        duration_ms: metadata.duration_ms,
        has_cover_art: metadata.cover_art.is_some(),
        artwork_thumb_path: None,
        cover_art: metadata.cover_art,
        imported_at,
        original_ext: Some(asset.audio_extension),
    })
}

/// Import a media asset into the content-addressed library store.
///
/// The destination is content-addressed, so a valid prior copy is a byte-for-byte
/// duplicate of `source` and therefore shares its size. When a file already exists
/// at `destination` and its size matches the source, it is trusted and left in
/// place. Otherwise — the file is missing, or is a truncated/partial leftover from
/// an interrupted copy (ENOSPC, crash, cancel) — the source is re-copied
/// atomically. Guarding only on existence (the previous behaviour) let a truncated
/// file at the canonical path be silently accepted as complete on re-import.
pub(super) fn import_media_file(source: &Path, destination: &Path) -> Result<()> {
    if existing_copy_is_intact(source, destination) {
        return Ok(());
    }
    copy_atomic(source, destination)
}

/// Returns true when `destination` already holds a complete copy of `source`,
/// judged by byte length. A content-addressed copy is byte-identical to its
/// source, so a size match is sufficient to trust it; a shorter (or otherwise
/// mismatched) file signals a partial write that must be re-copied.
fn existing_copy_is_intact(source: &Path, destination: &Path) -> bool {
    let Ok(dest_meta) = fs::metadata(destination) else {
        return false;
    };
    if !dest_meta.is_file() {
        return false;
    }
    match fs::metadata(source) {
        Ok(source_meta) => source_meta.len() == dest_meta.len(),
        Err(_) => false,
    }
}

/// Copy `source` to `destination` atomically: stream into a uniquely named temp
/// file in the destination directory (same filesystem), fsync it, then rename it
/// into place. On any failure the temp file is removed, so an interrupted copy
/// never leaves a partial file at the canonical content-addressed path. Mirrors
/// the temp+fsync+rename discipline of `StreamingOggWriter` and the model
/// download promotion.
fn copy_atomic(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create media directory {}", parent.display()))?;
    }

    let temp_path = temp_sibling_path(destination);

    // Stream the full payload into the temp file and fsync it before promotion.
    // Scope the handles so they are closed before the rename.
    let staged = (|| -> Result<()> {
        let mut reader = File::open(source)
            .with_context(|| format!("failed to open source file {}", source.display()))?;
        let mut writer = File::create(&temp_path)
            .with_context(|| format!("failed to create temp media file {}", temp_path.display()))?;
        io::copy(&mut reader, &mut writer).with_context(|| {
            format!(
                "failed to copy {} to {}",
                source.display(),
                temp_path.display()
            )
        })?;
        writer
            .sync_all()
            .with_context(|| format!("failed to fsync {}", temp_path.display()))?;
        Ok(())
    })();

    if let Err(error) = staged {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    if let Err(error) = fs::rename(&temp_path, destination) {
        let _ = fs::remove_file(&temp_path);
        return Err(error).with_context(|| {
            format!(
                "failed to promote temp media file from {} to {}",
                temp_path.display(),
                destination.display()
            )
        });
    }

    Ok(())
}

/// Build a unique temp path in the same directory as `destination` so the final
/// rename stays on one filesystem (and is therefore atomic). Uniqueness across
/// concurrent imports comes from the process id, a monotonic counter, and a
/// nanosecond timestamp.
fn temp_sibling_path(destination: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let file_name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "media".to_owned());
    destination.with_file_name(format!("{file_name}.import.{pid}.{counter}.{nanos}.tmp"))
}

pub(super) fn sha256_for_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open audio file at {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read audio file at {}", path.display()))?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hash::hex_lower(hasher.finalize()))
}

pub(super) fn try_extract_embedded_lyrics(
    connection: &Connection,
    song: &Song,
    library: &LibraryRoot,
) {
    if let Ok(Some(_)) = cache::lyrics::get_lyrics_cache_entry(connection, &song.hash) {
        return;
    }

    let Some(file_path) = song.file_path.as_deref() else {
        return;
    };

    let raw_lrc = match song.media_g_container.as_deref() {
        Some(MEDIA_G_ZIP) => {
            let archive_path = library.resolve(file_path);
            match media_g::inspect_zip_for_media_g(&archive_path).and_then(|asset| {
                read_embedded_lyrics_from_bytes(&asset.audio_bytes, &asset.audio_extension)
            }) {
                Ok(Some(lrc)) => lrc,
                _ => return,
            }
        }
        _ => {
            let resolved_path = library.resolve(file_path);
            match read_embedded_lyrics(&resolved_path) {
                Ok(Some(lrc)) => lrc,
                _ => return,
            }
        }
    };

    let fetched_at = current_unix_timestamp().unwrap_or(0);
    let entry = cache::lyrics::LyricsCacheEntry {
        song_hash: song.hash.clone(),
        lrc: raw_lrc,
        source: LyricsSource::Embedded,
        offset_ms: 0,
        fetched_at,
        word_timed_checked_at: None,
    };

    let _ = cache::lyrics::upsert_lyrics_cache_entry(connection, &entry);
}

pub(super) fn read_embedded_lyrics_from_bytes(
    bytes: &[u8],
    extension: &str,
) -> Result<Option<String>> {
    let reader = metadata::read_tagged_file_from_bytes(bytes, extension)
        .context("failed to inspect embedded lyrics in Media+G ZIP")?;

    for tag in reader.tags() {
        if let Some(lyrics) = tag.get_string(ItemKey::Lyrics) {
            let lyrics = lyrics.trim();
            if !lyrics.is_empty() {
                return Ok(Some(lyrics.to_owned()));
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Song;

    #[test]
    fn try_extract_embedded_lyrics_with_none_file_path_does_not_panic() {
        let connection =
            rusqlite::Connection::open_in_memory().expect("in-memory database should open");
        crate::cache::apply_migrations(&connection).expect("migrations should succeed");

        let song = Song {
            hash: "test-hash-r8".to_owned(),
            file_path: None,
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: Some("Test Song".to_owned()),
            artist: None,
            album: None,
            duration_ms: 0,
            cover_art: None,
            has_cover_art: false,
            artwork_thumb_path: None,
            imported_at: 0,
            original_ext: None,
        };

        let dir = std::env::temp_dir().join(format!("ingest_r8_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let library =
            crate::library_root::LibraryRoot::create(&dir).expect("should create test library");

        try_extract_embedded_lyrics(&connection, &song, &library);

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ingest_{label}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir should create");
        dir
    }

    #[test]
    fn copy_atomic_cleans_temp_and_never_writes_partial_dest_on_promotion_failure() {
        let dir = scratch_dir("copyatomic_promote_fail");
        let media = dir.join("media");
        fs::create_dir_all(&media).expect("media dir");

        let source = dir.join("source.bin");
        fs::write(&source, b"the complete source payload").expect("source write");

        let dest = media.join("deadbeef.bin");
        fs::create_dir_all(&dest).expect("dest directory stand-in");

        let result = copy_atomic(&source, &dest);
        assert!(result.is_err(), "promotion onto a directory must fail");
        assert!(dest.is_dir(), "canonical path must be left untouched");

        let remaining: Vec<_> = fs::read_dir(&media)
            .expect("read media dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(
            remaining.len(),
            1,
            "only the canonical path should remain, found: {remaining:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn copy_atomic_leaves_no_dest_and_cleans_temp_on_read_failure() {
        let dir = scratch_dir("copyatomic_read_fail");
        let media = dir.join("media");
        fs::create_dir_all(&media).expect("media dir");

        let source = dir.join("unreadable_source");
        fs::create_dir_all(&source).expect("source directory");
        let dest = media.join("deadbeef.bin");

        let result = copy_atomic(&source, &dest);
        assert!(result.is_err(), "copy from an unreadable source must fail");
        assert!(
            !dest.exists(),
            "canonical path must not exist after a failed copy"
        );

        let remaining: Vec<_> = fs::read_dir(&media)
            .expect("read media dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name())
            .collect();
        assert!(
            remaining.is_empty(),
            "temp files must be cleaned up, found: {remaining:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// #206 acceptance: a truncated file already at the content-addressed path is
    /// not trusted; `import_media_file` re-copies so the destination ends up
    /// byte-identical (hence equal size and sha256) to the source.
    #[test]
    fn import_media_file_repairs_truncated_existing_destination() {
        let dir = scratch_dir("import_repair_truncated");
        let media = dir.join("media");
        fs::create_dir_all(&media).expect("media dir");

        let source = dir.join("source.bin");
        let payload: Vec<u8> = (0..4096_u32).map(|i| (i % 251) as u8).collect();
        fs::write(&source, &payload).expect("source write");

        let dest = media.join("deadbeef.bin");
        fs::write(&dest, &payload[..128]).expect("truncated dest write");
        assert!(fs::metadata(&dest).unwrap().len() < payload.len() as u64);

        import_media_file(&source, &dest).expect("import should repair the truncated file");

        let repaired = fs::read(&dest).expect("dest read");
        assert_eq!(
            repaired.len(),
            payload.len(),
            "repaired size must equal source size"
        );
        assert_eq!(
            sha256_for_file(&dest).unwrap(),
            sha256_for_file(&source).unwrap(),
            "repaired sha256 must equal source sha256"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_media_file_trusts_intact_existing_destination() {
        let dir = scratch_dir("import_trust_intact");
        let media = dir.join("media");
        fs::create_dir_all(&media).expect("media dir");

        let source = dir.join("source.bin");
        let payload = b"identical bytes on both sides".to_vec();
        fs::write(&source, &payload).expect("source write");

        let dest = media.join("deadbeef.bin");
        fs::write(&dest, &payload).expect("dest write");
        let mtime_before = fs::metadata(&dest).unwrap().modified().ok();

        import_media_file(&source, &dest).expect("import of an intact copy should succeed");

        assert_eq!(fs::read(&dest).unwrap(), payload);
        let extra_files = fs::read_dir(&media)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path() != dest)
            .count();
        assert_eq!(extra_files, 0, "no temp files for a trusted copy");
        if let Some(before) = mtime_before {
            assert_eq!(
                fs::metadata(&dest).unwrap().modified().ok(),
                Some(before),
                "an intact destination should not be rewritten"
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
