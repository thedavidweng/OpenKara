use std::{
    fs,
    path::{Path, PathBuf},
};

mod support;

use lofty::{
    config::WriteOptions,
    file::{AudioFile, TaggedFileExt},
    picture::{MimeType, Picture, PictureType},
    prelude::Accessor,
    probe::Probe,
    tag::{Tag, TagType},
};
use openkara_lib::{
    audio::encode::StreamingOggWriter,
    cache::{self, stems},
    config::StemMode,
    library::Song,
    library_root::LibraryRoot,
};
use rusqlite::Connection;

fn metadata_fixture_path(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("metadata")
        .join(filename)
}

fn unique_library_root() -> LibraryRoot {
    let path = support::unique_temp_path("phase3-cache");
    LibraryRoot::create(&path)
        .or_else(|_| LibraryRoot::open(&path))
        .expect("library root should be creatable")
}

fn cleanup_dir(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).expect("temporary cache directory should be removable");
    }
}

fn sample_song(hash: &str, extension: &str) -> Song {
    Song {
        hash: hash.to_owned(),
        file_path: Some(format!("media/{hash}.{extension}")),
        cdg_path: None,
        media_g_container: None,
        instrumental: false,
        language: None,
        audio_source_kind: "original".to_owned(),
        title: Some("Fixture Song MP3".to_owned()),
        artist: Some("Fixture Artist".to_owned()),
        album: Some("Fixture Album".to_owned()),
        duration_ms: 1,
        cover_art: None,
        has_cover_art: true,
        artwork_thumb_path: None,
        imported_at: 1,
        original_ext: Some(extension.to_owned()),
    }
}

fn copy_mp3_with_embedded_cover(destination: &Path) {
    fs::copy(metadata_fixture_path("fixture.mp3"), destination).expect("fixture audio should copy");

    let mut tagged_file = Probe::open(destination)
        .expect("fixture should open")
        .read()
        .expect("fixture tags should read");
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Fixture Song MP3".to_owned());
    tag.set_artist("Fixture Artist".to_owned());
    tag.set_album("Fixture Album".to_owned());
    tag.push_picture(
        Picture::unchecked(vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Png)
            .build(),
    );
    tagged_file.insert_tag(tag);
    tagged_file
        .save_to_path(destination, WriteOptions::default())
        .expect("fixture cover art should save");
}

fn tagged_song_in_library(library: &LibraryRoot, hash: &str) -> Song {
    let destination = library.media_path(hash, "mp3");
    copy_mp3_with_embedded_cover(&destination);
    sample_song(hash, "mp3")
}

fn read_tagged_title(path: &Path) -> String {
    let tagged_file = Probe::open(path)
        .expect("output should open")
        .read()
        .expect("output tags should read");

    tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
        .and_then(|tag| tag.title().map(|value| value.into_owned()))
        .expect("output title should exist")
}

fn assert_preserved_artist_album_and_cover(path: &Path) {
    let tagged_file = Probe::open(path)
        .expect("output should open")
        .read()
        .expect("output tags should read");
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
        .expect("output should contain a tag");

    assert_eq!(tag.artist().as_deref(), Some("Fixture Artist"));
    assert_eq!(tag.album().as_deref(), Some("Fixture Album"));
    assert_eq!(tag.pictures().len(), 1);
}

