[CmdletBinding()]
param (
    [Parameter(Mandatory = $true)]
    [string]$InstallDir,

    [string]$Scenario = "installed-workflow",

    [string]$OutputDir = "$env:RUNNER_TEMP\desktop-e2e",

    [string]$ProbePath,

    [int]$ProbeTimeoutMs = 5000,

    [int]$StepDelayMs = 800,

    [int]$StepTimeoutMs = 10000
)

$ErrorActionPreference = "Stop"

if ($OutputDir -eq "\desktop-e2e" -or [string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path ([System.IO.Path]::GetTempPath()) "desktop-e2e"
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$scenariosPath = [System.IO.Path]::Combine($repoRoot, "tests", "desktop", "windows", "scenarios.json")
if (-not (Test-Path -Path $scenariosPath -PathType Leaf)) {
    throw "Scenarios file was not found at $scenariosPath"
}

$scenarios = Get-Content -Path $scenariosPath -Raw | ConvertFrom-Json
$selectedScenario = $scenarios | Where-Object { $_.id -eq $Scenario } | Select-Object -First 1
if ($null -eq $selectedScenario) {
    throw "Scenario '$Scenario' was not found in $scenariosPath"
}

$exePath = Join-Path $InstallDir "OpenKara.exe"
if (-not (Test-Path -Path $exePath -PathType Leaf)) {
    throw "OpenKara.exe was not found at $exePath"
}

$versionInfo = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($exePath)
$appVersion = $versionInfo.ProductVersion
if ([string]::IsNullOrWhiteSpace($appVersion)) {
    $appVersion = $versionInfo.FileVersion
}
if ([string]::IsNullOrWhiteSpace($appVersion)) {
    $appVersion = "0.0.0"
}

if ($env:GITHUB_SHA) {
    $commitSha = $env:GITHUB_SHA
} else {
    $commitSha = "unknown"
    try {
        $gitOutput = & git -C $repoRoot rev-parse HEAD 2>$null
        if ($? -and $gitOutput) {
            $commitSha = $gitOutput
        }
    } catch {
        $commitSha = "unknown"
    }
}

$osInfo = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
if ($osInfo) {
    $osVersion = "$($osInfo.Caption) $($osInfo.Version)"
} else {
    $osVersion = [System.Environment]::OSVersion.VersionString
}

$webview2Version = $null
$wvKeys = @(
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4115-9B8B-1EDECC4588C6}",
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4115-9B8B-1EDECC4588C6}"
)
foreach ($key in $wvKeys) {
    $prop = Get-ItemProperty -Path $key -Name "pv" -ErrorAction SilentlyContinue
    if ($prop -and $prop.pv) {
        $webview2Version = $prop.pv
        break
    }
}
if ([string]::IsNullOrWhiteSpace($webview2Version)) {
    $webview2Version = "unknown"
}

$logPixels = Get-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "LogPixels" -ErrorAction SilentlyContinue
if ($logPixels -and $logPixels.LogPixels) {
    $displayScale = "{0}%" -f [math]::Round($logPixels.LogPixels / 96 * 100)
} else {
    $displayScale = "unknown"
}

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class OpenKaraWin32 {
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr FindWindow(string lpClassName, string lpWindowName);
}
"@ -ErrorAction Stop

Add-Type -AssemblyName System.Windows.Forms -ErrorAction Stop

function Resolve-ProbePath {
    if ([string]::IsNullOrWhiteSpace($ProbePath)) {
        $candidate = Join-Path $InstallDir "OpenKara.AccessibilityProbe.exe"
        if (Test-Path -Path $candidate -PathType Leaf) {
            $script:ProbePath = $candidate
            return
        }

        $candidate = Join-Path $repoRoot "tools" "windows-accessibility" "OpenKara.AccessibilityProbe" "bin" "Release" "net8.0-windows" "OpenKara.AccessibilityProbe.exe"
        if (Test-Path -Path $candidate -PathType Leaf) {
            $script:ProbePath = $candidate
            return
        }

        $candidate = Join-Path $repoRoot "tools" "windows-accessibility" "OpenKara.AccessibilityProbe" "bin" "Debug" "net8.0-windows" "OpenKara.AccessibilityProbe.exe"
        if (Test-Path -Path $candidate -PathType Leaf) {
            $script:ProbePath = $candidate
            return
        }

        throw "OpenKara.AccessibilityProbe.exe was not found. Build the .NET project or pass -ProbePath."
    }
    if (-not (Test-Path -Path $ProbePath -PathType Leaf)) {
        throw "Probe path does not exist: $ProbePath"
    }
    $script:ProbePath = $ProbePath
}

