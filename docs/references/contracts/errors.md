# 错误处理契约

结构化错误模型：`CommandError` 统一用于播放、歌词、分离和导入命令。

## 接口

1. `play / pause / seek / set_volume / set_playback_mode / get_playback_state` 失败时返回 `CommandError`
2. `fetch_lyrics / set_lyrics_offset` 失败时返回 `CommandError`
3. `SeparationStatusSnapshot.error` 变为 `Option<CommandError>`
4. `separation-error` 事件 payload 的 `error` 字段变为 `CommandError`
5. `import_songs.failed[].error` 变为 `CommandError`
6. `get_library / search_library` 顶层命令失败时返回 `CommandError`

## Shared type: `CommandError`

```json
{
  "code": "karaoke_not_ready",
  "message": "song with hash song-a does not have cached stems",
  "retryable": true,
  "fallback": "stay_in_original_mode"
}
```

| Field       | Type             | Notes                                  |
| ----------- | ---------------- | -------------------------------------- |
| `code`      | `ErrorCode`      | 稳定错误码，UI 不应再解析 message 文本 |
| `message`   | `String`         | 仍保留给日志、调试和用户提示           |
| `retryable` | `bool`           | 当前动作是否值得展示“重试”             |
| `fallback`  | `FallbackAction` | UI 默认回退策略提示                    |

## Shared enum: `ErrorCode`

- `database_unavailable`
- `remote_repository_unavailable`
- `media_read_failed`
- `song_not_found`
- `model_unavailable`
- `audio_decode_failed`
- `audio_output_unavailable`
- `karaoke_not_ready`
- `lyrics_not_ready`
- `network_unavailable`
- `invalid_playback_state`
- `execution_provider_unavailable`
- `separation_failed`
- `internal`

## Shared enum: `FallbackAction`

- `retry`
- `refresh_library`
- `reimport_song`
- `check_audio_output_device`
- `stay_in_original_mode`
- `show_empty_state`
- `keep_current_state`

## Current mapping semantics

### Library / Import

1. 导入单个文件时无法打开、无法读元数据、无法 canonicalize 路径：
   - `code = media_read_failed`
   - `fallback = reimport_song`
2. 资料库相关的 SQLite 打开或查询失败：
   - `code = database_unavailable`
   - `fallback = retry`
3. Remote Repository control DB or cache catalog cannot open:
   - `code = remote_repository_unavailable`
   - `fallback = retry`
   - Local library commands continue to use the local SQLite database.

### Playback

1. 找不到歌曲：
   - `code = song_not_found`
   - `fallback = refresh_library`
2. 音频解码失败或文件损坏：
   - `code = audio_decode_failed`
   - `fallback = reimport_song`
3. Karaoke 模式缺少 cached stems：
   - `code = karaoke_not_ready`
   - `fallback = stay_in_original_mode`
4. 没有默认输出设备或设备配置失败：
   - `code = audio_output_unavailable`
   - `fallback = check_audio_output_device`

### Lyrics

1. 歌词缓存缺失或 LRC 不可用：
   - `code = lyrics_not_ready`
   - `fallback = show_empty_state`
2. 在线 timed lyrics provider 请求失败：
   - `code = network_unavailable`
   - `fallback = retry`
3. 歌曲不存在：
   - `code = song_not_found`
   - `fallback = refresh_library`

### Separation

1. The saved execution provider is not compatible with the current host:
   - `code = execution_provider_unavailable`
   - `fallback = keep_current_state`
   - The Settings screen marks the saved provider and asks the user to switch
     to CPU.
2. 分离 worker 失败：
   - `code = separation_failed`
   - `fallback = retry`
3. 分离输入歌曲已丢失：
   - `code = song_not_found`
   - `fallback = refresh_library`
4. 分离前解码失败：
   - `code = audio_decode_failed`
   - `fallback = reimport_song`
5. 运行时模型校验失败、bootstrap 已失败或旧模型需要用户删除：
   - `code = model_unavailable`
   - `fallback = retry`
6. ONNX Runtime 下载、校验或加载失败：
   - `code = model_unavailable`
   - `fallback = retry`
   - 触发场景：`separate`、`upgrade_to_four_stem`、`re_separate`、`batch_separate` 的后台前置 bootstrap 失败，或 `download_model` 命令在 Runtime 未就绪时返回此错误

## Important boundaries

1. 当前错误分类先在 command 边界完成，底层模块仍主要返回 `anyhow::Error`
2. 如果后续把底层模块也切到 typed domain errors，必须保持这里定义的 `ErrorCode` 和 `FallbackAction` 对外稳定
