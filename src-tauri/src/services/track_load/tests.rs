use super::*;
use crate::{
    audio::{
        coordinator::{spawn_coordinator, CoordinatorRuntime},
        decode::DecodedAudio,
        remote_source::FetchEvent,
    },
    cache::stems::StemCacheEntry,
    state::AppState,
};
use std::{
    fs,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tauri::test::{mock_app, MockRuntime};
use tempfile::TempDir;

type SnapshotReply = tokio::sync::oneshot::Sender<Result<PlaybackStateSnapshot, PlaybackError>>;
type StartResult = Result<PlaybackStateSnapshot, PlaybackError>;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const QUIESCE: Duration = Duration::from_millis(300);

fn fixture_wav() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("audio")
        .join("fixture.wav")
}

fn dummy_audio() -> DecodedAudio {
    DecodedAudio {
        sample_rate_hz: 44_100,
        channels: 2,
        duration_ms: 5_000,
        samples: vec![0.0; 44_100 * 2],
    }
}

fn describe(command: &PlaybackCommand) -> &'static str {
    match command {
        PlaybackCommand::BeginLoad { .. } => "BeginLoad",
        PlaybackCommand::InstallReady { .. } => "InstallReady",
        PlaybackCommand::FailLoad { .. } => "FailLoad",
        PlaybackCommand::AttachStems { .. } => "AttachStems",
        PlaybackCommand::ReplaceStreamingSource { .. } => "ReplaceStreamingSource",
        _ => "another command",
    }
}

/// A real `AppState` over a real on-disk library, with the coordinator
/// replaced by a channel the test drains, so assertions land on the command
/// sequence `track_load` produces. `start_coordinator` adds a real
/// coordinator over the same controller when a command has to be adjudicated.
struct Fixture {
    _library_dir: TempDir,
    _app_data_dir: TempDir,
    library: LibraryRoot,
    state: AppState,
    commands: mpsc::Receiver<PlaybackCommand>,
    app: tauri::App<MockRuntime>,
    coordinator: Option<(mpsc::Sender<PlaybackCommand>, JoinHandle<()>)>,
}

impl Fixture {
    fn new() -> Self {
        let library_dir = tempfile::tempdir().expect("library temp dir");
        let app_data_dir = tempfile::tempdir().expect("app data temp dir");
        let library = LibraryRoot::create(&library_dir.path().join("library"))
            .expect("library should create");
        cache::initialize_library_database(&library.database_path())
            .expect("library database should initialize");

        let (command_tx, commands) = mpsc::channel();
        let mut state = AppState::test_fixture();
        state.playback.command_tx = command_tx;
        state.playback.playback_request_id = Arc::new(AtomicU64::new(0));
        state
            .playback
            .audio_output_started
            .store(true, Ordering::SeqCst);
        state.shell.app_data_dir = app_data_dir.path().to_path_buf();
        *state.shell.library.lock().expect("library lock") = Some(library.clone());

        Self {
            _library_dir: library_dir,
            _app_data_dir: app_data_dir,
            library,
            state,
            commands,
            app: mock_app(),
            coordinator: None,
        }
    }

    fn open_database(&self) -> Connection {
        cache::open_database(&self.library.database_path()).expect("library database should open")
    }

    fn add_song(&self, hash: &str) -> String {
        let relative = format!("media/{hash}.wav");
        let absolute = self.library.resolve(&relative);
        fs::create_dir_all(absolute.parent().expect("media directory")).expect("media directory");
        fs::copy(fixture_wav(), &absolute).expect("fixture audio should copy");
        self.upsert_song(hash, &relative);
        hash.to_owned()
    }

    /// A catalogued song whose media file was never written: the load resolves
    /// the row and then fails to open the source.
    fn add_song_without_media(&self, hash: &str) -> String {
        self.upsert_song(hash, &format!("media/{hash}.wav"));
        hash.to_owned()
    }

    fn upsert_song(&self, hash: &str, relative_path: &str) {
        cache::upsert_song(
            &self.open_database(),
            &Song {
                hash: hash.to_owned(),
                file_path: Some(relative_path.to_owned()),
                cdg_path: None,
                media_g_container: None,
                instrumental: false,
                language: None,
                audio_source_kind: "original".to_owned(),
                title: Some(hash.to_owned()),
                artist: None,
                album: None,
                duration_ms: 1_000,
                cover_art: None,
                has_cover_art: false,
                artwork_thumb_path: None,
                imported_at: 1,
                original_ext: Some("wav".to_owned()),
            },
        )
        .expect("song upsert should succeed");
    }

