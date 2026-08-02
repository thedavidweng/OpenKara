use std::{
    fs,
    path::{Path, PathBuf},
};

use lofty::{
    config::WriteOptions,
    tag::{ItemKey, Tag, TagExt, TagType},
};
mod support;

use openkara_lib::{
    cache,
    library::Song,
    library_root::LibraryRoot,
    lyrics::{
        fetch::{
            fetch_lyrics_for_song_local, fetch_online_timed_lyrics, lookup_query_from_song,
            read_embedded_lyrics, LyricsFetchResult, LyricsSource, OnlineLyricsResult,
            TimedLyricsProvider,
        },
        lrcapi::LrcApiClient,
        lrclib::LrcLibClient,
    },
};

fn metadata_fixture_path(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("metadata")
        .join(filename)
}

fn unique_fixture_dir() -> PathBuf {
    support::unique_temp_path("phase4-fetch")
}

fn cleanup_dir(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).expect("temporary fixture directory should be removable");
    }
}

fn fixture_song(file_path: &Path) -> Song {
    Song {
        hash: "fixture-song".to_owned(),
        file_path: Some(file_path.display().to_string()),
        cdg_path: None,
        media_g_container: None,
        instrumental: false,
        language: None,
        audio_source_kind: "original".to_owned(),
        title: Some("Yellow".to_owned()),
        artist: Some("Coldplay".to_owned()),
        album: Some("Parachutes".to_owned()),
        duration_ms: 267_000,
        cover_art: None,
        has_cover_art: true,
        artwork_thumb_path: None,
        imported_at: 1,
        original_ext: None,
    }
}

#[test]
fn fetch_chain_prefers_sidecar_without_calling_online_sources() {
    let fixture_dir = unique_fixture_dir();
    cleanup_dir(&fixture_dir);
    fs::create_dir_all(&fixture_dir).expect("fixture directory should create");

    let audio_path = fixture_dir.join("yellow.mp3");
    fs::copy(metadata_fixture_path("fixture.mp3"), &audio_path).expect("fixture audio should copy");
    fs::write(audio_path.with_extension("lrc"), "[00:10.00] from sidecar")
        .expect("sidecar should write");

    let fetched = fetch_lyrics_for_song_local(&fixture_song(&audio_path), &audio_path)
        .expect("fetch chain should succeed")
        .expect("lyrics should be returned");

    assert_eq!(
        fetched,
        LyricsFetchResult {
            source: LyricsSource::Sidecar,
            raw_lrc: "[00:10.00] from sidecar".to_owned(),
        }
    );

    cleanup_dir(&fixture_dir);
}

#[test]
fn fetch_chain_uses_lrcapi_when_no_local_lyrics_exist() {
    let fixture_dir = unique_fixture_dir();
    cleanup_dir(&fixture_dir);
    fs::create_dir_all(&fixture_dir).expect("fixture directory should create");

    let audio_path = fixture_dir.join("yellow.mp3");
    fs::copy(metadata_fixture_path("fixture.mp3"), &audio_path).expect("fixture audio should copy");

    let mut lrclib_server = mockito::Server::new();
    let lrclib_mock = lrclib_server
        .mock("GET", "/api/get")
        .match_query(mockito::Matcher::Any)
        .with_status(404)
        .create();

    let mut lrcapi_server = mockito::Server::new();
    let lrcapi_mock = lrcapi_server
        .mock("GET", "/jsonapi")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("title".into(), "Yellow".into()),
            mockito::Matcher::UrlEncoded("artist".into(), "Coldplay".into()),
            mockito::Matcher::UrlEncoded("album".into(), "Parachutes".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
                {
                    "id": "2",
                    "title": "Yellow",
                    "artist": "Coldplay",
                    "album": "Parachutes",
                    "score": 99.0,
                    "lrc": "[00:33.64] from lrcapi",
                    "lrc_ttml": null,
                    "lyric_path": "/lyrics/yellow"
                }
            ]"#,
        )
        .create();

    let lrclib_client = LrcLibClient::new(lrclib_server.url());
    let lrcapi = LrcApiClient::new(lrcapi_server.url());
    let providers = [
        TimedLyricsProvider::LrcLib(&lrclib_client),
        TimedLyricsProvider::LrcApi(&lrcapi),
    ];

    let song = fixture_song(&audio_path);
    let query = lookup_query_from_song(&song).expect("fixture song should have a lookup query");
    let fetched = match fetch_online_timed_lyrics(&providers, &query) {
        OnlineLyricsResult::Found(fetched) => fetched,
        result => panic!("LrcApi lyrics should be returned, got {result:?}"),
    };

    assert_eq!(
        fetched,
        LyricsFetchResult {
            source: LyricsSource::LrcApi,
            raw_lrc: "[00:33.64] from lrcapi".to_owned(),
        }
    );

    lrclib_mock.assert();
    lrcapi_mock.assert();
    cleanup_dir(&fixture_dir);
}

