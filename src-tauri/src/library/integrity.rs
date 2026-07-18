//! Library integrity audit and cleanup.
//!
//! Audits the active managed library for missing/empty referenced files and
//! unreferenced managed files. Presents a deterministic report. After explicit
//! confirmation, removes selected database entries only when their primary
//! media is still missing or empty at mutation time.
//!
//! This is a fast metadata audit. It does not hash/decode media, watch source
//! folders, restore files, import filesystem orphans, or delete orphaned files
//! automatically.

use crate::{cache, library::delete, library_root::LibraryRoot};
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

/// A single asset issue found during the integrity audit.
#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct ManagedAssetIssue {
    pub song_hash: String,
    pub asset_type: String,
    pub path: String,
}

/// The complete integrity audit report.
#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct IntegrityReport {
    pub checked_local_songs: usize,
    pub skipped_remote_songs: usize,
    pub missing_primary_media: Vec<ManagedAssetIssue>,
    pub empty_primary_media: Vec<ManagedAssetIssue>,
    pub missing_optional_assets: Vec<ManagedAssetIssue>,
    pub empty_optional_assets: Vec<ManagedAssetIssue>,
    pub orphaned_managed_files: Vec<String>,
}

/// The result of a cleanup operation.
#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct IntegrityCleanupResult {
    pub deleted_song_hashes: Vec<String>,
    pub skipped_song_hashes: Vec<String>,
}

/// Fixed asset type strings used in `ManagedAssetIssue`.
pub const ASSET_PRIMARY_MEDIA: &str = "primary_media";
pub const ASSET_CDG: &str = "cdg";
pub const ASSET_STEM_VOCALS: &str = "stem_vocals";
pub const ASSET_STEM_ACCOMP: &str = "stem_accomp";
pub const ASSET_STEM_DRUMS: &str = "stem_drums";
pub const ASSET_STEM_BASS: &str = "stem_bass";
pub const ASSET_STEM_OTHER: &str = "stem_other";
pub const ASSET_ARTWORK_THUMB: &str = "artwork_thumb";
pub const ASSET_ARTWORK_PREVIEW: &str = "artwork_preview";

/// Top-level managed directories that are scanned for orphaned files.
const MANAGED_TOP_LEVEL_DIRS: &[&str] = &["media", "media-g", "stems", "artwork"];

/// Validate and resolve a database-relative path safely within the library root.
///
/// Returns `Ok(absolute_path)` if the path is valid and its nearest existing
/// ancestor canonicalizes within the root. Returns `Err` for invalid paths
/// (absolute, parent traversal, wrong prefix, symlink escape, etc.).
///
/// Uses `symlink_metadata` so dangling symlinks are reported as missing and
/// never followed.
fn resolve_safe_path(library: &LibraryRoot, relative: &str) -> Result<PathBuf> {
    let normalized = normalize_relative_path(relative)?;

    // Check the top-level directory matches the expected prefix.
    // The path must start with one of the managed top-level directories.
    let top_level = normalized
        .split('/')
        .next()
        .filter(|s| !s.is_empty())
        .context("path has no top-level directory")?;

    if !MANAGED_TOP_LEVEL_DIRS.contains(&top_level) {
        anyhow::bail!("path does not start with a managed top-level directory");
    }

    let absolute = library.resolve(&normalized);

    // Canonicalize the library root.
    let canonical_root = library
        .root()
        .canonicalize()
        .context("failed to canonicalize library root")?;

    // Walk each component and reject symlinks. Canonicalize the nearest
    // existing ancestor to verify the final path stays within the root.
    let mut current = canonical_root.clone();
    let components: Vec<&str> = normalized.split('/').collect();

    for component in &components {
        current = current.join(component);

        // Use symlink_metadata to detect symlinks without following them.
        let metadata = match fs::symlink_metadata(&current) {
            Ok(m) => m,
            Err(_) => {
                // Path doesn't exist yet — that's fine for missing-file checks.
                // We still need to verify the ancestor is within the root.
                break;
            }
        };

        if metadata.file_type().is_symlink() {
            // Reject symlinks at any traversed component.
            anyhow::bail!("symlink encountered in path traversal");
        }

        // Canonicalize to verify we haven't escaped the root.
        if current.exists() {
            let canonical = current
                .canonicalize()
                .context("failed to canonicalize path component")?;
            if !canonical.starts_with(&canonical_root) {
                anyhow::bail!("path escapes library root");
            }
            current = canonical;
        }
    }

    Ok(absolute)
}

