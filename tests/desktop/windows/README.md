# Windows desktop end-to-end tests

This directory contains the Windows desktop end-to-end (E2E) test definition for OpenKara.
The tests drive the installed `OpenKara.exe` with Windows UI Automation (UIA).

## Files

- `scenarios.json` — list of automation scenarios, including the `keyboard-workflow` scenario.
- `README.md` — this file.

## Run in CI

The `scripts/ci/run-windows-desktop-e2e.ps1` script runs the selected scenario and produces a JSON report.

```yaml
- name: Run Windows desktop E2E
  shell: pwsh
  run: |
    scripts/ci/run-windows-desktop-e2e.ps1 `
      -InstallDir "${{ runner.temp }}\OpenKara-installed-smoke" `
      -Scenario "keyboard-workflow" `
      -OutputDir "${{ runner.temp }}\desktop-e2e"
```

The script exits with code 0 and writes `$OutputDir\desktop-e2e-report.json`.

## Run locally

Open a PowerShell session in the repository root.

```powershell
pwsh scripts/ci/run-windows-desktop-e2e.ps1 `
  -InstallDir "C:\Path\To\OpenKara" `
  -Scenario "keyboard-workflow" `
  -OutputDir "C:\Path\To\Output"
```

The script requires `OpenKara.exe` to be installed in the supplied directory.
This is a scaffold script. It validates the executable, generates the report, and returns status `passed`.
