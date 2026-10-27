use super::{save, types::*, SourceHost};
use std::{collections::BTreeSet, fs};

const NODE_RUNTIME: &str = "node";

pub(super) fn replacement_required() -> HostError {
    HostError::new(
        "replacement_required",
        "This source used the retired Node runtime and was removed. Add a Source Registry, install its replacement, and sign in again.",
    )
}

pub(super) fn is_node(release: &Release) -> bool {
    release.manifest.runtime.kind == NODE_RUNTIME
}

/// Marks every signed-index installation whose active or staged release runs
/// on Node as needing replacement. Returns whether any source changed.
pub(super) fn retire_node_installs(stored: &mut StoredState) -> bool {
    let mut changed = false;
    for source in stored.sources.values_mut() {
        if source.package.is_some() {
            continue;
        }
        let Some(release) = [&source.active, &source.staged]
            .into_iter()
            .flatten()
            .find(|release| is_node(release))
        else {
            continue;
        };
        source.retired = Some(RetiredInstall {
            name: release.manifest.name.clone(),
            version: release.manifest.version.clone(),
            credentials_removed: false,
            notice_dismissed: false,
        });
        source.active = None;
        source.staged = None;
        source.enabled = false;
        source.pinned_version = None;
        changed = true;
    }
    changed
}

/// Deletes the managed Node runtimes and every package tree whose manifest
/// names the Node runtime. A failed deletion is retried on the next launch.
pub(super) fn remove_node_files(config: &HostConfig) {
    let runtimes = config.data_root.join("runtimes");
    if runtimes.exists() {
        if let Err(error) = fs::remove_dir_all(&runtimes) {
            tracing::warn!(%error, "could not delete retired Node runtimes");
        }
    }
    let Ok(entries) = fs::read_dir(config.data_root.join("packages")) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let node = fs::read(path.join("manifest.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .is_some_and(|manifest| manifest["runtime"]["kind"] == NODE_RUNTIME);
        if node {
            if let Err(error) = fs::remove_dir_all(&path) {
                tracing::warn!(%error, "could not delete a retired Node package");
            }
        }
    }
}

impl SourceHost {
    /// Lists the retired Node-era sources that the one-time replacement notice
    /// names until the user dismisses it.
    pub fn replacement_notice(&self) -> Vec<RetiredSource> {
        let state = self.inner.state.lock().unwrap();
        state
            .stored
            .sources
            .iter()
            .filter_map(|(id, source)| {
                source
                    .retired
                    .as_ref()
                    .filter(|retired| !retired.notice_dismissed)
                    .map(|retired| RetiredSource {
                        source_id: id.clone(),
                        name: retired.name.clone(),
                        version: retired.version.clone(),
                    })
            })
            .collect()
    }
    pub fn dismiss_replacement_notice(&self) -> HostResult<()> {
        self.update_retired(|_, retired| {
            let changed = !retired.notice_dismissed;
            retired.notice_dismissed = true;
            changed
        })
    }
    /// Calls `remove(source_id, owner_binding)` for each retired source whose
    /// Streaming Credentials are not removed yet. A failed removal stays
    /// pending for the next call.
    pub fn remove_retired_credentials<E>(
        &self,
        mut remove: impl FnMut(&str, &str) -> Result<(), E>,
    ) -> HostResult<()> {
        let pending: Vec<(String, String)> = {
            let state = self.inner.state.lock().unwrap();
            state
                .stored
                .sources
                .iter()
                .filter(|(_, source)| {
                    source
                        .retired
                        .as_ref()
                        .is_some_and(|retired| !retired.credentials_removed)
                })
                .map(|(id, source)| (id.clone(), source.owner_binding.clone()))
                .collect()
        };
        let removed: BTreeSet<String> = pending
            .into_iter()
            .filter(|(id, owner)| remove(id, owner).is_ok())
            .map(|(id, _)| id)
            .collect();
        self.update_retired(|id, retired| {
            let changed = removed.contains(id) && !retired.credentials_removed;
            retired.credentials_removed |= changed;
            changed
        })
    }
    fn update_retired(
        &self,
        mut update: impl FnMut(&str, &mut RetiredInstall) -> bool,
    ) -> HostResult<()> {
        self.inner.check()?;
        let mut state = self.inner.state.lock().unwrap();
        let mut stored = state.stored.clone();
        let mut changed = false;
        for (id, source) in &mut stored.sources {
            if let Some(retired) = source.retired.as_mut() {
                changed |= update(id, retired);
            }
        }
        if changed {
            save(&self.inner.config, &stored)?;
            state.stored = stored;
        }
        Ok(())
    }
}
