---
layout: home

hero:
  name: OpenKara
  text: 把你的音乐库变成 Karaoke 舞台。
  tagline: >-
    基于端侧 AI 人声分离和同步歌词的开源桌面 Karaoke 应用。无订阅。
  image:
    src: /img/openkara-app-icon.png
    alt: OpenKara 应用图标
  actions:
    - theme: brand
      text: 下载
      link: https://github.com/thedavidweng/OpenKara/releases
    - theme: alt
      text: 常见问题
      link: /zh/faq

features:
  - icon: 🎤
    title: AI 人声分离
    details: >-
      在本地完成歌曲的人声与伴奏分离。支持双轨或四轨模式，
      各乐器独立音量控制。
  - icon: 📝
    title: 同步歌词
    details: >-
      可从在线来源、内嵌标签或 .lrc 伴随文件加载时间同步歌词。
      支持 CD+G 图形渲染。
  - icon: 📂
    title: 可移植曲库
    details: >-
      自包含的曲库目录，可放置在 NAS、USB 硬盘上，
      跨 macOS、Windows 和 Linux 共享。
  - icon: ▶️
    title: 断点续传
    details: >-
      逐块检查点机制，中途关闭应用后重启
      会自动从上次进度继续。
---

## 应用界面

<YouTubeLink href="https://youtu.be/OznVDmp9igk">
  <img
    src="https://github.com/user-attachments/assets/33fb3c92-460c-44fb-abf7-19d8ab0977b1"
    alt="OpenKara 应用界面"
    style="border-radius: 12px; width: 100%"
  />
</YouTubeLink>

## 导入你的第一首歌

拖入文件或使用导入按钮，OpenKara 会自动处理元数据提取、人声分离和歌词获取。

<img
  src="/img/OpenKara_Import.webp"
  alt="OpenKara 导入歌曲录屏"
  style="border-radius: 12px; width: 100%"
/>

## 下载

大多数用户只需下载即可。仅当你想贡献代码或查看源码时才需要从源码构建。

### Release 构建

| 平台                  | 格式                 |
| --------------------- | -------------------- |
| macOS (Apple Silicon) | `.dmg`               |
| macOS (Intel)         | `.dmg`               |
| Windows               | `.exe` 安装包        |
| Linux                 | `.AppImage` / `.deb` |

macOS 也可使用 Homebrew 安装：

```bash
brew install thedavidweng/tap/openkara
```

如果 macOS 首次启动时阻止应用运行：

```bash
xattr -rd com.apple.quarantine /Applications/OpenKara.app
```

首次启动时，OpenKara 会引导你创建曲库，并在后台下载默认音频模型。

[查看所有下载](https://github.com/thedavidweng/OpenKara/releases){.VPButton.brand}

### 从源码构建

- Node.js 20 或更高版本
- pnpm 10 或更高版本
- Rust stable 工具链
- Tauri 2 桌面端前置依赖

```bash
git clone https://github.com/thedavidweng/OpenKara.git
cd OpenKara
pnpm install
./scripts/setup.sh
pnpm tauri dev
```
