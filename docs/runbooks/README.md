# Operational Runbooks

This directory holds incident playbooks and post-deployment verification guides for OpenKara.

## Playbooks

- [CI and Nightly Incidents](ci-and-nightly-incidents.md): triage failed `main` pushes, red Nightly runs, flaky Playwright smoke tests, and live Source Registry checks.
- [Release Verification and Rollback](release-and-post-deploy.md): verify live GitHub Releases, updater feeds, website deployments, package channels, and execute release or catalog rollbacks.
- [Security and Dependency Incidents](security-incidents.md): handle private vulnerability disclosures, `cargo deny` advisories, `pnpm audit` alerts, and CodeQL findings.

## Live Operational Surfaces

| Surface                                 | Inspection command or URL                                                    |
| --------------------------------------- | ---------------------------------------------------------------------------- |
| Active CI and Nightly alerts            | `gh issue list --label bug --search "CI failure on main/nightly in:title"`   |
| Active release cut alerts               | `gh issue list --search "Release cut failed for in:title"`                   |
| Latest desktop release and updater feed | `https://github.com/thedavidweng/OpenKara/releases/latest` and `latest.json` |
| Rolling Nightly prerelease              | `https://github.com/thedavidweng/OpenKara/releases/tag/nightly`              |
| Website deployment (`github-pages`)     | `gh api repos/thedavidweng/OpenKara/deployments?environment=github-pages`    |
| Workflow run history                    | `gh run list --limit 20`                                                     |