/// Normalize a relative path: reject absolute, parent, and current components.
/// Require forward-slash normalized relative paths.
fn normalize_relative_path(path: &str) -> Result<String> {
    if path.is_empty() {
        anyhow::bail!("path is empty");
    }

    // Reject backslashes — DB paths must use forward slashes.
    if path.contains('\\') {
        anyhow::bail!("path contains backslash; use forward slashes");
    }

    // Reject absolute paths.
    if path.starts_with('/') {
        anyhow::bail!("path is absolute");
    }

    // Check for Windows-style drive prefixes.
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        anyhow::bail!("path has drive prefix");
    }

    let p = Path::new(path);

    // Reject parent (..) and current (.) components.
    for component in p.components() {
        match component {
            Component::ParentDir => anyhow::bail!("path contains parent directory (..)"),
            Component::CurDir => anyhow::bail!("path contains current directory (.)"),
            Component::RootDir => anyhow::bail!("path is absolute"),
            Component::Prefix(_) => anyhow::bail!("path has prefix"),
            Component::Normal(_) => {}
        }
    }

    // Normalize: strip leading/trailing slashes, collapse multiple slashes.
    let normalized: String = path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/");

    if normalized.is_empty() {
        anyhow::bail!("path is empty after normalization");
    }

    Ok(normalized)
}

/// Check if a file is a regular file (not a directory, symlink, or special file).
fn is_regular_file(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) => metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        Err(_) => false,
    }
}

/// Check if a regular file has zero bytes.
fn is_empty_file(path: &Path) -> bool {
    match fs::metadata(path) {
        Ok(metadata) => metadata.is_file() && metadata.len() == 0,
        Err(_) => false,
    }
}

/// A row from the audit query.
struct AuditRow {
    hash: String,
    audio_source_kind: String,
    file_path: Option<String>,
    cdg_path: Option<String>,
    artwork_thumb_path: Option<String>,
    artwork_preview_path: Option<String>,
    vocals_path: Option<String>,
    accomp_path: Option<String>,
    drums_path: Option<String>,
    bass_path: Option<String>,
    other_path: Option<String>,
}

/// Run the complete integrity audit on the active library.
///
/// Opens a fresh connection, queries all songs with a LEFT JOIN on stems,
/// classifies each asset, scans managed directories for orphans, and returns
/// a deterministic, sorted, deduplicated report.
pub fn check_library_integrity(library: &LibraryRoot) -> Result<IntegrityReport> {
    let connection = cache::open_database(&library.database_path())?;
    run_audit(&connection, library)
}

