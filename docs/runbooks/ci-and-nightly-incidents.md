# Runbook: CI and Nightly Incidents

Use this playbook when a `main` push or Nightly workflow fails.

## Alert Trigger

A red `main` push or `schedule` run opens or updates the GitHub issue `CI failure on main/nightly` with the `bug` label. Pull request failures do not update that issue.

## Triage Steps

1. Inspect the open tracking issue and read the latest comment:
   ```bash
   gh issue list --state open --search "CI failure on main/nightly in:title" --json number,title,url
   ```
2. List recent workflow runs on `main`:
   ```bash
   gh run list --branch main --limit 10
   ```
3. View failed job logs for the run ID from the issue comment:
   ```bash
   gh run view <run-id> --log-failed
   ```
4. Check whether every operating system failed on the same step. A shared step failure usually indicates a repository-wide formatting, lockfile, or schema drift error rather than a platform defect.

## Common Failure Modes

### Shared Formatting or Drift Failure

Run the local checks from the repository root:

```bash
pnpm format
pnpm lint
pnpm check:duplication
pnpm check:standards
pnpm generate:db-schema && git diff --exit-code docs/references/generated/db-schema.md
```

### Flaky Playwright UI Smoke

Download the `playwright-report` artifact and inspect the step summary:

```bash
gh run download <run-id> --name playwright-report --dir /tmp/playwright-report
```

Fix the root race condition (for example, dialog focus readiness or animation timing) rather than raising retry counts.

### Windows Installed-App or ONNX Runtime Failure

Check two constraints first:

- Every Windows compile job must run `node scripts/prepare-onnx-runtime.mjs --target x86_64-pc-windows-msvc` before `cargo` runs, because Tauri checks `bundle.resources` at compile time.
- Headless Windows Server runners cannot execute desktop UIA binaries outside `windows-installed-app-smoke-test.yml` and `nightly-hardening.yml`.

### Live Source Registry Gate Failure

If `Accept live registry streaming source on Windows` fails when `OPENKARA_LIVE_REGISTRY_URL` is set, verify whether the external Source Registry HTTPS endpoint, package SHA-512 seal, or protocol major version changed.

## Resolution

1. Land the fix on `main` through a pull request with passing checks.
2. Dispatch the failed workflow on `main` when the failure occurred in `nightly-hardening.yml`:
   ```bash
   gh workflow run nightly-hardening.yml --ref main
   ```
3. Confirm the new run succeeds, record the green run URL on the tracking issue, and close the issue.
