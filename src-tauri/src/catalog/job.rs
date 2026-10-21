use super::streaming::ResolveProgress;
use super::types::{
    StreamingImportEvent, StreamingImportJobPhase, StreamingImportJobSnapshot,
    StreamingImportTrackSnapshot, StreamingImportTrackState,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type Emitter = Box<dyn Fn(&StreamingImportEvent) + Send + Sync>;

/// Shared progress and cancel handle of one Streaming Import. The import
/// session reports per-track state here; the command layer reads snapshots
/// and flips the cancel flag.
pub struct StreamingImportJob {
    job_id: String,
    source_id: String,
    cancel: AtomicBool,
    cancellation: Mutex<Option<crate::source_host::Cancellation>>,
    snapshot: Mutex<StreamingImportJobSnapshot>,
    emit: Emitter,
}

impl StreamingImportJob {
    pub fn new(
        job_id: String,
        source_id: String,
        tracks: Vec<(String, String, String)>,
        emit: impl Fn(&StreamingImportEvent) + Send + Sync + 'static,
    ) -> Arc<Self> {
        let total = tracks.len() as u32;
        let snapshot = StreamingImportJobSnapshot {
            source_id: source_id.clone(),
            job_id: job_id.clone(),
            phase: StreamingImportJobPhase::Running,
            completed: 0,
            total,
            tracks: tracks
                .into_iter()
                .map(
                    |(remote_track_id, title, artist)| StreamingImportTrackSnapshot {
                        remote_track_id,
                        title,
                        artist,
                        state: StreamingImportTrackState::Queued,
                    },
                )
                .collect(),
        };
        Arc::new(Self {
            job_id,
            source_id,
            cancel: AtomicBool::new(false),
            cancellation: Mutex::new(None),
            snapshot: Mutex::new(snapshot),
            emit: Box::new(emit),
        })
    }

    pub fn job_id(&self) -> &str {
        &self.job_id
    }
    pub fn set_tracks(&self, tracks: Vec<(String, String, String)>) {
        let mut snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        snapshot.total = tracks.len() as u32;
        snapshot.tracks = tracks
            .into_iter()
            .map(
                |(remote_track_id, title, artist)| StreamingImportTrackSnapshot {
                    remote_track_id,
                    title,
                    artist,
                    state: StreamingImportTrackState::Queued,
                },
            )
            .collect();
    }

    pub fn bind_cancellation(&self, cancellation: crate::source_host::Cancellation) {
        *self
            .cancellation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(cancellation);
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Some(cancellation) = self
            .cancellation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            cancellation.cancel();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn snapshot(&self) -> StreamingImportJobSnapshot {
        self.snapshot
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    pub fn set_phase(&self, phase: StreamingImportJobPhase) {
        let mut guard = self
            .snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.phase = phase;
    }

    pub fn report(&self, remote_track_id: &str, state: StreamingImportTrackState) {
        let event = {
            let mut guard = self
                .snapshot
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(track) = guard
                .tracks
                .iter_mut()
                .find(|track| track.remote_track_id == remote_track_id)
            else {
                return;
            };
            track.state = state.clone();
            let (title, artist) = (track.title.clone(), track.artist.clone());
            guard.completed = guard
                .tracks
                .iter()
                .filter(|track| track.state.is_terminal())
                .count() as u32;
            StreamingImportEvent {
                job_id: self.job_id.clone(),
                source_id: self.source_id.clone(),
                remote_track_id: remote_track_id.to_owned(),
                title,
                artist,
                state,
                completed: guard.completed,
                total: guard.total,
            }
        };
        (self.emit)(&event);
    }
}

pub(super) struct JobResolveProgress<'a> {
    pub job: &'a StreamingImportJob,
    pub remote_track_id: &'a str,
}

impl ResolveProgress for JobResolveProgress<'_> {
    fn downloading(&self, percent: Option<u8>) {
        self.job.report(
            self.remote_track_id,
            StreamingImportTrackState::Downloading {
                percent: percent.map(|value| value.min(100)),
            },
        );
    }

    fn is_cancelled(&self) -> bool {
        self.job.is_cancelled()
    }
}
