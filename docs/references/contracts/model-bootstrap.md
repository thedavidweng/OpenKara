# 模型 Bootstrap 契约

运行时模型解析、首次启动下载、状态查询和分离前置条件。

## 接口

1. 应用启动时优先检查 `<app_data_dir>/models/htdemucs.onnx`
2. 若运行时安装目录缺失模型，则回退检查开发目录 `src-tauri/models/htdemucs.onnx`
3. 若两处都没有可验证的模型，应用启动后会在后台下载模型到 `<app_data_dir>/models/`
4. 应用启动时会先显式加载 `src-tauri/generated/onnxruntime/` 或已打包资源中的
   ONNX Runtime 动态库
5. 分离会复用同一份动态库初始化，不再依赖 `ort`/pyke 自动下载预编译运行时
6. `get_model_bootstrap_status() -> ModelBootstrapStatusSnapshot`
7. `separate(song_id)` 在模型未 ready 时立即返回 `CommandError`
8. 事件：
   - `model-bootstrap-progress`
   - `model-bootstrap-ready`
   - `model-bootstrap-error`

## 开发仓库与运行时分发规则

1. 开发仓库中的 `src-tauri/models/` 只保留 `.gitkeep` 与说明文档；下载得到的
   `.onnx` 文件必须保持为本地忽略文件，不进入 git 历史
2. `scripts/setup.sh` 只用于本地开发、离线验证或需要稳定模型输入的测试
3. `scripts/prepare-onnx-runtime.mjs` 负责把目标平台对应的 ONNX Runtime
   动态库下载并规整到 `src-tauri/generated/onnxruntime/`
4. 面向终端用户时，默认安装位置是 `<app_data_dir>/models/`，不是仓库目录；
   但打包产物会随应用一起分发 ONNX Runtime 动态库
5. 模型的 URL、SHA-256、文件名和版本 **只** 来自 pinned catalog 快照
   `src-tauri/catalog/release-manifest.json`（见下节）。应用（Rust catalog
   客户端）、`scripts/resolve-model.mjs`、`scripts/setup.sh` 与 CI 全部消费同
   一份快照；任何地方都不允许再出现手写的模型 URL/SHA 常量。更新模型 pin
   = 更新该快照文件（连同 `src-tauri/catalog/stable-pointer.json`）。
6. `openkara-models` 模型资源会携带：
   - `openkara.model_cache_key`
   - `openkara.optimized_by=onnxruntime`
     Rust 运行时必须把前者纳入 session cache key 失效条件，并对后者关闭重复图优化。

## Catalog-driven model resolution (openkara-models)

模型基础设施的权威来源是 `thedavidweng/openkara-models` 发布的两层 catalog：

1. **稳定指针**（可变，位于该仓库 `main` 分支
   `catalog/channels/stable.json`）：声明当前 generation、release ID，以及
   不可变清单的 URL、字节数与 SHA-256。schema 为
   `openkara.catalog/channel-v1`。
2. **不可变发布清单**（内容寻址的 release 资产）：列出模型与 runtime 工件
   （artifact ID、digest、byte size、下载 URL、兼容性边）。schema 为
   `openkara.catalog/release-v1`。

应用侧规则（`src-tauri/src/separator/catalog.rs`）：

- 二进制内嵌一份指针 + 清单的逐字快照（`src-tauri/catalog/`），作为离线信任
  锚：模型解析永不依赖网络；catalog 刷新失败绝不使已验证的安装失效。
- 网络刷新时：先按指针声明的字节数与 SHA-256 验证清单原始字节，**验证通过
  后才解析**；拒绝 generation 低于内嵌快照的指针（stable 通道单调递增）。
- 清单结构校验：artifact ID 唯一、digest 为 64 位十六进制、URL 必须 HTTPS、
  `tensor_interface` 必须为 `waveform` 或 `spectral-core`、每个模型的
  `compatible_runtime_ids` 非空且指向已知 runtime、兼容性边非空、每个模型
  必须声明恰好一个 `.onnx` 安装文件。
