//! Shared worker-describe contract: the checks a running worker's
//! `describe` payload must pass against its sealed registry manifest.
//!
//! Mock installs, the real HTTPS gate and the offline privacy regression all
//! call [`must_match_worker`], so a future registry serves the same failure
//! path. Every comparison is a boolean with one constant failure text: a
//! failing `assert!` or `panic!` never echoes worker bytes, so session state,
//! account objects, names and capabilities cannot leak into test logs.

use super::types::*;
use serde_json::Value;

/// The only failure this module ever reports. Constant on purpose: the
/// untrusted worker payload must not appear in any log line.
pub(crate) const CONTRACT_FAILURE: &str = "worker description failed contract";

/// Validate an untrusted worker `describe` payload against its sealed
/// manifest. Name, the full capability set, and the mandatory `search`
/// capability must match; the session must be present, signed out, and carry
/// an explicit null account (a missing account also fails).
pub(crate) fn must_match_worker(
    description: &Value,
    manifest: &SourceManifest,
) -> Result<(), String> {
    const FAIL: &str = CONTRACT_FAILURE;
    if description["protocol"]["major"].as_u64() != Some(super::engine::PROTOCOL_MAJOR as u64) {
        return Err(FAIL.into());
    }
    if description["source_id"].as_str() != Some(manifest.source_id.as_str()) {
        return Err(FAIL.into());
    }
    if description["package_id"].as_str() != Some(manifest.package_id.as_str()) {
        return Err(FAIL.into());
    }
    if description["package_version"].as_str() != Some(manifest.version.as_str()) {
        return Err(FAIL.into());
    }
    if description["name"].as_str() != Some(manifest.name.as_str()) {
        return Err(FAIL.into());
    }
    let mut reported: Vec<&str> = description["capabilities"]
        .as_array()
        .ok_or_else(|| FAIL.to_string())?
        .iter()
        .map(|value| value.as_str().ok_or_else(|| FAIL.to_string()))
        .collect::<Result<Vec<&str>, String>>()?;
    let mut declared: Vec<&str> = manifest.capabilities.iter().map(String::as_str).collect();
    reported.sort_unstable();
    declared.sort_unstable();
    if reported != declared {
        return Err(FAIL.into());
    }
    if !declared.contains(&"search") {
        return Err(FAIL.into());
    }
    if description["session"].get("state").and_then(Value::as_str) != Some("signed_out") {
        return Err(FAIL.into());
    }
    match description["session"].get("account") {
        Some(Value::Null) => Ok(()),
        _ => Err(FAIL.into()),
    }
}
