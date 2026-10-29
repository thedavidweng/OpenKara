use super::types::*;
use super::{migration::replacement_required, registry, unknown, Inner, SourceHost, State};

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
    let retired = stored.retired.as_ref();
    let listed = stored
        .package
        .as_ref()
        .and_then(|identity| registry::package(&state.registries, identity));
    let manifest = stored
        .active
        .as_ref()
        .map(|release| release.manifest.clone());
    let invalid = stored
        .active
        .as_ref()
        .is_some_and(|release| !inner.compatible(release));
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
    // A carried-over retired record on an installed replacement is only a
    // pending old-owner credential cleanup; the source still runs.
    let needs_replacement = retired.is_some() && stored.package.is_none();
    let status = if invalid || needs_replacement {
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
    } else if live.is_some_and(|live| live.start_failures >= super::supervisor::MAX_START_FAILURES)
    {
        SourceState::Unavailable
    } else {
        SourceState::Ready
    };
    Ok(SourceSnapshot {
        source_id: id.into(),
        owner_binding: stored.owner_binding.clone(),
        registry_id: stored
            .package
            .as_ref()
            .map(|identity| identity.registry_id.clone()),
        package_id: stored
            .package
            .as_ref()
            .map(|identity| identity.package_id.clone()),
        name: manifest
            .as_ref()
            .map(|manifest| manifest.name.clone())
            .or_else(|| listed.map(|package| package.name.clone()))
            .or_else(|| retired.map(|retired| retired.name.clone()))
            .unwrap_or_else(|| id.into()),
        description: manifest
            .as_ref()
            .map(|manifest| manifest.description.clone())
            .or_else(|| listed.map(|package| package.description.clone()))
            .unwrap_or_default(),
        license: manifest
            .as_ref()
            .map(|manifest| manifest.license.clone())
            .or_else(|| listed.map(|package| package.license.clone()))
            .unwrap_or_default(),
        installed_version: stored.active.as_ref().map(|r| r.manifest.version.clone()),
        available_version: listed
            .and_then(registry::newest)
            .map(|version| version.version.clone()),
        staged_version: stored.staged.as_ref().map(|r| r.manifest.version.clone()),
        pinned_version: stored.pinned_version.clone(),
        enabled: stored.enabled,
        loaded: stored.active.is_some() && !invalid,
        running: live
            .and_then(|live| live.instance.as_ref())
            .is_some_and(|instance| instance.alive()),
        state: status,
        progress: live.and_then(|live| live.progress.clone()),
        last_error: match retired {
            Some(_) if needs_replacement => Some(replacement_required()),
            _ => live.and_then(|live| live.error.clone()),
        },
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
