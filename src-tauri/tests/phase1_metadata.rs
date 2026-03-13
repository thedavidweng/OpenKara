use std::path::PathBuf;

use openkara_lib::metadata;

fn fixture_path(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("metadata")
        .join(filename)
}

#[test]
fn reads_mp3_fixture_metadata() {
    let metadata = metadata::read_from_path(&fixture_path("fixture.mp3"))
        .expect("fixture mp3 metadata should parse");

    assert_eq!(metadata.title.as_deref(), Some("Fixture Song MP3"));
    assert_eq!(metadata.artist.as_deref(), Some("Fixture Artist"));
    assert_eq!(metadata.album.as_deref(), Some("Fixture Album"));
    assert!(metadata.duration_ms > 0);
}

#[test]
fn reads_flac_fixture_metadata() {
    let metadata = metadata::read_from_path(&fixture_path("fixture.flac"))
        .expect("fixture flac metadata should parse");

    assert_eq!(metadata.title.as_deref(), Some("Fixture Song FLAC"));
    assert_eq!(metadata.artist.as_deref(), Some("Fixture Artist"));
    assert_eq!(metadata.album.as_deref(), Some("Fixture Album"));
    assert!(metadata.duration_ms > 0);
}

#[test]
fn reads_m4a_fixture_metadata() {
    let metadata = metadata::read_from_path(&fixture_path("fixture.m4a"))
        .expect("fixture m4a metadata should parse");

    assert_eq!(metadata.title.as_deref(), Some("Fixture Song M4A"));
    assert_eq!(metadata.artist.as_deref(), Some("Fixture Artist"));
    assert_eq!(metadata.album.as_deref(), Some("Fixture Album"));
    assert!(metadata.duration_ms > 0);
}
