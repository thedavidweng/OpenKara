# Architecture Deepening — Design Spec

Five coordinated refactors to deepen shallow modules, introduce real seams, and improve testability across the OpenKara codebase. Each refactor is a separate PR with tests passing after each step.

## PR Ordering and Dependencies

```
PR 1: AppState decomposition (backend)
  ↓ enables
PR 2: Typed error variants (backend)
  ↓ enables
PR 3: Remote provider trait seam (backend)

PR 4: Cross-store workflow extraction (frontend) — independent, can parallel with PR 2/3
PR 5: Event subscription factory (frontend) — independent, can parallel with PR 2/3
```

---

## PR 1: AppState Decomposition

### Problem

`AppState` in `src-tauri/src/lib.rs` is a god module — 31 fields with no domain grouping. Every command receives the entire struct. Adding a field touches `AppState`, `clone_for_background()`, and every test fixture.

### Solution

Split `AppState` into 5 domain-scoped modules. Commands declare only the modules they use via Tauri's `State<T>` extractor.

### Domain Modules

**PlaybackState** (`src-tauri/src/state/playback.rs`):
- `playback: Arc<Mutex<PlaybackController>>`
- `playback_request_id: Arc<AtomicU64>`
- `audio_output_started: Arc<AtomicBool>`
- `audio_output_start_lock: Arc<Mutex<()>>`

**AirPlayState** (`src-tauri/src/state/airplay.rs`):
- `airplay_audio_tap: Arc<AirPlayAudioTap>`
- `airplay_stream_generation: Arc<AtomicU64>`
- `airplay_audience_active: Arc<AtomicBool>`
- `airplay_control_refresh_token: Arc<AtomicU64>`
- `airplay_http_server: Arc<Mutex<Option<AirPlayHttpServer>>>`
- `airplay_local_output_suppressed: Arc<AtomicBool>`

**SeparationState** (`src-tauri/src/state/separation.rs`):
- `separation_statuses: Arc<Mutex<HashMap<String, SeparationStatusSnapshot>>>`
- `separator_model_cache: Arc<Mutex<ModelCache<LoadedModel>>>`
- `batch_running: Arc<AtomicBool>`
- `batch_cancel: Arc<AtomicBool>`

**RemoteState** (`src-tauri/src/state/remote.rs`):
- `remote_auth_sessions: Arc<Mutex<HashMap<String, RemoteAuthSession>>>`
- `remote_upload_statuses: Arc<Mutex<HashMap<String, UploadStatusSnapshot>>>`

**AppShell** (`src-tauri/src/state/shell.rs`):
- `library: Arc<Mutex<Option<LibraryRoot>>>`
- `app_data_dir: PathBuf`
- `app_resource_dir: PathBuf`
- `model_path: PathBuf`
- `model_bootstrap_status: Arc<Mutex<ModelBootstrapStatusSnapshot>>`
- `shutdown: Arc<AtomicBool>`

### Composition

`AppState` becomes a composition in `src-tauri/src/state/mod.rs`:

```rust
pub struct AppState {
    pub playback: PlaybackState,
    pub airplay: AirPlayState,
    pub separation: SeparationState,
    pub remote: RemoteState,
    pub shell: AppShell,
}
```

### State Registration

Each domain module is registered separately in Tauri's state management:

```rust
builder.manage(PlaybackState::new(...));
builder.manage(AirPlayState::new(...));
// etc.
```

Commands extract only what they need:

```rust
#[tauri::command]
async fn play(state: State<'_, PlaybackState>, ...) -> Result<PlaybackStateSnapshot, CommandError> {
    // only has access to playback state
}
```

### Clone Semantics

Each domain module implements `Clone` (cloning Arcs, not data). `clone_for_background()` delegates to each module's clone. Test fixtures construct only the domain modules they exercise.

### Test Fixtures

Each domain module provides its own test fixture:

```rust
impl PlaybackState {
    pub fn test_fixture() -> Self { /* ... */ }
}

impl AppShell {
    pub fn test_fixture() -> Self { /* ... */ }
}
```

Tests construct only the modules they need — a playback test builds `PlaybackState::test_fixture()` and doesn't touch AirPlay or Remote state.

### Files

- New: `src-tauri/src/state/mod.rs`, `playback.rs`, `airplay.rs`, `separation.rs`, `remote.rs`, `shell.rs`
- Modified: `src-tauri/src/lib.rs` (AppState becomes composition), all command handlers in `src-tauri/src/commands/`, test fixtures in `services/playback.rs` and `lib.rs`

---

## PR 2: Typed Error Variants

### Problem

`commands/error.rs` classifies errors via ~40 substring checks on error message strings. Any wording change in a dependency silently breaks error routing.

### Solution

Define typed error enums per domain. Services return typed errors. The commands layer uses `From` impls to convert into `CommandError`.

### Error Enums

**PlaybackError** (in `src-tauri/src/audio/mod.rs` or `audio/error.rs`):