/// Run the audit using an existing connection.
fn run_audit(connection: &Connection, library: &LibraryRoot) -> Result<IntegrityReport> {
    let has_artwork_thumb = column_exists(connection, "songs", "artwork_thumb_path")?;
    let has_artwork_preview = column_exists(connection, "songs", "artwork_preview_path")?;

    // Build the query dynamically based on available columns.
    let artwork_thumb_col = if has_artwork_thumb {
        "s.artwork_thumb_path"
    } else {
        "NULL"
    };
    let artwork_preview_col = if has_artwork_preview {
        "s.artwork_preview_path"
    } else {
        "NULL"
    };

    let sql = format!(
        "SELECT
            s.hash, s.audio_source_kind, s.file_path, s.cdg_path,
            {artwork_thumb_col} AS artwork_thumb_path,
            {artwork_preview_col} AS artwork_preview_path,
            st.vocals_path, st.accomp_path, st.drums_path,
            st.bass_path, st.other_path
        FROM songs s
        LEFT JOIN stems st ON st.song_hash = s.hash
        ORDER BY s.hash ASC"
    );

    let mut stmt = connection.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(AuditRow {
            hash: row.get(0)?,
            audio_source_kind: row.get(1)?,
            file_path: row.get(2)?,
            cdg_path: row.get(3)?,
            artwork_thumb_path: row.get(4)?,
            artwork_preview_path: row.get(5)?,
            vocals_path: row.get(6)?,
            accomp_path: row.get(7)?,
            drums_path: row.get(8)?,
            bass_path: row.get(9)?,
            other_path: row.get(10)?,
        })
    })?;

    let mut report = IntegrityReport {
        checked_local_songs: 0,
        skipped_remote_songs: 0,
        missing_primary_media: Vec::new(),
        empty_primary_media: Vec::new(),
        missing_optional_assets: Vec::new(),
        empty_optional_assets: Vec::new(),
        orphaned_managed_files: Vec::new(),
    };

    let mut referenced_paths: HashSet<String> = HashSet::new();

    for row in rows {
        let row = row.context("failed to read audit row")?;

        let is_remote = row.audio_source_kind != "original";

        if is_remote {
            report.skipped_remote_songs += 1;
            // Remote songs: artwork paths are still portable assets and audited.
            // But we skip primary/stem files as intentionally absent.
            // Artwork paths are still added to the referenced set.
            if let Some(ref path) = row.artwork_thumb_path {
                if is_valid_referenced_path(library, path) {
                    referenced_paths.insert(path.clone());
                }
            }
            if let Some(ref path) = row.artwork_preview_path {
                if is_valid_referenced_path(library, path) {
                    referenced_paths.insert(path.clone());
                }
            }
            continue;
        }

        report.checked_local_songs += 1;

        // Primary media (required for local originals).
        match row.file_path.as_ref() {
            Some(path) => {
                referenced_paths.insert(path.clone());
                match resolve_safe_path(library, path) {
                    Ok(absolute) => {
                        if !is_regular_file(&absolute) {
                            report.missing_primary_media.push(ManagedAssetIssue {
                                song_hash: row.hash.clone(),
                                asset_type: ASSET_PRIMARY_MEDIA.to_string(),
                                path: path.clone(),
                            });
                        } else if is_empty_file(&absolute) {
                            report.empty_primary_media.push(ManagedAssetIssue {
                                song_hash: row.hash.clone(),
                                asset_type: ASSET_PRIMARY_MEDIA.to_string(),
                                path: path.clone(),
                            });
                        }
                    }
                    Err(_) => {
                        report.missing_primary_media.push(ManagedAssetIssue {
                            song_hash: row.hash.clone(),
                            asset_type: ASSET_PRIMARY_MEDIA.to_string(),
                            path: path.clone(),
                        });
                    }
                }
            }
            None => {
                report.missing_primary_media.push(ManagedAssetIssue {
                    song_hash: row.hash.clone(),
                    asset_type: ASSET_PRIMARY_MEDIA.to_string(),
                    path: String::new(),
                });
            }
        }

        // Optional assets: CDG, stems, artwork.
        classify_optional(
            library,
            &row.hash,
            ASSET_CDG,
            row.cdg_path.as_deref(),
            &mut report,
            &mut referenced_paths,
        );
        classify_optional(
            library,
            &row.hash,
            ASSET_STEM_VOCALS,
            row.vocals_path.as_deref(),
            &mut report,
            &mut referenced_paths,
        );
        classify_optional(
            library,
            &row.hash,
            ASSET_STEM_ACCOMP,
            row.accomp_path.as_deref(),
            &mut report,
            &mut referenced_paths,
        );
        classify_optional(
            library,
            &row.hash,
            ASSET_STEM_DRUMS,
            row.drums_path.as_deref(),
            &mut report,
            &mut referenced_paths,
        );
        classify_optional(
            library,
            &row.hash,
            ASSET_STEM_BASS,
            row.bass_path.as_deref(),
            &mut report,
            &mut referenced_paths,
        );
        classify_optional(
            library,
            &row.hash,
            ASSET_STEM_OTHER,
            row.other_path.as_deref(),
            &mut report,
            &mut referenced_paths,
        );
        classify_optional(
            library,
            &row.hash,
            ASSET_ARTWORK_THUMB,
            row.artwork_thumb_path.as_deref(),
            &mut report,
            &mut referenced_paths,
        );
        classify_optional(
            library,
            &row.hash,
            ASSET_ARTWORK_PREVIEW,
            row.artwork_preview_path.as_deref(),
            &mut report,
            &mut referenced_paths,
        );
    }

    // Scan managed directories for orphaned files.
    scan_for_orphans(library, &referenced_paths, &mut report)?;

    // Sort and deduplicate all vectors.
    sort_and_dedup(&mut report);

    Ok(report)
}

