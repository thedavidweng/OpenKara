<!-- markdownlint-disable MD041 -->

---

## Installation

Download the latest build for your platform from the assets below:

| Platform              | Format                  |
| --------------------- | ----------------------- |
| macOS (Apple Silicon) | `.dmg`                  |
| macOS (Intel)         | `.dmg`                  |
| Windows               | `.exe` (NSIS installer) |
| Linux                 | `.AppImage` / `.deb`    |

**macOS (Homebrew):**

```bash
brew install thedavidweng/tap/openkara
```

**Windows (winget):**

```bash
winget install thedavidweng.OpenKara
```

**macOS Gatekeeper note:** If macOS says the app is damaged or can't be opened, run:

```bash
xattr -rd com.apple.quarantine /Applications/OpenKara.app
```