- **多工件解析**（generation ≥ 8）：同一变体可有多个交付形态（历史 waveform
  交付保留在清单中供 provenance）。确定性规则：先过滤到**可加载接口**
  （`tensor_interface == "spectral-core"`，频谱会话是唯一生产路径）且非弃用的
  工件，再取**下载体积最小者**——体积规则绝不允许跨接口比较（ft 变体的
  waveform dual 比 spectral 交付更小，若不过滤会解析出加载器拒绝的工件）。
- **压缩交付**：下载负载按 `archive_digest` 验证，安全解压后再按
  `extracted_file_digests` 验证安装的 `.onnx`；raw 交付两者为同一字节。
- **dual 双输出波形模型**（历史工件）：`outputs[0]=[1,4,2,N]` 四轨堆叠 +
  `outputs[1]=[1,2,2,N]`（vocals/accompaniment）。gen-8 之前作为首选交付形态，
  清单中仍列出以兼容旧消费者；但波形生产路径已在 issue #172 PR 5 删除，运行时
  已无法加载波形模型（`model::ensure_spectral_core_metadata` 会拒绝），因此这些
  工件只在清单解析层保留，不再有推理层读取逻辑。
- 每次成功安装都会在模型旁写入 `<model>.identity.json`
  （schema `openkara.app/installed-artifact-v1`），记录 generation、release ID、
  artifact ID、上游 tag、digest、字节数与兼容 runtime 列表。
- 就绪判定：文件 digest 匹配内嵌 pin，**或** 匹配其 identity 记录（因此从更
  新 generation 安装的模型在旧二进制/离线状态下仍可用）。identity 记录损坏
  时按未知处理，回退到 pin 校验。
- 更新判定：比较 identity 的 artifact ID 与 digest 和 catalog 目标工件；
  catalog generation 低于已安装 generation 时拒绝（隐式降级被禁止，恢复需要
  用户显式删除模型）。

## Inputs / outputs / required dependencies

### Command: `get_model_bootstrap_status`

**Output**

```json
{
  "state": "downloading",
  "modelPath": "/Users/example/Library/Application Support/com.openkara.desktop/models/htdemucs.onnx",
  "downloadedBytes": 1048576,
  "totalBytes": 52428800,
  "error": null
}
```

### Shared type: `ModelBootstrapStatusSnapshot`

| Field             | Type                                                              | Notes                                                                                                                         |
| ----------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `state`           | `"pending" \| "downloading" \| "outdated" \| "ready" \| "failed"` | 状态字段固定为 snake_case enum；`outdated` 表示托管路径上存在文件但 SHA-256 与当前 pin 不一致（文件保留，供用户在设置中删除） |
| `modelPath`       | `String`                                                          | 当前运行时实际模型路径或目标安装路径                                                                                          |
| `downloadedBytes` | `Option<u64>`                                                     | `downloading` 时存在                                                                                                          |
| `totalBytes`      | `Option<u64>`                                                     | 下载端若返回 `Content-Length` 则存在                                                                                          |
| `error`           | `Option<CommandError>`                                            | `failed` 时存在                                                                                                               |

### Events

#### `model-bootstrap-progress`

payload 为完整的 `ModelBootstrapStatusSnapshot`，其中：

- `state = "downloading"`：`downloadedBytes` 在事件流中应单调不减（实现侧可节流）；`model_path` 固定为运行时安装路径
- `state = "outdated"`：校验失败但文件未自动删除；`downloadedBytes` / `totalBytes` 为 `null`
- `state = "pending"`：等待后台 worker 或用户操作

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

## Model management commands (Settings)

These commands let the Settings UI inspect and manage model installations
per variant. They are separate from the startup bootstrap flow.

### Command: `get_model_status`

**Input:** `variant: String` (`"htdemucs"` or `"htdemucs_ft"`)

**Output:** `ModelStatusSnapshot`

```json
{
  "variant": "htdemucs",
  "downloaded": true,
  "legacy_install_present": false,
  "file_size_bytes": 52428800,
  "installed_version": "model-v2.1.0",
  "pinned_version": "model-v2.1.0"
}
```

### Shared type: `ModelStatusSnapshot`

