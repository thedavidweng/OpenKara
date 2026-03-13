use std::path::PathBuf;

use openkara_lib::{
    cache,
    commands::{
        error::{ErrorCode, FallbackAction},
        import::{get_library_from_connection, import_songs_from_paths},
    },
};
use rusqlite::Connection;

fn fixture_path(filename: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("metadata")
        .join(filename)
        .display()
        .to_string()
}

#[test]
fn imports_fixture_audio_and_persists_library_rows() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");

    let result = import_songs_from_paths(
        &connection,
        &[fixture_path("fixture.mp3"), fixture_path("fixture.flac")],
    );

    assert_eq!(result.imported.len(), 2);
    assert!(result.failed.is_empty());
    assert_eq!(
        result.imported[0].title.as_deref(),
        Some("Fixture Song MP3")
    );
    assert_eq!(
        result.imported[1].title.as_deref(),
        Some("Fixture Song FLAC")
    );
    assert_eq!(result.imported[0].hash.len(), 64);

    let library = get_library_from_connection(&connection).expect("library listing should succeed");
    assert_eq!(library.len(), 2);
}

#[test]
fn reports_failures_without_aborting_other_imports() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");

    let missing_path = fixture_path("missing.mp3");
    let result = import_songs_from_paths(
        &connection,
        &[fixture_path("fixture.m4a"), missing_path.clone()],
    );

    assert_eq!(result.imported.len(), 1);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].path, missing_path);
    assert_eq!(result.failed[0].error.code, ErrorCode::MediaReadFailed);
    assert_eq!(
        result.failed[0].error.fallback,
        FallbackAction::ReimportSong
    );
    assert!(!result.failed[0].error.retryable);

    let library = get_library_from_connection(&connection).expect("library listing should succeed");
    assert_eq!(library.len(), 1);
    assert_eq!(library[0].title.as_deref(), Some("Fixture Song M4A"));
}
