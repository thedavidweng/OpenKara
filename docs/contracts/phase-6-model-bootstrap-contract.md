# Phase 6 模型 Bootstrap 契约

**Goal:** 固定运行时模型解析、首次启动下载、状态查询和分离前置条件，避免 UI Agent 和后续代码接手者继续把“模型在哪里、什么时候可用”留成隐式约定。

**Current starting point:** 本契约对应分支 `codex/phase0-m0` 上首次启动模型 bootstrap、运行时状态快照和分离前置 gate 已接入之后的状态。

## Owner

- 代码 Agent：模型路径解析、下载、校验、状态快照、事件、分离前置条件
- UI Agent：消费状态命令和事件，不单方面改命令名、事件名、字段名

## 已冻结能力

1. 应用启动时优先检查 `<app_data_dir>/models/htdemucs_embedded.onnx`
2. 若运行时安装目录缺失模型，则回退检查开发目录 `src-tauri/models/htdemucs_embedded.onnx`
3. 若两处都没有可验证的模型，应用启动后会在后台下载模型到 `<app_data_dir>/models/`
4. `get_model_bootstrap_status() -> ModelBootstrapStatusSnapshot`
5. `separate(song_id)` 在模型未 ready 时立即返回 `CommandError`
6. 事件：
   - `model-bootstrap-progress`
   - `model-bootstrap-ready`
   - `model-bootstrap-error`

## Inputs / outputs / required dependencies

### Command: `get_model_bootstrap_status`

**Output**

```json
{
  "state": "downloading",
  "modelPath": "/Users/example/Library/Application Support/com.openkara.desktop/models/htdemucs_embedded.onnx",
  "downloadedBytes": 1048576,
  "totalBytes": 52428800,
  "error": null
}
```

### Shared type: `ModelBootstrapStatusSnapshot`

| Field             | Type                                                | Notes                                |
| ----------------- | --------------------------------------------------- | ------------------------------------ |
| `state`           | `"pending" \| "downloading" \| "ready" \| "failed"` | 状态字段固定为 snake_case enum       |
| `modelPath`       | `String`                                            | 当前运行时实际模型路径或目标安装路径 |
| `downloadedBytes` | `Option<u64>`                                       | `downloading` 时存在                 |
| `totalBytes`      | `Option<u64>`                                       | 下载端若返回 `Content-Length` 则存在 |
| `error`           | `Option<CommandError>`                              | `failed` 时存在                      |

### Events

#### `model-bootstrap-progress`

payload 为完整的 `ModelBootstrapStatusSnapshot`，其中：

- `state = "downloading"`
- `downloadedBytes` 单调递增
- `modelPath` 固定为运行时安装路径

#### `model-bootstrap-ready`

payload 为完整的 `ModelBootstrapStatusSnapshot`，其中：

- `state = "ready"`
- `downloadedBytes = null`
- `error = null`

#### `model-bootstrap-error`

payload 为完整的 `ModelBootstrapStatusSnapshot`，其中：

- `state = "failed"`
- `error.code = "model_unavailable"`
- `error.fallback = "retry"`

## Runtime path resolution semantics

1. 优先使用 `<app_data_dir>/models/htdemucs_embedded.onnx`
2. 若运行时安装目录已有模型且 SHA-256 校验通过，直接进入 `ready`
3. 若运行时安装目录模型存在但校验失败，会先删除损坏文件，再进入后台下载
4. 若运行时安装目录缺失，但开发目录 `src-tauri/models/htdemucs_embedded.onnx` 存在且校验通过，则直接进入 `ready`
5. 只有当两处都没有可用模型时，才会在后台从固定 URL 下载到运行时安装目录

## Separation gate semantics

1. `separate(song_id)` 现在依赖模型 bootstrap 状态
2. `state = ready` 时才允许启动推理 worker
3. `state = pending / downloading / failed` 时，命令立即返回：

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

1. `reqwest` 负责运行时模型下载
2. `sha2` 负责 SHA-256 完整性校验
3. `tauri::async_runtime::spawn_blocking` 负责后台下载，避免阻塞 app setup

## Verification commands

```bash
cd src-tauri
cargo test --test phase6_model_bootstrap
cargo test
cd ..
pnpm tauri build --debug --no-bundle --ci
```

**Expected evidence**

1. `phase6_model_bootstrap` 证明路径解析、已验证写盘、状态 gate 正常
2. 全量 `cargo test` 证明现有分离/播放/歌词链路未被打破
3. 调试构建成功

## Pause-and-resume instructions

1. 接手前先读本文件，再读 [../plans/2026-03-13-code-agent-plan.md](../plans/2026-03-13-code-agent-plan.md)
2. 若需要更换模型 URL、校验值或安装目录：
   - 先更新本契约
   - 再改 Rust 实现和测试
   - 最后通知 UI Agent
3. 若后续要给 UI 暴露下载重试按钮，优先在此契约基础上新增 `retry_model_bootstrap()`，不要改现有状态字段