/// Check if a referenced path is syntactically and top-level valid.
fn is_valid_referenced_path(library: &LibraryRoot, path: &str) -> bool {
    if let Ok(normalized) = normalize_relative_path(path) {
        let top_level = normalized.split('/').next();
        if let Some(tl) = top_level {
            if MANAGED_TOP_LEVEL_DIRS.contains(&tl) {
                return true;
            }
        }
    }
    let _ = library;
    false
}

/// Classify an optional asset as missing, empty, or valid.
fn classify_optional(
    library: &LibraryRoot,
    song_hash: &str,
    asset_type: &str,
    path: Option<&str>,
    report: &mut IntegrityReport,
    referenced: &mut HashSet<String>,
) {
    let Some(path) = path else { return };
    if path.is_empty() {
        return;
    }

    // Add to referenced set even if file is absent (so it's not called an orphan).
    referenced.insert(path.to_string());

    match resolve_safe_path(library, path) {
        Ok(absolute) => {
            if !is_regular_file(&absolute) {
                report.missing_optional_assets.push(ManagedAssetIssue {
                    song_hash: song_hash.to_string(),
                    asset_type: asset_type.to_string(),
                    path: path.to_string(),
                });
            } else if is_empty_file(&absolute) {
                report.empty_optional_assets.push(ManagedAssetIssue {
                    song_hash: song_hash.to_string(),
                    asset_type: asset_type.to_string(),
                    path: path.to_string(),
                });
            }
        }
        Err(_) => {
            report.missing_optional_assets.push(ManagedAssetIssue {
                song_hash: song_hash.to_string(),
                asset_type: asset_type.to_string(),
                path: path.to_string(),
            });
        }
    }
}

/// Recursively scan managed directories for orphaned files (not in the referenced set).
/// Never follows symlinks. Reports symlinks as orphans.
fn scan_for_orphans(
    library: &LibraryRoot,
    referenced: &HashSet<String>,
    report: &mut IntegrityReport,
) -> Result<()> {
    let canonical_root = library.root().canonicalize()?;

    for dir_name in MANAGED_TOP_LEVEL_DIRS {
        let dir = library.root().join(dir_name);
        if !dir.exists() {
            continue;
        }
        scan_directory(&dir, &canonical_root, library, referenced, report)?;
    }

    Ok(())
}

/// Recursively scan a directory for orphaned files.
fn scan_directory(
    dir: &Path,
    canonical_root: &Path,
    library: &LibraryRoot,
    referenced: &HashSet<String>,
    report: &mut IntegrityReport,
) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Use symlink_metadata to detect symlinks without following them.
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let file_type = metadata.file_type();

        if file_type.is_symlink() {
            // Symlinks are reported as orphans and never traversed.
            if let Ok(relative) = library.to_relative(&path) {
                if !referenced.contains(&relative) {
                    report.orphaned_managed_files.push(relative);
                }
            }
            continue;
        }

        if file_type.is_dir() {
            // Recurse into subdirectories (but not symlinks).
            scan_directory(&path, canonical_root, library, referenced, report)?;
            continue;
        }

        if file_type.is_file() {
            // Check if this file is in the referenced set.
            if let Ok(relative) = library.to_relative(&path) {
                // Exclude the library marker and database.
                if relative == ".openkara-library" || relative == "openkara.db" {
                    continue;
                }

                // Exclude temporary artwork files younger than 24 hours.
                if is_temp_artwork_file(&relative) {
                    if let Ok(age_secs) = file_age_seconds(&path) {
                        if age_secs < 86400 {
                            continue;
                        }
                    }
                }

                if !referenced.contains(&relative) {
                    report.orphaned_managed_files.push(relative);
                }
            }
        }
    }

    Ok(())
}

