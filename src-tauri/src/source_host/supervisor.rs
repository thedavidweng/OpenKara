use super::{
    engine, incompatible, runtime::SourceInstance, types::*, unknown, RequestHandle, SourceHost,
};
use serde_json::Value;
use std::sync::Arc;

/// Consecutive failed starts after which a source waits for Retry or an update.
pub(super) const MAX_START_FAILURES: u32 = 4;

impl SourceHost {
    pub(super) async fn running(&self, id: &str) -> HostResult<Arc<dyn SourceInstance>> {
        let gate = self.inner.gate(id, false);
        let _guard = gate.lock().await;
        self.running_locked(id).await
    }
    async fn running_locked(&self, id: &str) -> HostResult<Arc<dyn SourceInstance>> {
        let release = {
            let mut state = self.inner.state.lock().unwrap();
            let stored = state.stored.sources.get(id).ok_or_else(unknown)?;
            if !stored.enabled {
                return Err(HostError::new(
                    "source_disabled",
                    "Enable this source before using it.",
                ));
            }
            let release = stored.active.clone().ok_or_else(incompatible)?;
            if !self.inner.compatible(&release) {
                return Err(incompatible());
            }
            let live = state.sources.entry(id.into()).or_default();
            if live.stopping {
                return Err(HostError::new("busy", "Source work is stopping."));
            }
            if let Some(instance) = &live.instance {
                if instance.alive() {
                    return Ok(instance.clone());
                }
                live.instance = None;
            }
            if live.start_failures >= MAX_START_FAILURES {
                return Err(HostError::new(
                    "source_unavailable",
                    "Source repeatedly failed to start. Choose Retry or install an update.",
                ));
            }
            release
        };
        let result = async {
            let package = self.inner.verified_package(&release)?;
            let admit = |launch: &mut dyn FnMut() -> HostResult<()>| {
                let state = self.inner.state.lock().unwrap();
                let source = state.stored.sources.get(id).ok_or_else(unknown)?;
                if !source.enabled {
                    return Err(incompatible());
                }
                launch()
            };
            engine::start(&self.inner.config, &release, &package, &admit).await
        }
        .await;
        let still_allowed = {
            let state = self.inner.state.lock().unwrap();
            state.stored.sources.get(id).is_some_and(|source| {
                source.enabled
                    && source
                        .active
                        .as_ref()
                        .is_some_and(|active| active.archive.sha256 == release.archive.sha256)
            }) && state.sources.get(id).is_none_or(|live| !live.stopping)
        };
        if !still_allowed {
            if let Ok(instance) = &result {
                instance.stop().await;
            }
            return Err(HostError::new(
                "source_disabled",
                "Source was disabled or replaced while starting. Retry only after enabling it.",
            ));
        }
        {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.sources.entry(id.into()).or_default();
            match &result {
                Ok(instance) => {
                    live.instance = Some(instance.clone());
                    live.start_failures = 0;
                    live.error = None
                }
                Err(error) => {
                    live.start_failures += 1;
                    live.error = Some(error.clone());
                }
            }
        }
        self.inner.publish(id);
        result
    }
    pub async fn start_request(
        &self,
        id: &str,
        method: &str,
        params: Value,
    ) -> HostResult<RequestHandle> {
        if method == "handshake" || method == "request.cancel" {
            return Err(HostError::new(
                "invalid_request",
                "The host owns protocol control requests.",
            ));
        }
        let description = self.description(id)?;
        let capability = match method {
            "search" => Some("search"),
            "liked_tracks" => Some("liked_tracks"),
            "playlists" => Some("playlists"),
            "playlist.tracks" => Some("playlist_tracks"),
            "tracks.get" => Some("track_details"),
            "media.resolve" => Some(if params["purpose"] == "preview" {
                "preview"
            } else {
                "import"
            }),
            "describe" | "session.restore" | "auth.start" | "auth.poll" | "auth.complete"
            | "auth.cancel" | "auth.sign_out" => None,
            _ => {
                return Err(HostError::new(
                    "unsupported_operation",
                    "This source operation is not supported.",
                ))
            }
        };
        if capability.is_some_and(|capability| {
            !description
                .manifest
                .capabilities
                .iter()
                .any(|value| value == capability)
        }) {
            return Err(HostError::new(
                "unsupported_operation",
                "This source does not provide this capability.",
            ));
        }
        let gate = self.inner.gate(id, false);
        let _guard = gate.lock().await;
        self.running_locked(id)
            .await?
            .send(
                method,
                params,
                matches!(method, "auth.cancel" | "auth.sign_out"),
            )
            .await
    }
    pub async fn request(&self, id: &str, method: &str, params: Value) -> HostResult<Value> {
        self.start_request(id, method, params).await?.result().await
    }
    pub(super) async fn maintain(&self) {
        let ids: Vec<_> = {
            let state = self.inner.state.lock().unwrap();
            state.stored.sources.keys().cloned().collect()
        };
        for id in ids {
            let gate = self.inner.gate(&id, false);
            let Ok(guard) = gate.try_lock() else { continue };
            let (instance, activate, changed) = {
                let mut state = self.inner.state.lock().unwrap();
                let staged = state.stored.sources[&id].staged.is_some();
                let live = state.sources.entry(id.clone()).or_default();
                let mut changed = false;
                if live
                    .instance
                    .take_if(|instance| !instance.alive())
                    .is_some()
                {
                    live.error = Some(HostError::new(
                        "source_unavailable",
                        "Source stopped at a time or memory limit. Retry or install a source update.",
                    ));
                    changed = true;
                }
                let idle = live.work.is_empty()
                    && !live.stopping
                    && live
                        .instance
                        .as_ref()
                        .is_none_or(|instance| instance.pending_count() == 0);
                let stop = idle
                    && live.instance.as_ref().is_some_and(|instance| {
                        instance.idle_for() >= self.inner.config.idle_timeout
                    });
                let instance = if stop { live.instance.take() } else { None };
                (instance, staged && idle && live.install.is_none(), changed)
            };
            let stopped = instance.is_some();
            if let Some(instance) = instance {
                instance.stop().await;
            }
            drop(guard);
            if activate {
                if let Err(error) = self.activate(&id).await {
                    self.inner
                        .state
                        .lock()
                        .unwrap()
                        .sources
                        .entry(id.clone())
                        .or_default()
                        .error = Some(error);
                }
            }
            if stopped || activate || changed {
                self.inner.publish(&id);
            }
        }
    }
}
