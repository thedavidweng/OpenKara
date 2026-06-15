---
title: 常见问题
description: OpenKara 常见问题解答。
---

# 常见问题

开始唱歌前你需要知道的一切。

## 入门

### OpenKara 是什么？

OpenKara 是一款免费桌面应用，可以将你已有的歌曲转换为卡拉 OK 曲目。它在本地去除人声、同步显示歌词，所有处理都在你的电脑上完成。

### 使用它需要技术知识吗？

不需要。大多数用户无需修改高级设置即可导入音乐、准备歌曲并开始唱歌。

### 支持哪些平台？

macOS、Windows 和 Linux。

## 隐私与你的音乐

### 它会上传我的音乐吗？

不会。OpenKara 在本地处理音频。它可能会联网查找歌词，但你的音乐文件始终留在你的电脑上。

### 为什么 OpenKara 会保留原始音频文件的副本？

因为分离模型会随时间改进。保留原始文件可以让你升级到更好的模型，或在 2 轨和 4 轨分离之间切换，而无需重新导入音乐。

### 我可以在不同机器之间迁移曲库吗？

可以。你的卡拉 OK 曲库保存在一个文件夹中，可以将它移到 USB 硬盘、NAS 或另一台电脑上。

## 功能

### 歌词从哪里来？

OpenKara 首先在线查找时间同步歌词。它也可以使用音频文件中嵌入的歌词或与歌曲同目录的 `.lrc` 文件。

### OpenKara 支持 CD+G 吗？

支持。OpenKara 支持同名音频 + `.cdg` 配对以及 MP3+G ZIP 文件。如果导入时一个 `.cdg` 匹配多个选中的音频文件，OpenKara 会询问使用哪个音轨。

### 可以选择分离程度吗？

可以。默认模式将歌曲分为人声和伴奏。如果需要更多控制，详细模式还可以分离鼓、贝斯和其他乐器。之后可以随时升级而无需重新导入。

### OpenKara 何时下载 AI 模型？

OpenKara 在首次启动和曲库创建后在后台开始下载默认模型。可选的更高质量模型可以稍后从设置中下载。

## 故障排除

### macOS 提示应用已损坏或无法打开怎么办？

这是因为 OpenKara 未通过 Apple 公证。打开终端运行：

```bash
xattr -rd com.apple.quarantine /Applications/OpenKara.app
```

然后重新打开应用即可。此操作只需执行一次。

## 更多资料

- [项目 README](https://github.com/thedavidweng/OpenKara/blob/main/README_CN.md)
- [IPC 契约](https://github.com/thedavidweng/OpenKara/blob/main/docs/references/contracts/)
- [Changelog](https://github.com/thedavidweng/OpenKara/blob/main/CHANGELOG.md)
