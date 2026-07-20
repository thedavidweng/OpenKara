# 模型 Bootstrap 契约

运行时模型解析、首次启动下载、状态查询、更新检查和分离前置条件。

## 接口

1. 应用启动时优先检查 `<app_data_dir>/models/htdemucs.onnx`
2. 若运行时安装目录缺失模型，则回退检查开发目录 `src-tauri/models/htdemucs.onnx`
3. 若两处都没有可验证的模型，应用启动后会在后台下载模型到 `<app_data_dir>/models/`
4. 应用启动时会先显式加载 `src-tauri/generated/onnxruntime/` 或已打包资源中的
   ONNX Runtime 动态库
5. 分离会复用同一份动态库初始化，不再依赖 `ort`/pyke 自动下载预编译运行时
6. `get_model_bootstrap_status() -> ModelBootstrapStatusSnapshot`
7. `check_model_update(variant) -> ModelUpdateInfo`：查询上游 `latest.json`，
   对比已安装 manifest 的 SHA-256 与最新 release 的 SHA-256，返回更新信息
8. `separate(song_id)` 在模型未 ready 时立即返回 `CommandError`
9. 事件：
   - `model-bootstrap-progress`
   - `model-bootstrap-ready`
   - `model-bootstrap-error`

## 版本发现机制

1. 模型版本不再硬编码在应用代码中。应用在下载时从上游 `latest.json` 解析最新 release：
   `https://raw.githubusercontent.com/thedavidweng/openkara-models/main/latest.json`
2. `latest.json` 由 `openkara-models` 仓库的 `publish-latest-manifest` CI job 在每次
   release 后自动写入 `main` 分支，包含每个 variant 的 `tag`、`url`、`sha256`、`size`
3. 选择稳定 manifest URL 而非 GitHub Releases API 的原因：
   - Releases API 有 60 次/小时/IP 的未认证速率限制
   - manifest 只需一次 HTTP GET + JSON 解析，无需遍历 release 列表或下载 sha256 sidecar
   - `raw.githubusercontent.com` 通过 CDN 分发，无速率限制
4. CI (`prepare-model` job) 和 `scripts/setup.sh` 同样从 `latest.json` 解析最新版本，
   缓存 key 使用解析出的 SHA-256，上游升级时自动失效

## 开发仓库与运行时分发规则

1. 开发仓库中的 `src-tauri/models/` 只保留 `.gitkeep` 与说明文档；下载得到的
   `.onnx` 文件必须保持为本地忽略文件，不进入 git 历史
2. `scripts/setup.sh` 只用于本地开发、离线验证或需要稳定模型输入的测试
3. `scripts/prepare-onnx-runtime.mjs` 负责把目标平台对应的 ONNX Runtime
   动态库下载并规整到 `src-tauri/generated/onnxruntime/`
4. 面向终端用户时，默认安装位置是 `<app_data_dir>/models/`，不是仓库目录；
   但打包产物会随应用一起分发 ONNX Runtime 动态库
5. 后续如果调整模型来源、运行时库版本或文件名，必须同时更新：
   - 本契约
   - `scripts/setup.sh`
   - `scripts/prepare-onnx-runtime.mjs`
   - `src-tauri/models/README.md`
6. 模型版本不固定：应用始终从 `latest.json` 解析最新 release。当前最新版本为：
   - `htdemucs`: `model-v2.1.0`
   - `htdemucs_ft`: `model-ft-v2.1.0`
7. `openkara-models` 资源会携带：
   - `openkara.model_cache_key`
   - `openkara.optimized_by=onnxruntime`
     Rust 运行时必须把前者纳入 session cache key 失效条件，并对后者关闭重复图优化。

## Inputs / outputs / required dependencies

### Command: `get_model_bootstrap_status`

**Output**

```json
{
  "state": "downloading",
  "model_path": "/Users/example/Library/Application Support/com.openkara.desktop/models/htdemucs.onnx",
  "downloaded_bytes": 1048576,
  "total_bytes": 52428800,
  "error": null
}
```

### Shared type: `ModelBootstrapStatusSnapshot`

| Field              | Type                                                | Notes                                                                                  |
| ------------------ | --------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `state`            | `"pending" \| "downloading" \| "ready" \| "failed"` | 状态字段固定为 snake_case enum。`outdated` 已移除——启动时不再有固定 pin 可用于判定过期 |
| `model_path`       | `String`                                            | 当前运行时实际模型路径或目标安装路径                                                   |
| `downloaded_bytes` | `Option<u64>`                                       | `downloading` 时存在                                                                   |
| `total_bytes`      | `Option<u64>`                                       | 下载端若返回 `Content-Length` 则存在                                                   |
| `error`            | `Option<CommandError>`                              | `failed` 时存在                                                                        |