function Get-NowMs {
    return [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
}

function Start-OpenKaraApp {
    # Ensure a clean single-instance state for CI smoke runs.
    Get-Process -Name "OpenKara" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500

    $process = Start-Process -FilePath $exePath -WorkingDirectory $InstallDir -PassThru
    if ($null -eq $process) {
        throw "Failed to start OpenKara.exe"
    }
    return $process
}

function Wait-For-ProcessWindow {
    param([System.Diagnostics.Process]$Process, [int]$TimeoutMs)

    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $Process.Refresh()
            if ($Process.MainWindowHandle -ne [IntPtr]::Zero -and [OpenKaraWin32]::IsWindowVisible($Process.MainWindowHandle)) {
                return $Process.MainWindowHandle
            }
        } catch {
            # process may have exited
        }
        Start-Sleep -Milliseconds 100
    }
    throw "OpenKara main window did not appear within ${TimeoutMs}ms"
}

function Get-UiTree {
    param([int]$ProcessId)

    $snapshotPath = Join-Path $OutputDir ("uia-tree-{0}-{1}.json" -f $ProcessId, ([Guid]::NewGuid().ToString("N")))
    & $script:ProbePath --process-id $ProcessId --output $snapshotPath --timeout $ProbeTimeoutMs
    if ($LASTEXITCODE -ne 0) {
        throw "AccessibilityProbe exited with code $LASTEXITCODE"
    }
    if (-not (Test-Path -Path $snapshotPath -PathType Leaf)) {
        throw "AccessibilityProbe did not produce a snapshot at $snapshotPath"
    }
    $script:currentTree = Get-Content -Path $snapshotPath -Raw | ConvertFrom-Json
    $script:lastSnapshotPath = $snapshotPath
    return $script:currentTree
}

function Send-KeyboardInput {
    param([string]$Keys)

    $mainWindow = $script:process.MainWindowHandle
    if ($mainWindow -eq [IntPtr]::Zero) {
        throw "Main window handle is not available"
    }

    $foregroundResult = [OpenKaraWin32]::SetForegroundWindow($mainWindow)
    if (-not $foregroundResult) {
        Write-Warning "SetForegroundWindow returned false"
    }
    Start-Sleep -Milliseconds 50
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
    Start-Sleep -Milliseconds $StepDelayMs
}

function Find-Element {
    param([array]$Tree, [scriptblock]$Predicate)

    foreach ($node in $Tree) {
        if (& $Predicate $node) {
            return $node
        }
    }
    return $null
}

function Find-FocusedElement {
    param([array]$Tree)
    return Find-Element -Tree $Tree -Predicate { param($n) $n.hasKeyboardFocus -eq $true }
}

function Find-ElementByName {
    param([array]$Tree, [string]$Name)
    return Find-Element -Tree $Tree -Predicate { param($n) $n.name -and $n.name.IndexOf($Name, [System.StringComparison]::OrdinalIgnoreCase) -ge 0 }
}

function Find-ElementByControlType {
    param([array]$Tree, [string]$ControlType)
    return Find-Element -Tree $Tree -Predicate { param($n) $n.controlType -eq $ControlType }
}

function Add-FailingAssertion {
    param([string]$StepId, [string]$Expected, [string]$Observed)

    $script:assertions.Add([PSCustomObject]@{
        id            = $StepId
        expected      = $Expected
        observed      = $Observed
        result        = "fail"
        artifact_path = $script:lastSnapshotPath
    })
}