```rust
pub enum PlaybackError {
    SongNotFound { id: String },
    AudioDecodeFailed { path: String, source: anyhow::Error },
    NoOutputDevice,
    KaraokeNotReady { reason: String },
    InvalidPlaybackState,
}
```

**LibraryError** (in `src-tauri/src/library/mod.rs`):

```rust
pub enum LibraryError {
    MediaReadFailed { path: String, source: anyhow::Error },
    DatabaseUnavailable { source: anyhow::Error },
}
```

**LyricsError** (in `src-tauri/src/lyrics/mod.rs`):

```rust
pub enum LyricsError {
    SongNotFound { id: String },
    LyricsNotReady { reason: String },
    NetworkUnavailable { source: anyhow::Error },
    CacheFailed { source: anyhow::Error },
}
```

**SeparationError** (in `src-tauri/src/separator/mod.rs`):

```rust
pub enum SeparationError {
    SongNotFound { id: String },
    AudioDecodeFailed { path: String, source: anyhow::Error },
    InferenceFailed { reason: String },
}
```

### From Impls

`commands/error.rs` keeps `CommandError` and `ErrorCode` but replaces string-matching functions with:

```rust
impl From<PlaybackError> for CommandError {
    fn from(err: PlaybackError) -> Self {
        match err {
            PlaybackError::SongNotFound { id } => CommandError::new(
                ErrorCode::SongNotFound,
                format!("song {id} was not found in the library"),
                false,
                FallbackAction::RefreshLibrary,
            ),
            // ...
        }
    }
}
```

### Migration

One domain at a time:
1. Define the error enum
2. Change service return types from `Result<T, String>` to `Result<T, DomainError>`
3. Update command handlers to use `From` impl
4. Remove the corresponding string-matching function from `error.rs`

### Files

- New: error enum definitions in each domain module
- Modified: `src-tauri/src/commands/error.rs`, service files in `audio/`, `library/`, `lyrics/`, `separator/`

---

## PR 3: Remote Provider Trait Seam

### Problem

Three Remote Provider implementations (`google_drive.rs`, `dropbox.rs`, `webdav.rs`) each duplicate auth, file listing, and upload logic in monolithic files totaling ~3,000 lines.

### Solution

Define a `RemoteProvider` trait. Each provider becomes a thin adapter. The sync engine works generically against the trait.

### Trait Definition

```rust
#[async_trait]
pub trait RemoteProvider: Send + Sync {
    async fn authenticate(&self, credentials: &RepositoryCredentials) -> Result<RepositoryCredentials, RemoteError>;
    async fn list_files(&self, credentials: &RepositoryCredentials, path: &str) -> Result<Vec<RemoteFile>, RemoteError>;
    async fn download(&self, credentials: &RepositoryCredentials, remote_path: &str, local_path: &Path) -> Result<(), RemoteError>;
    async fn upload(&self, credentials: &RepositoryCredentials, local_path: &Path, remote_path: &str) -> Result<(), RemoteError>;
    async fn delete(&self, credentials: &RepositoryCredentials, remote_path: &str) -> Result<(), RemoteError>;
    async fn get_revision(&self, credentials: &RepositoryCredentials, path: &str) -> Result<String, RemoteError>;
}
```

### RemoteError

```rust
pub enum RemoteError {
    AuthExpired { source: anyhow::Error },
    NetworkFailed { source: anyhow::Error },
    NotFound { path: String },
    Conflict { expected_rev: String, actual_rev: String },
    ProviderSpecific { message: String },
}
```

### Adapters

- `GoogleDriveProvider` — implements `RemoteProvider` using Google Drive API
- `DropboxProvider` — implements `RemoteProvider` using Dropbox API
- `WebDAVProvider` — implements `RemoteProvider` using WebDAV protocol

### Sync Engine

`sync.rs` works against `&dyn RemoteProvider` — no provider-specific branches. The provider is selected at registration time and stored in `RemoteState` (from PR 1).

### Testing

`MockProvider` adapter enables testing the sync engine without hitting real cloud APIs. Two adapters (real + mock) justify the seam.

### Migration

1. Extract trait + `RemoteError` + `MockProvider`
2. Migrate one provider at a time: Google Drive first, then Dropbox, then WebDAV
3. Refactor `sync.rs` to use `&dyn RemoteProvider`

### Files

- New: `src-tauri/src/commands/remote_library/provider.rs`, `mock.rs`
- Modified: `google_drive.rs`, `dropbox.rs`, `webdav.rs`, `sync.rs`, `registry.rs`

---

## PR 4: Cross-Store Workflow Extraction

### Problem

`player-store.ts` reaches into `queue-store` and `library-store` via `useXStore.getState()` inside action implementations. These cross-store dependencies are invisible in imports or signatures.

### Solution

Extract playback workflow logic into a dedicated module that receives store interfaces as injected dependencies.

### Workflow Module

