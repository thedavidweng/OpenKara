# OpenKara.AccessibilityProbe

A Windows-only .NET tool that captures an accessibility snapshot of the OpenKara top-level window.

## Requirements

- Windows
- .NET 8 SDK or later

## Build

```powershell
cd tools/windows-accessibility/OpenKara.AccessibilityProbe
dotnet build -c Release
```

The output is `bin/Release/net8.0-windows/OpenKara.AccessibilityProbe.exe`.

## Usage

Find the window by process ID:

```powershell
OpenKara.AccessibilityProbe --process-id 12345 --output snapshot.json
```

Find the window by process name (without `.exe`):

```powershell
OpenKara.AccessibilityProbe --process-name OpenKara --output snapshot.json
```

Omit `--output` to print the JSON to `stdout`.

## Output

The tool emits a canonical JSON array sorted by `path`. Each object contains:

- `path`: semantic path in the UI Automation tree
- `controlType`
- `name`
- `automationId`
- `isEnabled`
- `isFocusable`
- `isOffscreen`
- `boundingRectangle` as `"x,y,width,height"`
- `supportedPatterns`
- `isSelected` (when supported)
- `expandCollapseState` (when supported)
- `rangeValue` (when supported)
- `parent` and `children` paths for parent-child relationships

`path` does not contain process-specific IDs. When comparing golden snapshots, exclude `boundingRectangle` from the diff because coordinates are unstable.
