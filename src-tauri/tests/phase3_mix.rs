use std::{
    fs,
    path::{Path, PathBuf},
};

mod support;

use openkara_lib::{
    audio::decode::DecodedAudio,
    separator::{
        inference::{SeparatedStem, SeparationResult},
        mix,
    },
};

fn unique_output_dir() -> PathBuf {
    support::unique_temp_path("phase3-mix")
}

fn cleanup_dir(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).expect("temporary output directory should be removable");
    }
}

fn test_stem(name: &str, samples: Vec<f32>) -> SeparatedStem {
    SeparatedStem {
        name: name.to_owned(),
        audio: DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 1,
            samples,
        },
    }
}

#[test]
fn mixes_non_vocal_stems_into_normalized_accompaniment_and_writes_wav() {
    let separation = SeparationResult {
        stems: vec![
            test_stem("drums", vec![0.5, 0.25, -0.5, -0.25]),
            test_stem("bass", vec![0.5, 0.25, -0.5, -0.25]),
            test_stem("other", vec![0.5, 0.25, -0.5, -0.25]),
            test_stem("vocals", vec![0.9, 0.9, 0.9, 0.9]),
        ],
    };

    let accompaniment =
        mix::mix_accompaniment(&separation).expect("non-vocal stems should mix into accompaniment");

    assert_eq!(accompaniment.sample_rate, 44_100);
    assert_eq!(accompaniment.channels, 2);
    assert_eq!(accompaniment.samples, vec![1.0, 0.5, -1.0, -0.5]);

    let output_dir = unique_output_dir();
    cleanup_dir(&output_dir);
    let output_path = output_dir.join("accompaniment.wav");

    mix::write_accompaniment_wav(&accompaniment, &output_path)
        .expect("accompaniment wav should write");

    let mut reader = hound::WavReader::open(&output_path).expect("wav should open");
    assert_eq!(reader.spec().channels, 2);
    assert_eq!(reader.spec().sample_rate, 44_100);
    assert_eq!(reader.duration(), 2);
    assert_eq!(reader.samples::<i16>().count(), 4);

    cleanup_dir(&output_dir);
}
