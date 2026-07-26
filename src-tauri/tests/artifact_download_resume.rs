//! #270: a stalled transfer must resume, not restart.
//!
//! The reported failure was an hour spent reaching 30% of the model download,
//! then a transport error that discarded everything. These cover the two shapes
//! that produced it.

use openkara_lib::separator::artifacts::download_verified_to_temp;
use sha2::{Digest, Sha256};

fn payload() -> Vec<u8> {
    (0..64_u32 * 1024).map(|i| (i % 251) as u8).collect()
}

fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    openkara_lib::hash::hex_lower(hasher.finalize())
}

#[test]
fn resumes_from_the_byte_the_connection_dropped_on() {
    let body = payload();
    let split = body.len() / 3;
    let mut server = mockito::Server::new();

    // First attempt: the server hands back a body that ends early, which is
    // what a dropped connection looks like to the reader.
    let truncated = server
        .mock("GET", "/model.onnx")
        .match_header("range", mockito::Matcher::Missing)
        .with_status(200)
        .with_body(&body[..split])
        .create();

    // The retry asks for the remainder and gets a 206.
    let remainder = server
        .mock("GET", "/model.onnx")
        .match_header("range", format!("bytes={split}-").as_str())
        .with_status(206)
        .with_body(&body[split..])
        .create();

    let staging = tempfile::tempdir().expect("staging dir");
    let mut peak_progress = 0_u64;
    let path = download_verified_to_temp(
        &format!("{}/model.onnx", server.url()),
        body.len() as u64,
        &digest(&body),
        staging.path(),
        |downloaded, _| peak_progress = peak_progress.max(downloaded),
    )
    .expect("the download should resume and verify");

    truncated.assert();
    remainder.assert();
    assert_eq!(std::fs::read(&path).expect("downloaded file"), body);
    assert_eq!(peak_progress, body.len() as u64);
}

#[test]
fn restarts_when_the_server_ignores_the_range_request() {
    let body = payload();
    let split = body.len() / 4;
    let mut server = mockito::Server::new();

    let truncated = server
        .mock("GET", "/model.onnx")
        .match_header("range", mockito::Matcher::Missing)
        .with_status(200)
        .with_body(&body[..split])
        .create();

    // 200 instead of 206: the server sent the whole body again. Appending it
    // would corrupt the file, so the transfer has to start over.
    let full_again = server
        .mock("GET", "/model.onnx")
        .match_header("range", mockito::Matcher::Any)
        .with_status(200)
        .with_body(&body)
        .create();

    let staging = tempfile::tempdir().expect("staging dir");
    let path = download_verified_to_temp(
        &format!("{}/model.onnx", server.url()),
        body.len() as u64,
        &digest(&body),
        staging.path(),
        |_, _| {},
    )
    .expect("the download should restart and verify");

    truncated.assert();
    full_again.assert();
    assert_eq!(std::fs::read(&path).expect("downloaded file"), body);
}
