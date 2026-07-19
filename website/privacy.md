---
title: Privacy Policy
description: Privacy Policy for the OpenKara website and desktop application.
---

# Privacy Policy

This policy explains what data the OpenKara website and desktop app use, where that data is processed, and how users can remove it.

**Last updated:** April 21, 2026

## Scope

This Privacy Policy applies to the OpenKara website and the OpenKara desktop application. OpenKara is an open-source karaoke app that helps users prepare and play karaoke tracks from music they already own.

The OpenKara project does not operate a hosted backend service for the core desktop workflow. Most processing happens directly on the user's device.

## Project Operator

OpenKara is maintained as an open-source software project and website. Unless a separate commercial entity or support agreement is expressly identified for a given distribution channel, references in this policy to "OpenKara," "we," or "the project" mean the maintainers of the OpenKara project and the public website at [thedavidweng.github.io/OpenKara/](https://thedavidweng.github.io/OpenKara/).

## Website Data

The public OpenKara website is used to describe the project, provide downloads, and link to source code and documentation.

The project itself does not intentionally collect personal data through the website beyond what is normally included in standard web server logs or analytics provided by the site host. If the site is hosted by a third party such as GitHub Pages, that host may process technical information like IP address, browser information, or access logs under the host's own policies.

## Data We Do Not Intentionally Collect

OpenKara does not intentionally collect advertising identifiers, behavioral profiles, contact lists, precise location, payment card details, or the contents of a user's music library through an OpenKara-operated backend for the normal desktop workflow.

If a user chooses to interact with a third-party service through OpenKara, any information transferred to that provider is governed by the provider's own policies and by the permissions the user grants.

## Desktop App Data

OpenKara stores karaoke library data locally on the user's device. This may include song metadata, lyrics, cached stems, artwork, local configuration, and remote-library working copies.

OpenKara accesses local files only to provide app features requested by the user, such as importing songs, reading metadata, rendering lyrics, creating karaoke stems, or syncing a user-selected remote library.

## Cloud Provider Data

When a user connects Google Drive, Dropbox, WebDAV, or another remote storage provider supported by OpenKara, the app accesses only the data required to browse, create, read, update, sync, or remove files that belong to that OpenKara remote library.

For OAuth-based providers, OpenKara may receive account-identifying information such as an email address or stable account identifier so the connected account can be distinguished inside the app.

OpenKara does not use provider data for advertising, profiling, resale, or unrelated analytics.

If OpenKara receives information from Google APIs, OpenKara's use and transfer of that information to any other app will comply with the Google API Services User Data Policy, including its Limited Use requirements.

## Credential Storage

OpenKara stores sensitive remote-library credentials on the user's device using the operating system credential store where available, such as macOS Keychain, Windows Credential Manager, or the Linux system keyring.

Non-sensitive library registration metadata may be stored in the app's local configuration files so the user can reconnect to the same local or remote library later.

The official OAuth client metadata bundled with a desktop release may identify the OpenKara application itself, but it is separate from each user's own tokens. User-specific refresh and access tokens are intended to stay on that user's device in OS-managed secure storage.

## Lyrics And Network Requests

OpenKara may make network requests to fetch timed lyrics or to access remote storage providers that the user explicitly connects. Those requests are made only to deliver user-requested app features.

Music separation, local playback, and local library management do not require sending the user's songs to an OpenKara-operated server.

## Security

OpenKara is designed so that core processing happens on the user's own device, and sensitive remote-library credentials are stored using platform security features where available. However, no software or storage mechanism can guarantee absolute security.

Users remain responsible for protecting their devices, operating system accounts, backups, and any third-party cloud accounts they connect to OpenKara.

## Sharing

The OpenKara project does not sell or broker user data. User files, lyrics caches, and remote-library credentials are not sent to an OpenKara-operated backend because the project does not run one for the core desktop app.

Data may be transferred directly between the user's device and the third-party services the user chooses to use, such as Google Drive, Dropbox, or a WebDAV server.

## Retention And Deletion

Users control retention by deleting local library data, removing app configuration, or disconnecting a remote library inside OpenKara.

Users can also revoke OAuth access from the relevant provider account settings, which invalidates the app's access tokens.

Because the project does not run a central account system for the core desktop workflow, data deletion usually means removing local app data, deleting synchronized files from the user's chosen storage provider, and revoking provider access where applicable.

## Children's Privacy

OpenKara is not specifically directed to children. If a user is not legally permitted to authorize third-party services or manage content rights in their jurisdiction, they should use OpenKara only with appropriate permission from a parent, guardian, or other authorized adult.

## Changes To This Policy

This Privacy Policy may be updated as OpenKara's website, cloud integrations, or compliance obligations change. The latest published version on the OpenKara website is the current version for new uses of the website and app.

## Open Source

OpenKara is distributed as open-source software. The source code is publicly available, and users can review how local data handling, credential storage, and remote-library syncing work in the repository.

## Contact

For project questions, bug reports, or privacy-related concerns, use the GitHub repository issue tracker or contact channel published from the OpenKara project homepage and repository.
