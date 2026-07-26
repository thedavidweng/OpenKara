---
title: 常见问题
description: OpenKara 常见问题解答。
---

# 常见问题

开始唱歌前你可能想知道的事。

## 入门

### OpenKara 是什么？

OpenKara 是一款免费的开源桌面应用，把你已经拥有的音乐变成卡拉 OK。它在本地分离人声、跟着音乐同步显示歌词，所有处理都在你的电脑上完成。

### 用它需要懂技术吗？

不需要。大多数人导入音乐、准备一首歌、直接开唱，全程不用碰高级设置。

### 支持哪些平台？

macOS、Windows 和 Linux。

## 隐私和你的音乐

### 它会把我的音乐上传到服务器吗？

不会。OpenKara 在本地处理音频。它可能会联网查找歌词，但你的音乐文件始终留在你的电脑上。

### 为什么 OpenKara 要保留原始音频文件的副本？

因为分离模型会不断改进。保留原始文件，你就能升级到更好的模型，或者在双轨和四轨分离之间切换，而不用重新导入音乐。

### 我能把曲库搬到另一台电脑上吗？

能。你的卡拉 OK 曲库就是一个文件夹，可以拷到 U 盘、NAS 或者另一台电脑上直接打开用。

## 功能

### 歌词从哪里来？

OpenKara 会先在线查找时间同步歌词。也可以用音频文件里内嵌的歌词，或者放在歌曲同目录下的 `.lrc` 文件。

### OpenKara 支持 CD+G 吗？

支持。OpenKara 支持同名音频 + `.cdg` 配对，也支持 MP3+G ZIP 文件。导入时如果一个 `.cdg` 同时匹配多个选中的音频文件，OpenKara 会问你要配给哪一首。

### 我能选择分离到什么程度吗？

能。默认模式把一首歌分成人声和伴奏两轨。想要更多控制，可以切到四轨模式，把鼓、贝斯和其他乐器也分开。之后随时可以升级，不用重新导入。

### OpenKara 什么时候下载 AI 模型？

首次启动并创建曲库后，OpenKara 会在后台下载默认模型。更高质量的可选模型可以之后从设置里下载。

## 故障排除

### macOS 提示应用已损坏或者打不开，怎么办？

这是因为 OpenKara 没有经过 Apple 公证。打开终端运行：

```bash
xattr -rd com.apple.quarantine /Applications/OpenKara.app
```

然后重新打开应用就行。只需要做一次。

### Windows 提示"Windows 已保护你的电脑"，怎么办？

OpenKara 目前还没有代码签名，Windows SmartScreen 会在运行前弹出提示。点击**更多信息**，再点击**仍要运行**即可启动 OpenKara。每次运行新版本时只需操作一次。

### 怎么报告问题、日志在哪里？

打开**设置 → 关于**，点击**复制调试信息**，把结果粘贴到反馈里。其中包含应用版本、构建、操作系统，以及维护者需要的模型/运行时状态。macOS 上也可以从**帮助 → Copy Debug Info** 导出同样的信息。

OpenKara 还会保留一个滚动日志文件，可以一并附上：

- **macOS：** `~/Library/Logs/com.openkara.desktop/openkara.<date>.log`
- **Windows：** `%LOCALAPPDATA%\com.openkara.desktop\logs\openkara.<date>.log`
- **Linux：** `~/.local/share/com.openkara.desktop/logs/openkara.<date>.log`

`<date>` 是滚动日期（例如 `2026-07-25`）；上面的调试信息里也会打印确切路径。

## 想了解更多？

- [项目 README](https://github.com/thedavidweng/OpenKara/blob/main/README_CN.md)
- [IPC 契约](https://github.com/thedavidweng/OpenKara/blob/main/docs/references/contracts/)
- [更新日志](https://github.com/thedavidweng/OpenKara/blob/main/CHANGELOG.md)
