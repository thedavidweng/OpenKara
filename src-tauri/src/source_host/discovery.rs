use super::{registry, save, types::*, SourceHost};

impl SourceHost {
    /// Refreshes every added Source Registry. A registry failure keeps that
    /// registry's last good listing and appears in `registries()`.
    pub async fn refresh(&self) -> HostResult<Vec<SourceSnapshot>> {
        self.inner.check()?;
        self.refresh_registries(false).await;
        self.maintain().await;
        let snapshots = self.snapshots();
        for snapshot in &snapshots {
            let _ = self.inner.changes.send(snapshot.clone());
        }
        Ok(snapshots)
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
    /// Removes an added Source Registry. Its packages leave the market.
    /// Installed packages keep working but can no longer update.
    pub fn remove_registry(&self, registry_id: &str) -> HostResult<()> {
        self.inner.check()?;
        let mut state = self.inner.state.lock().unwrap();
        if !state
            .registries
            .iter()
            .any(|entry| entry.registry_id == registry_id)
        {
            return Err(HostError::new(
                "not_found",
                "No added Source Registry uses this registry id.",
            ));
        }
        let mut stored = state.stored.clone();
        stored
            .registries
            .retain(|entry| entry.registry_id != registry_id);
        save(&self.inner.config, &stored)?;
        state
            .registries
            .retain(|entry| entry.registry_id != registry_id);
        state.stored = stored;
        let _ = registry::save_cache(&self.inner.config, &state.registries);
        let affected: Vec<String> = state
            .stored
            .sources
            .iter()
            .filter(|(_, source)| {
                source
                    .package
                    .as_ref()
                    .is_some_and(|identity| identity.registry_id == registry_id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        drop(state);
        for id in affected {
            self.inner.publish(&id);
        }
        Ok(())
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