/// Write dummy stem OGG files using the streaming writer and register
/// the cache entry. This simulates the output of a streaming separation
/// run without requiring the actual Demucs model.
fn populate_stem_cache(
    connection: &Connection,
    library: &LibraryRoot,
    song: &Song,
    song_hash: &str,
    stem_mode: StemMode,
    model_variant: &str,
) -> stems::StemCacheResult {
    let source_audio_path = library.resolve(song.file_path.as_deref().unwrap());
    let stems_base = library.stems_dir();

    // Prepare the stem directory.
    let stem_directory =
        stems::prepare_stem_directory(&stems_base, song_hash).expect("stem dir should prepare");

    let sample_rate = 44_100;
    let channels = 2;
    let title_base = song.title.as_deref().unwrap_or("Fixture Song MP3");

    // Write dummy PCM data through streaming writers.
    let dummy_pcm = vec![0.5_f32; 1024 * channels];

    match stem_mode {
        StemMode::TwoStem => {
            let vocals_path = stem_directory.join("vocals.ogg");
            let accomp_path = stem_directory.join("accompaniment.ogg");

            let mut vocals_writer = StreamingOggWriter::new(
                &vocals_path,
                sample_rate,
                channels,
                Some(&source_audio_path),
                Some(&format!("{title_base} (Acapella)")),
            )
            .expect("vocals writer");
            vocals_writer
                .accept_frames(&dummy_pcm)
                .expect("vocals frames");
            vocals_writer.finish().expect("vocals finalize");

            let mut accomp_writer = StreamingOggWriter::new(
                &accomp_path,
                sample_rate,
                channels,
                Some(&source_audio_path),
                Some(&format!("{title_base} (Instrumental)")),
            )
            .expect("accompaniment writer");
            accomp_writer
                .accept_frames(&dummy_pcm)
                .expect("accompaniment frames");
            accomp_writer.finish().expect("accompaniment finalize");
        }
        StemMode::FourStem => {
            for (filename, suffix) in [
                ("vocals.ogg", "Acapella"),
                ("drums.ogg", "Drums"),
                ("bass.ogg", "Bass"),
                ("other.ogg", "Other"),
            ] {
                let path = stem_directory.join(filename);
                let mut writer = StreamingOggWriter::new(
                    &path,
                    sample_rate,
                    channels,
                    Some(&source_audio_path),
                    Some(&format!("{title_base} ({suffix})")),
                )
                .expect("stem writer");
                writer.accept_frames(&dummy_pcm).expect("stem frames");
                writer.finish().expect("stem finalize");
            }
        }
    }

    // Register the DB entry.
    stems::register_streamed_stem_cache(
        connection,
        &stems_base,
        song_hash,
        stem_mode,
        model_variant,
    )
    .expect("cache entry should register")
}

#[test]
fn prepare_stem_directory_removes_legacy_checkpoint_and_partial_output() {
    let root = support::unique_temp_path("phase3-cache-restart-from-zero");
    let stems_base = root.join("stems");
    let stem_directory = stems_base.join("legacy-checkpoint");
    fs::create_dir_all(stem_directory.join(".chunks"))
        .expect("legacy checkpoint directory should be created");
    fs::write(stem_directory.join(".chunks/manifest.json"), b"legacy")
        .expect("legacy checkpoint fixture should be written");
    fs::write(stem_directory.join("vocals.ogg.tmp"), b"partial")
        .expect("partial output fixture should be written");
    fs::write(stem_directory.join("vocals.ogg"), b"stale")
        .expect("stale final output fixture should be written");

    let prepared = stems::prepare_stem_directory(&stems_base, "legacy-checkpoint")
        .expect("stem directory should reset");

    assert!(prepared.exists());
    assert!(!prepared.join(".chunks").exists());
    assert!(!prepared.join("vocals.ogg.tmp").exists());
    assert!(!prepared.join("vocals.ogg").exists());
    cleanup_dir(&root);
}

#[test]
fn caches_stems_under_hash_directory_and_hits_cache_on_second_request() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");
    let library = unique_library_root();
    let library_root_path = library.root().to_owned();
    let song = tagged_song_in_library(&library, "song-hash");
    cache::upsert_song(&connection, &song).expect("song insert should succeed");

    let first = populate_stem_cache(
        &connection,
        &library,
        &song,
        "song-hash",
        StemMode::TwoStem,
        "htdemucs",
    );
    assert!(!first.cache_hit);
    assert!(library
        .stems_dir()
        .join("song-hash")
        .join("vocals.ogg")
        .exists());
    assert!(library
        .stems_dir()
        .join("song-hash")
        .join("accompaniment.ogg")
        .exists());

    // Second request should hit the cache.
    let second = stems::get_valid_cached_stem_entry(&connection, &library, "song-hash")
        .expect("cache lookup should succeed")
        .expect("cache entry should exist");
    assert!(second.cache_hit);

    let cached_entry = stems::get_cached_stem_entry(&connection, "song-hash")
        .expect("cache lookup should succeed")
        .expect("cache entry should exist");
    assert!(library.resolve(&cached_entry.vocals_path).exists());
    assert!(library.resolve(&cached_entry.accomp_path).exists());

    cleanup_dir(&library_root_path);
}

