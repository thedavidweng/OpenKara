use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use openkara_lib::{
    audio::decode,
    separator::{inference, model},
};

fn fixture_path(directory: &str, filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(directory)
        .join(filename)
}

fn unique_output_dir() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();

    std::env::temp_dir().join(format!("openkara-phase3-inference-{timestamp}"))
}

fn cleanup_dir(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).expect("temporary output directory should be removable");
    }
}

#[test]
fn separates_fixture_audio_into_named_stems_and_writes_wavs() {
    let mut loaded_model = model::load_from_path(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("models")
            .join("htdemucs_embedded.onnx"),
    )
    .expect("demucs model should load");
    let decoded = decode::decode_file(&fixture_path("audio", "fixture.wav"))
        .expect("wav fixture should decode");

    let separated = inference::separate_audio(&mut loaded_model, &decoded)
        .expect("fixture audio should separate into stems");

    assert_eq!(separated.stems.len(), 4);
    assert_eq!(
        separated
            .stems
            .iter()
            .map(|stem| stem.name.as_str())
            .collect::<Vec<_>>(),
        vec!["drums", "bass", "other", "vocals"]
    );
    assert!(separated
        .stems
        .iter()
        .all(|stem| stem.audio.sample_rate == decoded.sample_rate));
    assert!(separated
        .stems
        .iter()
        .all(|stem| stem.audio.channels == decoded.channels));
    assert!(separated
        .stems
        .iter()
        .all(|stem| stem.audio.samples.len() == decoded.samples.len()));

    let output_dir = unique_output_dir();
    cleanup_dir(&output_dir);

    let written_paths = inference::write_stems_to_directory(&separated, &output_dir)
        .expect("stem wav files should be written");

    assert_eq!(written_paths.len(), 4);
    for stem_name in ["drums", "bass", "other", "vocals"] {
        let stem_path = output_dir.join(format!("{stem_name}.wav"));
        assert!(stem_path.exists(), "{} should exist", stem_path.display());
    }

    cleanup_dir(&output_dir);
}
