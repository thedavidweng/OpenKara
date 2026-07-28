# Product Standards

This page routes a change to the standards that apply to it. It does not make
a whole-product certification claim.

## Required route

Before a product-surface change, identify the affected surfaces. Read only the
matching profile. Put the required automated or manual evidence in the pull
request. Record an exception with its standard clause, user effect, reason, and
compensating control in the same change.

| Changed surface                                                                                            | Read this profile                                                             | Required evidence                                                       |
| ---------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Feature outcome, acceptance, architecture, quality, or tests                                               | [Lifecycle, quality, and testing](standards/lifecycle-quality-and-testing.md) | Traceable acceptance evidence, tests, and ADR when required             |
| Rendered UI, keyboard input, dialogs, menus, drag operations, or task flow                                 | [Interaction and accessibility](standards/interaction-and-accessibility.md)   | Static, browser, and platform evidence that matches the widget or flow  |
| Product copy, locale, terminology, timestamps, units, or serialized data                                   | [Language, terminology, and data](standards/language-terminology-and-data.md) | Copy, locale, serialization, and contract evidence                      |
| IPC, WebDAV, OAuth, a public HTTP API, events, or compatibility                                            | [Interfaces and compatibility](standards/interfaces-and-compatibility.md)     | Contract, integration, migration, and error-model evidence              |
| Credentials, local data, remote providers, privacy, dependency, CI, or release artifacts                   | [Security, privacy, and release](standards/security-privacy-and-release.md)   | Threat, privacy, dependency, and release evidence                       |
| Model catalog, source separation, loudness, telemetry, a hosted service, container, or Kubernetes resource | [Models, media, and operations](standards/models-media-and-operations.md)     | Deterministic media or model evidence and service evidence when present |

The profiles are constraints when their scope matches. A profile never creates
a feature roadmap. A new product surface or a changed conformance target needs
an ADR.

See [the standards directory](standards/README.md) for the profile index.
