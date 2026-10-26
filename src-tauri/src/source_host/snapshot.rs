use super::types::*;
use super::{unknown, Inner, SourceHost, State};

impl Inner {
    pub(super) fn publish(&self, id: &str) {
        if let Ok(snapshot) = self.snapshot(id) {
            let _ = self.changes.send(snapshot);
        }
    }
    pub(super) fn snapshot(&self, id: &str) -> HostResult<SourceSnapshot> {
        self.check()?;
        let state = self.state.lock().unwrap();
        snapshot(self, &state, id)
    }
}
fn snapshot(inner: &Inner, state: &State, id: &str) -> HostResult<SourceSnapshot> {
    let stored = state.stored.sources.get(id).ok_or_else(unknown)?;
    let live = state.sources.get(id);
    let available = state
        .index
        .as_ref()
        .and_then(|index| index.sources.iter().find(|source| source.source_id == id))
        .and_then(|source| {
            source
                .releases
                .iter()
                .filter(|release| !stored.revoked.contains(&release.manifest.version))
                .max_by_key(|release| semver::Version::parse(&release.manifest.version).ok())
        });
    let manifest = stored
        .active
        .as_ref()
        .or(available)
        .map(|release| release.manifest.clone());
    let invalid = stored
        .active
        .as_ref()
        .is_some_and(|release| inner.blocked(stored, release));
    let installing = live.is_some_and(|live| live.install.is_some());
    let active_work = live
        .map(|live| {
            live.work
                .iter()
                .map(|(id, (kind, cancel))| ActiveWork {
                    id: *id,
                    kind: kind.clone(),
                    cancelling: cancel.is_cancelled(),
                })
                .collect()
        })
        .unwrap_or_default();
    let status = if invalid {
        SourceState::UpdateRequired
    } else if live.is_some_and(|live| live.stopping) {
        SourceState::Cancelling
    } else if installing {
        SourceState::Installing
    } else if stored.staged.is_some() {
        SourceState::WaitingForIdle
    } else if stored.active.is_none() {
        SourceState::NotInstalled
    } else if !stored.enabled {
        SourceState::Disabled
    } else if live.is_some_and(|live| live.failures >= 4) {
        SourceState::Unavailable
    } else {
        SourceState::Ready
    };
    Ok(SourceSnapshot {
        source_id: id.into(),
        owner_binding: stored.owner_binding.clone(),
        name: manifest
            .as_ref()
            .map(|manifest| manifest.name.clone())
            .unwrap_or_else(|| id.into()),
        description: manifest
            .as_ref()
            .map(|manifest| manifest.description.clone())
            .unwrap_or_default(),
        license: manifest
            .as_ref()
            .map(|manifest| manifest.license.clone())
            .unwrap_or_default(),
        installed_version: stored.active.as_ref().map(|r| r.manifest.version.clone()),
        available_version: available.map(|r| r.manifest.version.clone()),
        staged_version: stored.staged.as_ref().map(|r| r.manifest.version.clone()),
        pinned_version: stored.pinned_version.clone(),
        enabled: stored.enabled,
        runtime_ready: stored.active.is_some() && !invalid,
        running: live
            .and_then(|live| live.instance.as_ref())
            .is_some_and(|instance| instance.alive()),
        state: status,
        progress: live.and_then(|live| live.progress.clone()),
        last_error: live.and_then(|live| live.error.clone()),
        active_work,
        manifest,
    })
}
impl SourceHost {
    pub fn snapshots(&self) -> Vec<SourceSnapshot> {
        let state = self.inner.state.lock().unwrap();
        state
            .stored
            .sources
            .keys()
            .filter_map(|id| snapshot(&self.inner, &state, id).ok())
            .collect()
    }
    pub fn snapshot(&self, id: &str) -> HostResult<SourceSnapshot> {
        self.inner.snapshot(id)
    }
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SourceSnapshot> {
        self.inner.changes.subscribe()
    }
    pub fn description(&self, id: &str) -> HostResult<VerifiedSource> {
        let snapshot = self.snapshot(id)?;
        Ok(VerifiedSource {
            source_id: id.into(),
            owner_binding: snapshot.owner_binding,
            manifest: snapshot.manifest.ok_or_else(unknown)?,
        })
    }
}