```typescript
// src/stores/playback-workflow.ts
export interface PlaybackWorkflowDeps {
  player: PlayerActions;
  queue: QueueActions;
  library: { separationStatuses: Record<string, SeparationStatusSnapshot> };
}

export function createPlaybackWorkflow(deps: PlaybackWorkflowDeps) {
  return {
    playSong: async (songId: string) => { /* ... */ },
    playNow: async (songId: string) => { /* ... */ },
    skipForward: async () => { /* ... */ },
    skipBack: async () => { /* ... */ },
    playNextFromQueue: async (endedSongId: string) => { /* ... */ },
  };
}
```

### Store Separation

- `player-store.ts` becomes a thin state container — snapshot, position, AirPlay state, UI-only actions (resume, pause, seek, setVolume).
- `playback-workflow.ts` owns all orchestration — playSong, skipForward, playNextFromQueue.
- `player-workflows.ts` (existing) is absorbed into the workflow module.

### Store Initialization

```typescript
// In player-store.ts
const workflow = createPlaybackWorkflow({
  player: { applySnapshot: ..., },
  queue: useQueueStore.getState(),
  library: { separationStatuses: useLibraryStore.getState().separationStatuses },
});
```

### Testing

The workflow module is testable with mock store interfaces:

```typescript
const mockDeps = {
  player: { applySnapshot: vi.fn() },
  queue: { addToQueue: vi.fn(), dequeue: vi.fn(), pushToHistory: vi.fn() },
  library: { separationStatuses: {} },
};
const workflow = createPlaybackWorkflow(mockDeps);
```

### Files

- New: `src/stores/playback-workflow.ts`, `src/stores/playback-workflow.test.ts`
- Modified: `src/stores/player-store.ts` (slim down), `src/stores/player-workflows.ts` (absorbed)

---

## PR 5: Event Subscription Factory

### Problem

`use-playback-runtime.ts` has 7 nearly-identical subscription hooks, each repeating the same `useEffect` + `cancelled` flag + `unlisten` cleanup pattern (~370 lines of boilerplate).

### Solution

Extract a generic `useEventSubscriptions` factory that encapsulates the pattern.

### Factory

```typescript
// src/hooks/use-event-subscription.ts
interface EventSubscription<T> {
  event: string;
  handler: (payload: T) => void;
}

function useEventSubscriptions<T>(
  subscriptions: EventSubscription<T>[],
  enabled: boolean,
) {
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const unlisteners: (() => void)[] = [];

    const setup = async () => {
      for (const sub of subscriptions) {
        const unlisten = await listen(sub.event, (e) => {
          if (!cancelled) sub.handler(e.payload);
        });
        if (cancelled) unlisten();
        else unlisteners.push(unlisten);
      }
    };

    void setup();
    return () => { cancelled = true; unlisteners.forEach(fn => fn()); };
  }, [enabled, ...subscriptions.flatMap(s => [s.event, s.handler])]);
}
```

### Consolidation

Each hook becomes a one-liner:

```typescript
function useSeparationEvents(enabled: boolean) {
  const updateSeparationStatus = useLibraryStore(s => s.updateSeparationStatus);
  const loadStems = usePlayerStore(s => s.loadStems);

  useEventSubscriptions([
    { event: "separation-progress", handler: (e) => updateSeparationStatus(separationProgressStatus(e)) },
    { event: "separation-complete", handler: (e) => { updateSeparationStatus(e.status); if (e.song_id === currentSongId) loadStems(); } },
    { event: "separation-error", handler: (e) => { updateSeparationStatus(separationErrorStatus(e)); notifyError(e.error); } },
  ], enabled);
}
```

### Scope

- `use-playback-runtime.ts` shrinks from ~370 lines to ~80 lines
- `useLyricsAutoFetch` stays separate — it's a reactive effect on song ID changes, not an event subscription
- `useFullscreenPlaybackRuntime` stays separate — it has different initialization logic

### Files

- New: `src/hooks/use-event-subscription.ts`, `src/hooks/use-event-subscription.test.ts`
- Modified: `src/hooks/use-playback-runtime.ts` (consolidate into factory calls)

---

## Testing Strategy

Each PR maintains test coverage:

- **PR 1**: Existing tests continue to pass. New domain module tests verify construction and clone semantics.
- **PR 2**: Error routing tests verify that each variant maps to the correct `ErrorCode` + `FallbackAction`.
- **PR 3**: Sync engine tests use `MockProvider` — no real cloud API calls.
- **PR 4**: Workflow tests use mock store interfaces — no store coupling at test time.
- **PR 5**: Subscription factory tests verify setup/cleanup lifecycle.

## Migration Risk

- **PR 1** is the highest-risk PR (touches all command handlers). Mitigate by migrating one domain at a time within the PR.
- **PR 2** is medium-risk (service return type changes). Mitigate by migrating one domain at a time.
- **PR 3** is medium-risk (trait extraction). Mitigate by migrating one provider at a time.
- **PR 4** is low-risk (frontend only, well-tested stores).
- **PR 5** is low-risk (frontend only, pure boilerplate reduction).
