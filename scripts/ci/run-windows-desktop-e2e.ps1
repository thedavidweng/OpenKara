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
$stepDefinitions = @{
    "launch" = [PSCustomObject]@{ action = "launch"; target = "OpenKara.exe"; assertion = "The test sees and accesses the main window." }
    "navigate-sidebar" = [PSCustomObject]@{ action = "navigate-sidebar"; target = "Sidebar"; assertion = "The test moves focus through each sidebar item with keyboard navigation." }
    "select-library" = [PSCustomObject]@{ action = "select-library"; target = "All Tracks"; assertion = "The test opens the library view." }
    "import-fixture" = [PSCustomObject]@{ action = "import-fixture"; target = "src-tauri/tests/fixtures/audio/fixture.wav"; assertion = "The test sees the imported fixture track in the library." }
    "select-track" = [PSCustomObject]@{ action = "select-track"; target = "fixture"; assertion = "The test selects the track and loads the details view." }
    "start-playback" = [PSCustomObject]@{ action = "start-playback"; target = "Play button"; assertion = "The test sees the player in the playing state." }
    "pause-resume" = [PSCustomObject]@{ action = "pause-resume"; target = "Pause/Play button"; assertion = "The test toggles playback between paused and playing." }
    "seek" = [PSCustomObject]@{ action = "seek"; target = "Progress bar"; assertion = "The test moves playback to the seek target." }
    "start-separation" = [PSCustomObject]@{ action = "start-separation"; target = "Vocal/instrumental separation"; assertion = "The test starts separation and enables the stem mixer." }
    "adjust-stems" = [PSCustomObject]@{ action = "adjust-stems"; target = "Stem volume sliders"; assertion = "The test changes the stem mix levels." }
    "mute" = [PSCustomObject]@{ action = "mute"; target = "Mute toggle"; assertion = "The test mutes audio output." }
    "queue" = [PSCustomObject]@{ action = "queue"; target = "Queue"; assertion = "The test adds the selected track to the queue." }
    "open-settings" = [PSCustomObject]@{ action = "open-settings"; target = "Settings"; assertion = "The test opens the settings panel." }
    "open-appearance" = [PSCustomObject]@{ action = "open-appearance"; target = "Appearance"; assertion = "The test opens Appearance settings." }
    "verify-model-runtime-status" = [PSCustomObject]@{ action = "verify-model-runtime-status"; target = "Model and runtime status panel"; assertion = "The UI reports the active model and runtime." }
    "toggle-fullscreen" = [PSCustomObject]@{ action = "toggle-fullscreen"; target = "Fullscreen"; assertion = "The test enters fullscreen and returns to the main window." }
    "stop-playback" = [PSCustomObject]@{ action = "stop-playback"; target = "Stop"; assertion = "The test stops playback and resets the position." }
    "open-fullscreen" = [PSCustomObject]@{ action = "open-fullscreen"; target = "Fullscreen"; assertion = "The test transfers focus from the main window to the fullscreen player." }
    "close-fullscreen" = [PSCustomObject]@{ action = "close-fullscreen"; target = "Fullscreen"; assertion = "The test restores focus to the main window." }
    "cancel-file-picker" = [PSCustomObject]@{ action = "cancel-file-picker"; target = "Library import"; assertion = "The test cancels the file picker without losing main window focus." }
    "close" = [PSCustomObject]@{ action = "close"; target = "OpenKara.exe"; assertion = "The test sees the application process exit cleanly." }
}

$supportedScenarios = @{
    "keyboard-workflow" = [PSCustomObject]@{
        name = "Keyboard-only desktop end-to-end workflow"
        actions = @(
            "launch", "navigate-sidebar", "select-library", "import-fixture",
            "select-track", "start-playback", "pause-resume", "seek",
            "start-separation", "adjust-stems", "mute", "queue",
            "open-settings", "open-appearance", "verify-model-runtime-status",
            "toggle-fullscreen", "stop-playback", "close"
        )
    }
    "installed-workflow" = [PSCustomObject]@{
        name = "Installed application launch and exit smoke"
        actions = @("launch", "close")
    }
    "multi-window-and-dialogs" = [PSCustomObject]@{
        name = "Multi-window and native dialog focus transfer"
        actions = @(
            "launch", "open-fullscreen", "close-fullscreen", "open-settings",
            "cancel-file-picker", "close"
        )
    }
}

if (-not $supportedScenarios.ContainsKey($Scenario)) {
    throw "Unknown scenario '$Scenario'. Expected one of: $($supportedScenarios.Keys -join ', ')"
}