#[test]
fn fetch_chain_returns_none_when_online_sources_miss_and_no_local_lyrics() {
    let fixture_dir = unique_fixture_dir();
    cleanup_dir(&fixture_dir);
    fs::create_dir_all(&fixture_dir).expect("fixture directory should create");

    let audio_path = fixture_dir.join("yellow.mp3");
    fs::copy(metadata_fixture_path("fixture.mp3"), &audio_path).expect("fixture audio should copy");

    let mut lrclib_server = mockito::Server::new();
    let lrclib_mock = lrclib_server
        .mock("GET", "/api/get")
        .match_query(mockito::Matcher::Any)
        .with_status(404)
        .create();

    let mut lrcapi_server = mockito::Server::new();
    let lrcapi_mock = lrcapi_server
        .mock("GET", "/jsonapi")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"message":"未找到歌词"}"#)
        .create();

    let lrclib_client = LrcLibClient::new(lrclib_server.url());
    let lrcapi = LrcApiClient::new(lrcapi_server.url());
    let providers = [
        TimedLyricsProvider::LrcLib(&lrclib_client),
        TimedLyricsProvider::LrcApi(&lrcapi),
    ];

    let song = fixture_song(&audio_path);
    let query = lookup_query_from_song(&song).expect("fixture song should have a lookup query");
    assert!(matches!(
        fetch_online_timed_lyrics(&providers, &query),
        OnlineLyricsResult::DefiniteMissing
    ));

    lrclib_mock.assert();
    lrcapi_mock.assert();
    cleanup_dir(&fixture_dir);
}

#[test]
fn reads_embedded_lyrics_from_mp4_audio_even_when_extension_is_aac() {
    let fixture_dir = unique_fixture_dir();
    cleanup_dir(&fixture_dir);
    fs::create_dir_all(&fixture_dir).expect("fixture directory should create");

    let tagged_m4a_path = fixture_dir.join("lyrics-source.m4a");
    fs::copy(metadata_fixture_path("fixture.m4a"), &tagged_m4a_path)
        .expect("fixture m4a should copy");

    let mut tag = Tag::new(TagType::Mp4Ilst);
    tag.insert_text(ItemKey::Lyrics, "[00:10.00] embedded line".to_owned());
    tag.save_to_path(&tagged_m4a_path, WriteOptions::default())
        .expect("lyrics tag should save");

    let disguised_aac_path = fixture_dir.join("lyrics-source.aac");
    fs::copy(&tagged_m4a_path, &disguised_aac_path).expect("tagged m4a should copy to .aac");

    let embedded =
        read_embedded_lyrics(&disguised_aac_path).expect("embedded lyrics read should succeed");

    assert_eq!(embedded.as_deref(), Some("[00:10.00] embedded line"));

    cleanup_dir(&fixture_dir);
}