| Field                    | Type             | Notes                                                                                             |
| ------------------------ | ---------------- | ------------------------------------------------------------------------------------------------- |
| `variant`                | `String`         | The variant queried                                                                               |
| `downloaded`             | `bool`           | True when the managed file matches the pinned release digest or a valid installed identity record |
| `legacy_install_present` | `bool`           | True when the managed file exists but matches neither the pin nor its identity record             |
| `file_size_bytes`        | `Option<u64>`    | Size of the managed file in bytes, if it exists                                                   |
| `installed_version`      | `Option<String>` | Upstream release tag of the verified install (identity record, or pin when digests match)         |
| `pinned_version`         | `String`         | Upstream release tag pinned by the embedded catalog snapshot                                      |

### Command: `download_model`

**Input:** `variant: String`

**Output:** `ModelBootstrapStatusSnapshot`

Downloads the model for the given variant to `<app_data_dir>/models/<filename>`.
The download is single-flight per variant: a concurrent call for the same
variant while a download is in progress returns the current downloading
status instead of spawning a duplicate task. If the model is already
verified on disk, returns `ready` immediately without downloading.

### Command: `delete_model`

**Input:** `variant: String`

**Output:** `()`

Removes the managed model file, its verification manifest, and its installed
identity record for the given variant. The user invokes this from Settings to
clear a legacy/incorrect install before re-downloading.

### Command: `check_model_updates`

**Input:** none

**Output:** `ModelUpdateReport`

```json
{
  "generation": 3,
  "release_id": "2026-07-23-003",
  "models": [
    {
      "variant": "htdemucs",
      "state": "up_to_date",
      "installed_version": "model-v2.1.0",
      "available_version": "model-v2.1.0",
      "available_bytes": 354970480
    }
  ]
}
```

Fetches and verifies the current stable catalog from the network, compares
each variant's installed identity against the catalog artifact, and caches the
verified catalog so a subsequent `download_model` installs the newer artifact.
Per-variant `state` is one of `not_installed`, `up_to_date`,
`update_available`, `installed_without_identity`（该变体已安装但没有 identity
记录，来自旧版应用；下载 catalog 工件后即被收编）。

失败语义：检查失败返回普通 `CommandError`，**只** 影响"检查更新"这一 UI 状
态，绝不影响已安装模型的就绪状态。当 catalog 提供的 generation 低于某已安
装模型的 generation 时，本命令同样报错（拒绝把旧工件当作"更新"呈现）；
`download_model` 侧也会拒绝隐式降级——降级需要用户显式删除模型后重新下载。

## Runtime Bootstrap commands (Settings)

These commands let the Settings UI inspect and manage the ONNX Runtime
installation. They are separate from the model bootstrap flow.

### Command: `get_runtime_bootstrap_status`

**Input**: none

**Output:** `RuntimeBootstrapStatusSnapshot`

```json
{
  "state": "ready",
  "runtime_path": "/path/to/onnxruntime/lib",
  "downloaded_bytes": null,
  "total_bytes": null,
  "version": "v1.27.1",
  "active_artifact_id": "onnxruntime-1.27.1-openkara-aarch64-apple-darwin",
  "target_triple": "aarch64-apple-darwin",
  "candidate_version": null,
  "restart_required": false,
  "error": null
}
```

### Shared type: `RuntimeBootstrapStatusSnapshot`

