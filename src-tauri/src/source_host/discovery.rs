use super::{binding, distribution::FetchedIndex, save, types::*, SourceHost};

const CHANGED_OWNER: &str =
    "Source publisher ownership changed. The existing installation and credentials remain protected.";

impl SourceHost {
    pub async fn refresh(&self) -> HostResult<Vec<SourceSnapshot>> {
        self.inner.check()?;
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
                            stored.owner_binding != binding(&source.publisher, &source.package_id)
                        })
                    {
                        return Err(HostError::new("owner_mismatch", CHANGED_OWNER));
                    }
                }
                let mut stored = state.stored.clone();
                stored.generation = index.generation;
                for source in &index.sources {
                    let entry = stored.sources.entry(source.source_id.clone()).or_default();
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
        self.maintain().await;
        if let Err(error) = &result {
            let mut state = self.inner.state.lock().unwrap();
            let ids: Vec<_> = state.stored.sources.keys().cloned().collect();
            for id in ids {
                state.sources.entry(id).or_default().error = Some(error.clone());
            }
        }
        for snapshot in self.snapshots() {
            let _ = self.inner.changes.send(snapshot);
        }
        result
    }
}
