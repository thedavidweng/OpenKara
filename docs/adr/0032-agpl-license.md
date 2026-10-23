# ADR 0032 — AGPL application license

Date: 2026-10-05
Status: accepted

## Context

The maintainer wants distributed derivatives and modified network services to
provide corresponding source. Historical Apache grants and third-party
licenses remain effective.

## Decision

Distribute the application under AGPL-3.0-only. Require the comment-signed CLA for external contributions; contributors retain
their copyright and grant commercial and proprietary sublicensing rights. Document historical grants
and third-party exceptions in the root licensing documents.

This replaces ADR 0028's decision against relicensing the project. Its ban on
copying or depending on AMLL player packages remains. The Cargo license
exception applies only to the `openkara` package; it does not allow arbitrary
AGPL dependencies.

The CLA bot uses the same pinned CLA Assistant action as Vapourfly. Signatures
are stored on `cla-signatures`, with a commit-pinned agreement URL. A separate
`CLA` commit status targets the checked PR head, including after comment-based
signing. No maintainer approval comment is needed. Require this status after
the workflow is published.

## Consequences

Earlier Apache releases and grants remain usable under their original terms.
Third-party models, runtimes, libraries, and artwork retain their own licenses.
Proprietary editions require sufficient rights to every component they use;
the CLA covers only contributions within its signed scope and does not alter
third-party licenses or historical Apache grants.

Private modifications are not automatically public; AGPL obligations follow
distribution and the applicable network-interaction provisions. AGPL permits
charging for compliant releases and does not require submitting changes here.