#[test]
fn two_stem_cache_preserves_metadata_and_overrides_titles() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");
    let library = unique_library_root();
    let library_root_path = library.root().to_owned();
    let song = tagged_song_in_library(&library, "song-two-stem");
    cache::upsert_song(&connection, &song).expect("song insert should succeed");

    let cached = populate_stem_cache(
        &connection,
        &library,
        &song,
        "song-two-stem",
        StemMode::TwoStem,
        "htdemucs",
    );

    let vocals_path = library.resolve(&cached.entry.vocals_path);
    let accompaniment_path = library.resolve(&cached.entry.accomp_path);

    assert_eq!(
        read_tagged_title(&vocals_path),
        "Fixture Song MP3 (Acapella)"
    );
    assert_eq!(
        read_tagged_title(&accompaniment_path),
        "Fixture Song MP3 (Instrumental)"
    );
    assert_preserved_artist_album_and_cover(&vocals_path);
    assert_preserved_artist_album_and_cover(&accompaniment_path);

    cleanup_dir(&library_root_path);
}

#[test]
fn four_stem_cache_writes_per_stem_titles() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");
    let library = unique_library_root();
    let library_root_path = library.root().to_owned();
    let song = tagged_song_in_library(&library, "song-four-stem");
    cache::upsert_song(&connection, &song).expect("song insert should succeed");

    let cached = populate_stem_cache(
        &connection,
        &library,
        &song,
        "song-four-stem",
        StemMode::FourStem,
        "htdemucs",
    );

    assert_eq!(
        read_tagged_title(&library.resolve(&cached.entry.vocals_path)),
        "Fixture Song MP3 (Acapella)"
    );
    assert_eq!(
        read_tagged_title(&library.resolve(cached.entry.drums_path.as_ref().unwrap())),
        "Fixture Song MP3 (Drums)"
    );
    assert_eq!(
        read_tagged_title(&library.resolve(cached.entry.bass_path.as_ref().unwrap())),
        "Fixture Song MP3 (Bass)"
    );
    assert_eq!(
        read_tagged_title(&library.resolve(cached.entry.other_path.as_ref().unwrap())),
        "Fixture Song MP3 (Other)"
    );

    cleanup_dir(&library_root_path);
}

#[test]
fn downgrade_to_two_stem_rewrites_accompaniment_metadata() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");
    let library = unique_library_root();
    let library_root_path = library.root().to_owned();
    let song = tagged_song_in_library(&library, "song-downgrade");
    cache::upsert_song(&connection, &song).expect("song insert should succeed");

    populate_stem_cache(
        &connection,
        &library,
        &song,
        "song-downgrade",
        StemMode::FourStem,
        "htdemucs",
    );

    let (updated_entry, _) = stems::downgrade_to_two_stem(&connection, &library, "song-downgrade")
        .expect("downgrade should succeed");

    assert_eq!(
        read_tagged_title(&library.resolve(&updated_entry.accomp_path)),
        "Fixture Song MP3 (Instrumental)"
    );
    assert_preserved_artist_album_and_cover(&library.resolve(&updated_entry.accomp_path));

    cleanup_dir(&library_root_path);
}

/// #207: `downgrade_to_two_stem` must report the NET disk reclaimed, i.e. the
/// deleted drums+bass+other bytes minus the newly written accompaniment.ogg —
/// not the gross sum of the deleted stems.
#[test]
fn downgrade_freed_bytes_nets_out_written_accompaniment() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");
    let library = unique_library_root();
    let library_root_path = library.root().to_owned();
    let song = tagged_song_in_library(&library, "song-downgrade-net");
    cache::upsert_song(&connection, &song).expect("song insert should succeed");

    let cached = populate_stem_cache(
        &connection,
        &library,
        &song,
        "song-downgrade-net",
        StemMode::FourStem,
        "htdemucs",
    );

    // Sizes of the individual stems on disk, captured before they are deleted.
    let file_len = |rel: &str| {
        fs::metadata(library.resolve(rel))
            .expect("stem metadata")
            .len()
    };
    let deleted_bytes = file_len(cached.entry.drums_path.as_ref().unwrap())
        + file_len(cached.entry.bass_path.as_ref().unwrap())
        + file_len(cached.entry.other_path.as_ref().unwrap());

    let (updated_entry, freed_bytes) =
        stems::downgrade_to_two_stem(&connection, &library, "song-downgrade-net")
            .expect("downgrade should succeed");

    let accompaniment_bytes = fs::metadata(library.resolve(&updated_entry.accomp_path))
        .expect("accompaniment metadata")
        .len();

    assert!(
        accompaniment_bytes > 0,
        "a new accompaniment.ogg should have been written"
    );
    assert_eq!(
        freed_bytes,
        deleted_bytes.saturating_sub(accompaniment_bytes),
        "freed bytes must net out the newly written accompaniment"
    );
    assert!(
        freed_bytes < deleted_bytes,
        "net savings must be less than the gross deleted-stem sum"
    );

    cleanup_dir(&library_root_path);
}

