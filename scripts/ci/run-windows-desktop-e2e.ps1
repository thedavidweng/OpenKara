[CmdletBinding()]
param (
    [Parameter(Mandatory = $true)]
    [string]$InstallDir,

    [string]$Scenario = "keyboard-workflow",

    [string]$OutputDir = "$env:RUNNER_TEMP\desktop-e2e",

    [string]$ProbePath,

    [int]$ProbeTimeoutMs = 5000,

    [int]$StepDelayMs = 800,

    # Keyboard-workflow separation and multi-step tab searches need more headroom
    # than the short installed-workflow launch/close smoke.
    [int]$StepTimeoutMs = 30000
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
using System.Text;
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

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);

    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);

    public static IntPtr FindWindowByTitle(int processId, string title) {
        if (string.IsNullOrWhiteSpace(title)) return IntPtr.Zero;
        IntPtr found = IntPtr.Zero;
        string lowerTitle = title.ToLowerInvariant();
        EnumWindows((hWnd, lParam) => {
            if (!IsWindowVisible(hWnd)) return true;
            if (GetWindowThreadProcessId(hWnd, out uint pid) == 0 || (int)pid != processId) return true;
            StringBuilder sb = new StringBuilder(512);
            if (GetWindowText(hWnd, sb, sb.Capacity) > 0) {
                if (sb.ToString().ToLowerInvariant().Equals(lowerTitle, StringComparison.Ordinal)) {
                    found = hWnd;
                    return false;
                }
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static IntPtr FindFirstTopLevelWindow(int processId, string[] titles) {
        if (titles is null || titles.Length == 0) return IntPtr.Zero;
        foreach (string title in titles) {
            IntPtr hWnd = FindWindowByTitle(processId, title);
            if (hWnd != IntPtr.Zero) return hWnd;
        }
        return IntPtr.Zero;
    }
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
    Get-Process -Name "OpenKara" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500

    # Pass OPENKARA_APP_DATA_DIR explicitly so UI Automation reuses the seeded
    # smoke app-data (managed runtime/model) instead of a fresh user profile.
    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $exePath
    $startInfo.WorkingDirectory = $InstallDir
    $startInfo.UseShellExecute = $false
    if (-not [string]::IsNullOrWhiteSpace($env:OPENKARA_APP_DATA_DIR)) {
        $startInfo.EnvironmentVariables["OPENKARA_APP_DATA_DIR"] = $env:OPENKARA_APP_DATA_DIR
        Write-Host "Launching OpenKara with OPENKARA_APP_DATA_DIR=$($env:OPENKARA_APP_DATA_DIR)"
    }
    if (-not [string]::IsNullOrWhiteSpace($env:OPENKARA_SMOKE_EP)) {
        $startInfo.EnvironmentVariables["OPENKARA_SMOKE_EP"] = $env:OPENKARA_SMOKE_EP
    }

    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
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
        }
        Start-Sleep -Milliseconds 100
    }
    throw "OpenKara main window did not appear within ${TimeoutMs}ms"
}

function Get-UiTree {
    param([int]$ProcessId, [string]$WindowTitle = "", [int]$TimeoutMs = $ProbeTimeoutMs)

    $snapshotPath = Join-Path $OutputDir ("uia-tree-{0}-{1}.json" -f $ProcessId, ([Guid]::NewGuid().ToString("N")))
    $argList = @("--process-id", $ProcessId, "--output", $snapshotPath, "--timeout", $TimeoutMs)
    if (-not [string]::IsNullOrWhiteSpace($WindowTitle)) {
        $argList += @("--window-title", $WindowTitle)
    }

    & $script:ProbePath @argList
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
    param([string]$Keys, [IntPtr]$Handle = [IntPtr]::Zero)

    if ($Handle -eq [IntPtr]::Zero) {
        $Handle = $script:process.MainWindowHandle
    }
    if ($Handle -eq [IntPtr]::Zero) {
        throw "Main window handle is not available"
    }

    $foregroundResult = [OpenKaraWin32]::SetForegroundWindow($Handle)
    if (-not $foregroundResult) {
        Write-Warning "SetForegroundWindow returned false"
    }
    Start-Sleep -Milliseconds 100
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
    Start-Sleep -Milliseconds $StepDelayMs
}

function Send-Keys-To-Window-By-Title {
    param([string]$Title, [string]$Keys)

    $hWnd = [OpenKaraWin32]::FindWindowByTitle($script:process.Id, $Title)
    if ($hWnd -eq [IntPtr]::Zero) {
        throw "Window with title '$Title' not found"
    }
    Send-KeyboardInput -Keys $Keys -Handle $hWnd
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

function Find-ElementByNameVisible {
    param([array]$Tree, [string]$Name)
    return Find-Element -Tree $Tree -Predicate { param($n) $n.name -and $n.name.IndexOf($Name, [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -and $n.isOffscreen -eq $false }
}

function Find-ElementByControlType {
    param([array]$Tree, [string]$ControlType)
    return Find-Element -Tree $Tree -Predicate { param($n) $n.controlType -eq $ControlType }
}

function Find-Play-Pause-Button {
    param([array]$Tree)
    $byId = Find-ElementByAutomationId -Tree $Tree -AutomationId "play-pause"
    if ($null -ne $byId) { return $byId }
    return Find-Element -Tree $Tree -Predicate {
        param($n)
        $n.controlType -eq "Button" -and $n.name -and ($n.name -eq "Play" -or $n.name -eq "Pause" -or $n.name -eq "Loading")
    }
}

function Find-Mute-Button {
    param([array]$Tree)
    $master = Find-ElementByAutomationId -Tree $Tree -AutomationId "master-mute"
    if ($null -ne $master -and $master.controlType -eq "Button") { return $master }

    $candidates = $Tree | Where-Object {
        $n = $_
        $n.controlType -eq "Button" -and $n.name -and (
            $n.name.Equals("Mute", [System.StringComparison]::OrdinalIgnoreCase) -or
            $n.name.Equals("Unmute", [System.StringComparison]::OrdinalIgnoreCase) -or
            $n.name.StartsWith("Mute ", [System.StringComparison]::OrdinalIgnoreCase) -or
            $n.name.StartsWith("Unmute ", [System.StringComparison]::OrdinalIgnoreCase)
        )
    }

    $masterByName = $candidates | Where-Object { $_.name -eq "Mute" -or $_.name -eq "Unmute" } | Select-Object -First 1
    if ($null -ne $masterByName) { return $masterByName }

    return $candidates | Where-Object { $_.isEnabled -ne $false } | Select-Object -First 1
}

function Find-Queue-Button {
    param([array]$Tree)
    $byId = Find-ElementByAutomationId -Tree $Tree -AutomationId "queue-button"
    if ($null -ne $byId -and $byId.controlType -eq "Button") { return $byId }
    return Find-Element -Tree $Tree -Predicate {
        param($n)
        $n.controlType -eq "Button" -and $n.name -and $n.name.Equals("Queue", [System.StringComparison]::OrdinalIgnoreCase)
    }
}

function Find-Queue-Panel {
    param([array]$Tree)
    $byId = Find-ElementByAutomationId -Tree $Tree -AutomationId "queue-panel"
    if ($null -ne $byId) { return $byId }
    return Find-Element -Tree $Tree -Predicate {
        param($n)
        ($n.controlType -eq "Pane" -or $n.controlType -eq "Group" -or $n.controlType -eq "Window") -and
        $n.name -and $n.name.Equals("Queue", [System.StringComparison]::OrdinalIgnoreCase) -and
        $n.isOffscreen -eq $false
    }
}

function Find-Expand-Stems-Button {
    param([array]$Tree)
    return Find-Element -Tree $Tree -Predicate {
        param($n)
        $n.controlType -eq "Button" -and $n.name -and (
            $n.name.Equals("Expand stems", [System.StringComparison]::OrdinalIgnoreCase) -or
            $n.name.Equals("Collapse stems", [System.StringComparison]::OrdinalIgnoreCase)
        )
    }
}

function Find-Seek-Slider {
    param([array]$Tree)
    $byId = Find-ElementByAutomationId -Tree $Tree -AutomationId "seek-slider"
    if ($null -ne $byId -and $byId.controlType -eq "Slider") { return $byId }
    return Find-Element -Tree $Tree -Predicate {
        param($n)
        $n.controlType -eq "Slider" -and $n.name -and $n.name.Equals("Seek", [System.StringComparison]::OrdinalIgnoreCase)
    }
}

function Find-Settings-Overlay {
    param([array]$Tree)
    $byId = Find-ElementByAutomationId -Tree $Tree -AutomationId "settings-overlay"
    if ($null -ne $byId) { return $byId }
    return Find-Element -Tree $Tree -Predicate {
        param($n)
        ($n.controlType -eq "Pane" -or $n.controlType -eq "Group" -or $n.controlType -eq "Dialog" -or $n.controlType -eq "Window") -and
        $n.name -and $n.name.Equals("Preferences", [System.StringComparison]::OrdinalIgnoreCase) -and
        $n.isOffscreen -eq $false
    }
}

function Find-Track {
    param([array]$Tree, [string]$Name = "")
    return Find-Element -Tree $Tree -Predicate {
        param($n)
        ($n.controlType -eq "Button" -or $n.controlType -eq "ListItem") -and
        $n.name -and
        ($Name -eq "" -or $n.name.IndexOf($Name, [System.StringComparison]::OrdinalIgnoreCase) -ge 0)
    }
}

function Find-ElementByRegex {
    param([array]$Tree, [string]$Pattern, [string]$Property = "name")
    $regex = [System.Text.RegularExpressions.Regex]::new($Pattern, [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
    return Find-Element -Tree $Tree -Predicate { param($n) $n.$Property -and $regex.IsMatch($n.$Property) }
}

function Find-ElementByAutomationId {
    param([array]$Tree, [string]$AutomationId)
    return Find-Element -Tree $Tree -Predicate { param($n) $n.automationId -and $n.automationId -eq $AutomationId }
}

function Find-Descendants {
    param([array]$Tree, [scriptblock]$Predicate, [string]$AncestorPath = "")
    $prefix = if ([string]::IsNullOrWhiteSpace($AncestorPath)) { "" } else { "$AncestorPath/" }
    $matches = @()
    foreach ($node in $Tree) {
        if (($prefix -eq "" -or ($node.path -and $node.path.StartsWith($prefix, [System.StringComparison]::Ordinal))) -and (& $Predicate $node)) {
            $matches += $node
        }
    }
    return $matches
}

function Wait-For-Condition {
    param([scriptblock]$Condition, [int]$TimeoutMs = $StepTimeoutMs, [int]$PollMs = 500)

    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    while ([DateTime]::UtcNow -lt $deadline) {
        $tree = $null
        try {
            $tree = Get-UiTree -ProcessId $script:process.Id
        } catch {
            $tree = $null
        }
        if ($null -ne $tree -and (& $Condition $tree)) {
            return $tree
        }
        Start-Sleep -Milliseconds $PollMs
    }
    return $null
}

function Wait-For-Element {
    param([scriptblock]$Predicate, [int]$TimeoutMs = $StepTimeoutMs)
    $tree = Wait-For-Condition -Condition {
        param($t)
        return $null -ne (Find-Element -Tree $t -Predicate $Predicate)
    } -TimeoutMs $TimeoutMs
    if ($null -ne $tree) {
        return Find-Element -Tree $tree -Predicate $Predicate
    }
    return $null
}

function Wait-For-Dialog {
    param([string[]]$Titles = @(), [int]$TimeoutMs = $StepTimeoutMs)

    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    while ([DateTime]::UtcNow -lt $deadline) {
        $hWnd = [OpenKaraWin32]::FindFirstTopLevelWindow($script:process.Id, $Titles)
        if ($hWnd -ne [IntPtr]::Zero) {
            return $hWnd
        }
        Start-Sleep -Milliseconds 250
    }
    return [IntPtr]::Zero
}

function Tab-To-Element {
    param([scriptblock]$Predicate, [int]$MaxTabs = 30, [int]$TimeoutMs = $StepTimeoutMs)

    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    for ($i = 0; $i -lt $MaxTabs; $i++) {
        $tree = Get-UiTree -ProcessId $script:process.Id
        $focused = Find-FocusedElement -Tree $tree
        if ($null -ne $focused -and $focused.isOffscreen -eq $false -and (& $Predicate $focused)) {
            return $focused
        }
        Send-KeyboardInput "{TAB}"
        if ([DateTime]::UtcNow -gt $deadline) {
            break
        }
    }
    return $null
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

                # Structural UIA: fail on focusable interactive controls with no name.
                # Non-interactive unnamed focusables (Document/Pane/etc.) only warn.
                $structural = Assert-Step -StepId "structural-accessible-names" -Expected "No focusable interactive control lacks an accessible name" -Tree $tree -Check {
                    param($t)
                    $focusableUnnamed = @($t | Where-Object {
                        $_.isFocusable -eq $true -and
                        [string]::IsNullOrWhiteSpace($_.name) -and
                        $_.isOffscreen -ne $true
                    })
                    $interactiveTypes = @(
                        "Button", "Edit", "CheckBox", "RadioButton", "Hyperlink",
                        "ComboBox", "ListItem", "MenuItem", "TabItem", "Slider",
                        "SplitButton", "TreeItem"
                    )
                    $violations = @($focusableUnnamed | Where-Object {
                        $interactiveTypes -contains $_.controlType
                    })
                    if ($violations.Count -gt 0) {
                        $sample = ($violations | Select-Object -First 5 | ForEach-Object {
                            "{0}:{1}" -f $_.controlType, $_.path
                        }) -join "; "
                        return "focusable unnamed interactive controls ($($violations.Count)): $sample"
                    }
                    $other = $focusableUnnamed.Count
                    if ($other -gt 0) {
                        Write-Warning "Found $other non-interactive focusable control(s) without accessible names"
                    }
                    return $true
                }
                if ($structural.result -ne "pass") { $stepStatus = "failed" }
            }

            "navigate-sidebar" {
                if ($null -eq $script:process) { throw "Application has not been launched" }
                Send-KeyboardInput "{TAB}"
                $tree = Get-UiTree -ProcessId $script:process.Id

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    $focused = Find-FocusedElement -Tree $t
                    if ($null -eq $focused) { return "no element has keyboard focus" }
                    if ($focused.controlType -eq "Window" -or $focused.controlType -eq "Document") { return "focus is still on the top-level window or document" }
                    if ($focused.isOffscreen -eq $true) { return "focused element is offscreen" }
                    if ([string]::IsNullOrWhiteSpace($focused.name)) { return "focused control has no name" }
                    return $true
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "select-library" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $targetName = if ($Step.target) { $Step.target } else { "All Tracks" }
                $found = Tab-To-Element -Predicate {
                    param($n)
                    $n.controlType -eq "Button" -and $n.name -and (
                        $n.name.StartsWith("All Tracks", [System.StringComparison]::OrdinalIgnoreCase) -or
                        $n.name.IndexOf("Library", [System.StringComparison]::OrdinalIgnoreCase) -ge 0
                    )
                } -MaxTabs 30 -TimeoutMs $StepTimeoutMs

                if ($null -eq $found) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "could not Tab to a Library/All Tracks control"
                    $stepStatus = "failed"
                } else {
                    Send-KeyboardInput "~"
                    Start-Sleep -Milliseconds $StepDelayMs
                    $tree = Get-UiTree -ProcessId $script:process.Id
                    $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                        param($t)
                        $lib = Find-Element -Tree $t -Predicate {
                            param($n)
                            $n.controlType -eq "Button" -and $n.name -and (
                                $n.name.StartsWith("All Tracks", [System.StringComparison]::OrdinalIgnoreCase) -or
                                $n.name.Equals("Library", [System.StringComparison]::OrdinalIgnoreCase)
                            )
                        }
                        if ($null -eq $lib) { return "Library/All Tracks element not found after selection" }
                        if ($lib.isOffscreen -eq $true) { return "Library/All Tracks element is offscreen" }
                        if ($lib.hasKeyboardFocus -eq $true) { return $true }
                        $focused = Find-FocusedElement -Tree $t
                        if ($null -ne $focused -and $focused.name -and $focused.name.StartsWith("All Tracks", [System.StringComparison]::OrdinalIgnoreCase)) { return $true }
                        return "Library/All Tracks element is not focused or active"
                    }
                    if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                }
            }

            "import-fixture" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $fixturePath = $Step.target
                if ([System.IO.Path]::IsPathRooted($fixturePath) -eq $false) {
                    $fixturePath = [System.IO.Path]::Combine($repoRoot, $fixturePath)
                }
                if (-not (Test-Path -Path $fixturePath -PathType Leaf)) {
                    throw "Fixture file was not found at $fixturePath"
                }
                $fixturePath = [System.IO.Path]::GetFullPath($fixturePath)

                Send-KeyboardInput "^o"

                $dialog = Wait-For-Dialog -Titles @("Open", "Open File") -TimeoutMs $StepTimeoutMs
                if ($dialog -eq [IntPtr]::Zero) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "file picker dialog with title 'Open' did not appear"
                    $stepStatus = "failed"
                } else {
                    Send-KeyboardInput -Keys $fixturePath -Handle $dialog
                    Start-Sleep -Milliseconds 100
                    Send-KeyboardInput -Keys "~" -Handle $dialog

                    $track = Wait-For-Element -Predicate {
                        param($n)
                            ($n.controlType -eq "Button" -or $n.controlType -eq "ListItem") -and
                            $n.name -and $n.name.IndexOf("fixture", [System.StringComparison]::OrdinalIgnoreCase) -ge 0
                    } -TimeoutMs ([math]::Max($StepTimeoutMs, 30000))

                    $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $script:currentTree -Check {
                        param($t)
                        if ($null -eq $track) { return "imported fixture track did not appear in the UIA tree" }
                        $btn = Find-Track -Tree $t -Name "fixture"
                        if ($null -eq $btn) { return "no track named fixture in the current tree" }
                        if ($btn.isOffscreen -eq $true) { return "fixture track is offscreen" }
                        return $true
                    }
                    if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                }
            }

            "select-track" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $targetName = if ($Step.target) { $Step.target } else { "fixture" }
                $found = Tab-To-Element -Predicate {
                    param($n)
                    $n.name -and $n.name.IndexOf($targetName, [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -and
                    ($n.controlType -eq "Button" -or $n.controlType -eq "ListItem")
                } -MaxTabs 40 -TimeoutMs ([math]::Max($StepTimeoutMs, 30000))

                if ($null -eq $found) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "could not Tab to a track named '$targetName'"
                    $stepStatus = "failed"
                } else {
                    Send-KeyboardInput "~"
                    Start-Sleep -Milliseconds $StepDelayMs
                    $tree = Get-UiTree -ProcessId $script:process.Id

                    $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                        param($t)
                        $track = Find-Track -Tree $t -Name $targetName
                        if ($null -eq $track) { return "track '$targetName' is not in the tree" }
                        if ($track.isOffscreen -eq $true) { return "track '$targetName' is offscreen" }
                        $btn = Find-Play-Pause-Button -Tree $t
                        if ($null -eq $btn) { return "Play/Pause button not found; track may not be selected" }
                        if ($btn.isEnabled -eq $false) { return "Play/Pause button is disabled" }
                        return $true
                    }
                    if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                }
            }

            "start-playback" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $btn = Wait-For-Element -Predicate {
                    param($n)
                    $n.controlType -eq "Button" -and ($n.name -eq "Play" -or $n.name -eq "Pause" -or $n.name -eq "Loading")
                } -TimeoutMs ([math]::Max($StepTimeoutMs, 30000))

                if ($null -eq $btn) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "Play/Pause button not found"
                    $stepStatus = "failed"
                } else {
                    if ($btn.name -eq "Loading") {
                        Wait-For-Condition -Condition {
                            param($t)
                            $b = Find-Play-Pause-Button -Tree $t
                            return ($null -ne $b -and ($b.name -eq "Play" -or $b.name -eq "Pause"))
                        } -TimeoutMs ([math]::Max($StepTimeoutMs, 30000)) | Out-Null
                        $btn = Find-Play-Pause-Button -Tree $script:currentTree
                    }

                    if ($null -eq $btn) {
                        Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "Play/Pause button disappeared"
                        $stepStatus = "failed"
                    } elseif ($btn.name -eq "Play") {
                        $focused = Tab-To-Element -Predicate {
                            param($n)
                            $n.controlType -eq "Button" -and ($n.name -eq "Play" -or $n.name -eq "Pause" -or $n.name -eq "Loading")
                        } -MaxTabs 40 -TimeoutMs $StepTimeoutMs

                        if ($null -ne $focused) {
                            Send-KeyboardInput "~"
                            Start-Sleep -Milliseconds $StepDelayMs
                        }
                    }

                    if ($stepStatus -ne "failed") {
                        Wait-For-Condition -Condition {
                            param($t)
                            $b = Find-Play-Pause-Button -Tree $t
                            return ($null -ne $b -and $b.name -eq "Pause")
                        } -TimeoutMs ([math]::Max($StepTimeoutMs, 30000)) | Out-Null

                        $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $script:currentTree -Check {
                            param($t)
                            $b = Find-Play-Pause-Button -Tree $t
                            if ($null -eq $b) { return "Play/Pause button not found" }
                            if ($b.name -eq "Pause") { return $true }
                            return "Play/Pause button is '$($b.name)' instead of Pause"
                        }
                        if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                    }
                }
            }

            "pause-resume" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $btn = Wait-For-Element -Predicate {
                    param($n)
                    $n.controlType -eq "Button" -and ($n.name -eq "Play" -or $n.name -eq "Pause" -or $n.name -eq "Loading")
                } -TimeoutMs ([math]::Max($StepTimeoutMs, 30000))

                if ($null -eq $btn) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "Play/Pause button not found"
                    $stepStatus = "failed"
                } else {
                    if ($btn.name -eq "Loading") {
                        Wait-For-Condition -Condition {
                            param($t)
                            $b = Find-Play-Pause-Button -Tree $t
                            return ($null -ne $b -and ($b.name -eq "Play" -or $b.name -eq "Pause"))
                        } -TimeoutMs ([math]::Max($StepTimeoutMs, 30000)) | Out-Null
                        $btn = Find-Play-Pause-Button -Tree $script:currentTree
                    }

                    if ($null -eq $btn) {
                        Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "Play/Pause button disappeared"
                        $stepStatus = "failed"
                    } else {
                        # If already playing, pause first, then resume.
                        if ($btn.name -eq "Pause") {
                            $focused = Tab-To-Element -Predicate {
                                param($n)
                                $n.controlType -eq "Button" -and ($n.name -eq "Play" -or $n.name -eq "Pause")
                            } -MaxTabs 40 -TimeoutMs $StepTimeoutMs

                            if ($null -ne $focused) {
                                Send-KeyboardInput "~"
                                Start-Sleep -Milliseconds $StepDelayMs
                                Wait-For-Condition -Condition {
                                    param($t)
                                    $b = Find-Play-Pause-Button -Tree $t
                                    return ($null -ne $b -and $b.name -eq "Play")
                                } -TimeoutMs ([math]::Max($StepTimeoutMs, 30000)) | Out-Null
                            }
                        }

                        $focused = Tab-To-Element -Predicate {
                            param($n)
                            $n.controlType -eq "Button" -and ($n.name -eq "Play" -or $n.name -eq "Pause")
                        } -MaxTabs 40 -TimeoutMs $StepTimeoutMs

                        if ($null -ne $focused) {
                            Send-KeyboardInput "~"
                            Start-Sleep -Milliseconds $StepDelayMs
                        }

                        Wait-For-Condition -Condition {
                            param($t)
                            $b = Find-Play-Pause-Button -Tree $t
                            return ($null -ne $b -and $b.name -eq "Pause")
                        } -TimeoutMs ([math]::Max($StepTimeoutMs, 30000)) | Out-Null

                        $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $script:currentTree -Check {
                            param($t)
                            $b = Find-Play-Pause-Button -Tree $t
                            if ($null -eq $b) { return "Play/Pause button not found" }
                            if ($b.name -eq "Pause") { return $true }
                            return "Play/Pause button is '$($b.name)' instead of Pause after pause-resume"
                        }
                        if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                    }
                }
            }

            "seek" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $before = Find-Seek-Slider -Tree $script:currentTree
                $beforeValue = if ($before -and $null -ne $before.rangeValue) { $before.rangeValue } else { -1 }

                if ($null -eq $before -or $beforeValue -lt 0) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "Seek slider not found or does not expose a RangeValue"
                    $stepStatus = "failed"
                } else {
                    Send-KeyboardInput "^{RIGHT}"

                    Wait-For-Condition -Condition {
                        param($t)
                        $slider = Find-Seek-Slider -Tree $t
                        return ($null -ne $slider -and $null -ne $slider.rangeValue -and ($beforeValue -lt 0 -or $slider.rangeValue -gt $beforeValue))
                    } -TimeoutMs ([math]::Max($StepTimeoutMs, 10000)) | Out-Null

                    $tree = Get-UiTree -ProcessId $script:process.Id
                    $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                        param($t)
                        $slider = Find-Seek-Slider -Tree $t
                        if ($null -eq $slider) { return "Seek slider not found" }
                        if ($null -eq $slider.rangeValue) { return "Seek slider does not expose a RangeValue" }
                        if ($slider.rangeValue -le $beforeValue) { return "Seek RangeValue did not increase (before: $beforeValue, after: $($slider.rangeValue))" }
                        return $true
                    }
                    if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                }
            }

            "start-separation" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $stemsBefore = Find-Element -Tree $script:currentTree -Predicate {
                    param($n)
                    $n.controlType -eq "Slider" -and $n.name -and ($n.name -eq "Vocals" -or $n.name -eq "Accompaniment") -and $n.isEnabled -eq $true
                }

                Send-KeyboardInput "+^s"

                $sawProgress = $false
                $slider = $null
                $deadline = [DateTime]::UtcNow.AddMilliseconds([math]::Max($StepTimeoutMs, 120000))
                while ([DateTime]::UtcNow -lt $deadline) {
                    $tree = Get-UiTree -ProcessId $script:process.Id
                    if (-not $sawProgress) {
                        $prog = Find-ElementByControlType -Tree $tree -ControlType "ProgressBar"
                        $text = Find-ElementByRegex -Tree $tree -Pattern "(separating|separated|complete)"
                        if ($null -ne $prog -or $null -ne $text) { $sawProgress = $true }
                    }
                    $slider = Find-Element -Tree $tree -Predicate {
                        param($n)
                        $n.controlType -eq "Slider" -and $n.name -and ($n.name -eq "Vocals" -or $n.name -eq "Accompaniment") -and $n.isEnabled -eq $true
                    }
                    if ($null -ne $slider) { break }
                    Start-Sleep -Milliseconds 250
                }

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $script:currentTree -Check {
                    param($t)
                    if ($null -eq $slider) { return "stem mixer did not become enabled" }
                    if (-not $sawProgress -and -not $stemsBefore) { return "stem mixer enabled, but no progress or status text was observed while it was disabled" }
                    return $true
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "adjust-stems" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $slider = Wait-For-Element -Predicate {
                    param($n)
                    $n.controlType -eq "Slider" -and $n.isEnabled -eq $true -and
                    $n.name -and ($n.name -eq "Vocals" -or $n.name -eq "Accompaniment")
                } -TimeoutMs ([math]::Max($StepTimeoutMs, 10000))

                if ($null -eq $slider) {
                    $expand = Find-Expand-Stems-Button -Tree $script:currentTree
                    if ($null -eq $expand -or $expand.isEnabled -eq $false) {
                        Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "no enabled stem slider or stem-mixer button found"
                        $stepStatus = "failed"
                    } else {
                        $focused = Tab-To-Element -Predicate {
                            param($n)
                            $n.controlType -eq "Button" -and $n.name -and (
                                $n.name.Equals("Expand stems", [System.StringComparison]::OrdinalIgnoreCase) -or
                                $n.name.Equals("Collapse stems", [System.StringComparison]::OrdinalIgnoreCase)
                            )
                        } -MaxTabs 40 -TimeoutMs $StepTimeoutMs

                        if ($null -eq $focused) {
                            Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "could not Tab to stem-mixer button"
                            $stepStatus = "failed"
                        } else {
                            Send-KeyboardInput "~"
                            Start-Sleep -Milliseconds 500
                            $slider = Wait-For-Element -Predicate {
                                param($n)
                                $n.controlType -eq "Slider" -and $n.isEnabled -eq $true -and
                                $n.name -and ($n.name -eq "Vocals" -or $n.name -eq "Accompaniment")
                            } -TimeoutMs ([math]::Max($StepTimeoutMs, 10000))
                        }
                    }
                }

                if ($null -ne $slider) {
                    $focused = Tab-To-Element -Predicate {
                        param($n)
                        $n.controlType -eq "Slider" -and $n.name -and ($n.name -eq $slider.name)
                    } -MaxTabs 40 -TimeoutMs $StepTimeoutMs

                    if ($null -eq $focused) {
                        Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "could not Tab to the selected stem slider"
                        $stepStatus = "failed"
                    } else {
                        $beforeValue = if ($null -ne $slider.rangeValue) { $slider.rangeValue } else { -1 }
                        Send-KeyboardInput "{LEFT}{LEFT}{LEFT}{LEFT}{LEFT}"
                        Start-Sleep -Milliseconds 500

                        $tree = Get-UiTree -ProcessId $script:process.Id
                        $intermediate = Find-Element -Tree $tree -Predicate {
                            param($n)
                            $n.controlType -eq "Slider" -and $n.name -and ($n.name -eq $slider.name)
                        }
                        $intermediateValue = if ($null -ne $intermediate -and $null -ne $intermediate.rangeValue) { $intermediate.rangeValue } else { $beforeValue }

                        Send-KeyboardInput "{RIGHT}"
                        Start-Sleep -Milliseconds 500

                        $tree = Get-UiTree -ProcessId $script:process.Id
                        $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                            param($t)
                            $s = Find-Element -Tree $t -Predicate {
                                param($n)
                                $n.controlType -eq "Slider" -and $n.name -and ($n.name -eq $slider.name)
                            }
                            if ($null -eq $s) { return "stem slider disappeared" }
                            if ($null -eq $s.rangeValue) { return "stem slider does not expose a RangeValue" }
                            if ($s.rangeValue -le $intermediateValue) { return "stem slider value did not increase (before: $beforeValue, intermediate: $intermediateValue, after: $($s.rangeValue))" }
                            return $true
                        }
                        if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                    }
                }
            }

            "mute" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $before = Find-Mute-Button -Tree $script:currentTree
                $alreadyMuted = $before -and ($before.name -eq "Unmute" -or $before.toggleState -eq "On")

                if (-not $alreadyMuted) {
                    Send-KeyboardInput "m"
                    Wait-For-Condition -Condition {
                        param($t)
                        $btn = Find-Mute-Button -Tree $t
                        return ($null -ne $btn -and ($btn.name -eq "Unmute" -or $btn.toggleState -eq "On"))
                    } -TimeoutMs $StepTimeoutMs | Out-Null
                }

                $tree = Get-UiTree -ProcessId $script:process.Id

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    $btn = Find-Mute-Button -Tree $t
                    if ($null -eq $btn) { return "Mute button not found" }
                    if ($btn.name -eq "Unmute") { return $true }
                    if ($btn.toggleState -and $btn.toggleState -eq "On") { return $true }
                    return "master mute did not switch to Unmute/On (name is '$($btn.name)', toggleState: '$($btn.toggleState)')"
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "queue" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                Send-KeyboardInput "q"

                Wait-For-Condition -Condition {
                    param($t)
                    $panel = Find-Queue-Panel -Tree $t
                    return ($null -ne $panel -and $panel.isOffscreen -eq $false)
                } -TimeoutMs $StepTimeoutMs | Out-Null

                $tree = Get-UiTree -ProcessId $script:process.Id

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    $btn = Find-Queue-Button -Tree $t
                    if ($null -eq $btn) { return "Queue button not found" }
                    $panel = Find-Queue-Panel -Tree $t
                    if ($null -eq $panel -or $panel.isOffscreen -eq $true) { return "Queue panel is not visible" }
                    if ($btn.toggleState -and $btn.toggleState -eq "On") { return $true }
                    if ($btn.name -and $btn.name.Equals("Queue", [System.StringComparison]::OrdinalIgnoreCase) -and $panel.isEnabled -ne $false) { return $true }
                    return "Queue button is not pressed/On (toggleState: '$($btn.toggleState)')"
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "open-settings" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                # Close any open panel/dialog first.
                Send-KeyboardInput "{ESC}"
                Start-Sleep -Milliseconds 200

                Send-KeyboardInput "^,"

                Wait-For-Condition -Condition {
                    param($t)
                    $settings = Find-Settings-Overlay -Tree $t
                    return ($null -ne $settings -and $settings.isOffscreen -eq $false)
                } -TimeoutMs $StepTimeoutMs | Out-Null

                $tree = Get-UiTree -ProcessId $script:process.Id

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    $settings = Find-Settings-Overlay -Tree $t
                    if ($null -eq $settings) { return "Settings overlay (Preferences) was not found" }
                    if ($settings.isOffscreen -eq $true) { return "Settings overlay is offscreen" }
                    return $true
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "open-appearance" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $tree = Get-UiTree -ProcessId $script:process.Id
                $hasAppearance = ($null -ne (Find-ElementByName -Tree $tree -Name "Appearance"))
                $hasThemeOptions = ($null -ne (Find-Element -Tree $tree -Predicate {
                    param($n)
                    $n.name -and ($n.name -eq "Light" -or $n.name -eq "Dark" -or $n.name -eq "System")
                }))

                if (-not $hasAppearance -or -not $hasThemeOptions) {
                    # Try to tab into the theme radio options, which scrolls the Appearance section into view.
                    Tab-To-Element -Predicate {
                        param($n)
                        $n.name -and ($n.name -eq "Light" -or $n.name -eq "Dark" -or $n.name -eq "System")
                    } -MaxTabs 40 -TimeoutMs $StepTimeoutMs | Out-Null
                    $tree = Get-UiTree -ProcessId $script:process.Id
                }

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    $settingsPanel = Find-Settings-Overlay -Tree $t
                    if ($null -eq $settingsPanel) { return "Settings overlay is not open" }

                    $app = Find-ElementByName -Tree $t -Name "Appearance"
                    if ($null -eq $app) { return "Appearance section was not found" }

                    $options = Find-Element -Tree $t -Predicate {
                        param($n)
                        $n.name -and ($n.name -eq "Light" -or $n.name -eq "Dark" -or $n.name -eq "System")
                    }
                    if ($null -eq $options) { return "Appearance section found but theme options are missing" }

                    if ($null -ne $settingsPanel -and $null -ne $app.path -and $app.path.StartsWith($settingsPanel.path + "/", [System.StringComparison]::Ordinal)) {
                        return $true
                    }
                    return "Appearance section is not inside the Settings overlay"
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "verify-model-runtime-status" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $tree = Get-UiTree -ProcessId $script:process.Id
                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    $runtimeText = Find-ElementByRegex -Tree $t -Pattern "(Ready|Missing|required|v\d+\.\d+|Downloaded|Not downloaded|Downloading)"
                    if ($null -eq $runtimeText) { return "no runtime/model status text found" }
                    return $true
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "toggle-fullscreen" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                # Close any open dialog/overlay first.
                Send-KeyboardInput "{ESC}"
                Start-Sleep -Milliseconds 500

                Send-KeyboardInput "f"

                $fs = $null
                $deadline = [DateTime]::UtcNow.AddMilliseconds([math]::Max($StepTimeoutMs, 30000))
                while ([DateTime]::UtcNow -lt $deadline) {
                    try {
                        $fs = Get-UiTree -ProcessId $script:process.Id -WindowTitle "OpenKara Player" -TimeoutMs 5000
                        if ($fs) { break }
                    } catch {
                    }
                    Start-Sleep -Milliseconds 500
                }

                if ($null -eq $fs) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "fullscreen window 'OpenKara Player' did not appear"
                    $stepStatus = "failed"
                } else {
                    $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $fs -Check {
                        param($t)
                        $window = Find-ElementByControlType -Tree $t -ControlType "Window"
                        if ($null -eq $window) { return "no Window control in fullscreen UIA tree" }
                        if ([string]::IsNullOrWhiteSpace($window.name)) { return "fullscreen window has no accessible name" }
                        return $true
                    }
                    if ($assertion.result -ne "pass") { $stepStatus = "failed" }

                    # Close the fullscreen window so the rest of the workflow runs in the main window.
                    $hWnd = [OpenKaraWin32]::FindWindowByTitle($script:process.Id, "OpenKara Player")
                    if ($hWnd -ne [IntPtr]::Zero) {
                        Send-KeyboardInput "{ESC}" -Handle $hWnd
                        Wait-For-Condition -Condition {
                            param($t)
                            return ([OpenKaraWin32]::FindWindowByTitle($script:process.Id, "OpenKara Player") -eq [IntPtr]::Zero)
                        } -TimeoutMs ([math]::Max($StepTimeoutMs, 30000)) | Out-Null
                        Get-UiTree -ProcessId $script:process.Id | Out-Null
                    }
                }
            }

            "stop-playback" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                Send-KeyboardInput "^."

                Wait-For-Condition -Condition {
                    param($t)
                    $btn = Find-Play-Pause-Button -Tree $t
                    return ($null -ne $btn -and $btn.name -eq "Play")
                } -TimeoutMs $StepTimeoutMs | Out-Null

                $tree = Get-UiTree -ProcessId $script:process.Id

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    $btn = Find-Play-Pause-Button -Tree $t
                    if ($null -eq $btn) { return "Play/Pause button not found" }
                    if ($btn.name -eq "Play") { return $true }
                    return "Play/Pause button is '$($btn.name)' instead of Play"
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "open-fullscreen" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                Send-KeyboardInput "f"
                $fs = $null
                $deadline = [DateTime]::UtcNow.AddMilliseconds([math]::Max($StepTimeoutMs, 30000))
                while ([DateTime]::UtcNow -lt $deadline) {
                    try {
                        $fs = Get-UiTree -ProcessId $script:process.Id -WindowTitle "OpenKara Player" -TimeoutMs 5000
                        if ($fs) { break }
                    } catch {
                    }
                    Start-Sleep -Milliseconds 500
                }

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $script:currentTree -Check {
                    param($t)
                    if ($null -eq $fs) { return "fullscreen window 'OpenKara Player' did not appear" }
                    return $true
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "close-fullscreen" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $hWnd = [OpenKaraWin32]::FindWindowByTitle($script:process.Id, "OpenKara Player")
                if ($hWnd -eq [IntPtr]::Zero) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "fullscreen window was not found"
                    $stepStatus = "failed"
                } else {
                    Send-KeyboardInput "{ESC}" -Handle $hWnd
                    Start-Sleep -Milliseconds 1000

                    $closed = $false
                    $deadline = [DateTime]::UtcNow.AddMilliseconds([math]::Max($StepTimeoutMs, 30000))
                    while ([DateTime]::UtcNow -lt $deadline) {
                        $hWnd = [OpenKaraWin32]::FindWindowByTitle($script:process.Id, "OpenKara Player")
                        if ($hWnd -eq [IntPtr]::Zero) {
                            $closed = $true
                            break
                        }
                        Start-Sleep -Milliseconds 500
                    }

                    $tree = Get-UiTree -ProcessId $script:process.Id
                    $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                        param($t)
                        if ($closed) { return $true }
                        return "fullscreen window did not close"
                    }
                    if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                }
            }

            "cancel-file-picker" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                Send-KeyboardInput "^o"

                $dialog = Wait-For-Dialog -Titles @("Open", "Open File") -TimeoutMs $StepTimeoutMs
                if ($dialog -eq [IntPtr]::Zero) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "file picker dialog with title 'Open' did not appear"
                    $stepStatus = "failed"
                } else {
                    Send-KeyboardInput "{ESC}" -Handle $dialog

                    $closed = $false
                    $deadline = [DateTime]::UtcNow.AddMilliseconds($StepTimeoutMs)
                    while ([DateTime]::UtcNow -lt $deadline) {
                        $hWnd = [OpenKaraWin32]::FindWindowByTitle($script:process.Id, "Open")
                        if ($hWnd -eq [IntPtr]::Zero) {
                            $closed = $true
                            break
                        }
                        Start-Sleep -Milliseconds 250
                    }

                    $tree = Get-UiTree -ProcessId $script:process.Id
                    $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                        param($t)
                        if (-not $closed) { return "file picker dialog is still open" }
                        $dialogWindow = Find-Element -Tree $t -Predicate {
                            param($n)
                            $n.controlType -eq "Window" -and $n.name -and $n.name.IndexOf("Open", [System.StringComparison]::OrdinalIgnoreCase) -ge 0
                        }
                        if ($null -ne $dialogWindow) { return "file picker dialog is still present in UIA tree" }
                        $main = Find-ElementByControlType -Tree $t -ControlType "Window"
                        if ($null -eq $main) { return "main window not found after cancelling file picker" }
                        return $true
                    }
                    if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                }
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
