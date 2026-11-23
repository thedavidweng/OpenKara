# Runbook: Security and Dependency Incidents

Use this playbook when a private vulnerability report arrives or when automated security gates fail.

## Intake and Disclosure Policy

Follow [SECURITY.md](../../SECURITY.md) for severity ratings (CVSS v3.1), response SLAs, and the 90-day coordinated disclosure window. Keep unpatched vulnerability details inside the GitHub Security Advisory draft until a release ships.

## Automated Security Gate Failures

### Rust Advisory (`cargo deny` / `RUSTSEC-*`)

Run each command from the repository root:

1. Reproduce locally:
   ```bash
   (cd src-tauri && cargo deny check advisories)
   ```
2. Check whether a patched crate version exists and update it:
   ```bash
   (cd src-tauri && cargo update -p <crate>)
   ```
3. Regenerate offline Flatpak Cargo sources whenever `src-tauri/Cargo.lock` changes:
   ```bash
   pnpm generate:flatpak-cargo-sources
   ```
4. Run `(cd src-tauri && cargo test -q)` before opening the fix pull request.

### JavaScript Advisory (`pnpm audit`)

1. Inspect the advisory path and whether it reaches shipped application dependencies or dev-only tooling:
   ```bash
   pnpm audit --audit-level=high
   ```
2. Update the affected dependency or add a scoped override in `pnpm-workspace.yaml`.
3. Regenerate offline Flatpak Node sources whenever `pnpm-lock.yaml` changes:
   ```bash
   pnpm generate:flatpak-node-sources
   ```
4. Run `pnpm lint`, `pnpm build`, and `pnpm test`.

### CodeQL Alert (`codeql.yml`)

1. Inspect all open code-scanning alerts across every API page:
   ```bash
   gh api --paginate repos/thedavidweng/OpenKara/code-scanning/alerts \
     --jq '.[] | select(.state=="open") | {number, rule: .rule.id, path: .most_recent_instance.location.path}'
   ```
2. Check `.agents/skills/ci-notes/SKILL.md` for known CodeQL patterns (for example, format `reqwest::Error` with `error.without_url()` so OAuth query tokens never reach logs).
3. Fix real data-flow findings in code rather than suppressing rules globally.
