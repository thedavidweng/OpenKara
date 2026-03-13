use std::path::PathBuf;

use openkara_lib::{
    audio::decode,
    separator::{model, preprocess},
};

fn fixture_path(directory: &str, filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(directory)
        .join(filename)
}

#[test]
fn preprocesses_stereo_audio_into_channels_first_model_tensor() {
    let loaded_model = model::load_from_path(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("models")
            .join("htdemucs_embedded.onnx"),
    )
    .expect("demucs model should load");
    let decoded = decode::decode_file(&fixture_path("audio", "fixture.wav"))
        .expect("wav fixture should decode");

    let prepared = preprocess::prepare_model_input(&loaded_model, &decoded)
        .expect("decoded audio should convert to model input");

    assert_eq!(prepared.shape, vec![1, 2, 44_100]);
    assert_eq!(prepared.samples.len(), 88_200);
    assert!(prepared.samples.iter().any(|sample| *sample != 0.0));
}

#[test]
fn rejects_audio_with_a_sample_rate_the_model_does_not_support() {
    let loaded_model = model::load_from_path(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("models")
            .join("htdemucs_embedded.onnx"),
    )
    .expect("demucs model should load");
    let mut decoded = decode::decode_file(&fixture_path("audio", "fixture.wav"))
        .expect("wav fixture should decode");
    decoded.sample_rate = 48_000;

    let error = preprocess::prepare_model_input(&loaded_model, &decoded)
        .expect_err("48k audio should fail");

    assert!(error.to_string().contains("44.1 kHz"));
}