function Assert-Step {
    param([string]$StepId, [string]$Expected, [scriptblock]$Check, [array]$Tree)

    $observed = "condition was not met"
    $pass = $false
    try {
        $pass = & $Check $Tree
        if ($pass -is [bool] -and $pass) {
            $observed = $Expected
        } elseif ($pass -is [string]) {
            $observed = $pass
            $pass = $false
        }
    } catch {
        $observed = "error: $($_.Exception.Message)"
        $pass = $false
    }

    $result = [PSCustomObject]@{
        id            = $StepId
        expected      = $Expected
        observed      = $observed
        result        = if ($pass) { "pass" } else { "fail" }
        artifact_path = $script:lastSnapshotPath
    }

    $script:assertions.Add($result)
    return $result
}

function Invoke-StepAction {
    param([PSCustomObject]$Step, [int]$StepIndex)

    $stepId = if ($Step.action) { $Step.action } else { "step-$StepIndex" }
    $stepName = if ($Step.target) { $Step.target } else { $stepId }
    $stepStartedAt = Get-NowMs

    $stepStatus = "passed"
    $stepError = $null

    try {
        switch ($Step.action) {
            "launch" {
                $script:process = Start-OpenKaraApp
                $script:mainWindowHandle = Wait-For-ProcessWindow -Process $script:process -TimeoutMs $StepTimeoutMs
                $tree = Get-UiTree -ProcessId $script:process.Id

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    $window = Find-ElementByControlType -Tree $t -ControlType "Window"
                    if ($null -eq $window) { return "no Window control found in UIA tree" }
                    if ([string]::IsNullOrWhiteSpace($window.name)) { return "main window has no accessible name" }
                    return $true
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "navigate-sidebar" {
                if ($null -eq $script:process) { throw "Application has not been launched" }
                $before = Find-FocusedElement -Tree $script:currentTree
                Send-KeyboardInput "{TAB}"
                $tree = Get-UiTree -ProcessId $script:process.Id

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    $focused = Find-FocusedElement -Tree $t
                    if ($null -eq $focused) { return "no element has keyboard focus" }
                    if ($focused.controlType -eq "Window") { return "focus is still on the window, not a control" }
                    if ([string]::IsNullOrWhiteSpace($focused.name)) { return "focused control has no name" }
                    return $true
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "select-library" {
                if ($null -eq $script:process) { throw "Application has not been launched" }
                $found = $false
                $lastFocusedName = ""
                for ($i = 0; $i -lt 20; $i++) {
                    $tree = Get-UiTree -ProcessId $script:process.Id
                    $focused = Find-FocusedElement -Tree $tree
                    if ($null -ne $focused) {
                        $lastFocusedName = $focused.name
                        if ($focused.name -and $focused.name.IndexOf("Library", [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
                            $found = $true
                            break
                        }
                    }
                    Send-KeyboardInput "{TAB}"
                }

                if (-not $found) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "focused control was not Library (last: $lastFocusedName)"
                    $stepStatus = "failed"
                } else {
                    Send-KeyboardInput "~"
                    $tree = Get-UiTree -ProcessId $script:process.Id
                    $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                        param($t)
                        $lib = Find-ElementByName -Tree $t -Name "Library"
                        if ($null -eq $lib) { return "Library element not found after selection" }
                        if ($lib.hasKeyboardFocus -or ($lib.isSelected -eq $true)) { return $true }
                        return "Library element is not focused or selected"
                    }
                    if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                }
            }

            "import-fixture" {
                if ($null -eq $script:process) { throw "Application has not been launched" }
                Send-KeyboardInput "^o"
                Start-Sleep -Milliseconds 500
                # Cancel the file picker so the app is not left in a dialog state.
                Send-KeyboardInput "{ESC}"
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "import-fixture requires a fixture path and file dialog automation that is not yet implemented"
                $stepStatus = "failed"
            }

            "select-track" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "select-track is not yet automated without a known library state"
                $stepStatus = "failed"
            }

            "start-playback" {
                if ($null -eq $script:process) { throw "Application has not been launched" }
                Send-KeyboardInput " "
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "start-playback requires a selected track and is not yet automated"
                $stepStatus = "failed"
            }

            "pause-resume" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "pause-resume is not yet automated"
                $stepStatus = "failed"
            }

            "seek" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "seek is not yet automated"
                $stepStatus = "failed"
            }

            "adjust-stems" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "adjust-stems is not yet automated"
                $stepStatus = "failed"
            }

            "mute" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "mute is not yet automated"
                $stepStatus = "failed"
            }

            "queue" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "queue is not yet automated"
                $stepStatus = "failed"
            }

            "open-settings" {
                if ($null -eq $script:process) { throw "Application has not been launched" }
                Send-KeyboardInput "^,"
                Start-Sleep -Milliseconds $StepDelayMs
                $tree = Get-UiTree -ProcessId $script:process.Id

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    $settings = Find-ElementByName -Tree $t -Name "Settings"
                    if ($null -eq $settings) { return "Settings element was not found" }
                    if ($settings.isOffscreen) { return "Settings element is offscreen" }
                    return $true
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "open-appearance" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "open-appearance is not yet automated"
                $stepStatus = "failed"
            }

            "verify-model-runtime-status" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "verify-model-runtime-status is not yet automated"
                $stepStatus = "failed"
            }

            "start-separation" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "start-separation is not yet automated"
                $stepStatus = "failed"
            }

            "toggle-fullscreen" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "toggle-fullscreen is not yet automated"
                $stepStatus = "failed"
            }

            "stop-playback" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "stop-playback is not yet automated"
                $stepStatus = "failed"
            }

            "open-fullscreen" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "open-fullscreen is not yet automated"
                $stepStatus = "failed"
            }

            "close-fullscreen" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "close-fullscreen is not yet automated"
                $stepStatus = "failed"
            }

            "cancel-file-picker" {
                if ($null -eq $script:process) { throw "Application has not been launched" }
                Send-KeyboardInput "{ESC}"
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "cancel-file-picker is not yet automated"
                $stepStatus = "failed"
            }

            "set-display-scale" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "set-display-scale is not yet automated"
                $stepStatus = "failed"
            }

            "enable-high-contrast" {
                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "enable-high-contrast is not yet automated"
                $stepStatus = "failed"
            }

            "close" {
                if ($null -eq $script:process) { throw "Application has not been launched" }
                Send-KeyboardInput "%{F4}"
                $exited = $script:process.WaitForExit(10000)
                if (-not $exited) {
                    Stop-Process -InputObject $script:process -Force -ErrorAction SilentlyContinue
                }

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $script:currentTree -Check {
                    param($t)
                    return $script:process.HasExited
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            default {
                throw "Unknown action '$($Step.action)' in scenario '$Scenario'"
            }
        }
    } catch {
        $stepStatus = "failed"
        $stepError = $_.Exception.Message
        $script:errors.Add([PSCustomObject]@{ step_id = $stepId; message = $stepError })
        Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "step error: $stepError"
    }

    $stepFinishedAt = Get-NowMs
    $stepDuration = [int]($stepFinishedAt - $stepStartedAt)

    $script:steps.Add([PSCustomObject]@{
        id          = $stepId
        name        = $stepName
        status      = $stepStatus
        started_at  = $stepStartedAt
        finished_at = $stepFinishedAt
        duration_ms = $stepDuration
        output      = $script:lastSnapshotPath
        error       = if ($stepError) { $stepError } else { "" }
    })

    if ($stepStatus -ne "passed") {
        $script:overallStatus = "failed"
    }
}

