# ADR 0047 — Include the official Source Registry and update its sources automatically

Date: 2026-11-06
Status: accepted

## Context

ADR 0041 ships OpenKara with no Streaming Source and no Source Registry. A new
user must find a registry URL outside the app before a Streaming Source can
install. Every update also waits for a manual action, so a fix to an upstream
break reaches users late. The OpenKara project already publishes reviewed
packages in the separate `openkara-sources` repository. Each package runs in
the ADR 0041 sandbox and can contact only the origins it declares. Magpie uses
the same split: plugin code lives in a separate community repository, and the
app knows that repository's listing and installs from it in the app.

## Decision

Supersede only the ADR 0041 rule that OpenKara ships with no Source Registry.
Keep the rule that OpenKara ships with no Streaming Source. Keep the sandbox,
the integrity checks, the package identity and the consent rules.

Source code for Streaming Sources stays out of the OpenKara repository and out
of the application bundle. It lives and releases in `openkara-sources`. The
application contains only the address of that repository's registry document.

Builds with the `streaming-sources` feature include the official Source
Registry at
`https://raw.githubusercontent.com/thedavidweng/openkara-sources/main/registry.json`
with registry id `org.openkara.sources`. The host lists it before every added
registry and fetches it on the normal refresh schedule. The user cannot remove
it. The host does not write it to `state.json`, so a later release can change
its URL. Adding the official URL again keeps one entry.

The user completes the full flow in the app: browse the listed packages,
review declared origins and capabilities, install, enable, and sign in. A
listing installs nothing. Every first install needs a user action and installs
disabled.

The host installs an update of a source from the official registry without a
user action when all of these are true: the user installed the source, the
source is enabled, no pin holds it, the update adds no origins and no
capabilities, and the last attempt at that version did not fail. The check
runs after each registry refresh poll. All other updates wait for the user.

## Consequences

- A new user can install a Streaming Source without a registry URL.
- Source packages keep an independent repository, review and release cycle.
  An OpenKara release never contains music-service code.
- A fix to an official source reaches users within one refresh interval.
- An update that widens access still needs consent, from any registry.
- Sources from added registries never update without a user action.
- Builds without the `streaming-sources` feature, including App Store builds,
  contain no registry address and no source host.
