//! Golden-vector validation of the native spectral contract implementation
//! (`openkara.spectral-contract/v1`, issue #172 PR 1).
//!
//! For every pinned fixture this test:
//!   1. verifies the SHA-256 of each stored array against
//!      `spectral-golden-v1.json` (`release_asset_sha256`) before trusting it;
//!   2. runs `spec(input)` and compares to the golden `spectral` tensor;
//!   3. checks the `magnitude` neural-core view against the golden `magnitude`;
//!   4. runs `ispec(spectral_golden, N)` and compares to the golden `roundtrip`.
//!
//! The contract gate for this fp32 implementation is `1e-3` max-abs per stage;
//! the achieved max-abs is printed for every fixture/stage.

use std::io::Read;
use std::path::PathBuf;

use openkara_lib::separator::spectral::{self, SpectralPlans, CHANNELS, CONTRACT_FREQS};
use sha2::{Digest, Sha256};

/// Contract gate: an fp32 implementation must stay within this max-abs of the
/// golden vectors at every stage.
const FP32_MAX_ABS: f32 = 1e-3;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("spectral")
}

/// A parsed `.npy` array: its shape and its raw little-endian `f32` bytes and
/// decoded values. The raw bytes are retained so the SHA-256 can be checked
/// against the digest that was computed over `ndarray.tobytes()`.
struct NpyArray {
    shape: Vec<usize>,
    raw: Vec<u8>,
    data: Vec<f32>,
}

/// Minimal `.npy` v1.0/v2.0 parser for `<f4`, C-order arrays (see the NumPy
/// format spec). Sufficient for the golden fixtures, which are all little-endian
/// float32 and C-contiguous.
fn parse_npy(bytes: &[u8]) -> NpyArray {
    assert_eq!(&bytes[0..6], b"\x93NUMPY", "npy magic");
    let major = bytes[6];
    let (header_start, header_len) = if major == 1 {
        let hl = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
        (10usize, hl)
    } else {
        let hl = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        (12usize, hl)
    };
    let header = std::str::from_utf8(&bytes[header_start..header_start + header_len])
        .expect("npy header is utf-8");
    assert!(
        header.contains("'<f4'"),
        "expected little-endian f32: {header}"
    );
    assert!(
        header.contains("'fortran_order': False"),
        "expected C-order: {header}"
    );

    let s = header.find("'shape':").expect("shape key");
    let open = header[s..].find('(').expect("shape tuple open") + s;
    let close = header[open..].find(')').expect("shape tuple close") + open;
    let shape: Vec<usize> = header[open + 1..close]
        .split(',')
        .filter_map(|tok| {
            let tok = tok.trim();
            if tok.is_empty() {
                None
            } else {
                Some(tok.parse().expect("shape dim"))
            }
        })
        .collect();

    let data_start = header_start + header_len;
    let raw = bytes[data_start..].to_vec();
    assert_eq!(raw.len() % 4, 0, "f32 payload length");
    let data = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect();
    NpyArray { shape, raw, data }
}

/// Read one member array out of a `.npz` (a zip of `.npy` members).
fn read_npz_member(npz: &PathBuf, member: &str) -> NpyArray {
    let file = std::fs::File::open(npz).unwrap_or_else(|e| panic!("open {}: {e}", npz.display()));
    let mut archive = zip::ZipArchive::new(file).expect("valid npz (zip) archive");
    let name = format!("{member}.npy");
    let mut entry = archive
        .by_name(&name)
        .unwrap_or_else(|e| panic!("member {name} in {}: {e}", npz.display()));
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).expect("read npz member");
    parse_npy(&buf)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "length mismatch in comparison");
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max)
}

/// The five pinned fixtures and their input sample counts.
const FIXTURES: &[(&str, usize)] = &[
    ("silence-10240", 10240),
    ("impulse-10240", 10240),
    ("tone440-10240", 10240),
    ("bandlimited-noise-10240", 10240),
    ("sweep-10000", 10000),
];

/// Load the `release_asset_sha256` table from the pinned golden manifest.
fn load_digests() -> serde_json::Value {
    let path = fixtures_dir().join("spectral-golden-v1.json");
    let text = std::fs::read_to_string(&path).expect("read spectral-golden-v1.json");
    let json: serde_json::Value = serde_json::from_str(&text).expect("parse golden json");
    assert_eq!(
        json["contract_version"],
        spectral::SPECTRAL_CONTRACT_VERSION,
        "golden manifest contract version must match the implementation"
    );
    json["release_asset_sha256"].clone()
}

