use super::{binding, distribution::FetchedIndex, registry, save, types::*, SourceHost};

const CHANGED_OWNER: &str =
    "Source publisher ownership changed. The existing installation and credentials remain protected.";

impl SourceHost {
    /// Refreshes every added Source Registry and the signed index. A registry
    /// failure keeps that registry's last good listing and appears in
    /// `registries()`; only a signed-index failure fails the refresh.
    pub async fn refresh(&self) -> HostResult<Vec<SourceSnapshot>> {
        self.inner.check()?;
        self.refresh_registries(false).await;
        let result = if self.inner.config.index_url.is_empty() {
            Ok(self.snapshots())
        } else {
            self.refresh_index().await
        };
        self.maintain().await;
        if let Err(error) = &result {
            let mut state = self.inner.state.lock().unwrap();
            let ids: Vec<_> = state
                .stored
                .sources
                .iter()
                .filter(|(_, source)| source.package.is_none())
                .map(|(id, _)| id.clone())
                .collect();
            for id in ids {
                state.sources.entry(id).or_default().error = Some(error.clone());
            }
        }
        for snapshot in self.snapshots() {
            let _ = self.inner.changes.send(snapshot);
        }
        result
    }
    async fn refresh_index(&self) -> HostResult<Vec<SourceSnapshot>> {
        let result: HostResult<Vec<SourceSnapshot>> = async {
            let config = &self.inner.config;
            let FetchedIndex { index, bytes } = self.inner.distribution.fetch_index(config).await?;
            {
                let mut state = self.inner.state.lock().unwrap();
                if index.generation < state.stored.generation {
                    return Err(HostError::new(
                        "untrusted_index",
                        "Source index is older than the last verified index. Retry later.",
                    ));
                }
                for source in &index.sources {
                    if state
                        .stored
                        .sources
                        .get(&source.source_id)
                        .is_some_and(|stored| {
                            stored.package.is_none()
                                && stored.owner_binding
                                    != binding(&source.publisher, &source.package_id)
                        })
                    {
                        return Err(HostError::new("owner_mismatch", CHANGED_OWNER));
                    }
                }
                let mut stored = state.stored.clone();
                stored.generation = index.generation;
                for source in &index.sources {
                    let entry = stored.sources.entry(source.source_id.clone()).or_default();
                    if entry.package.is_some() {
                        continue;
                    }
                    entry.owner_binding = binding(&source.publisher, &source.package_id);
                    for version in &source.revoked_versions {
                        if !entry.revoked.contains(version) {
                            entry.revoked.push(version.clone());
                        }
                    }
                }
                state.stored = stored;
                state.index = Some(index);
                let revoked_ids: Vec<_> = state
                    .stored
                    .sources
                    .iter()
                    .filter(|(_, source)| {
                        source.active.as_ref().is_some_and(|release| {
                            source.revoked.contains(&release.manifest.version)
                        })
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in revoked_ids {
                    if let Some(live) = state.sources.get(&id) {
                        for (_, cancel) in live.work.values() {
                            cancel.cancel();
                        }
                    }
                }
                let cached = self.inner.distribution.cache_index(config, &bytes);
                let saved = save(config, &state.stored);
                cached?;
                saved?;
            }
            Ok(self.snapshots())
        }
        .await;
        result
    }

    /// Adds a Source Registry by its HTTPS URL after its document validates.
    /// Adding a registry installs nothing.
    pub async fn add_registry(&self, url: &str) -> HostResult<RegistryStatus> {
        self.inner.check()?;
        let config = &self.inner.config;
        let url = registry::https_url(config, url)?;
        let _guard = self.inner.registry_refresh.lock().await;
        let document = registry::fetch(config, &url).await?;
        let mut state = self.inner.state.lock().unwrap();
        if state
            .registries
            .iter()
            .any(|entry| (entry.url == url) != (entry.registry_id == document.registry_id))
        {
            return Err(HostError::new(
                "invalid_registry",
                "Another added Source Registry already uses this registry id, or this URL now serves a different registry. Remove the other registry first.",
            ));
        }
        let mut stored = state.stored.clone();
        if !stored.registries.iter().any(|entry| entry.url == url) {
            stored.registries.push(StoredRegistry {
                url: url.clone(),
                registry_id: document.registry_id.clone(),
            });
        }
        save(config, &stored)?;
        state.stored = stored;
        let position = match state.registries.iter().position(|entry| entry.url == url) {
            Some(position) => position,
            None => {
                state.registries.push(registry::RegistryEntry {
                    url: url.clone(),
                    registry_id: document.registry_id.clone(),
                    document: None,
                    refreshed_at: None,
                    attempted_at: None,
                    error: None,
                });
                state.registries.len() - 1
            }
        };
        let entry = &mut state.registries[position];
        entry.document = Some(document);
        entry.refreshed_at = Some(registry::now_ms());
        entry.attempted_at = Some(std::time::Instant::now());
        entry.error = None;
        let cached = registry::save_cache(config, &state.registries);
        let entry = &mut state.registries[position];
        entry.error = cached.err();
        Ok(entry.status())
    }
    pub fn registries(&self) -> Vec<RegistryStatus> {
        self.inner
            .state
            .lock()
            .unwrap()
            .registries
            .iter()
            .map(registry::RegistryEntry::status)
            .collect()
    }
    pub(super) async fn refresh_due_registries(&self) {
        if self.refresh_registries(true).await {
            for snapshot in self.snapshots() {
                let _ = self.inner.changes.send(snapshot);
            }
        }
    }
    /// Fetches each added registry (only the overdue ones when `only_due`) and
    /// returns whether any listing changed.
    async fn refresh_registries(&self, only_due: bool) -> bool {
        let config = &self.inner.config;
        let _guard = if only_due {
            match self.inner.registry_refresh.try_lock() {
                Ok(guard) => guard,
                Err(_) => return false,
            }
        } else {
            self.inner.registry_refresh.lock().await
        };
        let targets: Vec<_> = self
            .inner
            .state
            .lock()
            .unwrap()
            .registries
            .iter()
            .filter(|entry| !only_due || entry.due(config.registry_refresh_interval))
            .map(|entry| (entry.url.clone(), entry.registry_id.clone()))
            .collect();
        let mut changed = false;
        for (url, registry_id) in targets {
            let result = registry::fetch(config, &url).await.and_then(|document| {
                if document.registry_id == registry_id {
                    Ok(document)
                } else {
                    Err(HostError::new(
                        "invalid_registry",
                        "This URL now serves a different Source Registry. OpenKara keeps the last good listing.",
                    ))
                }
            });
            let mut state = self.inner.state.lock().unwrap();
            let Some(position) = state.registries.iter().position(|entry| entry.url == url) else {
                continue;
            };
            let entry = &mut state.registries[position];
            entry.attempted_at = Some(std::time::Instant::now());
            match result {
                Ok(document) => {
                    changed |= entry.document.as_ref() != Some(&document);
                    entry.document = Some(document);
                    entry.refreshed_at = Some(registry::now_ms());
                    entry.error = None;
                    if let Err(error) = registry::save_cache(config, &state.registries) {
                        state.registries[position].error = Some(error);
                    }
                }
                Err(error) => entry.error = Some(error),
            }
        }
        changed
    }
}
