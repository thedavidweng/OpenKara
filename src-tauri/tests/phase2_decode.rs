use std::path::PathBuf;

use openkara_lib::audio::decode;

fn fixture_path(directory: &str, filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(directory)
        .join(filename)
}

#[test]
fn decodes_wav_fixture_to_expected_sample_shape() {
    let decoded =
        decode::decode_file(&fixture_path("audio", "fixture.wav")).expect("wav should decode");

    assert_eq!(decoded.sample_rate, 44_100);
    assert_eq!(decoded.channels, 2);
    assert_eq!(decoded.samples.len(), 44_100 * 2);
    assert!(decoded.duration_ms >= 999);
    assert!(decoded.duration_ms <= 1_001);
}

#[test]
fn decodes_phase_fixtures_across_supported_formats() {
    for path in [
        fixture_path("metadata", "fixture.mp3"),
        fixture_path("metadata", "fixture.flac"),
        fixture_path("metadata", "fixture.m4a"),
        fixture_path("audio", "fixture.ogg"),
    ] {
        let decoded = decode::decode_file(&path)
            .unwrap_or_else(|error| panic!("{} should decode: {error:#}", path.display()));

        assert_eq!(decoded.channels, 2, "{} channel count", path.display());
        assert_eq!(
            decoded.sample_rate,
            44_100,
            "{} sample rate",
            path.display()
        );
        assert!(
            !decoded.samples.is_empty(),
            "{} has samples",
            path.display()
        );
        assert!(decoded.duration_ms > 0, "{} has duration", path.display());
    }
}
