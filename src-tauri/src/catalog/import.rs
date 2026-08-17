use super::identity::{
    find_song_hash_by_file_hash, lookup_playlist_id, lookup_song_hash, stamp_identity,
    stamp_playlist_origin, StreamingTrackIdentity,
};
use super::streaming::StreamingSource;
use super::types::{
    CatalogError, ImportConflictPrompt, ImportRefusal, LibraryDecisionAction, LibraryDecisionMeta,
    StreamingImportFailure, StreamingImportFailureReason, StreamingImportProgress,
    StreamingImportStatus, StreamingResolveOutcome, StreamingTrack,
};
use crate::cache::{self, lyrics as lyrics_cache, waveforms};
use crate::hash;
use crate::library::import::{
    import_songs_from_paths, inspect_import_candidate, ImportCandidateDetails,
};
use crate::library::playlist;
use crate::library_root::LibraryRoot;
use rusqlite::{params, Connection};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct StreamingImportItem {
    pub remote_track_id: String,
    pub title: String,
    pub artist: String,
}

#[derive(Debug, Clone)]
pub struct StreamingPlaylistOrigin {
    pub remote_playlist_id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct StreamingImportRequest {
    pub source_id: String,
    pub items: Vec<StreamingImportItem>,
    pub playlist: Option<StreamingPlaylistOrigin>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeepOrReplace {
    Keep,
    Replace,
}

pub struct StreamingImportSession {
    source_id: String,
    remaining: VecDeque<StreamingImportItem>,
    apply_remaining: Option<KeepOrReplace>,
    failures: Vec<StreamingImportFailure>,
    imported_song_ids: Vec<String>,
    playlist: Option<StreamingPlaylistOrigin>,
    playlist_id: Option<String>,
    paused: Option<PausedConflict>,
}

struct PausedConflict {
    item: StreamingImportItem,
    incoming_path: PathBuf,
    identity: StreamingTrackIdentity,
    prompt: ImportConflictPrompt,
}

impl StreamingImportSession {
    pub fn new(request: StreamingImportRequest) -> Self {
        Self {
            source_id: request.source_id,
            remaining: request.items.into(),
            apply_remaining: None,
            failures: Vec::new(),
            imported_song_ids: Vec::new(),
            playlist: request.playlist,
            playlist_id: None,
            paused: None,
        }
    }
}

pub fn run_streaming_import<S: StreamingSource>(
    connection: &mut Connection,
    library: &LibraryRoot,
    source: &S,
    request: StreamingImportRequest,
    mut decide: impl FnMut(&ImportConflictPrompt) -> LibraryDecisionAction,
) -> Result<StreamingImportProgress, CatalogError> {
    let mut session = StreamingImportSession::new(request);
    loop {
        let progress = advance_import_session(connection, library, source, &mut session, None)?;
        if progress.status == StreamingImportStatus::Completed {
            return Ok(progress);
        }
        let prompt = progress
            .conflict
            .expect("awaiting decision includes a conflict");
        let action = decide(&prompt);
        let progress =
            advance_import_session(connection, library, source, &mut session, Some(action))?;
        if progress.status == StreamingImportStatus::Completed {
            return Ok(progress);
        }
    }
}

pub fn advance_import_session<S: StreamingSource>(
    connection: &mut Connection,
    library: &LibraryRoot,
    source: &S,
    session: &mut StreamingImportSession,
    incoming_action: Option<LibraryDecisionAction>,
) -> Result<StreamingImportProgress, CatalogError> {
    if let Some(action) = incoming_action {
        if let Some(paused) = session.paused.take() {
            match resolve_action(action, &mut session.apply_remaining) {
                Decision::Cancel => {
                    push_cancelled(&mut session.failures, &paused.item);
                    drain_cancelled(&mut session.remaining, &mut session.failures);
                    return Ok(progress_of(session, StreamingImportStatus::Completed, None));
                }
                Decision::Keep => {
                    apply_keep(connection, session, &paused.identity)?;
                    let _ = std::fs::remove_file(&paused.incoming_path);
                }
                Decision::Replace => {
                    let hash = apply_replace(
                        connection,
                        library,
                        &paused.identity,
                        &paused.incoming_path,
                    )?;
                    add_to_playlist_if_needed(connection, session, &hash)?;
                    remember_import(session, hash);
                }
            }
        }
    }

    ensure_playlist(connection, session)?;

    while let Some(item) = session.remaining.pop_front() {
        match process_item(connection, library, source, session, item)? {
            ItemOutcome::Continue => {}
            ItemOutcome::Pause(paused) => {
                let prompt = paused.prompt.clone();
                session.paused = Some(*paused);
                return Ok(progress_of(
                    session,
                    StreamingImportStatus::AwaitingDecision,
                    Some(prompt),
                ));
            }
        }
    }

    Ok(progress_of(session, StreamingImportStatus::Completed, None))
}

enum ItemOutcome {
    Continue,
    Pause(Box<PausedConflict>),
}

enum Decision {
    Keep,
    Replace,
    Cancel,
}

fn resolve_action(
    action: LibraryDecisionAction,
    apply_remaining: &mut Option<KeepOrReplace>,
) -> Decision {
    match action {
        LibraryDecisionAction::Keep => Decision::Keep,
        LibraryDecisionAction::Replace => Decision::Replace,
        LibraryDecisionAction::ApplyKeep => {
            *apply_remaining = Some(KeepOrReplace::Keep);
            Decision::Keep
        }
        LibraryDecisionAction::ApplyReplace => {
            *apply_remaining = Some(KeepOrReplace::Replace);
            Decision::Replace
        }
        LibraryDecisionAction::Cancel => Decision::Cancel,
    }
}

fn process_item<S: StreamingSource>(
    connection: &mut Connection,
    library: &LibraryRoot,
    source: &S,
    session: &mut StreamingImportSession,
    item: StreamingImportItem,
) -> Result<ItemOutcome, CatalogError> {
    match source.resolve(&item.remote_track_id)? {
        StreamingResolveOutcome::Refusal(refusal) => {
            session.failures.push(failure_from_refusal(&item, refusal));
            Ok(ItemOutcome::Continue)
        }
        StreamingResolveOutcome::File(file) => {
            let file_hash = hash::sha256_file(&file.path)
                .map_err(|error| CatalogError::Internal(error.to_string()))?;
            let identity = StreamingTrackIdentity {
                source: session.source_id.clone(),
                remote_track_id: item.remote_track_id.clone(),
            };

            if find_song_hash_by_file_hash(connection, &file_hash)?.is_some() {
                stamp_identity(connection, &identity, &file_hash)?;
                add_to_playlist_if_needed(connection, session, &file_hash)?;
                let _ = std::fs::remove_file(&file.path);
                return Ok(ItemOutcome::Continue);
            }

            if let Some(existing_hash) = lookup_song_hash(connection, &identity)? {
                if existing_hash != file_hash {
                    if let Some(applied) = session.apply_remaining {
                        match applied {
                            KeepOrReplace::Keep => {
                                apply_keep(connection, session, &identity)?;
                                let _ = std::fs::remove_file(&file.path);
                                return Ok(ItemOutcome::Continue);
                            }
                            KeepOrReplace::Replace => {
                                let hash =
                                    apply_replace(connection, library, &identity, &file.path)?;
                                add_to_playlist_if_needed(connection, session, &hash)?;
                                remember_import(session, hash);
                                return Ok(ItemOutcome::Continue);
                            }
                        }
                    }

                    let prompt = ImportConflictPrompt {
                        source_id: session.source_id.clone(),
                        remote_track_id: item.remote_track_id.clone(),
                        library: decision_meta_for_song(connection, library, &existing_hash)?,
                        incoming: decision_meta_for_path(
                            &file.path,
                            &file.title,
                            &file.artist,
                            &file.album,
                        )?,
                    };
                    return Ok(ItemOutcome::Pause(Box::new(PausedConflict {
                        item,
                        incoming_path: file.path,
                        identity,
                        prompt,
                    })));
                }
            }

            let imported = import_file(connection, library, &file.path)?;
            stamp_identity(connection, &identity, &imported)?;
            add_to_playlist_if_needed(connection, session, &imported)?;
            remember_import(session, imported);
            let _ = std::fs::remove_file(&file.path);
            Ok(ItemOutcome::Continue)
        }
    }
}

fn import_file(
    connection: &Connection,
    library: &LibraryRoot,
    path: &Path,
) -> Result<String, CatalogError> {
    let result = import_songs_from_paths(connection, library, &[path.display().to_string()]);
    if let Some(song) = result.imported.into_iter().next() {
        return Ok(song.hash);
    }
    let message = result
        .failed
        .into_iter()
        .next()
        .map(|failure| failure.error.message)
        .unwrap_or_else(|| "import produced no song".to_owned());
    Err(CatalogError::Internal(message))
}

fn apply_keep(
    connection: &mut Connection,
    session: &mut StreamingImportSession,
    identity: &StreamingTrackIdentity,
) -> Result<(), CatalogError> {
    if let Some(existing) = lookup_song_hash(connection, identity)? {
        stamp_identity(connection, identity, &existing)?;
        add_to_playlist_if_needed(connection, session, &existing)?;
    }
    Ok(())
}

fn add_to_playlist_if_needed(
    connection: &mut Connection,
    session: &mut StreamingImportSession,
    song_hash: &str,
) -> Result<(), CatalogError> {
    if let Some(playlist_id) = session.playlist_id.clone() {
        playlist::add_songs_to_playlist(connection, &playlist_id, &[song_hash.to_owned()])
            .map_err(|error| CatalogError::Internal(error.message))?;
    }
    Ok(())
}

fn apply_replace(
    connection: &mut Connection,
    library: &LibraryRoot,
    identity: &StreamingTrackIdentity,
    incoming_path: &Path,
) -> Result<String, CatalogError> {
    let old_hash = lookup_song_hash(connection, identity)?.ok_or_else(|| {
        CatalogError::Internal("import conflict is missing a library song".to_owned())
    })?;
    let new_hash = import_file(connection, library, incoming_path)?;
    if new_hash == old_hash {
        stamp_identity(connection, identity, &new_hash)?;
        return Ok(new_hash);
    }

    if let Ok(Some(entry)) = lyrics_cache::get_lyrics_cache_entry(connection, &old_hash) {
        let mut copied = entry;
        copied.song_hash = new_hash.clone();
        lyrics_cache::upsert_lyrics_cache_entry(connection, &copied)
            .map_err(|error| CatalogError::Internal(error.to_string()))?;
    }

    connection
        .execute(
            "UPDATE OR IGNORE playlist_songs SET song_hash = ?1 WHERE song_hash = ?2",
            params![new_hash, old_hash],
        )
        .map_err(|error| CatalogError::Internal(error.to_string()))?;
    connection
        .execute(
            "DELETE FROM playlist_songs WHERE song_hash = ?1",
            params![old_hash],
        )
        .map_err(|error| CatalogError::Internal(error.to_string()))?;

    let _ = cache::stems::delete_stem_cache_entry(connection, library, &old_hash);
    let _ = waveforms::delete_waveforms_for_song(connection, &old_hash);

    if let Ok(Some(old_song)) = cache::get_song_by_hash(connection, &old_hash) {
        let _ = crate::library::delete_song_files_from_working_copy(library, &old_song);
    }
    let _ = crate::library::delete_song_rows_from_database(connection, library, &old_hash);

    stamp_identity(connection, identity, &new_hash)?;
    let _ = std::fs::remove_file(incoming_path);
    Ok(new_hash)
}

fn ensure_playlist(
    connection: &mut Connection,
    session: &mut StreamingImportSession,
) -> Result<(), CatalogError> {
    let Some(origin) = session.playlist.clone() else {
        return Ok(());
    };
    if session.playlist_id.is_some() {
        return Ok(());
    }
    if let Some(existing) =
        lookup_playlist_id(connection, &session.source_id, &origin.remote_playlist_id)?
    {
        session.playlist_id = Some(existing);
        return Ok(());
    }
    let created = playlist::create_playlist(connection, origin.name.clone())
        .map_err(|error| CatalogError::Internal(error.message))?;
    stamp_playlist_origin(
        connection,
        &session.source_id,
        &origin.remote_playlist_id,
        &created.id,
    )?;
    session.playlist_id = Some(created.id);
    Ok(())
}

fn decision_meta_for_song(
    connection: &Connection,
    library: &LibraryRoot,
    song_hash: &str,
) -> Result<LibraryDecisionMeta, CatalogError> {
    let song = cache::get_song_by_hash(connection, song_hash)
        .map_err(|error| CatalogError::Internal(error.to_string()))?
        .ok_or_else(|| CatalogError::Internal("library song missing".to_owned()))?;
    let path = song
        .file_path
        .as_deref()
        .map(|relative| library.resolve(relative));
    let inspected = path
        .as_ref()
        .and_then(|path| inspect_import_candidate(&path.display().to_string()).ok());
    Ok(meta_from_parts(
        song.title,
        song.artist,
        song.album,
        inspected,
        song.duration_ms as u64,
    ))
}

fn decision_meta_for_path(
    path: &Path,
    title: &Option<String>,
    artist: &Option<String>,
    album: &Option<String>,
) -> Result<LibraryDecisionMeta, CatalogError> {
    let inspected = inspect_import_candidate(&path.display().to_string())
        .map_err(|error| CatalogError::Internal(error.to_string()))?;
    Ok(meta_from_parts(
        title.clone(),
        artist.clone(),
        album.clone(),
        Some(inspected.clone()),
        inspected.duration_ms.unwrap_or(0) as u64,
    ))
}

fn meta_from_parts(
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    inspected: Option<ImportCandidateDetails>,
    fallback_duration_ms: u64,
) -> LibraryDecisionMeta {
    LibraryDecisionMeta {
        title,
        artist,
        album,
        format: inspected
            .as_ref()
            .map(|details| details.format.clone())
            .unwrap_or_else(|| "bin".to_owned()),
        bit_rate_bps: inspected.as_ref().and_then(|details| details.bit_rate_bps),
        duration_ms: inspected
            .as_ref()
            .and_then(|details| details.duration_ms.map(|ms| ms as u64))
            .or(Some(fallback_duration_ms)),
        file_size_bytes: inspected
            .as_ref()
            .map(|details| details.file_size_bytes)
            .unwrap_or(0),
    }
}

fn failure_from_refusal(
    item: &StreamingImportItem,
    refusal: ImportRefusal,
) -> StreamingImportFailure {
    StreamingImportFailure {
        remote_track_id: item.remote_track_id.clone(),
        title: item.title.clone(),
        artist: item.artist.clone(),
        reason: StreamingImportFailureReason::Refusal,
        refusal: Some(refusal),
    }
}

fn push_cancelled(failures: &mut Vec<StreamingImportFailure>, item: &StreamingImportItem) {
    failures.push(StreamingImportFailure {
        remote_track_id: item.remote_track_id.clone(),
        title: item.title.clone(),
        artist: item.artist.clone(),
        reason: StreamingImportFailureReason::Cancelled,
        refusal: None,
    });
}

fn drain_cancelled(
    remaining: &mut VecDeque<StreamingImportItem>,
    failures: &mut Vec<StreamingImportFailure>,
) {
    while let Some(item) = remaining.pop_front() {
        push_cancelled(failures, &item);
    }
}

fn remember_import(session: &mut StreamingImportSession, hash: String) {
    if !session.imported_song_ids.contains(&hash) {
        session.imported_song_ids.push(hash);
    }
}

fn progress_of(
    session: &StreamingImportSession,
    status: StreamingImportStatus,
    conflict: Option<ImportConflictPrompt>,
) -> StreamingImportProgress {
    StreamingImportProgress {
        status,
        imported_song_ids: session.imported_song_ids.clone(),
        failed: session.failures.clone(),
        playlist_id: session.playlist_id.clone(),
        conflict,
    }
}

pub fn items_from_tracks(tracks: &[StreamingTrack]) -> Vec<StreamingImportItem> {
    tracks
        .iter()
        .map(|track| StreamingImportItem {
            remote_track_id: track.remote_track_id.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{self, lyrics as lyrics_cache, waveforms};
    use crate::catalog::streaming::{FakeResolveSpec, FakeStreamingSource, FakeTrackSpec};
    use crate::catalog::types::ImportRefusalReason;
    use crate::library::playlist;
    use crate::library_root::LibraryRoot;
    use crate::lyrics::fetch::LyricsSource;
    use rusqlite::Connection;
    use std::path::{Path, PathBuf};

    fn fixture(kind: &str, name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(kind)
            .join(name)
    }

    fn copy_into(dir: &Path, kind: &str, name: &str) -> PathBuf {
        let dest = dir.join(format!("{kind}-{name}"));
        std::fs::copy(fixture(kind, name), &dest).expect("copy fixture");
        dest
    }

    fn setup() -> (
        tempfile::TempDir,
        LibraryRoot,
        Connection,
        FakeStreamingSource,
        PathBuf,
    ) {
        let tmp = tempfile::tempdir().expect("tmp");
        let copies = tmp.path().join("copies");
        std::fs::create_dir_all(&copies).expect("copies");
        let library = LibraryRoot::create(&tmp.path().join("lib")).expect("library");
        let connection = Connection::open_in_memory().expect("db");
        cache::apply_migrations(&connection).expect("migrations");
        let source = FakeStreamingSource::new("netease");
        source
            .sign_in_password(
                crate::catalog::types::StreamingPasswordMethod::Email,
                "user@example.com",
                "pw",
                None,
            )
            .expect("sign in");
        (tmp, library, connection, source, copies)
    }

    fn write_tone(dir: &Path, name: &str, seed: i16) -> PathBuf {
        let path = dir.join(name);
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).expect("wav");
        for index in 0..1_600 {
            let sample = ((index as i16).wrapping_mul(seed)).saturating_add(seed);
            writer.write_sample(sample).expect("sample");
        }
        writer.finalize().expect("finalize");
        path
    }

    fn add_tone_track(source: &FakeStreamingSource, copies: &Path, id: &str, seed: i16) {
        source.insert_track(FakeTrackSpec {
            remote_track_id: id.to_owned(),
            title: id.to_owned(),
            artist: "Artist".to_owned(),
            album: Some("Album".to_owned()),
            duration_ms: Some(200),
            resolve: FakeResolveSpec::File {
                path: write_tone(copies, &format!("{id}-{seed}.wav"), seed),
            },
        });
    }

    fn add_file_track(
        source: &FakeStreamingSource,
        copies: &Path,
        id: &str,
        kind: &str,
        file: &str,
    ) {
        source.insert_track(FakeTrackSpec {
            remote_track_id: id.to_owned(),
            title: id.to_owned(),
            artist: "Artist".to_owned(),
            album: Some("Album".to_owned()),
            duration_ms: Some(1000),
            resolve: FakeResolveSpec::File {
                path: copy_into(copies, kind, file),
            },
        });
    }

    #[test]
    fn playlist_import_stamps_origin_and_adds_only_missing() {
        let (_tmp, library, mut connection, source, copies) = setup();
        add_file_track(&source, &copies, "t1", "metadata", "fixture.mp3");
        add_file_track(&source, &copies, "t2", "metadata", "fixture.flac");
        source.insert_playlist(
            crate::catalog::types::StreamingPlaylistSummary {
                remote_id: "pl-1".to_owned(),
                name: "Night set".to_owned(),
                track_count: 2,
            },
            vec!["t1".to_owned(), "t2".to_owned()],
        );

        let first = run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![
                    StreamingImportItem {
                        remote_track_id: "t1".to_owned(),
                        title: "t1".to_owned(),
                        artist: "Artist".to_owned(),
                    },
                    StreamingImportItem {
                        remote_track_id: "t2".to_owned(),
                        title: "t2".to_owned(),
                        artist: "Artist".to_owned(),
                    },
                ],
                playlist: Some(StreamingPlaylistOrigin {
                    remote_playlist_id: "pl-1".to_owned(),
                    name: "Night set".to_owned(),
                }),
            },
            |_| panic!("no conflict"),
        )
        .expect("first import");
        let playlist_id = first.playlist_id.expect("playlist");
        let songs = playlist::get_playlist_songs(&connection, &playlist_id).expect("songs");
        assert_eq!(songs.len(), 2);

        add_file_track(&source, &copies, "t3", "metadata", "fixture.m4a");
        let second = run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![
                    StreamingImportItem {
                        remote_track_id: "t1".to_owned(),
                        title: "t1".to_owned(),
                        artist: "Artist".to_owned(),
                    },
                    StreamingImportItem {
                        remote_track_id: "t3".to_owned(),
                        title: "t3".to_owned(),
                        artist: "Artist".to_owned(),
                    },
                ],
                playlist: Some(StreamingPlaylistOrigin {
                    remote_playlist_id: "pl-1".to_owned(),
                    name: "Night set".to_owned(),
                }),
            },
            |_| panic!("no conflict"),
        )
        .expect("second import");
        assert_eq!(second.playlist_id.as_deref(), Some(playlist_id.as_str()));
        let songs = playlist::get_playlist_songs(&connection, &playlist_id).expect("songs");
        assert_eq!(songs.len(), 3);
    }

    #[test]
    fn remote_removal_does_not_delete_local_playlist_rows() {
        let (_tmp, library, mut connection, source, copies) = setup();
        add_file_track(&source, &copies, "t1", "metadata", "fixture.mp3");
        add_file_track(&source, &copies, "t2", "metadata", "fixture.flac");
        let first = run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![
                    StreamingImportItem {
                        remote_track_id: "t1".to_owned(),
                        title: "t1".to_owned(),
                        artist: "A".to_owned(),
                    },
                    StreamingImportItem {
                        remote_track_id: "t2".to_owned(),
                        title: "t2".to_owned(),
                        artist: "A".to_owned(),
                    },
                ],
                playlist: Some(StreamingPlaylistOrigin {
                    remote_playlist_id: "pl-1".to_owned(),
                    name: "Set".to_owned(),
                }),
            },
            |_| panic!("no conflict"),
        )
        .expect("first");
        let playlist_id = first.playlist_id.expect("id");
        run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![StreamingImportItem {
                    remote_track_id: "t1".to_owned(),
                    title: "t1".to_owned(),
                    artist: "A".to_owned(),
                }],
                playlist: Some(StreamingPlaylistOrigin {
                    remote_playlist_id: "pl-1".to_owned(),
                    name: "Set".to_owned(),
                }),
            },
            |_| panic!("no conflict"),
        )
        .expect("second");
        let songs = playlist::get_playlist_songs(&connection, &playlist_id).expect("songs");
        assert_eq!(songs.len(), 2);
    }

    #[test]
    fn same_hash_is_silent_and_refusal_lands_on_failure_list() {
        let (_tmp, library, mut connection, source, copies) = setup();
        add_file_track(&source, &copies, "playable", "metadata", "fixture.mp3");
        source.insert_track(FakeTrackSpec {
            remote_track_id: "trial".to_owned(),
            title: "Trial".to_owned(),
            artist: "Artist".to_owned(),
            album: None,
            duration_ms: None,
            resolve: FakeResolveSpec::Refusal(ImportRefusalReason::TrialClip),
        });
        let first = run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![StreamingImportItem {
                    remote_track_id: "playable".to_owned(),
                    title: "Playable".to_owned(),
                    artist: "Artist".to_owned(),
                }],
                playlist: None,
            },
            |_| panic!("no conflict"),
        )
        .expect("first");
        assert_eq!(first.imported_song_ids.len(), 1);

        add_file_track(&source, &copies, "playable", "metadata", "fixture.mp3");
        let second = run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![
                    StreamingImportItem {
                        remote_track_id: "playable".to_owned(),
                        title: "Playable".to_owned(),
                        artist: "Artist".to_owned(),
                    },
                    StreamingImportItem {
                        remote_track_id: "trial".to_owned(),
                        title: "Trial".to_owned(),
                        artist: "Artist".to_owned(),
                    },
                ],
                playlist: None,
            },
            |_| panic!("same hash is silent"),
        )
        .expect("second");
        assert!(second.imported_song_ids.is_empty());
        assert_eq!(second.failed.len(), 1);
        assert_eq!(
            second.failed[0].reason,
            StreamingImportFailureReason::Refusal
        );
        assert_eq!(second.failed[0].title, "Trial");
    }

    #[test]
    fn conflict_keep_replace_apply_and_cancel() {
        let (_tmp, library, mut connection, source, copies) = setup();
        add_file_track(&source, &copies, "a", "metadata", "fixture.mp3");
        let first = run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![StreamingImportItem {
                    remote_track_id: "a".to_owned(),
                    title: "A".to_owned(),
                    artist: "Artist".to_owned(),
                }],
                playlist: None,
            },
            |_| panic!("no conflict"),
        )
        .expect("seed");
        let original_hash = first.imported_song_ids[0].clone();
        lyrics_cache::upsert_lyrics_cache_entry(
            &connection,
            &lyrics_cache::LyricsCacheEntry {
                song_hash: original_hash.clone(),
                lrc: "[00:00.00]keep".to_owned(),
                source: LyricsSource::Manual,
                offset_ms: 0,
                fetched_at: 1,
                word_timed_checked_at: None,
            },
        )
        .expect("lyrics");
        connection
            .execute(
                "INSERT INTO stems (song_hash, vocals_path, accomp_path, separated_at) VALUES (?1, 'v', 'a', 1)",
                rusqlite::params![original_hash],
            )
            .expect("stems");
        let stems_dir = crate::cache::stems::stem_directory(&library.stems_dir(), &original_hash);
        std::fs::create_dir_all(&stems_dir).expect("stem dir");
        waveforms::save_waveform(&connection, &original_hash, 24, &[0.1; 24]).expect("wave");

        add_file_track(&source, &copies, "a", "metadata", "fixture.flac");
        let kept = run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![StreamingImportItem {
                    remote_track_id: "a".to_owned(),
                    title: "A".to_owned(),
                    artist: "Artist".to_owned(),
                }],
                playlist: None,
            },
            |prompt| {
                assert!(
                    prompt.library.format.contains("MP3") || prompt.library.file_size_bytes > 0
                );
                assert!(prompt.incoming.file_size_bytes > 0);
                assert!(serde_json::to_string(prompt).unwrap().contains("title"));
                assert!(!serde_json::to_string(prompt)
                    .unwrap()
                    .contains(&original_hash));
                LibraryDecisionAction::Keep
            },
        )
        .expect("keep");
        assert!(kept.imported_song_ids.is_empty());
        let identity = crate::catalog::identity::StreamingTrackIdentity {
            source: "netease".to_owned(),
            remote_track_id: "a".to_owned(),
        };
        assert_eq!(
            crate::catalog::identity::lookup_song_hash(&connection, &identity)
                .unwrap()
                .as_deref(),
            Some(original_hash.as_str())
        );

        add_file_track(&source, &copies, "a", "metadata", "fixture.flac");
        let replaced = run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![StreamingImportItem {
                    remote_track_id: "a".to_owned(),
                    title: "A".to_owned(),
                    artist: "Artist".to_owned(),
                }],
                playlist: None,
            },
            |_| LibraryDecisionAction::Replace,
        )
        .expect("replace");
        let new_hash = replaced.imported_song_ids[0].clone();
        assert_ne!(new_hash, original_hash);
        assert!(lyrics_cache::get_lyrics_cache_entry(&connection, &new_hash)
            .unwrap()
            .is_some());
        let stems: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM stems WHERE song_hash = ?1",
                rusqlite::params![original_hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stems, 0);
        assert!(!stems_dir.exists());
        assert!(
            waveforms::get_cached_waveform(&connection, &original_hash, 24)
                .unwrap()
                .is_none()
        );

        add_file_track(&source, &copies, "b", "audio", "fixture.wav");
        run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![StreamingImportItem {
                    remote_track_id: "b".to_owned(),
                    title: "B".to_owned(),
                    artist: "Artist".to_owned(),
                }],
                playlist: None,
            },
            |_| panic!("seed b"),
        )
        .expect("seed b");
        add_file_track(&source, &copies, "b", "metadata", "fixture.m4a");
        add_file_track(&source, &copies, "c", "metadata", "fixture.mp3");
        run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![StreamingImportItem {
                    remote_track_id: "c".to_owned(),
                    title: "C".to_owned(),
                    artist: "Artist".to_owned(),
                }],
                playlist: None,
            },
            |_| panic!("seed c"),
        )
        .expect("seed c");
        add_file_track(&source, &copies, "c", "audio", "fixture.wav");
        let mut seen = 0;
        let applied = run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![
                    StreamingImportItem {
                        remote_track_id: "b".to_owned(),
                        title: "B".to_owned(),
                        artist: "Artist".to_owned(),
                    },
                    StreamingImportItem {
                        remote_track_id: "c".to_owned(),
                        title: "C".to_owned(),
                        artist: "Artist".to_owned(),
                    },
                ],
                playlist: None,
            },
            |_| {
                seen += 1;
                LibraryDecisionAction::ApplyReplace
            },
        )
        .expect("apply");
        assert_eq!(seen, 1);
        assert_eq!(applied.imported_song_ids.len(), 2);

        add_tone_track(&source, &copies, "d", 17);
        run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![StreamingImportItem {
                    remote_track_id: "d".to_owned(),
                    title: "D".to_owned(),
                    artist: "Artist".to_owned(),
                }],
                playlist: None,
            },
            |_| panic!("seed d"),
        )
        .expect("seed d");
        add_tone_track(&source, &copies, "d", 31);
        add_tone_track(&source, &copies, "e", 47);
        let cancelled = run_streaming_import(
            &mut connection,
            &library,
            &source,
            StreamingImportRequest {
                source_id: "netease".to_owned(),
                items: vec![
                    StreamingImportItem {
                        remote_track_id: "d".to_owned(),
                        title: "D".to_owned(),
                        artist: "Artist".to_owned(),
                    },
                    StreamingImportItem {
                        remote_track_id: "e".to_owned(),
                        title: "E".to_owned(),
                        artist: "Artist".to_owned(),
                    },
                ],
                playlist: None,
            },
            |_| LibraryDecisionAction::Cancel,
        )
        .expect("cancel");
        assert!(cancelled.failed.iter().any(|failure| failure.reason
            == StreamingImportFailureReason::Cancelled
            && failure.remote_track_id == "e"));
    }
}