/// #207: `estimate_downgrade_savings` must net out an accompaniment estimate
/// (the existing vocals stem is used as a same-encoding proxy) rather than
/// returning the raw drums+bass+other sum.
#[test]
fn estimate_downgrade_savings_nets_out_accompaniment_proxy() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");
    let library = unique_library_root();
    let library_root_path = library.root().to_owned();
    let song = tagged_song_in_library(&library, "song-estimate-net");
    cache::upsert_song(&connection, &song).expect("song insert should succeed");

    let cached = populate_stem_cache(
        &connection,
        &library,
        &song,
        "song-estimate-net",
        StemMode::FourStem,
        "htdemucs",
    );

    let file_len = |rel: &str| {
        fs::metadata(library.resolve(rel))
            .expect("stem metadata")
            .len()
    };
    let deleted_bytes = file_len(cached.entry.drums_path.as_ref().unwrap())
        + file_len(cached.entry.bass_path.as_ref().unwrap())
        + file_len(cached.entry.other_path.as_ref().unwrap());
    let vocals_proxy = file_len(&cached.entry.vocals_path);

    let estimate =
        stems::estimate_downgrade_savings(&connection, &library).expect("estimate should succeed");

    assert_eq!(
        estimate,
        deleted_bytes.saturating_sub(vocals_proxy),
        "estimate must net out the vocals-sized accompaniment proxy"
    );
    assert!(
        estimate < deleted_bytes,
        "estimate must be less than the gross deleted-stem sum"
    );

    cleanup_dir(&library_root_path);
}

/// Verify that the streaming OGG writer produces a valid file that can
/// be decoded back, confirming the streaming path produces correct output.
#[test]
fn streaming_ogg_writer_produces_valid_ogg() {
    let output_dir = support::unique_temp_path("phase3-streaming-ogg");
    fs::create_dir_all(&output_dir).expect("output dir should be created");
    let output_path = output_dir.join("test.ogg");

    let sample_rate = 44_100;
    let channels = 2;
    let pcm = vec![0.5_f32; 44_100 * channels]; // 1 second of audio

    let mut writer = StreamingOggWriter::new(&output_path, sample_rate, channels, None, None)
        .expect("writer should be created");
    writer
        .accept_frames(&pcm)
        .expect("frames should be accepted");
    writer.finish().expect("writer should finalize");

    assert!(output_path.exists(), "output file should exist");

    // Decode the file back to verify it's valid.
    let decoded = openkara_lib::audio::decode::decode_file(&output_path)
        .expect("streaming OGG should decode");
    assert_eq!(decoded.sample_rate, sample_rate);
    assert_eq!(decoded.channels, channels);

    cleanup_dir(&output_dir);
}

/// Verify that dropping a writer without finishing cleans up the temp file.
#[test]
fn streaming_ogg_writer_drop_cleans_up_temp_file() {
    let output_dir = support::unique_temp_path("phase3-streaming-drop");
    fs::create_dir_all(&output_dir).expect("output dir should be created");
    let output_path = output_dir.join("dropped.ogg");
    let temp_path = output_path.with_extension("ogg.tmp");

    {
        let _writer = StreamingOggWriter::new(&output_path, 44_100, 2, None, None)
            .expect("writer should be created");
        assert!(
            temp_path.exists(),
            "temp file should exist while writer is alive"
        );
        // Writer is dropped here without finishing.
    }

    assert!(
        !temp_path.exists(),
        "temp file should be cleaned up on drop"
    );
    assert!(!output_path.exists(), "final file should not exist");

    cleanup_dir(&output_dir);
}

/// Verify that the streaming writer's atomic promotion works: the final
/// file appears only after finish, not before.
#[test]
fn streaming_ogg_writer_atomic_promotion() {
    let output_dir = support::unique_temp_path("phase3-streaming-atomic");
    fs::create_dir_all(&output_dir).expect("output dir should be created");
    let output_path = output_dir.join("atomic.ogg");

    let pcm = vec![0.3_f32; 1024 * 2];

    let mut writer = StreamingOggWriter::new(&output_path, 44_100, 2, None, None)
        .expect("writer should be created");

    writer
        .accept_frames(&pcm)
        .expect("frames should be accepted");
    assert!(
        !output_path.exists(),
        "final file should not exist before finish"
    );

    writer.finish().expect("writer should finalize");
    assert!(output_path.exists(), "final file should exist after finish");

    cleanup_dir(&output_dir);
}
