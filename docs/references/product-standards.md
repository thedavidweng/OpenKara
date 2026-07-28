# Product Standards

This page lists the selected standards for OpenKara. It gives each standard a
scope and an evidence type. It does not make a whole-product certification
claim.

## Use of standards

A change uses every profile that matches its affected product surface. An
exception records the standard clause, user effect, reason, and compensating
control in the same change.

Automated checks give repeatable evidence. Review of a complete user process
gives evidence for behavior that tools cannot measure.

## Accessibility and interaction

| Area                | Authority                                                        | Scope                                                                   | Evidence                                                                                                                    |
| ------------------- | ---------------------------------------------------------------- | ----------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Rendered UI         | [WCAG 2.2, level AA](https://www.w3.org/TR/WCAG22/)              | All React and WebView screens                                           | `jsx-a11y`, axe, keyboard Playwright tests, platform review                                                                 |
| Desktop behavior    | [WCAG2ICT 2.2](https://www.w3.org/TR/wcag2ict-22/)               | Windows, dialogs, system integration, and non-web behavior              | Keyboard, focus, assistive-technology, and platform review                                                                  |
| Custom widgets      | [WAI-ARIA Authoring Practices](https://www.w3.org/WAI/ARIA/apg/) | Dialogs, menus, lists, sliders, trees, drag operations, and live status | Native HTML first; APG role, state, and keyboard tests for custom widgets                                                   |
| Interaction quality | [ISO 9241-110:2020](https://www.iso.org/standard/75258.html)     | Every material interaction change                                       | Review of task fit, clarity, user control, error recovery, and consistency                                                  |
| Design process      | [ISO 9241-210:2019](https://www.iso.org/standard/77520.html)     | New or changed user flows                                               | User context, task scenario, prototype or implementation evidence, and outcome review                                       |
| Product quality     | [ISO/IEC 25010:2023](https://www.iso.org/standard/78176.html)    | Feature and release acceptance                                          | Functional, performance, compatibility, usability, reliability, security, maintainability, portability, and safety evidence |

Every action has a keyboard path. Every control has an accessible name. Native
HTML elements carry semantics by default. A custom widget uses the matching
APG pattern. Keyboard focus stays visible and unobscured. Interactive targets
meet WCAG 2.2 target-size requirements. A dialog traps focus while open,
closes by Escape where suitable, and returns focus to its invoking control.

[Apple Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/),
[Microsoft accessibility guidance](https://learn.microsoft.com/windows/apps/develop/accessibility),
and [GNOME Human Interface Guidelines](https://developer.gnome.org/hig/)
provide platform conventions. Platform conventions shape command names,
shortcuts, menus, and window behavior on their platform.

## Copy, language, and locale data

| Area              | Authority                                                                    | Scope                                                             | Evidence                                                     |
| ----------------- | ---------------------------------------------------------------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------ |
| Product copy      | [ISO 24495-1:2023](https://www.iso.org/standard/78907.html)                  | Labels, descriptions, errors, confirmations, and recovery text    | Copy review: users can find, understand, and use the message |
| Technical English | ASD-STE100 Simplified English                                                | Repository technical prose                                        | Repository documentation review                              |
| Language tags     | [BCP 47 / RFC 5646](https://www.rfc-editor.org/rfc/rfc5646)                  | Locale file names, HTML `lang`, settings, and API locale fields   | Canonical-tag tests and language-change tests                |
| Locale data       | [Unicode LDML / TR35](https://unicode.org/reports/tr35/) and platform `Intl` | Display names, collation, dates, times, numbers, and plural rules | Locale tests and native-platform formatting                  |
| Wire timestamps   | [RFC 3339](https://www.rfc-editor.org/rfc/rfc3339)                           | Public data, remote sync records, and logs with timestamps        | Contract and serialization tests                             |

Product text uses one term for one concept in each locale. Action labels start
with a verb when they start an action. Error text states the problem, current
state, and a safe recovery action. Translators preserve meaning and may change
word order for their language.

## Remote data, security, and releases

| Area                 | Authority                                                                                                 | Scope                                                                  | Evidence                                                               |
| -------------------- | --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| WebDAV and HTTP      | [RFC 4918](https://www.rfc-editor.org/rfc/rfc4918) and [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110) | Every WebDAV method and conditional write that OpenKara supports       | Provider contract and integration tests                                |
| OAuth                | [RFC 9700](https://www.rfc-editor.org/rfc/rfc9700) and [RFC 7636](https://www.rfc-editor.org/rfc/rfc7636) | Browser-based OAuth for public desktop clients                         | Redirect, PKCE, token-storage, and reauthorization tests               |
| Application security | [OWASP ASVS 5.0](https://owasp.org/www-project-application-security-verification-standard/)               | WebView, IPC input, local data, remote providers, and release services | Versioned security requirements, dependency checks, and security tests |
| Secure development   | [NIST SSDF 1.1](https://csrc.nist.gov/pubs/sp/800/218/final)                                              | Source, dependencies, review, build, release, and response process     | CI gates, pinned actions, dependency policy, and release review        |
| Privacy risk         | [NIST Privacy Framework 1.0](https://www.nist.gov/privacy-framework)                                      | Library data, remote sync, telemetry, diagnostics, and OAuth data      | Data map, minimization, user control, retention, and disclosure review |
| Release provenance   | [SLSA 1.2](https://slsa.dev/spec/v1.2/) and [SPDX 3.0.1](https://spdx.github.io/spdx-spec/v3.0.1/)        | Published installers, update artifacts, and dependency inventory       | Signed artifacts, provenance, and a published SBOM                     |

OpenKara applies ASVS requirements where the requirement matches the
application architecture. The project names the ASVS version and requirement
identifier when it records a security requirement. Release provenance and the
SBOM describe the exact release artifact and its build inputs.

## Audio

| Area                   | Authority                                                              | Scope                                                                       | Evidence                                               |
| ---------------------- | ---------------------------------------------------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------------ |
| Loudness and true peak | [ITU-R BS.1770-5](https://www.itu.int/rec/R-REC-BS.1770-5-202311-I/en) | Loudness display, normalization, limiter behavior, and exported measurement | Deterministic fixture measurements and tolerance tests |

BS.1770-5 applies when OpenKara measures or changes loudness. It gives no
source-separation quality score and no playback-clock synchronization model.
Those areas use product contracts, deterministic fixtures, and dedicated ADRs.