/// Check if a relative path matches the temporary artwork file convention.
/// Temp files match patterns like `artwork/.tmp-*` or `artwork/*-tmp.*`.
fn is_temp_artwork_file(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    if parts.is_empty() {
        return false;
    }
    let filename = parts.last().unwrap();
    filename.starts_with(".tmp-") || filename.contains("-tmp.") || filename.starts_with("tmp-")
}

/// Get the age of a file in seconds.
fn file_age_seconds(path: &Path) -> Result<u64> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?;
    let now = SystemTime::now();
    let duration = now.duration_since(modified).unwrap_or_default();
    Ok(duration.as_secs())
}

/// Sort and deduplicate all vectors in the report for deterministic output.
fn sort_and_dedup(report: &mut IntegrityReport) {
    report.missing_primary_media.sort_by(|a, b| {
        (&a.song_hash, &a.asset_type, &a.path).cmp(&(&b.song_hash, &b.asset_type, &b.path))
    });
    report.missing_primary_media.dedup();

    report.empty_primary_media.sort_by(|a, b| {
        (&a.song_hash, &a.asset_type, &a.path).cmp(&(&b.song_hash, &b.asset_type, &b.path))
    });
    report.empty_primary_media.dedup();

    report.missing_optional_assets.sort_by(|a, b| {
        (&a.song_hash, &a.asset_type, &a.path).cmp(&(&b.song_hash, &b.asset_type, &b.path))
    });
    report.missing_optional_assets.dedup();

    report.empty_optional_assets.sort_by(|a, b| {
        (&a.song_hash, &a.asset_type, &a.path).cmp(&(&b.song_hash, &b.asset_type, &b.path))
    });
    report.empty_optional_assets.dedup();

    report.orphaned_managed_files.sort();
    report.orphaned_managed_files.dedup();
}

/// Remove database entries for songs whose primary media is missing or empty.
///
/// Revalidates each song at mutation time. Uses a single transaction for
/// atomicity. Returns deleted and skipped hashes, sorted.
pub fn remove_missing_library_entries(
    connection: &Connection,
    library: &LibraryRoot,
    requested_hashes: Vec<String>,
) -> Result<IntegrityCleanupResult> {
    // Normalize input: remove empty strings, sort, dedup.
    let mut hashes: Vec<String> = requested_hashes
        .into_iter()
        .filter(|h| !h.is_empty())
        .collect();
    hashes.sort();
    hashes.dedup();

    if hashes.is_empty() {
        return Ok(IntegrityCleanupResult {
            deleted_song_hashes: Vec::new(),
            skipped_song_hashes: Vec::new(),
        });
    }

    let mut deleted = Vec::new();
    let mut skipped = Vec::new();

    // Start an immediate transaction.
    connection.execute_batch("BEGIN IMMEDIATE")?;

    let result: Result<()> = (|| {
        for hash in &hashes {
            // Re-read the song.
            let song = match cache::get_song_by_hash(connection, hash)? {
                Some(s) => s,
                None => {
                    // Unknown or already deleted.
                    skipped.push(hash.clone());
                    continue;
                }
            };

            // Only delete local originals.
            if song.audio_source_kind != "original" {
                skipped.push(hash.clone());
                continue;
            }

            // Revalidate: primary media must still be missing/non-regular/invalid
            // or a zero-byte regular file.
            let should_delete = match song.file_path.as_ref() {
                None => true,
                Some(path) => match resolve_safe_path(library, path) {
                    Ok(absolute) => !is_regular_file(&absolute) || is_empty_file(&absolute),
                    Err(_) => true,
                },
            };

            if !should_delete {
                // File was restored — skip.
                skipped.push(hash.clone());
                continue;
            }

            // Delete DB rows (safe inside transaction).
            delete::delete_song_rows_from_database(connection, library, hash)?;
            deleted.push(hash.clone());
        }

        Ok(())
    })();

    match result {
        Ok(()) => {
            connection.execute_batch("COMMIT")?;
        }
        Err(e) => {
            connection.execute_batch("ROLLBACK").ok();
            return Err(e);
        }
    }

    // After DB commit, clean up optional working-copy assets for deleted songs.
    for hash in &deleted {
        // Clean up stem directory.
        let _ = delete::delete_stem_files_from_working_copy(library, hash);
    }

    deleted.sort();
    skipped.sort();

    Ok(IntegrityCleanupResult {
        deleted_song_hashes: deleted,
        skipped_song_hashes: skipped,
    })
}

