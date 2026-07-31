[CmdletBinding()]
param (
    [Parameter(Mandatory = $true)]
    [string]$InstallDir,

    [string]$OutputDir = "$env:RUNNER_TEMP\display-matrix",

    [string]$ProbePath,

    [int[]]$Scales = @(100, 125, 150, 200)
)

$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$scriptPath = Join-Path $repoRoot "scripts\ci\run-windows-desktop-e2e.ps1"
if (-not (Test-Path -Path $scriptPath -PathType Leaf)) {
    throw "Desktop E2E script not found at $scriptPath"
}

$desktopKey = "HKCU:\Control Panel\Desktop"
$originalLogPixels = $null
try {
    $prop = Get-ItemProperty -Path $desktopKey -Name "LogPixels" -ErrorAction SilentlyContinue
    if ($prop -and $prop.LogPixels) {
        $originalLogPixels = [int]$prop.LogPixels
    }
} catch {
    $originalLogPixels = $null
}

$results = New-Object System.Collections.Generic.List[object]
$failed = $false

try {
    foreach ($scale in $Scales) {
        $logPixels = [int][math]::Round(96 * ($scale / 100.0))
        Write-Host "Applying display scale ${scale}% (LogPixels=$logPixels)"
        New-ItemProperty -Path $desktopKey -Name "LogPixels" -PropertyType DWord -Value $logPixels -Force | Out-Null

        $scaleOut = Join-Path $OutputDir "scale-$scale"
        New-Item -ItemType Directory -Force -Path $scaleOut | Out-Null

        $args = @{
            InstallDir = $InstallDir
            Scenario   = "installed-workflow"
            OutputDir  = $scaleOut
            StepTimeoutMs = 30000
        }
        if (-not [string]::IsNullOrWhiteSpace($ProbePath)) {
            $args.ProbePath = $ProbePath
        }

        try {
            & $scriptPath @args
            $exitCode = $LASTEXITCODE
        } catch {
            $exitCode = 1
            Write-Warning "Scale ${scale}% threw: $($_.Exception.Message)"
        }

        $passed = ($exitCode -eq 0)
        if (-not $passed) { $failed = $true }

        $results.Add([PSCustomObject]@{
            scale_percent = $scale
            log_pixels    = $logPixels
            exit_code     = $exitCode
            result        = if ($passed) { "pass" } else { "fail" }
            report_path   = (Join-Path $scaleOut "desktop-e2e-report.json")
        }) | Out-Null
    }
} finally {
    if ($null -ne $originalLogPixels) {
        New-ItemProperty -Path $desktopKey -Name "LogPixels" -PropertyType DWord -Value $originalLogPixels -Force | Out-Null
    } else {
        Remove-ItemProperty -Path $desktopKey -Name "LogPixels" -ErrorAction SilentlyContinue
    }
}

$summary = [PSCustomObject]@{
    scenario = "display-scale-matrix"
    status   = if ($failed) { "failed" } else { "passed" }
    scales   = $results
}
$summaryPath = Join-Path $OutputDir "display-matrix-report.json"
$summary | ConvertTo-Json -Depth 6 | Set-Content -Path $summaryPath -Encoding utf8
Write-Host "Wrote $summaryPath"

if ($failed) {
    throw "One or more display scale scenarios failed. See $summaryPath"
}