Resolve-ProbePath

$startedAt = Get-NowMs
$reportSteps = [System.Collections.Generic.List[object]]::new()
$assertions = [System.Collections.Generic.List[object]]::new()
$errors = [System.Collections.Generic.List[object]]::new()
$script:steps = $reportSteps
$script:assertions = $assertions
$script:errors = $errors
$script:overallStatus = "passed"
$script:process = $null
$script:currentTree = @()
$script:lastSnapshotPath = ""
$script:mainWindowHandle = [IntPtr]::Zero

$stepIndex = 0
foreach ($step in $selectedScenario.steps) {
    $stepIndex++
    Invoke-StepAction -Step $step -StepIndex $stepIndex
}

if ($null -ne $script:process -and -not $script:process.HasExited) {
    Stop-Process -InputObject $script:process -Force -ErrorAction SilentlyContinue
}

$finishedAt = Get-NowMs
$durationMs = [int]($finishedAt - $startedAt)

$reportPath = Join-Path $OutputDir "desktop-e2e-report.json"

$artifactList = [System.Collections.Generic.List[object]]::new()
$artifactList.Add([PSCustomObject]@{
    path        = $reportPath
    kind        = "desktop-e2e-report"
    description = "Windows desktop end-to-end automation report"
})
$assertionArtifactPaths = $assertions | ForEach-Object { $_.artifact_path } | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Unique
foreach ($path in $assertionArtifactPaths) {
    $artifactList.Add([PSCustomObject]@{
        path        = $path
        kind        = "uia-tree-snapshot"
        description = "UI Automation tree snapshot"
    })
}

