---
title: FAQ
description: Answers to common questions about OpenKara.
---

# FAQ

Everything you need to know before you start singing.

## Getting started

### What is OpenKara?

OpenKara is a free desktop app that turns songs you already own into karaoke tracks. It removes vocals, shows lyrics in time with the music, and keeps the work on your computer.

### Do I need technical knowledge to use this?

No. Most people can import music, prepare a song, and start singing without touching advanced settings.

### Which platforms are supported?

macOS, Windows, and Linux.

## Privacy & your music

### Does it upload my music?

No. OpenKara processes audio on your computer. It may go online to look up lyrics, but your music files stay on your machine.

### Why does OpenKara keep a copy of the original audio file?

Because separation models improve over time. Keeping the original lets you upgrade to a better model or switch between 2-stem and 4-stem separation without re-importing your music.

### Can I move my library between machines?

Yes. Your karaoke library lives in one folder, so you can move it to a USB drive, NAS, or another computer.

## Features

### Where do lyrics come from?

OpenKara first looks for timed lyrics online. It can also use lyrics embedded in your audio files or `.lrc` files stored next to the song.

### Does OpenKara support CD+G?

Yes. OpenKara supports same-name audio + `.cdg` pairs as well as MP3+G ZIP files. If one `.cdg` matches multiple selected audio files during import, OpenKara asks which track should use it.

### Can I choose how much to separate?

Yes. The default mode splits a song into vocals and accompaniment. If you want more control, the detailed mode separates drums, bass, and other instruments too. You can upgrade a song later without re-importing it.

### When does OpenKara download the AI model?

OpenKara starts downloading the default model in the background after first launch and library setup. An optional higher-quality model can be downloaded later from Settings.

## Troubleshooting

### macOS says the app is damaged or can't be opened. What do I do?

This happens because OpenKara isn't notarized with Apple. Open Terminal and run:

```bash
xattr -rd com.apple.quarantine /Applications/OpenKara.app
```

Then open the app again. This only needs to be done once.

### Windows says "Windows protected your PC". What do I do?

OpenKara isn't code-signed yet, so Windows SmartScreen warns before running it. Click **More info**, then **Run anyway** to launch OpenKara. You only need to do this the first time you run a new build.

### How do I report a bug or find the logs?

Open **Settings → About** and click **Copy debug info**, then paste the result into your bug report. It includes the app version, build, OS, and the model/runtime status a maintainer needs. On macOS the same export is available from **Help → Copy Debug Info**.

OpenKara also keeps a rolling log file you can attach:

- **macOS:** `~/Library/Logs/com.openkara.desktop/openkara.<date>.log`
- **Windows:** `%LOCALAPPDATA%\com.openkara.desktop\logs\openkara.<date>.log`
- **Linux:** `~/.local/share/com.openkara.desktop/logs/openkara.<date>.log`

`<date>` is the rotation day (for example `2026-07-25`); the debug info above also prints the exact path.

## Need more detail?

- [Project README](https://github.com/thedavidweng/OpenKara/blob/main/README.md)
- [IPC Contracts](https://github.com/thedavidweng/OpenKara/blob/main/docs/references/contracts/)
- [Changelog](https://github.com/thedavidweng/OpenKara/blob/main/CHANGELOG.md)