| Field                | Type                                                                                                                                                                                                                                          | Notes                                                                                                             |
| -------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `state`              | `"missing" \| "downloading" \| "installing" \| "probing" \| "activating" \| "ready" \| "update_available" \| "downloading_candidate" \| "candidate_ready_restart_required" \| "activation_failed_previous_restored" \| "corrupt" \| "failed"` | Runtime lifecycle state                                                                                           |
| `runtime_path`       | `String`                                                                                                                                                                                                                                      | Active runtime library path, or target install path                                                               |
| `downloaded_bytes`   | `Option<u64>`                                                                                                                                                                                                                                 | Present during `downloading` / `downloading_candidate`                                                            |
| `total_bytes`        | `Option<u64>`                                                                                                                                                                                                                                 | Present when the download endpoint returns `Content-Length`                                                       |
| `version`            | `String`                                                                                                                                                                                                                                      | Active runtime upstream version; `legacy` for pre-slot installs; pinned catalog version when nothing is installed |
| `active_artifact_id` | `Option<String>`                                                                                                                                                                                                                              | Active runtime artifact ID, when a slot install is active                                                         |
| `target_triple`      | `String`                                                                                                                                                                                                                                      | Runtime target triple for this build                                                                              |
| `candidate_version`  | `Option<String>`                                                                                                                                                                                                                              | Upstream version of the staged candidate, when one exists                                                         |
| `restart_required`   | `bool`                                                                                                                                                                                                                                        | `true` when a candidate is staged and activation requires restart                                                 |
| `error`              | `Option<CommandError>`                                                                                                                                                                                                                        | Present when `state = failed`                                                                                     |

### Command: `download_runtime`

**Input**: none

**Output:** `RuntimeBootstrapStatusSnapshot`

Downloads the ONNX Runtime for the current target triple. First install
downloads and loads immediately. When a runtime is already active (or a
legacy install is loaded), the download is staged as a next-launch
candidate instead — a loaded runtime is never replaced in place.

The download is single-flight: a concurrent call while a download is in
progress returns the current downloading status instead of spawning a
duplicate task.

### Command: `check_runtime_updates`

**Input**: none

**Output:** `RuntimeUpdateReport`

```json
{
  "generation": 3,
  "release_id": "2026-07-23-003",
  "target_triple": "aarch64-apple-darwin",
  "state": "up_to_date",
  "installed_version": "v1.27.1",
  "available_version": "v1.27.1",
  "available_bytes": 10485760,
  "restart_required": false
}
```

Fetches and verifies the current stable catalog from the network, compares
the installed runtime against the catalog artifact, and caches the
verified catalog so a subsequent `download_runtime` installs the newer
artifact. `state` is one of `not_installed`, `up_to_date`,
`update_available`, `installed_without_identity`. A failed check returns
`CommandError` and never affects the readiness of the installed runtime.

### Shared type: `RuntimeUpdateReport`

| Field               | Type               | Notes                                                                              |
| ------------------- | ------------------ | ---------------------------------------------------------------------------------- |
| `generation`        | `u64`              | Catalog generation                                                                 |
| `release_id`        | `String`           | Catalog release ID                                                                 |
| `target_triple`     | `String`           | Runtime target triple                                                              |
| `state`             | `ModelUpdateState` | `not_installed`, `up_to_date`, `update_available`, or `installed_without_identity` |
| `installed_version` | `Option<String>`   | Installed runtime upstream version                                                 |
| `available_version` | `String`           | Available runtime upstream version                                                 |
| `available_bytes`   | `u64`              | Download size in bytes                                                             |
| `restart_required`  | `bool`             | `true` when a runtime is already installed                                         |

### Command: `delete_runtime`

**Input**: none

**Output:** `()`

Removes the active runtime installation, its slot record, and the
candidate. The user invokes this from Settings to clear a corrupt or
incorrect install before re-downloading. A loaded runtime stays mapped
into the process until restart; `delete_runtime` only removes disk state
and slot metadata.

### Events

#### `runtime-bootstrap-progress`

Payload is a full `RuntimeBootstrapStatusSnapshot` with `state =
"downloading"` or `"downloading_candidate"`. `downloaded_bytes` increases
monotonically during the download. After the final byte is read, the first
install emits `installing`, then `probing`, then `activating`; these states
clear `downloaded_bytes` and `total_bytes` because the remaining work is not a
byte-counted download. A failed probe or activation emits
`runtime-bootstrap-error` with `state = "failed"`. A verified runtime
directory remains available for a retry, so a retry does not download the same
artifact again.

#### `runtime-bootstrap-ready`

Payload is a full `RuntimeBootstrapStatusSnapshot` with `state = "ready"`
or `"candidate_ready_restart_required"`. `downloaded_bytes` and `error`
are `null`.

#### `runtime-bootstrap-error`

Payload is a full `RuntimeBootstrapStatusSnapshot` with `state = "failed"`
and `error.code = "model_unavailable"`.