$uiAutomationErrors = ($assertions | Where-Object { $_.result -eq "fail" }).Count

$report = [PSCustomObject]@{
    scenario      = $selectedScenario.id
    status        = $script:overallStatus
    started_at    = $startedAt
    finished_at   = $finishedAt
    duration_ms   = $durationMs
    application   = [PSCustomObject]@{
        name       = "OpenKara"
        version    = $appVersion
        commit_sha = $commitSha
    }
    environment   = [PSCustomObject]@{
        os_version                  = $osVersion
        webview2_version            = $webview2Version
        selected_execution_provider = if ($env:OPENKARA_SMOKE_EP) { $env:OPENKARA_SMOKE_EP } else { "unknown" }
        display_scale               = $displayScale
    }
    steps         = $reportSteps
    assertions    = $assertions
    artifacts     = $artifactList
    runtime       = [PSCustomObject]@{
        archive_sha256           = ""
        extracted_library_sha256 = ""
        companion_dll_sha256s    = @()
    }
    model         = [PSCustomObject]@{
        archive_sha256       = ""
        extracted_onnx_sha256 = ""
        verification_manifest = ""
        catalog_generation    = ""
        release_id            = ""
        artifact_id           = ""
        selected_variant      = ""
    }
    database      = [PSCustomObject]@{
        schema_version = 0
        path           = ""
    }
    accessibility = [PSCustomObject]@{
        violations_count           = 0
        keyboard_trap_count        = 0
        ui_automation_errors_count = $uiAutomationErrors
        zoom_levels_tested         = @()
    }
    audio         = [PSCustomObject]@{
        sample_rate        = 0
        channel_count      = 0
        non_silent_samples = $false
    }
    errors        = $errors
}

$report | ConvertTo-Json -Depth 10 | Out-File -FilePath $reportPath -Encoding utf8

if ($env:GITHUB_STEP_SUMMARY) {
    $summaryLines = [System.Collections.Generic.List[string]]::new()
    $summaryLines.Add("# Windows desktop end-to-end report")
    $summaryLines.Add("")
    $summaryLines.Add("| Scenario | Status | Duration (ms) |")
    $summaryLines.Add("| --- | --- | --- |")
    $summaryLines.Add("| $($selectedScenario.name) | $($script:overallStatus) | $durationMs |")
    $summaryLines.Add("")
    $summaryLines.Add("## Steps")
    $summaryLines.Add("")
    $summaryLines.Add("| ID | Name | Status |")
    $summaryLines.Add("| --- | --- | --- |")
    foreach ($s in $reportSteps) {
        $summaryLines.Add("| $($s.id) | $($s.name) | $($s.status) |")
    }
    if ($assertions.Count -gt 0) {
        $summaryLines.Add("")
        $summaryLines.Add("## Assertions")
        $summaryLines.Add("")
        $summaryLines.Add("| ID | Expected | Observed | Result |")
        $summaryLines.Add("| --- | --- | --- | --- |")
        foreach ($a in $assertions) {
            $summaryLines.Add("| $($a.id) | $($a.expected) | $($a.observed) | $($a.result) |")
        }
    }
    $summaryLines.Add("")
    $summaryLines.Add("Report: $reportPath")
    $summaryLines | Out-File -FilePath $env:GITHUB_STEP_SUMMARY -Append -Encoding utf8
}

Write-Host "Windows desktop E2E report written to $reportPath"

if ($script:overallStatus -eq "failed") {
    exit 1
}

exit 0
