use std::{
    fs,
    path::{Path, PathBuf},
};

mod support;

use openkara_lib::{
    cache,
    library::Song,
    separator::{job, model},
};
use rusqlite::Connection;

fn fixture_path(directory: &str, filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(directory)
        .join(filename)
}

fn unique_cache_dir() -> PathBuf {
    support::unique_temp_path("phase3-job")
}

fn cleanup_dir(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).expect("temporary cache directory should be removable");
    }
}

fn fixture_song(hash: &str) -> Song {
    Song {
        hash: hash.to_owned(),
        file_path: fixture_path("audio", "fixture.wav").display().to_string(),
        title: Some("Fixture Song".to_owned()),
        artist: Some("Fixture Artist".to_owned()),
        album: Some("Fixture Album".to_owned()),
        duration_ms: 1_000,
        cover_art: None,
        imported_at: 1,
    }
}

#[test]
fn separation_job_reports_monotonic_progress_and_hits_cache_on_second_run() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    cache::apply_migrations(&connection).expect("migrations should succeed");
    cache::upsert_song(&connection, &fixture_song("fixture-song"))
        .expect("fixture song should insert");

    let model_path = model::default_model_path();
    let cache_dir = unique_cache_dir();
    cleanup_dir(&cache_dir);

    let mut first_progress = Vec::new();
    let first = job::separate_song_into_cache(
        &connection,
        &cache_dir,
        &model_path,
        "fixture-song",
        |percent| first_progress.push(percent),
    )
    .expect("first separation should succeed");

    assert!(!first.cache_hit);
    assert_eq!(first_progress.last(), Some(&100));
    assert!(first_progress
        .windows(2)
        .all(|window| window[0] <= window[1]));
    assert!(Path::new(&first.vocals_path).exists());
    assert!(Path::new(&first.accomp_path).exists());

    let mut second_progress = Vec::new();
    let second = job::separate_song_into_cache(
        &connection,
        &cache_dir,
        &model_path,
        "fixture-song",
        |percent| second_progress.push(percent),
    )
    .expect("second separation should hit cache");

    assert!(second.cache_hit);
    assert_eq!(second_progress, vec![100]);
    assert_eq!(first.vocals_path, second.vocals_path);
    assert_eq!(first.accomp_path, second.accomp_path);

    cleanup_dir(&cache_dir);
}