/// Check if a column exists in a table.
fn column_exists(connection: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let sql = format!("PRAGMA table_info({})", table);
    let mut stmt = connection.prepare(&sql)?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names.iter().any(|name| name == column))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache;
    use crate::library::Song;
    use tempfile::TempDir;

    fn create_test_library() -> (TempDir, LibraryRoot) {
        let temp = TempDir::new().unwrap();
        let library = LibraryRoot::create(temp.path()).unwrap();
        cache::initialize_library_database(&library.database_path()).unwrap();
        (temp, library)
    }

    fn add_song(
        connection: &Connection,
        hash: &str,
        file_path: Option<&str>,
        audio_source_kind: &str,
    ) {
        let song = Song {
            hash: hash.to_string(),
            file_path: file_path.map(|s| s.to_string()),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: audio_source_kind.to_string(),
            title: Some(format!("Song {}", hash)),
            artist: Some("Artist".to_string()),
            album: None,
            duration_ms: 120000,
            cover_art: None,
            has_cover_art: false,
            imported_at: 1000,
            original_ext: Some("mp3".to_string()),
        };
        cache::upsert_song(connection, &song).unwrap();
    }

    fn create_media_file(library: &LibraryRoot, relative_path: &str, content: &[u8]) {
        let absolute = library.resolve(relative_path);
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&absolute, content).unwrap();
    }

    #[test]
    fn audit_empty_library_returns_zero_counts() {
        let (_temp, library) = create_test_library();
        let report = check_library_integrity(&library).unwrap();
        assert_eq!(report.checked_local_songs, 0);
        assert_eq!(report.skipped_remote_songs, 0);
        assert!(report.missing_primary_media.is_empty());
        assert!(report.orphaned_managed_files.is_empty());
    }

    #[test]
    fn audit_detects_missing_primary_media() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        add_song(&conn, "hash1", Some("media/hash1.mp3"), "original");
        drop(conn);

        let report = check_library_integrity(&library).unwrap();
        assert_eq!(report.checked_local_songs, 1);
        assert_eq!(report.missing_primary_media.len(), 1);
        assert_eq!(report.missing_primary_media[0].song_hash, "hash1");
        assert_eq!(report.missing_primary_media[0].asset_type, "primary_media");
        assert_eq!(report.missing_primary_media[0].path, "media/hash1.mp3");
    }

    #[test]
    fn audit_detects_empty_primary_media() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        add_song(&conn, "hash1", Some("media/hash1.mp3"), "original");
        drop(conn);
        create_media_file(&library, "media/hash1.mp3", b"");

        let report = check_library_integrity(&library).unwrap();
        assert_eq!(report.checked_local_songs, 1);
        assert_eq!(report.empty_primary_media.len(), 1);
        assert!(report.missing_primary_media.is_empty());
    }

    #[test]
    fn audit_valid_primary_media_has_no_issues() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        add_song(&conn, "hash1", Some("media/hash1.mp3"), "original");
        drop(conn);
        create_media_file(&library, "media/hash1.mp3", b"audio data");

        let report = check_library_integrity(&library).unwrap();
        assert_eq!(report.checked_local_songs, 1);
        assert!(report.missing_primary_media.is_empty());
        assert!(report.empty_primary_media.is_empty());
    }

    #[test]
    fn audit_skips_remote_songs() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        add_song(&conn, "hash1", Some("media/hash1.mp3"), "remote");
        drop(conn);

        let report = check_library_integrity(&library).unwrap();
        assert_eq!(report.skipped_remote_songs, 1);
        assert_eq!(report.checked_local_songs, 0);
        assert!(report.missing_primary_media.is_empty());
    }

    #[test]
    fn audit_detects_orphaned_files() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        add_song(&conn, "hash1", Some("media/hash1.mp3"), "original");
        drop(conn);
        create_media_file(&library, "media/hash1.mp3", b"audio");
        create_media_file(&library, "media/orphan.mp3", b"orphan");

        let report = check_library_integrity(&library).unwrap();
        assert_eq!(report.orphaned_managed_files, vec!["media/orphan.mp3"]);
    }

    #[test]
    fn audit_is_deterministic() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        add_song(&conn, "hash2", Some("media/hash2.mp3"), "original");
        add_song(&conn, "hash1", Some("media/hash1.mp3"), "original");
        drop(conn);

        let report1 = check_library_integrity(&library).unwrap();
        let report2 = check_library_integrity(&library).unwrap();
        assert_eq!(report1, report2);
    }

    #[test]
    fn path_resolver_rejects_parent_traversal() {
        let (_temp, library) = create_test_library();
        let result = resolve_safe_path(&library, "media/../../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn path_resolver_rejects_absolute_path() {
        let (_temp, library) = create_test_library();
        let result = resolve_safe_path(&library, "/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn path_resolver_rejects_wrong_prefix() {
        let (_temp, library) = create_test_library();
        let result = resolve_safe_path(&library, "other/file.mp3");
        assert!(result.is_err());
    }

    #[test]
    fn cleanup_deletes_missing_primary_media() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        add_song(&conn, "hash1", Some("media/hash1.mp3"), "original");
        add_song(&conn, "hash2", Some("media/hash2.mp3"), "original");
        drop(conn);
        // hash1 file is missing, hash2 file exists
        create_media_file(&library, "media/hash2.mp3", b"audio");

        let conn = cache::open_database(&library.database_path()).unwrap();
        let result = remove_missing_library_entries(
            &conn,
            &library,
            vec!["hash1".to_string(), "hash2".to_string()],
        )
        .unwrap();
        drop(conn);

        assert_eq!(result.deleted_song_hashes, vec!["hash1"]);
        assert_eq!(result.skipped_song_hashes, vec!["hash2"]);

        // Verify hash1 is gone from DB
        let conn = cache::open_database(&library.database_path()).unwrap();
        assert!(cache::get_song_by_hash(&conn, "hash1").unwrap().is_none());
        assert!(cache::get_song_by_hash(&conn, "hash2").unwrap().is_some());
    }

    #[test]
    fn cleanup_empty_input_returns_empty() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        let result = remove_missing_library_entries(&conn, &library, vec![]).unwrap();
        assert!(result.deleted_song_hashes.is_empty());
        assert!(result.skipped_song_hashes.is_empty());
    }

    #[test]
    fn cleanup_skips_unknown_hashes() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        let result =
            remove_missing_library_entries(&conn, &library, vec!["unknown".to_string()]).unwrap();
        assert!(result.deleted_song_hashes.is_empty());
        assert_eq!(result.skipped_song_hashes, vec!["unknown"]);
    }

    #[test]
    fn cleanup_skips_remote_songs() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        add_song(&conn, "hash1", Some("media/hash1.mp3"), "remote");
        drop(conn);

        let conn = cache::open_database(&library.database_path()).unwrap();
        let result =
            remove_missing_library_entries(&conn, &library, vec!["hash1".to_string()]).unwrap();
        assert!(result.deleted_song_hashes.is_empty());
        assert_eq!(result.skipped_song_hashes, vec!["hash1"]);
    }

    #[test]
    fn cleanup_skips_restored_files() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        add_song(&conn, "hash1", Some("media/hash1.mp3"), "original");
        drop(conn);
        // File is missing during audit but restored before cleanup
        create_media_file(&library, "media/hash1.mp3", b"restored");

        let conn = cache::open_database(&library.database_path()).unwrap();
        let result =
            remove_missing_library_entries(&conn, &library, vec!["hash1".to_string()]).unwrap();
        assert!(result.deleted_song_hashes.is_empty());
        assert_eq!(result.skipped_song_hashes, vec!["hash1"]);
    }

    #[test]
    fn cleanup_dedup_requested_hashes() {
        let (_temp, library) = create_test_library();
        let conn = cache::open_database(&library.database_path()).unwrap();
        add_song(&conn, "hash1", Some("media/hash1.mp3"), "original");
        drop(conn);

        let conn = cache::open_database(&library.database_path()).unwrap();
        let result = remove_missing_library_entries(
            &conn,
            &library,
            vec!["hash1".to_string(), "hash1".to_string(), "".to_string()],
        )
        .unwrap();
        assert_eq!(result.deleted_song_hashes, vec!["hash1"]);
    }
}