    fn add_cached_stems(&self, hash: &str) {
        let entry = StemCacheEntry {
            song_hash: hash.to_owned(),
            vocals_path: format!("stems/{hash}/vocals.wav"),
            accomp_path: format!("stems/{hash}/accompaniment.wav"),
            separated_at: 1,
            drums_path: None,
            bass_path: None,
            other_path: None,
            model_variant: "htdemucs".to_owned(),
        };
        for relative in [&entry.vocals_path, &entry.accomp_path] {
            let absolute = self.library.resolve(relative);
            fs::create_dir_all(absolute.parent().expect("stem directory")).expect("stem directory");
            fs::copy(fixture_wav(), &absolute).expect("fixture stem should copy");
        }
        crate::cache::stems::upsert_stem_entry_test(&self.open_database(), &entry);
    }

    fn spawn_start(&self, song_id: &str) -> JoinHandle<StartResult> {
        let state = self.state.clone();
        let app_handle = self.app.handle().clone();
        let song_id = song_id.to_owned();
        std::thread::spawn(move || super::start(&state, &app_handle, &song_id))
    }

    fn next_command(&self) -> PlaybackCommand {
        self.commands
            .recv_timeout(COMMAND_TIMEOUT)
            .expect("track_load should send a command")
    }

    /// Receive the `BeginLoad` without answering it, so the caller controls
    /// exactly when the background worker is allowed to start.
    fn take_begin_load(&self, expected_song: &str) -> (u64, SnapshotReply) {
        match self.next_command() {
            PlaybackCommand::BeginLoad {
                request_id,
                song_id,
                reply,
            } => {
                assert_eq!(song_id, expected_song);
                (request_id, reply)
            }
            other => panic!("expected BeginLoad, got {}", describe(&other)),
        }
    }

    fn answer_begin_load(&self, reply: SnapshotReply, song_id: &str) {
        let snapshot = {
            let mut playback = self.state.playback.playback.lock().expect("playback lock");
            playback.start_track_loading(song_id)
        };
        reply
            .send(Ok(snapshot))
            .unwrap_or_else(|_| panic!("start should still be awaiting the BeginLoad reply"));
    }

    fn begin_load(&self, expected_song: &str) -> u64 {
        let (request_id, reply) = self.take_begin_load(expected_song);
        self.answer_begin_load(reply, expected_song);
        request_id
    }

    /// Wire a real coordinator over the same controller and request id, so a
    /// command produced by `track_load` can be adjudicated for real.
    fn start_coordinator(&mut self) -> mpsc::Sender<PlaybackCommand> {
        let (tx, rx) = mpsc::channel();
        let handle = spawn_coordinator(
            CoordinatorRuntime {
                app_handle: self.app.handle().clone(),
                playback: Arc::clone(&self.state.playback.playback),
                cdg_state: Arc::clone(&self.state.playback.cdg_state),
                latest_request_id: Arc::clone(&self.state.playback.playback_request_id),
                output_started: Arc::clone(&self.state.playback.audio_output_started),
                output_start_lock: Arc::clone(&self.state.playback.audio_output_start_lock),
                airplay: self.state.airplay.clone(),
                shutdown: Arc::clone(&self.state.shell.shutdown),
                peak_ring: Arc::clone(&self.state.playback.peak_ring),
                output_format: Arc::clone(&self.state.playback.output_format),
            },
            rx,
        );
        self.coordinator = Some((tx.clone(), handle));
        tx
    }

    /// Install `song_id` at the current request id and block until applied.
    fn install_track(&self, coordinator: &mpsc::Sender<PlaybackCommand>, song_id: &str) {
        coordinator
            .send(PlaybackCommand::InstallReady {
                request_id: self
                    .state
                    .playback
                    .playback_request_id
                    .load(Ordering::SeqCst),
                song_id: song_id.to_owned(),
                ready: Box::new(ReadyTrack::Decoded {
                    audio: dummy_audio(),
                    stems: None,
                    cdg: None,
                    cdg_error: None,
                }),
            })
            .expect("coordinator channel open");
        let (reply, rx) = tokio::sync::oneshot::channel();
        coordinator
            .send(PlaybackCommand::Pause { reply })
            .expect("coordinator channel open");
        let _ = rx.blocking_recv().expect("coordinator should reply");
    }

    fn snapshot(&self) -> PlaybackStateSnapshot {
        self.state
            .playback
            .playback
            .lock()
            .expect("playback lock")
            .snapshot()
    }