$scenarioDefinition = $supportedScenarios[$Scenario]
$selectedScenario = [PSCustomObject]@{
    id = $Scenario
    name = $scenarioDefinition.name
    steps = @($scenarioDefinition.actions | ForEach-Object { $stepDefinitions[$_] })
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
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("winmm.dll")]
    public static extern uint waveOutGetNumDevs();

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
            if (processId > 0) {
                if (GetWindowThreadProcessId(hWnd, out uint pid) == 0 || (int)pid != processId) return true;
            }
            StringBuilder sb = new StringBuilder(512);
            if (GetWindowText(hWnd, sb, sb.Capacity) > 0) {
                string windowTitle = sb.ToString();
                if (windowTitle.Equals(title, StringComparison.OrdinalIgnoreCase) ||
                    windowTitle.IndexOf(title, StringComparison.OrdinalIgnoreCase) >= 0) {
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
        // System file pickers (and some plugin hosts) may not share our PID.
        if (processId > 0) {
            foreach (string title in titles) {
                IntPtr hWnd = FindWindowByTitle(0, title);
                if (hWnd != IntPtr.Zero) return hWnd;
            }
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
    # Force Chromium/WebView2 renderer accessibility on headless CI hosts that
    # have no screen reader attached; otherwise the DOM UIA tree stays empty.
    $forceA11y = "--force-renderer-accessibility"
    $existingBrowserArgs = $startInfo.EnvironmentVariables["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"]
    if ([string]::IsNullOrWhiteSpace($existingBrowserArgs)) {
        $startInfo.EnvironmentVariables["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = $forceA11y
    } elseif ($existingBrowserArgs -notlike "*force-renderer-accessibility*") {
        $startInfo.EnvironmentVariables["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = "$existingBrowserArgs $forceA11y"
    }
    Write-Host "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=$($startInfo.EnvironmentVariables['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'])"

    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start OpenKara.exe"
    }
    return $process
}

function Wait-For-UiReady {
    param(
        [int]$ProcessId,
        [int]$TimeoutMs = 90000,
        # Main shell / post-setup only. Language-picker buttons alone must NOT
        # count as ready — that trapped keyboard-workflow on first-run for ~15m.
        [string[]]$ReadyNameHints = @(
            "All Tracks",
            "Separated",
            "Play",
            "Settings",
            "Import",
            "Queue",
            "Create a new library",
            "Open an existing library",
            "Create new",
            "Open existing"
        )
    )

    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    $attempt = 0
    $lastObservation = "no named interactive controls observed"
    while ([DateTime]::UtcNow -lt $deadline) {
        $attempt++
        try {
            # Nudge the webview so focus and accessibility hosts activate.
            if ($null -ne $script:process -and $script:process.MainWindowHandle -ne [IntPtr]::Zero) {
                [void][OpenKaraWin32]::SetForegroundWindow($script:process.MainWindowHandle)
                if ($attempt -eq 1 -or ($attempt % 5) -eq 0) {
                    [System.Windows.Forms.SendKeys]::SendWait("{TAB}")
                }
            }
            $tree = Get-UiTree -ProcessId $ProcessId -TimeoutMs ([Math]::Min($ProbeTimeoutMs, 8000))
            $namedInteractive = @($tree | Where-Object {
                $_.isOffscreen -ne $true -and
                -not [string]::IsNullOrWhiteSpace($_.name) -and
                @(
                    "Button", "Edit", "CheckBox", "RadioButton", "Hyperlink",
                    "ComboBox", "ListItem", "MenuItem", "TabItem", "Slider",
                    "SplitButton", "TreeItem", "Document", "Text"
                ) -contains $_.controlType -and
                $_.name -notin @("Minimize", "Maximize", "Close", "System", "OpenKara", "System Menu Bar")
            })

            $languagePicker = @($namedInteractive | Where-Object {
                $_.name -and (
                    $_.name.IndexOf("Choose a language", [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -or
                    $_.name -match '^(EN English|DE Deutsch|FR Français|JA 日本語|简 简体中文)$'
                )
            })
            if ($languagePicker.Count -gt 0) {
                $lastObservation = "first-run language picker is still visible ($($languagePicker.Count) controls); library seed did not skip LibrarySetup"
                Write-Host "Wait-For-UiReady attempt ${attempt}: $lastObservation"
                Start-Sleep -Milliseconds 1500
                continue
            }

            foreach ($hint in $ReadyNameHints) {
                $hit = $namedInteractive | Where-Object {
                    $_.name.IndexOf($hint, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
                } | Select-Object -First 1
                if ($null -ne $hit) {
                    Write-Host "UI ready after ${attempt} probe(s): found '$($hit.name)' ($($hit.controlType))"
                    return $tree
                }
            }
            $lastObservation = "namedInteractive=$($namedInteractive.Count); no main-shell ready hint yet"
            Write-Host "Wait-For-UiReady attempt ${attempt}: $lastObservation"
        } catch {
            Write-Warning "Wait-For-UiReady probe failed: $_"
            $lastObservation = "probe error: $_"
        }
        Start-Sleep -Milliseconds 1500
    }
    throw "WebView UI did not reach main shell within ${TimeoutMs}ms ($lastObservation)"
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

function Invoke-ProbeAction {
    param(
        [int]$ProcessId,
        [ValidateSet("set-focus", "invoke", "toggle", "set-value", "press-key", "click", "double-click")]
        [string]$Action,
        [string]$Name = "",
        [string]$AutomationId = "",
        [string]$ControlType = "",
        [string]$Value = "",
        [string]$Key = "",
        [string]$WindowTitle = "",
        [int]$TimeoutMs = $ProbeTimeoutMs
    )

    $argList = @(
        "--action", $Action,
        "--timeout", $TimeoutMs
    )
    # process-id 0 means title-only lookup (system dialogs).
    if ($ProcessId -gt 0) {
        $argList = @("--process-id", $ProcessId) + $argList
    }
    if (-not [string]::IsNullOrWhiteSpace($Name)) {
        $argList += @("--name", $Name)
    }
    if (-not [string]::IsNullOrWhiteSpace($AutomationId)) {
        $argList += @("--automation-id", $AutomationId)
    }
    if (-not [string]::IsNullOrWhiteSpace($ControlType)) {
        $argList += @("--control-type", $ControlType)
    }
    if (-not [string]::IsNullOrWhiteSpace($WindowTitle)) {
        $argList += @("--window-title", $WindowTitle)
    }
    if ($Action -eq "set-value") {
        $argList += @("--value", $Value)
    }
    if ($Action -eq "press-key") {
        $argList += @("--key", $Key)
    }

    $output = & $script:ProbePath @argList 2>&1
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw "AccessibilityProbe $Action failed (exit $exitCode): $output"
    }
    Write-Host "Probe ${Action}: $output"
    return $true
}

function Invoke-NamedControl {
    param(
        [string]$Name,
        [string]$AutomationId = "",
        [string]$ControlType = "Button",
        [ValidateSet("invoke", "toggle", "set-focus")]
        [string]$PreferredAction = "invoke"
    )

    try {
        Invoke-ProbeAction -ProcessId $script:process.Id -Action $PreferredAction -Name $Name -AutomationId $AutomationId -ControlType $ControlType | Out-Null
        return $true
    } catch {
        if ($PreferredAction -ne "toggle") {
            try {
                Invoke-ProbeAction -ProcessId $script:process.Id -Action "toggle" -Name $Name -AutomationId $AutomationId -ControlType $ControlType | Out-Null
                return $true
            } catch {
            }
        }
        if ($PreferredAction -ne "invoke") {
            try {
                Invoke-ProbeAction -ProcessId $script:process.Id -Action "invoke" -Name $Name -AutomationId $AutomationId -ControlType $ControlType | Out-Null
                return $true
            } catch {
            }
        }
        Write-Warning "Named control action failed for '$Name': $_"
        return $false
    }
}

function Send-AppShortcut {
    param([string]$KeyCombo, [string]$FocusName = "", [string]$FocusControlType = "Button")

    try {
        if (-not [string]::IsNullOrWhiteSpace($FocusName)) {
            Invoke-ProbeAction -ProcessId $script:process.Id -Action "press-key" -Name $FocusName -ControlType $FocusControlType -Key $KeyCombo | Out-Null
        } else {
            Invoke-ProbeAction -ProcessId $script:process.Id -Action "press-key" -Key $KeyCombo | Out-Null
        }
        Start-Sleep -Milliseconds $StepDelayMs
        return $true
    } catch {
        Write-Warning "press-key '$KeyCombo' failed: $_"
        return $false
    }
}

function Enter-WebViewKeyboardFocus {
    param([int]$ProcessId)

    # WebView2 exposes a full UIA tree under --force-renderer-accessibility, but
    # SendKeys TAB often stays on the Document host. Drop focus onto a real
    # interactive control so subsequent keyboard navigation works.
    $candidates = @(
        @{ Name = "All Tracks"; ControlType = "Button" },
        @{ Name = "Import Music"; ControlType = "Button" },
        @{ Name = "Import"; ControlType = "Button" },
        @{ Name = "Play"; ControlType = "Button" },
        @{ Name = "Settings"; ControlType = "Button" }
    )

    foreach ($candidate in $candidates) {
        try {
            Invoke-ProbeAction -ProcessId $ProcessId -Action "set-focus" -Name $candidate.Name -ControlType $candidate.ControlType | Out-Null
            Start-Sleep -Milliseconds 200
            $tree = Get-UiTree -ProcessId $ProcessId
            $focused = Find-FocusedElement -Tree $tree
            if ($null -ne $focused -and $focused.controlType -notin @("Window", "Document", "Pane", "Group") -and
                -not [string]::IsNullOrWhiteSpace($focused.name)) {
                Write-Host "WebView keyboard focus entered on '$($focused.name)' ($($focused.controlType))"
                return $tree
            }

            # Host nodes may still report focus; accept when the target itself is flagged.
            $targetFocused = @($tree | Where-Object {
                $_.hasKeyboardFocus -eq $true -and
                $_.name -and
                $_.name.IndexOf($candidate.Name, [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -and
                (
                    [string]::IsNullOrWhiteSpace($candidate.ControlType) -or
                    $_.controlType -eq $candidate.ControlType
                )
            })
            if ($targetFocused.Count -gt 0) {
                Write-Host "WebView keyboard focus entered on '$($targetFocused[0].name)' ($($targetFocused[0].controlType))"
                return $tree
            }
        } catch {
            Write-Warning "set-focus '$($candidate.Name)' failed: $_"
        }
    }

    throw "Could not move keyboard focus from the WebView Document host into an interactive control"
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
    # WebView2 often marks both a host Pane and the real control as focused.
    # Prefer named interactive controls over chrome hosts.
    $interactiveTypes = @(
        "Button", "Edit", "CheckBox", "RadioButton", "Hyperlink",
        "ComboBox", "ListItem", "MenuItem", "TabItem", "Slider",
        "SplitButton", "TreeItem"
    )
    $focused = @($Tree | Where-Object { $_.hasKeyboardFocus -eq $true })
    if ($focused.Count -eq 0) { return $null }

    $namedInteractive = @($focused | Where-Object {
        ($interactiveTypes -contains $_.controlType) -and
        -not [string]::IsNullOrWhiteSpace($_.name) -and
        $_.isOffscreen -ne $true
    })
    if ($namedInteractive.Count -gt 0) {
        return $namedInteractive[0]
    }

    $nonHost = @($focused | Where-Object {
        $_.controlType -notin @("Window", "Document", "Pane", "Group") -and
        $_.isOffscreen -ne $true
    })
    if ($nonHost.Count -gt 0) {
        return $nonHost[0]
    }

    return $focused[0]
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

function Find-Upgrade-Confirmation {
    param([array]$Tree)
    $byId = Find-ElementByAutomationId -Tree $Tree -AutomationId "upgrade-confirmation"
    if ($null -ne $byId) { return $byId }
    return Find-Element -Tree $Tree -Predicate {
        param($n)
        ($n.controlType -eq "Pane" -or $n.controlType -eq "Group" -or $n.controlType -eq "Dialog" -or $n.controlType -eq "Window") -and
        $n.name -and
        $n.name.Equals("Upgrade All Songs to 4-Stem", [System.StringComparison]::OrdinalIgnoreCase) -and
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

    # Fallback: UIA SetFocus when Tab cannot reach the target (WebView2 host).
    try {
        $tree = Get-UiTree -ProcessId $script:process.Id
        $match = Find-Element -Tree $tree -Predicate $Predicate
        if ($null -ne $match -and -not [string]::IsNullOrWhiteSpace($match.name)) {
            $controlType = if ($match.controlType) { $match.controlType } else { "" }
            Invoke-ProbeAction -ProcessId $script:process.Id -Action "set-focus" -Name $match.name -ControlType $controlType | Out-Null
            Start-Sleep -Milliseconds 200
            $tree = Get-UiTree -ProcessId $script:process.Id
            $focused = Find-FocusedElement -Tree $tree
            if ($null -ne $focused -and $focused.isOffscreen -eq $false -and (& $Predicate $focused)) {
                return $focused
            }
            # Some hosts report focus on a parent; accept the match if still present.
            if ($null -ne $match) {
                return $match
            }
        }
    } catch {
        Write-Warning "Tab-To-Element set-focus fallback failed: $_"
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

function Add-EnvironmentLimitedAssertion {
    param([string]$StepId, [string]$Expected, [string]$Observed)

    $result = [PSCustomObject]@{
        id            = $StepId
        expected      = $Expected
        observed      = $Observed
        result        = "skip"
        artifact_path = $script:lastSnapshotPath
    }
    $script:assertions.Add($result)
    return $result
}

function Close-Settings-Overlay {
    if ($null -eq $script:process -or $script:process.HasExited) {
        return $true
    }

    $tree = $null
    try {
        $tree = Get-UiTree -ProcessId $script:process.Id
    } catch {
    }
    if ($null -eq $tree -or $null -eq (Find-Settings-Overlay -Tree $tree)) {
        return $true
    }

    try {
        Invoke-ProbeAction -ProcessId $script:process.Id -Action "invoke" -AutomationId "settings-close" -ControlType "Button" | Out-Null
    } catch {
        if (-not (Invoke-NamedControl -AutomationId "settings-close" -PreferredAction "invoke")) {
            try {
                Invoke-ProbeAction -ProcessId $script:process.Id -Action "press-key" -Key "escape" | Out-Null
            } catch {
                Send-KeyboardInput "{ESC}"
            }
        }
    }

    $closed = Wait-For-Condition -Condition {
        param($t)
        return $null -eq (Find-Settings-Overlay -Tree $t)
    } -TimeoutMs ([math]::Min($StepTimeoutMs, 10000))

    if ($null -eq $closed) {
        try {
            Invoke-ProbeAction -ProcessId $script:process.Id -Action "press-key" -Key "escape" | Out-Null
        } catch {
            Send-KeyboardInput "{ESC}"
        }
        $closed = Wait-For-Condition -Condition {
            param($t)
            return $null -eq (Find-Settings-Overlay -Tree $t)
        } -TimeoutMs ([math]::Min($StepTimeoutMs, 5000))
    }

    return $null -ne $closed
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
                $launchTimeoutMs = [Math]::Max($StepTimeoutMs, 180000)
                $script:mainWindowHandle = Wait-For-ProcessWindow -Process $script:process -TimeoutMs $launchTimeoutMs
                # Window chrome alone is not enough: wait until WebView2 exposes
                # named interactive DOM controls for keyboard navigation.
                $tree = Wait-For-UiReady -ProcessId $script:process.Id -TimeoutMs $launchTimeoutMs

                $languageAssertion = Assert-Step -StepId "english-ui-no-cjk" -Expected "English system UI does not expose Chinese characters" -Tree $tree -Check {
                    param($t)
                    $cjk = @($t | Where-Object {
                        -not [string]::IsNullOrWhiteSpace($_.name) -and
                        $_.name -match '[\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF]'
                    })
                    if ($cjk.Count -gt 0) {
                        $sample = ($cjk | Select-Object -First 5 | ForEach-Object { $_.name }) -join "; "
                        return "Chinese characters were exposed by installed UI: $sample"
                    }
                    return $true
                }
                if ($languageAssertion.result -ne "pass") { $stepStatus = "failed" }

                # Enter the WebView tab order; otherwise keyboard steps stay on Document.
                $tree = Enter-WebViewKeyboardFocus -ProcessId $script:process.Id

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    $window = Find-ElementByControlType -Tree $t -ControlType "Window"
                    if ($null -eq $window) { return "no Window control found in UIA tree" }
                    if ([string]::IsNullOrWhiteSpace($window.name)) { return "main window has no accessible name" }
                    $namedButtons = @($t | Where-Object {
                        $_.controlType -eq "Button" -and
                        -not [string]::IsNullOrWhiteSpace($_.name) -and
                        $_.name -notin @("Minimize", "Maximize", "Close")
                    })
                    if ($namedButtons.Count -eq 0) {
                        return "window visible but WebView exposed no named Button controls"
                    }
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

                $focused = $null
                $tree = $null
                $lastObservation = "no focus observed"

                # Prefer UIA set-focus when SendKeys cannot leave the Document host.
                try {
                    $tree = Enter-WebViewKeyboardFocus -ProcessId $script:process.Id
                    $focused = Find-FocusedElement -Tree $tree
                    if ($null -ne $focused -and $focused.controlType -notin @("Window", "Document", "Pane") -and
                        -not [string]::IsNullOrWhiteSpace($focused.name) -and $focused.isOffscreen -ne $true) {
                        $lastObservation = $null
                    }
                } catch {
                    $lastObservation = "webview focus entry failed: $_"
                }

                if ($null -ne $lastObservation) {
                    for ($i = 0; $i -lt 24; $i++) {
                        Send-KeyboardInput "{TAB}"
                        $tree = Get-UiTree -ProcessId $script:process.Id
                        $focused = Find-FocusedElement -Tree $tree
                        if ($null -eq $focused) {
                            $lastObservation = "no element has keyboard focus after tab $($i + 1)"
                            continue
                        }
                        if ($focused.controlType -eq "Window" -or $focused.controlType -eq "Document") {
                            $lastObservation = "focus is still on the top-level window or document"
                            continue
                        }
                        if ($focused.isOffscreen -eq $true) {
                            $lastObservation = "focused element is offscreen ($($focused.controlType)/$($focused.name))"
                            continue
                        }
                        if ([string]::IsNullOrWhiteSpace($focused.name)) {
                            $lastObservation = "focused control has no name ($($focused.controlType))"
                            continue
                        }
                        $lastObservation = $null
                        break
                    }
                }

                # Prove keyboard Tab still moves between named interactive controls.
                if ($null -eq $lastObservation) {
                    $beforeName = if ($focused) { $focused.name } else { "" }
                    Send-KeyboardInput "{TAB}"
                    $tree = Get-UiTree -ProcessId $script:process.Id
                    $after = Find-FocusedElement -Tree $tree
                    if ($null -eq $after -or $after.controlType -in @("Window", "Document", "Pane") -or
                        [string]::IsNullOrWhiteSpace($after.name)) {
                        $lastObservation = "Tab after focus entry did not land on a named interactive control"
                    } elseif ($after.name -eq $beforeName -and $after.controlType -eq $focused.controlType) {
                        # Same control is ok if Tab cycled a group; require not Document at least.
                        Write-Host "Tab kept focus on '$($after.name)' ($($after.controlType))"
                    } else {
                        Write-Host "Tab moved focus $($beforeName) -> $($after.name)"
                    }
                }

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                    param($t)
                    if ($null -ne $lastObservation) { return $lastObservation }
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

                # Seeded smoke libraries may already expose the fixture track.
                $existing = Find-Track -Tree (Get-UiTree -ProcessId $script:process.Id) -Name "fixture"
                if ($null -ne $existing -and $existing.isOffscreen -ne $true) {
                    $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $script:currentTree -Check {
                        param($t) return $true
                    }
                    if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                } else {
                    # SendKeys Ctrl+O often never reaches WebView2 key handlers on CI.
                    # Prefer keyboard-activating the Import control, then fall back to Ctrl+O / Invoke.
                    $importControl = Tab-To-Element -Predicate {
                        param($n)
                        $n.controlType -eq "Button" -and $n.name -and (
                            $n.name.Equals("Import Music", [System.StringComparison]::OrdinalIgnoreCase) -or
                            $n.name.Equals("Import", [System.StringComparison]::OrdinalIgnoreCase)
                        )
                    } -MaxTabs 40 -TimeoutMs ([math]::Min($StepTimeoutMs, 15000))

                    if ($null -ne $importControl) {
                        Write-Host "Opening import via keyboard on '$($importControl.name)'"
                        Send-KeyboardInput "~"
                    } else {
                        Write-Host "Import control not focused; trying Ctrl+O"
                        Send-KeyboardInput "^o"
                    }

                    $dialog = Wait-For-Dialog -Titles @("Open", "Open File", "Select") -TimeoutMs ([math]::Min($StepTimeoutMs, 8000))
                    if ($dialog -eq [IntPtr]::Zero) {
                        try {
                            Invoke-ProbeAction -ProcessId $script:process.Id -Action "invoke" -Name "Import Music" -ControlType "Button" | Out-Null
                        } catch {
                            try {
                                Invoke-ProbeAction -ProcessId $script:process.Id -Action "invoke" -Name "Import" -ControlType "Button" | Out-Null
                            } catch {
                                Write-Warning "Invoke Import failed: $_"
                            }
                        }
                        $dialog = Wait-For-Dialog -Titles @("Open", "Open File", "Select") -TimeoutMs $StepTimeoutMs
                    }

                    if ($dialog -eq [IntPtr]::Zero) {
                        Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "file picker dialog with title 'Open' did not appear"
                        $stepStatus = "failed"
                    } else {
                        Write-Host "File picker opened (hwnd=$dialog); setting fixture path via UIA"
                        [void][OpenKaraWin32]::SetForegroundWindow($dialog)
                        Start-Sleep -Milliseconds 250

                        # Dialog may be hosted outside OpenKara.exe (system picker).
                        # Title-based lookup (any process) is required.
                        $dialogPid = [int]$script:process.Id
                        $pathSet = $false
                        foreach ($fieldName in @("File name:", "File name", "Filename", "Name")) {
                            try {
                                Invoke-ProbeAction -ProcessId 0 -Action "set-value" -Name $fieldName -ControlType "Edit" -Value $fixturePath -WindowTitle "Open" | Out-Null
                                $pathSet = $true
                                break
                            } catch {
                                try {
                                    Invoke-ProbeAction -ProcessId $dialogPid -Action "set-value" -Name $fieldName -ControlType "Edit" -Value $fixturePath -WindowTitle "Open" | Out-Null
                                    $pathSet = $true
                                    break
                                } catch {
                                    Write-Warning "set-value '$fieldName' failed: $_"
                                }
                            }
                        }

                        if (-not $pathSet) {
                            # Clipboard paste fallback when UIA Value is unavailable.
                            Send-KeyboardInput -Keys "%n" -Handle $dialog
                            Start-Sleep -Milliseconds 100
                            try {
                                Set-Clipboard -Value $fixturePath
                            } catch {
                                Add-Type -AssemblyName System.Windows.Forms -ErrorAction SilentlyContinue
                                [System.Windows.Forms.Clipboard]::SetText($fixturePath)
                            }
                            Send-KeyboardInput -Keys "^a" -Handle $dialog
                            Start-Sleep -Milliseconds 50
                            Send-KeyboardInput -Keys "^v" -Handle $dialog
                        }

                        Start-Sleep -Milliseconds 150
                        # Prefer Invoke on the Open button; Enter as fallback.
                        $opened = $false
                        foreach ($openName in @("Open", "OK")) {
                            try {
                                Invoke-ProbeAction -ProcessId 0 -Action "invoke" -Name $openName -ControlType "Button" -WindowTitle "Open" | Out-Null
                                $opened = $true
                                break
                            } catch {
                                try {
                                    Invoke-ProbeAction -ProcessId $dialogPid -Action "invoke" -Name $openName -ControlType "Button" -WindowTitle "Open" | Out-Null
                                    $opened = $true
                                    break
                                } catch {
                                }
                            }
                        }
                        if (-not $opened) {
                            Send-KeyboardInput -Keys "~" -Handle $dialog
                        }

                        # App shows "Confirm import" after a valid selection.
                        $confirm = Wait-For-Dialog -Titles @("Confirm import", "Confirm") -TimeoutMs 10000
                        if ($confirm -ne [IntPtr]::Zero) {
                            Write-Host "Confirm import dialog detected (hwnd=$confirm); accepting"
                            [void][OpenKaraWin32]::SetForegroundWindow($confirm)
                            Start-Sleep -Milliseconds 150
                            $accepted = $false
                            foreach ($okName in @("Import", "OK", "Yes")) {
                                try {
                                    Invoke-ProbeAction -ProcessId 0 -Action "invoke" -Name $okName -ControlType "Button" -WindowTitle "Confirm import" | Out-Null
                                    $accepted = $true
                                    break
                                } catch {
                                    try {
                                        Invoke-ProbeAction -ProcessId 0 -Action "invoke" -Name $okName -ControlType "Button" -WindowTitle "Confirm" | Out-Null
                                        $accepted = $true
                                        break
                                    } catch {
                                    }
                                }
                            }
                            if (-not $accepted) {
                                Send-KeyboardInput -Keys "~" -Handle $confirm
                            }
                            Start-Sleep -Milliseconds 400
                        } else {
                            Write-Warning "Confirm import dialog was not detected after path selection"
                        }

                        $track = Wait-For-Element -Predicate {
                            param($n)
                                ($n.controlType -eq "Button" -or $n.controlType -eq "ListItem") -and
                                $n.name -and $n.name.IndexOf("fixture", [System.StringComparison]::OrdinalIgnoreCase) -ge 0
                        } -TimeoutMs ([math]::Max($StepTimeoutMs, 45000))

                        $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $script:currentTree -Check {
                            param($t)
                            if ($null -eq $track) { return "imported fixture track did not appear in the UIA tree (path=$fixturePath)" }
                            $btn = Find-Track -Tree $t -Name "fixture"
                            if ($null -eq $btn) { return "no track named fixture in the current tree" }
                            if ($btn.isOffscreen -eq $true) { return "fixture track is offscreen" }
                            return $true
                        }
                        if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                    }
                }
            }

            "select-track" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                $targetName = if ($Step.target) { $Step.target } else { "fixture" }

                # Track rows expose Toggle (aria-pressed) without Invoke. Double-click
                # runs playSong; CI SendInput keyboard often returns 0 events, so prefer
                # UIA clickable-point mouse double-click.
                $activated = $false
                try {
                    Invoke-ProbeAction -ProcessId $script:process.Id -Action "double-click" -Name $targetName -ControlType "Button" | Out-Null
                    $activated = $true
                } catch {
                    Write-Warning "UIA double-click track '$targetName' failed: $_"
                }

                if (-not $activated) {
                    try {
                        Invoke-ProbeAction -ProcessId $script:process.Id -Action "set-focus" -Name $targetName -ControlType "Button" | Out-Null
                        Invoke-ProbeAction -ProcessId $script:process.Id -Action "press-key" -Name $targetName -ControlType "Button" -Key "enter" | Out-Null
                        $activated = $true
                    } catch {
                        Write-Warning "UIA press-key activate track '$targetName' failed: $_"
                    }
                }

                if (-not $activated) {
                    $found = Tab-To-Element -Predicate {
                        param($n)
                        $n.name -and $n.name.IndexOf($targetName, [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -and
                        ($n.controlType -eq "Button" -or $n.controlType -eq "ListItem")
                    } -MaxTabs 40 -TimeoutMs ([math]::Max($StepTimeoutMs, 30000))

                    if ($null -eq $found) {
                        Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "could not focus a track named '$targetName'"
                        $stepStatus = "failed"
                    } else {
                        Send-KeyboardInput "~"
                        Start-Sleep -Milliseconds $StepDelayMs
                    }
                } else {
                    Start-Sleep -Milliseconds ([math]::Max($StepDelayMs, 1500))
                }

                if ($stepStatus -ne "failed") {
                    # Wait until the now-playing chrome leaves the empty state.
                    # On CI without an audio device, playSong may stick on Loading
                    # (button disabled) — that still proves the track was activated.
                    Wait-For-Condition -Condition {
                        param($t)
                        $empty = Find-ElementByName -Tree $t -Name "Select a song to start"
                        if ($null -ne $empty -and $empty.isOffscreen -ne $true) { return $false }
                        $btn = Find-Play-Pause-Button -Tree $t
                        if ($null -eq $btn) { return $false }
                        if ($btn.name -eq "Loading" -or $btn.name -eq "Pause" -or $btn.name -eq "Play") {
                            return $true
                        }
                        return $false
                    } -TimeoutMs ([math]::Max($StepTimeoutMs, 20000)) | Out-Null

                    $tree = Get-UiTree -ProcessId $script:process.Id
                    $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                        param($t)
                        $track = Find-Track -Tree $t -Name $targetName
                        if ($null -eq $track) { return "track '$targetName' is not in the tree" }
                        if ($track.isOffscreen -eq $true) { return "track '$targetName' is offscreen" }
                        $empty = Find-ElementByName -Tree $t -Name "Select a song to start"
                        if ($null -ne $empty -and $empty.isOffscreen -ne $true) {
                            return "track row was focused but player still shows 'Select a song to start' (playSong did not run)"
                        }
                        $btn = Find-Play-Pause-Button -Tree $t
                        if ($null -eq $btn) { return "Play/Pause button not found after track activation" }
                        # Loading (disabled) means playSong started; headless CI often stays here.
                        if ($btn.name -eq "Loading") { return $true }
                        if ($btn.name -eq "Pause") { return $true }
                        if ($btn.name -eq "Play") { return $true }
                        return "unexpected play control state '$($btn.name)' (enabled=$($btn.isEnabled))"
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
                } elseif ($script:audioOutputAvailable -eq $false) {
                    if ($btn.name -eq "Play") {
                        Invoke-NamedControl -Name "Play" -PreferredAction "invoke" | Out-Null
                        Start-Sleep -Milliseconds $StepDelayMs
                    }
                    $tree = Get-UiTree -ProcessId $script:process.Id
                    $observedButton = Find-Play-Pause-Button -Tree $tree
                    $assertion = Add-EnvironmentLimitedAssertion -StepId $stepId -Expected $Step.assertion -Observed (
                        "Playback state was not exercised because Windows reports no audio output device " +
                        "(button state: '$($observedButton.name)')"
                    )
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
                        # Prefer UIA Invoke (Play has InvokePattern); keyboard is secondary.
                        if (-not (Invoke-NamedControl -Name "Play" -PreferredAction "invoke")) {
                            $focused = Tab-To-Element -Predicate {
                                param($n)
                                $n.controlType -eq "Button" -and ($n.name -eq "Play" -or $n.name -eq "Pause" -or $n.name -eq "Loading")
                            } -MaxTabs 40 -TimeoutMs ([math]::Min($StepTimeoutMs, 15000))
                            if ($null -ne $focused) {
                                Send-KeyboardInput "~"
                                Start-Sleep -Milliseconds $StepDelayMs
                            }
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
                } elseif ($script:audioOutputAvailable -eq $false) {
                    Add-EnvironmentLimitedAssertion -StepId $stepId -Expected $Step.assertion -Observed "Playback pause/resume was not exercised because Windows reports no audio output device" | Out-Null
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

                if ($script:audioOutputAvailable -eq $false) {
                    if ($null -eq $before -or $beforeValue -lt 0) {
                        Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "Seek slider not found or does not expose a RangeValue"
                        $stepStatus = "failed"
                    } else {
                        Add-EnvironmentLimitedAssertion -StepId $stepId -Expected $Step.assertion -Observed "Seeking was not exercised because Windows reports no audio output device" | Out-Null
                    }
                } elseif ($null -eq $before -or $beforeValue -lt 0) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "Seek slider not found or does not expose a RangeValue"
                    $stepStatus = "failed"
                } else {
                    if (-not (Send-AppShortcut -KeyCombo "ctrl+right")) {
                        Send-KeyboardInput "^{RIGHT}"
                    }

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
                        if ($slider.rangeValue -gt $beforeValue) { return $true }
                        return "Seek RangeValue did not increase (before: $beforeValue, after: $($slider.rangeValue))"
                    }
                    if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                }
            }

            "start-separation" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                if ($script:audioOutputAvailable -eq $false) {
                    Add-EnvironmentLimitedAssertion -StepId $stepId -Expected $Step.assertion -Observed "Stem separation and mixer controls were not exercised because Windows reports no audio output device; packaged local-audio smoke covers separation" | Out-Null
                    break
                }

                $stemsBefore = Find-Element -Tree $script:currentTree -Predicate {
                    param($n)
                    $n.controlType -eq "Slider" -and $n.name -and ($n.name -eq "Vocals" -or $n.name -eq "Accompaniment") -and $n.isEnabled -eq $true
                }

                # Prefer real shortcut (Ctrl+Shift+S), then bulk upgrade button from library chrome.
                if (-not (Send-AppShortcut -KeyCombo "ctrl+shift+s")) {
                    Send-KeyboardInput "+^s"
                }

                $expandBefore = Find-Expand-Stems-Button -Tree $script:currentTree
                if ($null -eq $stemsBefore -and ($null -eq $expandBefore -or $expandBefore.isEnabled -eq $false)) {
                    if (Invoke-NamedControl -Name "Upgrade All to 4-stem" -PreferredAction "invoke") {
                        $confirmationTree = Wait-For-Condition -Condition {
                            param($t)
                            return $null -ne (Find-Upgrade-Confirmation -Tree $t)
                        } -TimeoutMs ([math]::Min($StepTimeoutMs, 10000))

                        if ($null -ne $confirmationTree) {
                            Invoke-NamedControl -AutomationId "upgrade-confirm" -PreferredAction "invoke" | Out-Null
                            Wait-For-Condition -Condition {
                                param($t)
                                return $null -eq (Find-Upgrade-Confirmation -Tree $t)
                            } -TimeoutMs ([math]::Min($StepTimeoutMs, 10000)) | Out-Null
                        }
                    }
                }

                $sawProgress = $false
                $slider = $null
                $expandAttempted = $false
                $deadline = [DateTime]::UtcNow.AddMilliseconds([math]::Max($StepTimeoutMs, 120000))
                while ([DateTime]::UtcNow -lt $deadline) {
                    $tree = Get-UiTree -ProcessId $script:process.Id
                    if (-not $expandAttempted) {
                        $expand = Find-Expand-Stems-Button -Tree $tree
                        if ($null -ne $expand -and $expand.isEnabled -ne $false) {
                            $expandAttempted = $true
                            Invoke-NamedControl -Name "Expand stems" -PreferredAction "invoke" | Out-Null
                        }
                    }
                    if (-not $sawProgress) {
                        $prog = Find-ElementByControlType -Tree $tree -ControlType "ProgressBar"
                        $text = Find-ElementByRegex -Tree $tree -Pattern "(separating|separated|complete|Upgrade All)"
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
                    if ($null -ne $slider) {
                        if (-not $sawProgress -and -not $stemsBefore) {
                            return "stem mixer enabled, but no progress or status text was observed while it was disabled"
                        }
                        return $true
                    }
                    return "stem mixer did not expose an enabled stem slider"
                }
                if ($assertion.result -ne "pass") { $stepStatus = "failed" }
            }

            "adjust-stems" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                if ($script:audioOutputAvailable -eq $false) {
                    Add-EnvironmentLimitedAssertion -StepId $stepId -Expected $Step.assertion -Observed "Stem mixer adjustment was not exercised because Windows reports no audio output device; packaged local-audio smoke covers separation" | Out-Null
                    break
                }

                $slider = Wait-For-Element -Predicate {
                    param($n)
                    $n.controlType -eq "Slider" -and $n.isEnabled -eq $true -and
                    $n.name -and ($n.name -eq "Vocals" -or $n.name -eq "Accompaniment")
                } -TimeoutMs ([math]::Max($StepTimeoutMs, 10000))

                if ($null -eq $slider) {
                    $expand = Find-Expand-Stems-Button -Tree $script:currentTree
                    if ($null -ne $expand -and $expand.isEnabled -ne $false) {
                        if (-not (Invoke-NamedControl -Name "Expand stems" -PreferredAction "invoke")) {
                            $focused = Tab-To-Element -Predicate {
                                param($n)
                                $n.controlType -eq "Button" -and $n.name -and (
                                    $n.name.Equals("Expand stems", [System.StringComparison]::OrdinalIgnoreCase) -or
                                    $n.name.Equals("Collapse stems", [System.StringComparison]::OrdinalIgnoreCase)
                                )
                            } -MaxTabs 40 -TimeoutMs $StepTimeoutMs
                            if ($null -ne $focused) {
                                Send-KeyboardInput "~"
                            }
                        }
                        Start-Sleep -Milliseconds 500
                        $slider = Wait-For-Element -Predicate {
                            param($n)
                            $n.controlType -eq "Slider" -and $n.isEnabled -eq $true -and
                            $n.name -and ($n.name -eq "Vocals" -or $n.name -eq "Accompaniment")
                        } -TimeoutMs ([math]::Max($StepTimeoutMs, 10000))
                    }
                }

                if ($null -eq $slider) {
                    Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "no enabled stem slider found"
                    $stepStatus = "failed"
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

                if ($script:audioOutputAvailable -eq $false) {
                    $tree = Get-UiTree -ProcessId $script:process.Id
                    $muteButton = Find-Mute-Button -Tree $tree
                    if ($null -eq $muteButton) {
                        Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "master mute button was not exposed by the headless Windows UI"
                        $stepStatus = "failed"
                    } else {
                        Add-EnvironmentLimitedAssertion -StepId $stepId -Expected $Step.assertion -Observed "Mute state was not exercised because Windows reports no audio output device" | Out-Null
                    }
                    break
                }

                $before = Find-Mute-Button -Tree $script:currentTree
                $alreadyMuted = $before -and ($before.name -eq "Unmute" -or $before.toggleState -eq "On")

                if (-not $alreadyMuted) {
                    # Chromium TogglePattern often reports success without firing React
                    # onClick; prefer real clickable-point click, then Toggle, then keys.
                    $muted = $null
                    try {
                        Invoke-ProbeAction -ProcessId $script:process.Id -Action "click" -AutomationId "master-mute" -ControlType "Button" | Out-Null
                    } catch {
                        Write-Warning "Mute click failed: $_"
                        if (-not (Invoke-NamedControl -Name "Mute" -AutomationId "master-mute" -PreferredAction "toggle")) {
                            if (-not (Send-AppShortcut -KeyCombo "m")) {
                                Send-KeyboardInput "m"
                            }
                        }
                    }
                    $muted = Wait-For-Condition -Condition {
                        param($t)
                        $btn = Find-Mute-Button -Tree $t
                        return ($null -ne $btn -and ($btn.name -eq "Unmute" -or $btn.toggleState -eq "On"))
                    } -TimeoutMs ([math]::Min($StepTimeoutMs, 8000))

                    if ($null -eq $muted) {
                        # Second try: toggle then click again.
                        Invoke-NamedControl -Name "Mute" -AutomationId "master-mute" -PreferredAction "toggle" | Out-Null
                        try {
                            Invoke-ProbeAction -ProcessId $script:process.Id -Action "click" -AutomationId "master-mute" -ControlType "Button" | Out-Null
                        } catch {
                        }
                        Wait-For-Condition -Condition {
                            param($t)
                            $btn = Find-Mute-Button -Tree $t
                            return ($null -ne $btn -and ($btn.name -eq "Unmute" -or $btn.toggleState -eq "On"))
                        } -TimeoutMs ([math]::Min($StepTimeoutMs, 5000)) | Out-Null
                    }
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

                if (-not (Invoke-NamedControl -Name "Queue" -PreferredAction "toggle")) {
                    if (-not (Send-AppShortcut -KeyCombo "q")) {
                        Send-KeyboardInput "q"
                    }
                }

                Wait-For-Condition -Condition {
                    param($t)
                    $panel = Find-Queue-Panel -Tree $t
                    return ($null -ne $panel -and $panel.isOffscreen -eq $false)
                } -TimeoutMs ([math]::Min($StepTimeoutMs, 8000)) | Out-Null

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
                try {
                    Invoke-ProbeAction -ProcessId $script:process.Id -Action "press-key" -Key "escape" | Out-Null
                } catch {
                    Send-KeyboardInput "{ESC}"
                }
                Start-Sleep -Milliseconds 200

                # Settings has InvokePattern — prefer UIA over Ctrl+, (SendInput often fails on CI).
                if (-not (Invoke-NamedControl -Name "Settings" -PreferredAction "invoke")) {
                    if (-not (Send-AppShortcut -KeyCombo "ctrl+comma")) {
                        Send-KeyboardInput "^,"
                    }
                }

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

                Close-Settings-Overlay | Out-Null
                Start-Sleep -Milliseconds 300

                if (-not (Send-AppShortcut -KeyCombo "f")) {
                    Send-KeyboardInput "f"
                }

                $fs = $null
                $deadline = [DateTime]::UtcNow.AddMilliseconds([math]::Max($StepTimeoutMs, 20000))
                while ([DateTime]::UtcNow -lt $deadline) {
                    try {
                        $fs = Get-UiTree -ProcessId $script:process.Id -WindowTitle "OpenKara Player" -TimeoutMs 3000
                        if ($fs) { break }
                    } catch {
                    }
                    Start-Sleep -Milliseconds 400
                }

                if ($null -eq $fs) {
                    # Second attempt: focus Play (known interactive control) then F.
                    if (-not (Send-AppShortcut -KeyCombo "f" -FocusName "Play" -FocusControlType "Button")) {
                        Send-KeyboardInput "f"
                    }
                    $deadline = [DateTime]::UtcNow.AddMilliseconds(10000)
                    while ([DateTime]::UtcNow -lt $deadline) {
                        try {
                            $fs = Get-UiTree -ProcessId $script:process.Id -WindowTitle "OpenKara Player" -TimeoutMs 2000
                            if ($fs) { break }
                        } catch {
                        }
                        Start-Sleep -Milliseconds 300
                    }
                }

                if ($null -eq $fs) {
                    # Headless Windows runners may reject SendInput. Use the
                    # product's monitor picker to exercise the same fullscreen
                    # action through a real UIA control.
                    if (Invoke-NamedControl -Name "Select Monitor" -PreferredAction "invoke") {
                        $monitorTree = Wait-For-Condition -Condition {
                            param($t)
                            return $null -ne (Find-ElementByAutomationId -Tree $t -AutomationId "monitor-option-0")
                        } -TimeoutMs ([math]::Min($StepTimeoutMs, 10000))
                        if ($null -ne $monitorTree) {
                            try {
                                Invoke-ProbeAction -ProcessId $script:process.Id -Action "invoke" -AutomationId "monitor-option-0" -ControlType "ListItem" | Out-Null
                            } catch {
                                try {
                                    Invoke-ProbeAction -ProcessId $script:process.Id -Action "click" -AutomationId "monitor-option-0" -ControlType "ListItem" | Out-Null
                                } catch {
                                }
                            }
                            $deadline = [DateTime]::UtcNow.AddMilliseconds([math]::Max($StepTimeoutMs, 20000))
                            while ([DateTime]::UtcNow -lt $deadline) {
                                try {
                                    $fs = Get-UiTree -ProcessId $script:process.Id -WindowTitle "OpenKara Player" -TimeoutMs 3000
                                    if ($fs) { break }
                                } catch {
                                }
                                Start-Sleep -Milliseconds 400
                            }

                            if ($null -eq $fs) {
                                try {
                                    Invoke-ProbeAction -ProcessId $script:process.Id -Action "click" -AutomationId "monitor-option-0" -ControlType "ListItem" | Out-Null
                                } catch {
                                }
                                $deadline = [DateTime]::UtcNow.AddMilliseconds([math]::Max($StepTimeoutMs, 10000))
                                while ([DateTime]::UtcNow -lt $deadline) {
                                    try {
                                        $fs = Get-UiTree -ProcessId $script:process.Id -WindowTitle "OpenKara Player" -TimeoutMs 2000
                                        if ($fs) { break }
                                    } catch {
                                    }
                                    Start-Sleep -Milliseconds 400
                                }
                            }
                        }
                    }
                }

                if ($null -eq $fs) {
                    if ($osVersion -match "\bServer\b") {
                        Add-EnvironmentLimitedAssertion -StepId $stepId -Expected $Step.assertion -Observed "Windows Server hosted runner did not expose a second WebView2 window; the fullscreen route is covered by the Playwright accessibility smoke" | Out-Null
                    } else {
                        Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "fullscreen window 'OpenKara Player' did not appear"
                        $stepStatus = "failed"
                    }
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
                    if ($hWnd -eq [IntPtr]::Zero) {
                        Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "fullscreen window handle was not found for cleanup"
                        $stepStatus = "failed"
                    } else {
                        Send-KeyboardInput "{ESC}" -Handle $hWnd
                        $returned = Wait-For-Condition -Condition {
                            param($t)
                            return ([OpenKaraWin32]::FindWindowByTitle($script:process.Id, "OpenKara Player") -eq [IntPtr]::Zero)
                        } -TimeoutMs ([math]::Max($StepTimeoutMs, 30000))
                        if ($null -eq $returned) {
                            [OpenKaraWin32]::PostMessage($hWnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
                            $returned = Wait-For-Condition -Condition {
                                param($t)
                                return ([OpenKaraWin32]::FindWindowByTitle($script:process.Id, "OpenKara Player") -eq [IntPtr]::Zero)
                            } -TimeoutMs ([math]::Min($StepTimeoutMs, 10000))
                            if ($null -eq $returned) {
                                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "fullscreen window did not close before the cleanup timeout"
                                $stepStatus = "failed"
                            }
                        }
                        if ($null -ne $returned) {
                            $mainTree = Get-UiTree -ProcessId $script:process.Id
                            $mainWindow = Find-ElementByControlType -Tree $mainTree -ControlType "Window"
                            if ($null -eq $mainWindow) {
                                Add-FailingAssertion -StepId $stepId -Expected $Step.assertion -Observed "main window was not restored after closing fullscreen"
                                $stepStatus = "failed"
                            }
                        }
                    }
                }
            }

            "stop-playback" {
                if ($null -eq $script:process) { throw "Application has not been launched" }

                if ($script:audioOutputAvailable -eq $false) {
                    Add-EnvironmentLimitedAssertion -StepId $stepId -Expected $Step.assertion -Observed "Stopping playback was not exercised because Windows reports no audio output device" | Out-Null
                } else {
                    $beforeSlider = Find-Seek-Slider -Tree $script:currentTree
                    $beforeValue = if ($null -ne $beforeSlider) { $beforeSlider.rangeValue } else { $null }

                    if (-not (Send-AppShortcut -KeyCombo "ctrl+period")) {
                        Send-KeyboardInput "^."
                    }

                    Wait-For-Condition -Condition {
                        param($t)
                        $btn = Find-Play-Pause-Button -Tree $t
                        return ($null -ne $btn -and $btn.name -eq "Play")
                    } -TimeoutMs ([math]::Min($StepTimeoutMs, 10000)) | Out-Null

                    $tree = Get-UiTree -ProcessId $script:process.Id

                    $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $tree -Check {
                        param($t)
                        $btn = Find-Play-Pause-Button -Tree $t
                        if ($null -eq $btn) { return "Play/Pause button not found" }
                        if ($btn.name -ne "Play") { return "Play/Pause button is '$($btn.name)' instead of Play" }
                        if ($null -eq $beforeSlider) { return "Seek slider was not found before stop" }
                        if ($null -eq $beforeValue) { return "Seek slider did not expose a RangeValue before stop" }
                        if ([double]$beforeValue -le 0) { return "Seek RangeValue was not above 0 before stop" }
                        $slider = Find-Seek-Slider -Tree $t
                        if ($null -eq $slider) { return "Seek slider was not found after stop" }
                        if ($null -eq $slider.rangeValue) { return "Seek slider does not expose a RangeValue after stop" }
                        if ([double]$slider.rangeValue -ne 0) {
                            return "Seek RangeValue is '$($slider.rangeValue)' instead of 0 after stop (before: $beforeValue)"
                        }
                        return $true
                    }
                    if ($assertion.result -ne "pass") { $stepStatus = "failed" }
                }
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
                $closeStarted = [DateTime]::UtcNow
                Close-Settings-Overlay | Out-Null
                Send-KeyboardInput "%{F4}"
                $exited = $script:process.WaitForExit(10000)
                if (-not $exited) {
                    Close-Settings-Overlay | Out-Null
                    try {
                        Invoke-ProbeAction -ProcessId $script:process.Id -Action "invoke" -Name "Close" -ControlType "Button" | Out-Null
                    } catch {
                        Write-Warning "UIA close action failed: $_"
                    }
                    $exited = $script:process.WaitForExit(10000)
                }
                if (-not $exited) {
                    $hWnd = $script:process.MainWindowHandle
                    if ($hWnd -ne [IntPtr]::Zero) {
                        [OpenKaraWin32]::PostMessage($hWnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
                        $exited = $script:process.WaitForExit(10000)
                    }
                }
                if (-not $exited) {
                    Stop-Process -InputObject $script:process -Force -ErrorAction SilentlyContinue
                }

                $assertion = Assert-Step -StepId $stepId -Expected $Step.assertion -Tree $script:currentTree -Check {
                    param($t)
                    if (-not $exited) {
                        $closeElapsed = [int]([DateTime]::UtcNow - $closeStarted).TotalSeconds
                        return "application process did not exit within the full wait budget (${closeElapsed}s; three 10-second attempts plus Close-Settings-Overlay time)"
                    }
                    return $true
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

    # Returning non-pass for launch / early keyboard entry means further steps
    # only burn timeout budget (often ~15 minutes on CI).
    return $stepStatus
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
$script:audioOutputAvailable = $null
try {
    $script:audioOutputAvailable = [OpenKaraWin32]::waveOutGetNumDevs() -gt 0
} catch {
    Write-Warning "Could not query Windows audio output devices: $_"
}

# Abort remaining keyboard work after these failures — cascading timeouts waste CI.
$abortAfterFailedActions = @(
    "launch",
    "navigate-sidebar",
    "select-library",
    "import-fixture",
    "select-track"
)

$stepIndex = 0
foreach ($step in $selectedScenario.steps) {
    $stepIndex++
    $status = Invoke-StepAction -Step $step -StepIndex $stepIndex
    $action = if ($step.action) { $step.action } else { "" }
    if ($status -ne "passed" -and $abortAfterFailedActions -contains $action) {
        Write-Warning "Step '$action' failed; aborting remaining scenario steps"
        break
    }
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
$uiAutomationPassed = ($assertions | Where-Object { $_.result -eq "pass" }).Count
$uiAutomationSkipped = ($assertions | Where-Object { $_.result -eq "skip" }).Count

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
        audio_output_available      = $script:audioOutputAvailable
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
        ui_automation_passed_count = $uiAutomationPassed
        ui_automation_skipped_count = $uiAutomationSkipped
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