## Runtime path resolution semantics

1. 优先使用活动模型 variant 对应的 `<app_data_dir>/models/<descriptor.filename>`
2. 若运行时安装目录已有模型且 SHA-256 校验通过，直接进入 `ready`
3. 运行时安装目录的模型在完整 SHA-256 校验通过后，会在同目录写入
   `<filename>.verified.json`。后续启动时若该 manifest 的文件名、pinned
   SHA-256、文件大小和修改时间都匹配当前模型文件，则直接进入 `ready`，不再读取整个
   ONNX 文件；manifest 缺失或不匹配时必须重新执行完整 SHA-256 校验，并在通过后重写
   manifest。The installation is resolved (file exists, metadata is current)
   before the manifest is trusted, so a stale manifest from a replaced file
   is always detected via metadata mismatch.
4. 若运行时安装目录模型存在但校验失败（含旧版本 pin 不匹配），进入 `outdated`，**保留**托管文件以便用户在设置的危险区删除；不会静默删除后再下载
5. 若运行时安装目录缺失，但开发目录 `src-tauri/models/<descriptor.filename>` 存在且校验通过，则直接进入 `ready`。开发目录同样允许写入本地 manifest；该目录仍只是开发/测试缓存，不是生产运行时依赖。
6. 只有当两处都没有可用模型时，才会在后台从固定 URL 下载到运行时安装目录

## ONNX Runtime lifecycle semantics (catalog-driven, slot-based)

1. **Runtime 外部化 + catalog 授权:** ONNX Runtime 不打包在安装包中，也不再
   有任何硬编码的版本/URL/SHA 常量。运行时工件来自 openkara-models catalog
   （每个 target triple 一个工件，源码构建，携带逐文件摘要与
   `ort_c_api_level`）。开发/CI 通过 `scripts/prepare-onnx-runtime.mjs`
   把同一 catalog 工件 stage 到 `src-tauri/generated/onnxruntime/`。
2. **槽位化安装布局:**
   - `<app_data_dir>/runtimes/<artifact_id>/` — 不可变安装目录（解压文件 +
     `record.json`，即统一安装记录 `openkara.app/installed-artifact-v1`）
   - `<app_data_dir>/runtimes/slots.json` — `active` / `candidate` /
     `previous` 槽位 + `activation_pending` 崩溃安全标记
   - `<app_data_dir>/runtime/<platform-lib>` — 旧版（pre-slot）安装；其
     verified.json 自洽时仍可加载，首次 catalog runtime 激活后删除
3. **安装事务** (`install_runtime_artifact`)：流式下载（固定内存、传输中哈
   希、先验证归档大小 + SHA-256）→ 安全解压（拒绝绝对路径/穿越/链接/重复
   路径/成员数与展开体积超限）→ 按 catalog 逐文件摘要验证（未声明的文件
   同样拒绝）→ 写入记录 → 原子 rename 到最终目录。部分安装永远不可见。
4. **启动激活事务** (`begin_startup`)：
   - 存在有效 candidate → 先持久化槽位交换（带 `activation_pending`），再
     加载动态库；加载成功 → `finish_activation_success`（清标记、修剪多余
     代次、删除 legacy）；加载失败 → `rollback_failed_activation`（恢复
     previous、记录失败、删除失败目录），状态
     `activation_failed_previous_restored`。
   - 上次启动在交换后崩溃（`activation_pending` 残留）→ 自动回滚到
     previous。
   - candidate 验证失败 → 丢弃并记录，active 不受影响。
   - **已加载进程内的 runtime 永远不被原地替换**：更新一律走 candidate +
     重启激活；仅首次安装（进程内无 runtime）允许立即激活加载。
5. Rust 侧固定使用 `ort 2.0.0-rc.12` 的 `load-dynamic` 模式；crate 的
   `api-N` 特性与 catalog runtime 的 `ort_c_api_level` 兼容性由 CI 的
   `scripts/ci/check-ort-api-level.mjs` 把关（要求 crate N ≤ runtime 级别）。