    /// Collect load outcomes until `expected` installs, then keep draining
    /// briefly so a late install from a retired worker would still be seen.
    fn drain_until_installed(&self, expected: u64) -> (Vec<(u64, String)>, Vec<u64>) {
        let mut installs: Vec<(u64, String)> = Vec::new();
        let mut failures: Vec<u64> = Vec::new();

        let deadline = Instant::now() + COMMAND_TIMEOUT;
        while Instant::now() < deadline && !installs.iter().any(|(id, _)| *id == expected) {
            self.drain_once(&mut installs, &mut failures);
        }

        let quiesce = Instant::now() + QUIESCE;
        while Instant::now() < quiesce {
            self.drain_once(&mut installs, &mut failures);
        }

        (installs, failures)
    }

    fn drain_once(&self, installs: &mut Vec<(u64, String)>, failures: &mut Vec<u64>) {
        match self.commands.recv_timeout(Duration::from_millis(50)) {
            Ok(PlaybackCommand::InstallReady {
                request_id,
                song_id,
                ..
            }) => installs.push((request_id, song_id)),
            Ok(PlaybackCommand::FailLoad { request_id, .. }) => failures.push(request_id),
            Ok(_) | Err(_) => {}
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some((tx, handle)) = self.coordinator.take() {
            self.state.shell.shutdown.store(true, Ordering::Relaxed);
            let (reply, _rx) = tokio::sync::oneshot::channel();
            let _ = tx.send(PlaybackCommand::Pause { reply });
            let _ = handle.join();
        }
    }
}

#[test]
fn a_load_produces_begin_load_then_install_ready_for_one_request() {
    let fixture = Fixture::new();
    let song_id = fixture.add_song("song-happy");

    let start = fixture.spawn_start(&song_id);
    let request_id = fixture.begin_load(&song_id);
    let loading = start
        .join()
        .expect("start thread should not panic")
        .expect("start should return the loading snapshot");
    assert_eq!(loading.song_id.as_deref(), Some(song_id.as_str()));
    assert_eq!(loading.state, "loading");

    match fixture.next_command() {
        PlaybackCommand::InstallReady {
            request_id: installed,
            song_id: installed_song,
            ..
        } => {
            assert_eq!(installed, request_id);
            assert_eq!(installed_song, song_id);
        }
        other => panic!("expected InstallReady, got {}", describe(&other)),
    }
}

#[test]
fn a_superseded_load_is_dropped_instead_of_installed() {
    let fixture = Fixture::new();
    let first_song = fixture.add_song("song-first");
    let second_song = fixture.add_song("song-second");

    // Hold the first BeginLoad unanswered: its background worker cannot start
    // until the reply lands, by which point the second request has retired it.
    let first_start = fixture.spawn_start(&first_song);
    let (first_request, first_reply) = fixture.take_begin_load(&first_song);

    let second_start = fixture.spawn_start(&second_song);
    let (second_request, second_reply) = fixture.take_begin_load(&second_song);
    assert_eq!(second_request, first_request + 1);

    fixture.answer_begin_load(first_reply, &first_song);
    let _ = first_start.join().expect("first start thread");
    fixture.answer_begin_load(second_reply, &second_song);
    let _ = second_start
        .join()
        .expect("second start thread")
        .expect("second start should return the loading snapshot");

    let (installs, failures) = fixture.drain_until_installed(second_request);
    assert!(
        installs.contains(&(second_request, second_song)),
        "the current request must install, saw {installs:?}"
    );
    assert!(
        !installs.iter().any(|(id, _)| *id == first_request),
        "the superseded request must not install, saw {installs:?}"
    );
    assert!(failures.is_empty(), "no load should fail, saw {failures:?}");
}

#[test]
fn a_load_that_cannot_resolve_its_source_produces_fail_load() {
    let fixture = Fixture::new();
    let song_id = fixture.add_song_without_media("song-broken");

    let start = fixture.spawn_start(&song_id);
    let request_id = fixture.begin_load(&song_id);
    let _ = start
        .join()
        .expect("start thread should not panic")
        .expect("start should return the loading snapshot");

    match fixture.next_command() {
        PlaybackCommand::FailLoad {
            request_id: failed,
            song_id: failed_song,
            ..
        } => {
            assert_eq!(failed, request_id);
            assert_eq!(failed_song, song_id);
        }
        other => panic!("expected FailLoad, got {}", describe(&other)),
    }
}

#[test]
fn stem_attachment_against_a_stale_request_does_not_attach() {
    let mut fixture = Fixture::new();
    let song_id = fixture.add_song("song-stems-stale");
    fixture.add_cached_stems(&song_id);
    let coordinator = fixture.start_coordinator();

    fixture
        .state
        .playback
        .playback_request_id
        .store(1, Ordering::SeqCst);
    fixture.install_track(&coordinator, &song_id);

    let producer = fixture.state.clone();
    let attaching = std::thread::spawn(move || super::attach_stems(&producer));

    let command = fixture.next_command();
    let attached_request = match &command {
        PlaybackCommand::AttachStems { request_id, .. } => *request_id,
        other => panic!("expected AttachStems, got {}", describe(other)),
    };
    assert_eq!(
        attached_request, 1,
        "the attachment carries the request id it captured"
    );

    // The user starts another track before the coordinator reaches the
    // queued attachment.
    fixture
        .state
        .playback
        .playback_request_id
        .store(2, Ordering::SeqCst);
    coordinator.send(command).expect("coordinator channel open");

    let snapshot = attaching
        .join()
        .expect("attach_stems thread should not panic")
        .expect("attach_stems should return a snapshot");
    assert!(
        !snapshot.has_stems,
        "stems for a superseded request must not attach"
    );
    assert!(!fixture.snapshot().has_stems);
}

#[test]
fn stem_attachment_for_the_current_request_attaches() {
    let mut fixture = Fixture::new();
    let song_id = fixture.add_song("song-stems-current");
    fixture.add_cached_stems(&song_id);
    let coordinator = fixture.start_coordinator();

    fixture
        .state
        .playback
        .playback_request_id
        .store(1, Ordering::SeqCst);
    fixture.install_track(&coordinator, &song_id);

    let producer = fixture.state.clone();
    let attaching = std::thread::spawn(move || super::attach_stems(&producer));

    let command = fixture.next_command();
    assert!(
        matches!(command, PlaybackCommand::AttachStems { .. }),
        "expected AttachStems, got {}",
        describe(&command)
    );
    coordinator.send(command).expect("coordinator channel open");

    let snapshot = attaching
        .join()
        .expect("attach_stems thread should not panic")
        .expect("attach_stems should return a snapshot");
    assert!(snapshot.has_stems);
}

#[test]
fn a_fetch_event_listener_for_a_superseded_request_stops_draining() {
    let fixture = Fixture::new();
    let request = PlaybackRequest::begin(&fixture.state.playback).expect("request");
    let ctx = LoadContext {
        state: fixture.state.clone(),
        app_handle: fixture.app.handle().clone(),
        app_data_dir: fixture.state.shell.app_data_dir.clone(),
        library_root: fixture.library.clone(),
        song_id: "song-superseded".to_owned(),
        request,
    };
    let _current = PlaybackRequest::begin(&fixture.state.playback).expect("newer request");

    let (fetch_event_tx, fetch_event_rx) = mpsc::channel();
    streaming::spawn_fetch_event_listener(ctx, None, fetch_event_rx, "test fetch");

    fetch_event_tx
        .send(FetchEvent::ConsecutiveFailures { count: 5 })
        .expect("the listener should be draining before it sees the event");

    let deadline = Instant::now() + COMMAND_TIMEOUT;
    while fetch_event_tx
        .send(FetchEvent::ConsecutiveFailures { count: 5 })
        .is_ok()
    {
        assert!(
            Instant::now() < deadline,
            "the listener must drop its receiver once its request is superseded"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn beginning_a_request_supersedes_and_retires_the_previous_one() {
    let fixture = Fixture::new();

    let first = PlaybackRequest::begin(&fixture.state.playback).expect("first request");
    assert_eq!(first.id(), 1);
    assert!(first.guard().is_current());
    assert!(!first.is_cancelled());

    let second = PlaybackRequest::begin(&fixture.state.playback).expect("second request");
    assert_eq!(second.id(), 2);
    assert!(first.is_cancelled(), "the previous worker must be retired");
    assert!(!first.guard().is_current());
    assert!(second.guard().is_current());
    assert!(!second.is_cancelled());
}

#[test]
fn a_staleness_predicate_tracks_the_request_it_was_built_from() {
    let fixture = Fixture::new();
    let request = PlaybackRequest::begin(&fixture.state.playback).expect("request");

    let predicate = request.guard().predicate();
    assert!(predicate());

    fixture
        .state
        .playback
        .playback_request_id
        .fetch_add(1, Ordering::SeqCst);
    assert!(!predicate());
    assert!(!request.guard().is_current());
}
