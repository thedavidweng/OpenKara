# Windows desktop end-to-end tests

This directory defines Windows desktop E2E scenarios for the installed
`OpenKara.exe`. CI drives them with Windows UI Automation through
`scripts/ci/run-windows-desktop-e2e.ps1` and
`tools/windows-accessibility/OpenKara.AccessibilityProbe`.

## Files

- `scenarios.json` — scenario definitions (`keyboard-workflow`,
  `installed-workflow`, `multi-window-and-dialogs`)
- `README.md` — this file

## CI usage

The reusable Windows installed-app workflow seeds `OPENKARA_APP_DATA_DIR` from
the automation driver smoke tree (managed runtime and model), then runs:

```powershell
scripts/ci/run-windows-desktop-e2e.ps1 `
  -InstallDir $env:OPENKARA_WINDOWS_INSTALL_DIR `
  -Scenario "keyboard-workflow" `
  -OutputDir "$env:RUNNER_TEMP\desktop-e2e" `
  -ProbePath $probePath `
  -StepTimeoutMs 30000
```

The script exits non-zero when any step assertion fails. The validator
`scripts/validate-desktop-e2e-report.mjs` re-checks the report for release and
PR gates.

## Local usage

```powershell
$env:OPENKARA_APP_DATA_DIR = "C:\Path\To\Seeded\AppData"
pwsh scripts/ci/run-windows-desktop-e2e.ps1 `
  -InstallDir "C:\Path\To\OpenKara" `
  -Scenario "keyboard-workflow" `
  -OutputDir "C:\Path\To\Output"
```

`OPENKARA_APP_DATA_DIR` is only honored by builds that include the
`automation-smoke` feature (CI and release candidates).