### Command: `check_model_update`

**Input**

```json
{ "variant": "htdemucs" }
```

**Output**

```json
{
  "variant": "htdemucs",
  "installed_tag": "model-v2.0.1",
  "latest_tag": "model-v2.1.0",
  "latest_size": 354970480,
  "update_available": true
}
```

### Shared type: `ModelUpdateInfo`

| Field              | Type             | Notes                                                                                |
| ------------------ | ---------------- | ------------------------------------------------------------------------------------ |
| `variant`          | `String`         | 请求的 variant 名称                                                                  |
| `installed_tag`    | `Option<String>` | 已安装 manifest 中记录的 release tag；无 manifest 或 manifest 早于 tag 字段时为 null |
| `latest_tag`       | `String`         | 上游 `latest.json` 中该 variant 的最新 release tag                                   |
| `latest_size`      | `u64`            | 最新 release asset 的磁盘大小（字节）                                                |
| `update_available` | `bool`           | 已安装 SHA-256 与最新 SHA-256 不一致，或无已安装模型时为 `true`                      |

### Events

#### `model-bootstrap-progress`

payload 为完整的 `ModelBootstrapStatusSnapshot`，其中：

- `state = "downloading"`：`downloaded_bytes` 在事件流中应单调不减（实现侧可节流）；`model_path` 固定为运行时安装路径
- `state = "pending"`：等待后台 worker 或用户操作

#### `model-bootstrap-ready`

payload 为完整的 `ModelBootstrapStatusSnapshot`，其中：

- `state = "ready"`
- `downloaded_bytes = null`
- `error = null`

#### `model-bootstrap-error`

payload 为完整的 `ModelBootstrapStatusSnapshot`，其中：

- `state = "failed"`
- `error.code = "model_unavailable"`
- `error.fallback = "retry"`

## Runtime path resolution semantics

1. 优先使用活动模型 variant 对应的 `<app_data_dir>/models/<descriptor.filename>`
2. 若运行时安装目录已有模型且验证 manifest 的元数据（文件名、大小、修改时间）匹配，
   直接进入 `ready`，不再读取整个 ONNX 文件
3. 运行时安装目录的模型在下载校验通过后，会在同目录写入
   `<filename>.verified.json`，包含 `sha256`、`file_size`、`modified_unix_nanos`
   和 `release_tag`。后续启动时若 manifest 元数据匹配当前模型文件，则直接进入
   `ready`；manifest 缺失时视为未安装并重新下载；元数据不匹配时重算 SHA-256
   并与 manifest 记录值比对，匹配则刷新 manifest，不匹配视为损坏并重新下载
4. 若运行时安装目录缺失，但开发目录 `src-tauri/models/<descriptor.filename>` 存在且
   manifest 验证通过，则直接进入 `ready`。开发目录同样允许写入本地 manifest；该目录
   仍只是开发/测试缓存，不是生产运行时依赖
5. 只有当两处都没有可用模型时，才会在后台从 `latest.json` 解析出的 URL 下载到运行时
   安装目录
6. 启动时不再判定"过期"——没有固定 pin 可用于比较。更新检测完全由设置中的
   "检查更新"按钮显式驱动：`check_model_update` 对比已安装 manifest 的 SHA-256
   与上游最新 SHA-256，若有差异则前端提供"下载并替换"流程（`delete_model` +
   `download_model`）

## ONNX Runtime path resolution semantics

1. **Runtime 外部化:** ONNX Runtime 不再打包在安装包中。应用启动时按以下顺序查找：
   1. **Managed app-data:** `<app_data_dir>/runtime/<platform-lib>`（已验证 SHA-256）
   2. **Development fallback:** `src-tauri/generated/onnxruntime/<platform-lib>`（开发构建用）
   3. **Legacy bundled:** 打包资源目录 `onnxruntime/<platform-lib>`（过渡期兼容）
2. 若 managed 路径存在但 SHA-256 不匹配，状态标记为 `corrupt` 并删除无效文件
3. 若所有路径都找不到运行时，应用以 Runtime 缺失状态启动；分离 worker 会在实际推理前自动下载并校验 Runtime
4. Rust 侧固定使用 `ort 2.0.0-rc.12` 的 `load-dynamic` + `api-24` 模式；
   不允许重新打开 `download-binaries` 或 `copy-dylibs`
