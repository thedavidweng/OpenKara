# ADR 0043 — One license declaration for every product surface

Date: 2026-10-24
Status: accepted

## Context

ADR 0032 distributes the application under AGPL-3.0-only. The root package, the
Cargo package, the Flathub metadata, and the WinGet template state this
license. Four surfaces still said Apache-2.0: the README badges, the website
footer, the website Terms of Service, and the historical context of ADR 0035.
The website Terms page is a legal statement about the license. A stale
statement there contradicts the LICENSE file.

## Decision

Use the SPDX identifier `AGPL-3.0-only` everywhere the project states its own
license: README badges (English and Chinese), website footer copy, website
Terms paragraphs (English and Chinese), the `license` field of every
first-party `package.json`, bundle license metadata in `tauri.conf.json`, and
the root LICENSE file. Link the Terms paragraph to the repository LICENSE
file.

Historical records do not change. ADR 0028, ADR 0032, NOTICE, LICENSING.md,
and LICENSES/Apache-2.0.txt keep their existing references to earlier Apache
grants. Third-party dependency license policy in `deny.toml` and
`dependency-review.yml` does not change.

## Consequences

- A release note or a page that names the project license must use the SPDX
  identifier, not a display name such as "Apache 2.0".
- The Apache-2.0 grant for revisions before `f461b2a` stays in force. Do not
  delete LICENSES/Apache-2.0.txt or NOTICE.
- The website Terms page has a later date after a license statement change.
