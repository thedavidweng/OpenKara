# Security, Privacy, and Release

Use this profile for credentials, local or remote data, dependencies, build
automation, releases, vulnerability handling, and privacy behavior.

## Authorities

| Authority                                                                                                                       | Use in OpenKara                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| [OWASP ASVS 5.0](https://owasp.org/www-project-application-security-verification-standard/)                                     | Versioned application-security requirements for WebView, IPC, data, providers, and release services |
| [NIST SSDF 1.1](https://csrc.nist.gov/pubs/sp/800/218/final)                                                                    | Secure development, code review, dependency, build, release, and response controls                  |
| [NIST Privacy Framework 1.0](https://www.nist.gov/privacy-framework)                                                            | Data map, minimization, user control, retention, and disclosure review                              |
| [ISO/IEC 27001:2022](https://www.iso.org/standard/27001) and [ISO/IEC 27701:2025](https://www.iso.org/standard/27701)           | Governance reference for information-security and privacy management systems                        |
| [ISO/IEC 29147:2018](https://www.iso.org/standard/72311.html) and [ISO/IEC 30111:2019](https://www.iso.org/standard/69725.html) | Vulnerability disclosure and handling process                                                       |
| [SLSA 1.2](https://slsa.dev/spec/v1.2/) and [SPDX 3.0.1](https://spdx.github.io/spdx-spec/v3.0.1/)                              | Artifact provenance and software bill of materials                                                  |

## Constraints

- Apply the ASVS requirement that matches the changed architecture. Record its
  version and identifier when a security requirement drives the design.
- Treat credentials, library data, OAuth data, diagnostics, and telemetry as
  data subjects to privacy review. Keep only the data needed for the stated
  user outcome.
- Do not claim ISO 27001 or ISO 27701 certification. These standards guide
  governance controls when the project has evidence for them.
- A public vulnerability-disclosure SLA requires `SECURITY.md`, a private
  reporting channel, severity handling, and coordinated disclosure evidence.
- A release may claim SLSA provenance or an SPDX SBOM only when the release
  workflow creates and publishes that exact artifact evidence.

## Required evidence

- Security tests, threat review, or ASVS requirement evidence for a changed
  trust boundary.
- Privacy review for changed data collection, storage, sync, diagnostics, or
  user control.
- Dependency policy and build evidence for changed dependencies or workflows.
- Signed artifacts, provenance, and SBOM evidence when a release claim uses
  those terms.
