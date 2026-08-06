# ADR 0021 — App language is sticky after choice

Date: 2026-08-06
Status: accepted

## Context

OpenKara recommends a UI language from the host locale during first-run setup.
Users can also change language in Settings. If user-visible copy later followed
the OS locale again, labels and errors could diverge from the language the user
chose. That mismatch is hard to diagnose and fails the product rule that UI
language stays consistent.

## Decision

Store an explicit app language after OOBE or Settings choose one. Resolve every
user-visible string (labels, errors, menus, progress) from that stored code
through i18next. Use the host locale only when no app language is stored yet.
Do not re-bind UI copy to later OS locale changes while a language remains set.
OOBE must persist the chosen language when setup finishes so a transient IPC
failure does not leave language unset.

## Consequences

- `resolveAppLanguage` prefers a non-empty stored code and falls back to the
  system recommend only for the unset case.
- Settings shows the resolved active language. Changing language always writes
  a concrete code.
- Backend technical error details may stay English for support. Localized
  titles and recovery text always use the app language.
- Contracts describe `language: null` as “not chosen yet”, not “follow the OS
  forever”.