#[test]
fn spectral_golden_vectors_validate_every_stage() {
    let digests = load_digests();
    let mut plans = SpectralPlans::new();

    // Accumulate the worst achieved max-abs across all fixtures per stage.
    let mut worst_spec = 0.0f32;
    let mut worst_mag = 0.0f32;
    let mut worst_ispec = 0.0f32;

    for &(name, samples) in FIXTURES {
        let npz = fixtures_dir().join(format!("{name}.npz"));
        let input = read_npz_member(&npz, "input");
        let spectral_golden = read_npz_member(&npz, "spectral");
        let magnitude_golden = read_npz_member(&npz, "magnitude");
        let roundtrip_golden = read_npz_member(&npz, "roundtrip");

        // 1. Digest gate — verify every stored array before trusting it.
        for (arr, parsed) in [
            ("input", &input),
            ("spectral", &spectral_golden),
            ("magnitude", &magnitude_golden),
            ("roundtrip", &roundtrip_golden),
        ] {
            let expected = digests[name][arr]
                .as_str()
                .unwrap_or_else(|| panic!("digest for {name}/{arr}"));
            let actual = sha256_hex(&parsed.raw);
            assert_eq!(actual, expected, "sha256 mismatch for {name}/{arr}");
        }

        // Shape sanity against the contract.
        assert_eq!(input.shape, vec![1, CHANNELS, samples]);
        let le = spectral::forward_frames(samples);
        assert_eq!(
            spectral_golden.shape,
            vec![1, CHANNELS, 2, CONTRACT_FREQS, le]
        );
        assert_eq!(
            magnitude_golden.shape,
            vec![1, CHANNELS * 2, CONTRACT_FREQS, le]
        );
        assert_eq!(roundtrip_golden.shape, vec![1, CHANNELS, samples]);

        // 2. Forward: spec(input) vs golden spectral.
        let spec_out = plans.spec(&input.data, CHANNELS, samples);
        let d_spec = max_abs_diff(&spec_out, &spectral_golden.data);
        worst_spec = worst_spec.max(d_spec);
        assert!(
            d_spec <= FP32_MAX_ABS,
            "{name}: spec max-abs {d_spec:e} exceeds {FP32_MAX_ABS:e}"
        );

        // 3. Magnitude neural-core view vs golden magnitude (a pure reshape;
        //    the golden magnitude/spectral share one digest by construction).
        let mag_view = spectral::magnitude(&spec_out);
        let d_mag = max_abs_diff(mag_view, &magnitude_golden.data);
        worst_mag = worst_mag.max(d_mag);
        assert!(
            d_mag <= FP32_MAX_ABS,
            "{name}: magnitude max-abs {d_mag:e} exceeds {FP32_MAX_ABS:e}"
        );

        // 4. Inverse: ispec(golden spectral) vs golden roundtrip.
        let ispec_out = plans.ispec(&spectral_golden.data, CHANNELS, samples);
        let d_ispec = max_abs_diff(&ispec_out, &roundtrip_golden.data);
        worst_ispec = worst_ispec.max(d_ispec);
        assert!(
            d_ispec <= FP32_MAX_ABS,
            "{name}: ispec max-abs {d_ispec:e} exceeds {FP32_MAX_ABS:e}"
        );

        println!("{name:26} spec={d_spec:.3e}  magnitude={d_mag:.3e}  ispec={d_ispec:.3e}");
    }

    println!(
        "WORST across fixtures  spec={worst_spec:.3e}  magnitude={worst_mag:.3e}  ispec={worst_ispec:.3e}  (gate {FP32_MAX_ABS:e})"
    );
}

#[test]
fn spectral_plans_reused_across_fixtures_match_fresh_plans() {
    // A single reused SpectralPlans must produce identical output to a freshly
    // constructed one for every fixture (no cross-call scratch contamination).
    let mut reused = SpectralPlans::new();
    for &(name, samples) in FIXTURES {
        let npz = fixtures_dir().join(format!("{name}.npz"));
        let input = read_npz_member(&npz, "input");
        let spectral_golden = read_npz_member(&npz, "spectral");

        let a = reused.spec(&input.data, CHANNELS, samples);
        let mut fresh = SpectralPlans::new();
        let b = fresh.spec(&input.data, CHANNELS, samples);
        assert_eq!(a, b, "{name}: reused spec differs from fresh spec");

        let ra = reused.ispec(&spectral_golden.data, CHANNELS, samples);
        let rb = fresh.ispec(&spectral_golden.data, CHANNELS, samples);
        assert_eq!(ra, rb, "{name}: reused ispec differs from fresh ispec");
    }
}
