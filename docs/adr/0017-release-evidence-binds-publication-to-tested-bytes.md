# ADR 0017 — Release evidence binds publication to tested bytes

Date: 2026-08-01
Status: accepted

## Context

The installed-app smoke jobs and the release publish jobs can build different
files. A passing smoke report does not prove that the published artifact was
tested. Several scripts also repeat the release gate rules. This makes the
release result hard to audit and easy to split.

## Decision

Each platform builds one signed Release Candidate. The smoke job tests that
candidate and records its byte digest in versioned Release Evidence. A Rust
evidence module owns assertion and gate semantics. The publish job accepts only
the tested candidates and creates `release-evidence.json`, `SHA256SUMS`, and
`latest.json` from the same evidence subject. The publish job does not rebuild
or infer evidence from mutable release assets.

## Consequences

- A digest mismatch blocks publication.
- Node, PowerShell, and workflow code may collect observations, but they may
  not implement a second release policy.
- Evidence must not contain credentials or user media content.
- The evidence schema is a versioned wire contract and needs contract tests.
