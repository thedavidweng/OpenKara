---
layout: home

hero:
  name: OpenKara
  text: Turn your own songs into karaoke tracks.
  tagline: >-
    Open-source desktop karaoke powered by on-device AI stem separation
    and synced lyrics. No subscriptions.
  image:
    src: /img/openkara-app-icon.png
    alt: OpenKara app icon
  actions:
    - theme: brand
      text: Download
      link: https://github.com/thedavidweng/OpenKara/releases
    - theme: alt
      text: FAQ
      link: /faq

features:
  - icon: 🎤
    title: AI Stem Separation
    details: >-
      Separate vocals and accompaniment on-device. 2-stem or 4-stem mode
      with individual instrument volume controls.
  - icon: 📝
    title: Synced Lyrics
    details: >-
      Automatic timed lyrics from online sources, embedded tags, or sidecar
      .lrc files. CD+G graphics rendering included.
  - icon: 📂
    title: Portable Library
    details: >-
      Self-contained library directory works on NAS, USB drives, and across
      macOS, Windows, and Linux.
  - icon: ▶️
    title: Resumable Processing
    details: >-
      Per-chunk checkpointing means separation resumes from where it left
      off if the app is closed mid-process.
---

## The App

<a href="https://youtu.be/OznVDmp9igk" target="_blank" rel="noopener noreferrer">
  <img
    src="https://github.com/user-attachments/assets/33fb3c92-460c-44fb-abf7-19d8ab0977b1"
    alt="OpenKara application interface"
    style="border-radius: 12px; width: 100%"
  />
</a>

## Import Your First Song

Drag a file in or use the import button. OpenKara handles the rest — extracting metadata, separating stems, and fetching lyrics in the background.

<img
  src="/img/OpenKara_Import.webp"
  alt="Screen recording showing a song being imported into OpenKara"
  style="border-radius: 12px; width: 100%"
/>

## Download

Most people only need the download. Build from source only if you want to contribute or inspect the code.

### Release builds

| Platform              | Format               |
| --------------------- | -------------------- |
| macOS (Apple Silicon) | `.dmg`               |
| macOS (Intel)         | `.dmg`               |
| Windows               | `.exe` installer     |
| Linux                 | `.AppImage` / `.deb` |

Or install with a package manager:

```bash
# macOS (Homebrew)
brew install thedavidweng/tap/openkara

# Windows (winget)
winget install thedavidweng.OpenKara
```

If macOS blocks the app on first launch:

```bash
xattr -rd com.apple.quarantine /Applications/OpenKara.app
```

On first launch, OpenKara helps you create a karaoke library and starts downloading the default audio model in the background.

[See all downloads](https://github.com/thedavidweng/OpenKara/releases/latest){.VPButton.brand}

### Build from source

- Node.js 20 or later
- pnpm 10 or later
- Rust stable toolchain
- Tauri 2 desktop prerequisites

```bash
git clone https://github.com/thedavidweng/OpenKara.git
cd OpenKara
pnpm install
./scripts/setup.sh
pnpm tauri dev
```
