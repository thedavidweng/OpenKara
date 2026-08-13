use anyhow::{bail, Context, Result};
use openkara_lib::{
    audio::{
        coordinator::{spawn_coordinator, CoordinatorRuntime, PlaybackCommand},
        playback::{PlaybackController, PlaybackStateSnapshot},
    },
    library_root::LibraryRoot,
    lyrics::{
        acquisition::{LyricsAcquisition, LyricsPersistenceResult},
        lrcapi::LrcApiClient,
        lrclib::LrcLibClient,
    },
    separator::verified_manifest::{sha256_hex, write_verified_manifest},
    state::{AppState, PlaybackState},
};
use rusqlite::Connection;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::test::{mock_app, MockRuntime};

static TEMP_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

#[allow(dead_code)]
pub fn unique_temp_path(prefix: &str) -> PathBuf {
    // Parallel integration tests were colliding on timestamp-only names and deleting each
    // other's fixtures. A per-process counter keeps temp paths unique even when the clock
    // resolution is coarser than the test scheduler.
    let sequence = TEMP_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "openkara-{prefix}-{pid}-{timestamp}-{sequence}",
        pid = std::process::id()
    ))
}

#[allow(dead_code)]
pub fn acquire_and_persist_lyrics(
    connection: &Connection,
    library_root: &LibraryRoot,
    lrclib_client: &LrcLibClient,
    lrcapi_client: &LrcApiClient,
    song_id: &str,
) -> Result<LyricsPersistenceResult> {
    let acquisition = LyricsAcquisition::new(lrclib_client, lrcapi_client);
    let result = acquisition.acquire(connection, library_root, song_id)?;
    Ok(LyricsAcquisition::persist_acquisition(
        connection, song_id, &result,
    )?)
}

/// Materialize a verified managed install from an in-memory payload: verify the
/// payload against `expected_sha256`, write the model bytes, then persist the
/// startup verification manifest — the exact on-disk shape a real streaming
/// install leaves behind. Used by the phase6 integration tests to stage a
/// trusted install without exercising the network download path.
// Not every integration-test binary that pulls in this shared support module
// stages a managed install, so this helper is dead code in some of them.
#[allow(dead_code)]
pub fn install_verified_model_bytes(
    destination: &Path,
    payload: &[u8],
    expected_sha256: &str,
) -> Result<()> {
    let actual_sha256 = sha256_hex(payload);
    if actual_sha256 != expected_sha256 {
        // Reject before touching the filesystem so a mismatch never leaves a
        // partial install behind.
        bail!(
            "downloaded model checksum mismatch: expected {expected_sha256}, got {actual_sha256}"
        );
    }

    let parent = destination.parent().with_context(|| {
        format!(
            "model destination {} is missing a parent directory",
            destination.display()
        )
    })?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create model destination directory {}",
            parent.display()
        )
    })?;
    fs::write(destination, payload)
        .with_context(|| format!("failed to write model fixture {}", destination.display()))?;
    write_verified_manifest(destination, expected_sha256)?;

    Ok(())
}

/// A live playback stack — real `AppState`, real coordinator thread — so a
/// test can drive `services::track_load` the way the `play` command does.
///
/// The output thread is marked started because CI machines have no audio
/// device; everything else is the production wiring.
#[allow(dead_code)]
pub struct PlaybackHarness {
    app: tauri::App<MockRuntime>,
    state: AppState,
    coordinator: Option<JoinHandle<()>>,
    app_data_dir: PathBuf,
}

#[allow(dead_code)]
impl PlaybackHarness {
    pub fn new(library: &LibraryRoot) -> Self {
        let app = mock_app();
        let controller = Arc::new(Mutex::new(PlaybackController::default()));
        let (playback_state, command_rx) = PlaybackState::new(Arc::clone(&controller));
        playback_state
            .audio_output_started
            .store(true, Ordering::SeqCst);

        let app_data_dir = unique_temp_path("playback-harness-data");
        fs::create_dir_all(&app_data_dir).expect("harness app data directory should create");

        let mut state = AppState::test_fixture();
        state.playback = playback_state;
        state.shell.app_data_dir = app_data_dir.clone();
        *state
            .shell
            .library
            .lock()
            .expect("library lock should not be poisoned") = Some(library.clone());

        let coordinator = spawn_coordinator(
            CoordinatorRuntime {
                app_handle: app.handle().clone(),
                playback: controller,
                cdg_state: Arc::clone(&state.playback.cdg_state),
                latest_request_id: Arc::clone(&state.playback.playback_request_id),
                output_started: Arc::clone(&state.playback.audio_output_started),
                output_start_lock: Arc::clone(&state.playback.audio_output_start_lock),
                airplay: state.airplay.clone(),
                shutdown: Arc::clone(&state.shell.shutdown),
                peak_ring: Arc::clone(&state.playback.peak_ring),
                output_format: Arc::clone(&state.playback.output_format),
            },
            command_rx,
        );

        Self {
            app,
            state,
            coordinator: Some(coordinator),
            app_data_dir,
        }
    }

    /// Start the real load path and block until the coordinator has installed
    /// the track.
    pub fn play(&self, song_id: &str) -> Result<PlaybackStateSnapshot> {
        let app_handle = self.app.handle().clone();
        let loading = openkara_lib::services::track_load::start(&self.state, &app_handle, song_id)?;
        if loading.state != "loading" {
            bail!("expected a loading snapshot, got {}", loading.state);
        }
        self.wait_until_installed(song_id)
    }

    pub fn snapshot(&self) -> PlaybackStateSnapshot {
        self.state
            .playback
            .playback
            .lock()
            .expect("playback lock should not be poisoned")
            .snapshot()
    }

    fn wait_until_installed(&self, song_id: &str) -> Result<PlaybackStateSnapshot> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let snapshot = self.snapshot();
            match snapshot.song_id.as_deref() {
                Some(id) if id == song_id && snapshot.state != "loading" => return Ok(snapshot),
                None => bail!("playback load for {song_id} failed"),
                _ => {}
            }
            if Instant::now() >= deadline {
                bail!("timed out waiting for {song_id} to install");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for PlaybackHarness {
    fn drop(&mut self) {
        self.state.shell.shutdown.store(true, Ordering::Relaxed);
        let (reply, _rx) = tokio::sync::oneshot::channel();
        let _ = self
            .state
            .playback
            .command_tx
            .send(PlaybackCommand::Pause { reply });
        if let Some(handle) = self.coordinator.take() {
            let _ = handle.join();
        }
        let _ = fs::remove_dir_all(&self.app_data_dir);
    }
}