6. macOS 发布包启用 hardened runtime 时必须携带
   `com.apple.security.cs.disable-library-validation` entitlement 以加载
   外部 ORT 动态库。
7. **Runtime lifecycle IPC:**
   - `get_runtime_bootstrap_status() -> RuntimeBootstrapStatusSnapshot`
   - `download_runtime() -> RuntimeBootstrapStatusSnapshot` — 首次安装立即
     激活加载；已有 active/legacy 时下载为 candidate（状态
     `downloading_candidate` → `candidate_ready_restart_required`）
   - `check_runtime_updates() -> RuntimeUpdateReport` — 与模型更新共享
     stable catalog 获取与缓存；检查失败绝不影响已安装 runtime 的就绪状态
   - `delete_runtime() -> ()`
   - 状态机：`missing | downloading | installing | probing | activating | ready |
update_available | downloading_candidate | candidate_ready_restart_required |
activation_failed_previous_restored | corrupt | failed`
   - 快照字段新增 `active_artifact_id`、`target_triple`、
     `candidate_version`、`restart_required`；`version` 为 ACTIVE runtime
     的上游版本（如 `v1.27.1`），legacy 安装报告 `legacy`，未安装时报告
     catalog pin 版本。**不存在全局单一版本常量** —— 每个平台报告它实际
     安装的工件。
8. **更新策略** (`update_policy`，默认 `notify`)：`manual`（仅手动检查）/
   `notify`（启动后台检查并提示）/ `auto_download`（后台下载 candidate，
   激活仍需重启）。
9. **Runtime/model state matrix:**

   | Runtime                                                                 | Model   | Settings model action      | Separation action                               |
   | ----------------------------------------------------------------------- | ------- | -------------------------- | ----------------------------------------------- |
   | Missing                                                                 | Missing | Disabled; show runtime CTA | Download runtime -> download model -> separate  |
   | Missing                                                                 | Present | Disabled; show runtime CTA | Download runtime -> separate                    |
   | Active (any state with a loaded runtime, incl. update/candidate states) | Missing | Enabled                    | Download model -> separate                      |
   | Active                                                                  | Present | Enabled for management     | Separate                                        |
   | Downloading (first install)                                             | Any     | Disabled until ready       | Wait for the active runtime task, then re-check |
   | Corrupt / Failed                                                        | Any     | Disabled; show runtime CTA | Repair via delete + re-download                 |

## Product UX target

现有后端行为支持“启动后自动 bootstrap + 状态事件 + 分离前置 bootstrap”。后续
UI 与产品行为应以以下目标为准，而不是把后台下载继续当成隐式行为：

1. 启动时检查模型是否存在且校验通过
2. 若缺失，提示模型大小、安装位置和用途，并提供：
   - `Download now`
   - `Later`
3. 用户选择下载后，后台执行下载并显示真实进度
4. 用户选择稍后时，资料库和原曲播放仍然可用；首次分离会自动补齐 Runtime 和模型后继续
5. 当用户首次进入 Karaoke 或主动触发分离时，如 Runtime 或模型仍未 ready，后台 worker 必须按 Runtime -> model -> separation 的顺序继续
6. 下载失败时，UI 使用现有 `model-bootstrap-error` 状态提供重试入口，而不是要求用户手动找脚本

## Separation gate semantics

1. `separate(song_id)`、`upgrade_to_four_stem(song_id)`、`re_separate(song_id, stem_mode)` 和 `batch_separate(song_ids)` 会立即创建后台任务
2. 后台任务先确保 Runtime ready：缺失或损坏时下载、校验、写入 manifest，并在当前进程中加载 ORT
3. Runtime ready 后，后台任务确保 active model ready：缺失时下载并校验 active variant，然后继续推理
4. 只有 Runtime/model 下载或校验失败时，任务以 `separation-error` / batch terminal event 结束；命令入口不因缺失 Runtime 或模型直接返回 `model_unavailable`
5. `outdated` 模型仍然不会被静默覆盖，错误文案应引导用户打开设置删除旧文件并重新下载：

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
4. `ort 2.0.0-rc.12` 仅以 `load-dynamic` + `api-24` 模式加载预先规整的
   ONNX Runtime 动态库