/// Regression test for the `[offset:]` metadata tag being propagated through
/// `LyricsAcquisition`. A sidecar `.lrc` with an `[offset:-250]` tag must
/// persist the parsed offset in the lyrics cache.
#[test]
fn acquisition_persists_lrc_offset_tag() {
    let lib_dir = support::unique_temp_path("phase4-offset");
    cleanup_dir(&lib_dir);
    let library = LibraryRoot::create(&lib_dir).expect("library should create");

    // Copy the fixture audio into the library's media directory.
    let audio_path = library.resolve("media/song.mp3");
    fs::copy(metadata_fixture_path("fixture.mp3"), &audio_path).expect("fixture audio should copy");

    // Write a sidecar LRC with an [offset:] tag.
    let lrc_content = "[offset:-250]\n[00:10.00]Look at the stars\n";
    fs::write(audio_path.with_extension("lrc"), lrc_content).expect("sidecar LRC should write");

    // Set up an in-memory database with the song registered.
    let connection =
        rusqlite::Connection::open(library.database_path()).expect("library database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");
    cache::upsert_song(
        &connection,
        &Song {
            hash: "offset-test-song".to_owned(),
            file_path: Some("media/song.mp3".to_owned()),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: Some("Yellow".to_owned()),
            artist: Some("Coldplay".to_owned()),
            album: Some("Parachutes".to_owned()),
            duration_ms: 267_000,
            cover_art: None,
            has_cover_art: true,
            artwork_thumb_path: None,
            imported_at: 1,
            original_ext: None,
        },
    )
    .expect("song insert should succeed");

    // Point both online providers at unreachable addresses so the sidecar
    // is the only source that can return lyrics.
    let lrclib_client = LrcLibClient::new("http://127.0.0.1:9");
    let lrcapi_client = LrcApiClient::new("http://127.0.0.1:9");

    let persisted = support::acquire_and_persist_lyrics(
        &connection,
        &library,
        &lrclib_client,
        &lrcapi_client,
        "offset-test-song",
    )
    .expect("lyrics acquisition should succeed");

    assert!(persisted.changed);

    // Verify the offset was also persisted to the cache.
    let cached = cache::lyrics::get_lyrics_cache_entry(&connection, "offset-test-song")
        .expect("cache lookup should succeed")
        .expect("cache entry should exist");
    assert_eq!(cached.offset_ms, -250);
    assert_eq!(cached.source, LyricsSource::Sidecar);

    cleanup_dir(&lib_dir);
}

/// When the LRC has no `[offset:]` tag, `offset_ms` must default to 0.
#[test]
fn acquisition_defaults_offset_to_zero_without_tag() {
    let lib_dir = support::unique_temp_path("phase4-offset-none");
    cleanup_dir(&lib_dir);
    let library = LibraryRoot::create(&lib_dir).expect("library should create");

    let audio_path = library.resolve("media/song.mp3");
    fs::copy(metadata_fixture_path("fixture.mp3"), &audio_path).expect("fixture audio should copy");

    // Sidecar LRC without an [offset:] tag.
    let lrc_content = "[00:10.00]Look at the stars\n";
    fs::write(audio_path.with_extension("lrc"), lrc_content).expect("sidecar LRC should write");

    let connection =
        rusqlite::Connection::open(library.database_path()).expect("library database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");
    cache::upsert_song(
        &connection,
        &Song {
            hash: "no-offset-test-song".to_owned(),
            file_path: Some("media/song.mp3".to_owned()),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: Some("Yellow".to_owned()),
            artist: Some("Coldplay".to_owned()),
            album: Some("Parachutes".to_owned()),
            duration_ms: 267_000,
            cover_art: None,
            has_cover_art: true,
            artwork_thumb_path: None,
            imported_at: 1,
            original_ext: None,
        },
    )
    .expect("song insert should succeed");

    let lrclib_client = LrcLibClient::new("http://127.0.0.1:9");
    let lrcapi_client = LrcApiClient::new("http://127.0.0.1:9");

    let persisted = support::acquire_and_persist_lyrics(
        &connection,
        &library,
        &lrclib_client,
        &lrcapi_client,
        "no-offset-test-song",
    )
    .expect("lyrics acquisition should succeed");

    assert!(persisted.changed);
    let cached = cache::lyrics::get_lyrics_cache_entry(&connection, "no-offset-test-song")
        .expect("cache lookup should succeed")
        .expect("lyrics cache should exist");
    assert_eq!(cached.offset_ms, 0);

    cleanup_dir(&lib_dir);
}