5. macOS / Linux 使用官方 ONNX Runtime 1.26.0 release 动态库；Windows 使用
   `Microsoft.ML.OnnxRuntime.DirectML` 1.24.4 NuGet runtime。
6. macOS 发布产物必须按目标架构分别准备 `arm64` / `x86_64` 动态库，不允许再把
   universal2 ORT 放进两个安装包里浪费体积
7. macOS 发布包启用 hardened runtime 时必须携带
   `com.apple.security.cs.disable-library-validation` entitlement；官方 ORT dylib
   由 Microsoft Developer ID 签名，应用需要该最小豁免才能在启动阶段加载它。
8. **Runtime status IPC:**
   - `get_runtime_bootstrap_status() -> RuntimeBootstrapStatusSnapshot`
   - `download_runtime() -> RuntimeBootstrapStatusSnapshot`
   - `delete_runtime() -> ()`
9. **Runtime/model state matrix:**

   | Runtime     | Model   | Settings model action        | Separation action                                       |
   | ----------- | ------- | ---------------------------- | ------------------------------------------------------- |
   | Missing     | Missing | Disabled; show runtime CTA   | Download runtime -> download model -> separate          |
   | Missing     | Present | Disabled; show runtime CTA   | Download runtime -> separate                            |
   | Present     | Missing | Enabled                      | Download model -> separate                              |
   | Present     | Present | Enabled for management       | Separate                                                |
   | Downloading | Any     | Disabled until runtime ready | Wait for the active runtime task, then re-check         |
   | Corrupt     | Any     | Disabled; show runtime CTA   | Delete invalid artifact -> download runtime -> re-check |

10. **Runtime verification manifest:** 管理的运行时文件在 SHA-256 校验通过后，会在同目录写入
    `<filename>.verified.json`。后续启动时若该 manifest 的文件名、SHA-256、文件大小和修改时间
    都匹配当前运行时文件，则直接进入 `ready`，不再读取整个动态库文件。

## Product UX target

现有后端行为支持"启动后自动 bootstrap + 状态事件 + 分离前置 bootstrap"。后续
UI 与产品行为应以以下目标为准，而不是把后台下载继续当成隐式行为：

1. 启动时检查模型是否存在且 manifest 验证通过
2. 若缺失，提示模型大小、安装位置和用途，并提供：
   - `Download now`
   - `Later`
3. 用户选择下载后，后台执行下载并显示真实进度
4. 用户选择稍后时，资料库和原曲播放仍然可用；首次分离会自动补齐 Runtime 和模型后继续
5. 当用户首次进入 Karaoke 或主动触发分离时，如 Runtime 或模型仍未 ready，后台 worker 必须按 Runtime -> model -> separation 的顺序继续
6. 下载失败时，UI 使用现有 `model-bootstrap-error` 状态提供重试入口，而不是要求用户手动找脚本
7. 设置页提供"检查更新"按钮：调用 `check_model_update` 对比已安装与上游最新版本；
   若有更新，提供"下载并替换"按钮执行 `delete_model` + `download_model` 流程

## Separation gate semantics

1. `separate(song_id)`、`upgrade_to_four_stem(song_id)`、`re_separate(song_id, stem_mode)` 和 `batch_separate(song_ids)` 会立即创建后台任务
2. 后台任务先确保 Runtime ready：缺失或损坏时下载、校验、写入 manifest，并在当前进程中加载 ORT
3. Runtime ready 后，后台任务确保 active model ready：缺失时从 `latest.json` 解析最新版本并下载校验 active variant，然后继续推理
4. 只有 Runtime/model 下载或校验失败时，任务以 `separation-error` / batch terminal event 结束；命令入口不因缺失 Runtime 或模型直接返回 `model_unavailable`
5. 模型缺失时错误文案应引导用户在设置中下载：

```json
{
  "code": "model_unavailable",
  "message": "model bootstrap is still downloading to ...",
  "retryable": true,
  "fallback": "retry"
}
```

4. 该前置条件不会修改 `get_separation_status(song_id)` 的语义；状态查询仍只反映分离任务自身状态

## Required dependencies

1. `reqwest` 负责运行时模型下载和 `latest.json` 获取
2. `sha2` 负责 SHA-256 完整性校验
3. `tauri::async_runtime::spawn_blocking` 负责后台下载，避免阻塞 app setup
4. `ort 2.0.0-rc.12` 仅以 `load-dynamic` + `api-24` 模式加载预先规整的
   ONNX Runtime 动态库
