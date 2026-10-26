use super::{incompatible, save, types::*, unknown, SourceHost};

impl SourceHost {
    fn progress(&self, id: &str, phase: &str, completed: u64, total: Option<u64>) {
        self.inner
            .state
            .lock()
            .unwrap()
            .sources
            .entry(id.into())
            .or_default()
            .progress = Some(InstallProgress {
            phase: phase.into(),
            completed,
            total,
        });
        self.inner.publish(id);
    }
    pub fn cancel_install(&self, id: &str) -> HostResult<()> {
        self.inner.check()?;
        let state = self.inner.state.lock().unwrap();
        let live = state.sources.get(id).ok_or_else(unknown)?;
        if let Some(cancel) = &live.install {
            cancel.cancel();
        }
        Ok(())
    }
    pub async fn install(&self, id: &str, version: Option<&str>) -> HostResult<SourceSnapshot> {
        self.inner.check()?;
        let gate = self.inner.gate(id, true);
        let _guard = gate
            .try_lock()
            .map_err(|_| HostError::new("busy", "A source installation is already active."))?;
        let selection: HostResult<(Release, Cancellation)> = (|| {
            let mut state = self.inner.state.lock().unwrap();
            let stored = state.stored.sources.get(id).ok_or_else(unknown)?;
            let wanted = version.or(stored.pinned_version.as_deref());
            let source = state
                .index
                .as_ref()
                .and_then(|index| index.sources.iter().find(|source| source.source_id == id))
                .ok_or_else(unknown)?;
            let release = source
                .releases
                .iter()
                .filter(|release| {
                    !stored.revoked.contains(&release.manifest.version)
                        && wanted.is_none_or(|wanted| wanted == release.manifest.version)
                        && self.inner.compatible(release)
                })
                .max_by_key(|release| semver::Version::parse(&release.manifest.version).ok())
                .cloned()
                .ok_or_else(incompatible)?;
            let cancel = Cancellation::new();
            let live = state.sources.entry(id.into()).or_default();
            live.install = Some(cancel.clone());
            live.error = None;
            Ok((release, cancel))
        })();
        let (mut release, cancel) = match selection {
            Ok(selection) => selection,
            Err(error) => {
                self.inner
                    .state
                    .lock()
                    .unwrap()
                    .sources
                    .entry(id.into())
                    .or_default()
                    .error = Some(error.clone());
                self.inner.publish(id);
                return Err(error);
            }
        };
        let result = async {
            let config = &self.inner.config;
            self.inner
                .distribution
                .choose_repair_slot(config, &mut release);
            self.inner
                .runtime
                .choose_repair_slot(config, &mut release)?;
            self.install_release(id, &release, &cancel).await
        }
        .await;
        {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.sources.entry(id.into()).or_default();
            live.install = None;
            live.progress = None;
            if let Err(error) = &result {
                live.error = Some(error.clone());
                live.failed_install = Some(release.manifest.version.clone());
            } else {
                live.failed_install = None;
            }
        }
        self.inner.publish(id);
        result?;
        self.snapshot(id)
    }
    async fn install_release(
        &self,
        id: &str,
        release: &Release,
        cancel: &Cancellation,
    ) -> HostResult<()> {
        let config = &self.inner.config;
        self.progress(id, "runtime", 0, None);
        let runtime = self
            .inner
            .runtime
            .prepare(config, release, cancel, &|done, total| {
                self.progress(id, "runtime", done, total)
            })
            .await?;
        let package = self
            .inner
            .distribution
            .install_package(config, release, cancel, &|done, total| {
                self.progress(id, "package", done, total)
            })
            .await?;
        cancel.check()?;
        self.progress(id, "handshake", 0, None);
        {
            let state = self.inner.state.lock().unwrap();
            let source = state.stored.sources.get(id).ok_or_else(unknown)?;
            if self.inner.blocked(source, release) {
                return Err(incompatible());
            }
        }
        cancel.check()?;
        let admit = |launch: &mut dyn FnMut() -> HostResult<()>| {
            let state = self.inner.state.lock().unwrap();
            let source = state.stored.sources.get(id).ok_or_else(unknown)?;
            cancel.check()?;
            if self.inner.blocked(source, release) {
                return Err(incompatible());
            }
            launch()
        };
        let candidate = tokio::select! {
            _ = cancel.cancelled() => return cancel.check(),
            candidate = runtime.start(config, release, &package, &admit) => candidate?,
        };
        candidate.stop().await;
        cancel.check()?;
        self.inner.verify_installed(release)?;
        {
            let mut state = self.inner.state.lock().unwrap();
            let mut stored = state.stored.clone();
            let source = stored.sources.get_mut(id).ok_or_else(unknown)?;
            if source.revoked.contains(&release.manifest.version) {
                return Err(incompatible());
            }
            source.staged = Some(release.clone());
            save(config, &stored)?;
            state.stored = stored;
        }
        self.activate(id).await
    }
    pub(super) async fn activate(&self, id: &str) -> HostResult<()> {
        let gate = self.inner.gate(id, false);
        let _guard = gate.lock().await;
        {
            let mut state = self.inner.state.lock().unwrap();
            let source = state.stored.sources.get(id).ok_or_else(unknown)?;
            if source
                .staged
                .as_ref()
                .is_some_and(|release| self.inner.blocked(source, release))
            {
                let mut stored = state.stored.clone();
                stored.sources.get_mut(id).ok_or_else(unknown)?.staged = None;
                save(&self.inner.config, &stored)?;
                state.stored = stored;
                return Err(incompatible());
            }
        }
        let staged = self
            .inner
            .state
            .lock()
            .unwrap()
            .stored
            .sources
            .get(id)
            .and_then(|source| source.staged.clone());
        if let Some(staged) = staged {
            if let Err(error) = self.inner.verify_installed(&staged) {
                let mut state = self.inner.state.lock().unwrap();
                let mut stored = state.stored.clone();
                stored.sources.get_mut(id).ok_or_else(unknown)?.staged = None;
                let saved = save(&self.inner.config, &stored);
                state.stored = stored;
                saved?;
                return Err(error);
            }
        }
        let old = {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.sources.entry(id.into()).or_default();
            if !live.work.is_empty()
                || live.stopping
                || live
                    .instance
                    .as_ref()
                    .is_some_and(|instance| instance.pending_count() > 0)
            {
                return Ok(());
            }
            live.stopping = true;
            live.instance.take()
        };
        if let Some(old) = old {
            old.stop().await;
        }
        let result = {
            let mut state = self.inner.state.lock().unwrap();
            let mut stored = state.stored.clone();
            let source = stored.sources.get_mut(id).ok_or_else(unknown)?;
            let result = if let Some(release) = source.staged.take() {
                if source.revoked.contains(&release.manifest.version) {
                    Err(incompatible())
                } else {
                    source.active = Some(release);
                    save(&self.inner.config, &stored).map(|_| {
                        state.stored = stored;
                        let live = state.sources.entry(id.into()).or_default();
                        live.failures = 0;
                        live.error = None;
                    })
                }
            } else {
                Ok(())
            };
            state.sources.entry(id.into()).or_default().stopping = false;
            result
        };
        self.inner.publish(id);
        result
    }
    pub fn set_pin(&self, id: &str, version: Option<&str>) -> HostResult<SourceSnapshot> {
        self.inner.check()?;
        let mut state = self.inner.state.lock().unwrap();
        let mut stored = state.stored.clone();
        let source = stored.sources.get_mut(id).ok_or_else(unknown)?;
        if version.is_some_and(|version| {
            source
                .active
                .as_ref()
                .is_none_or(|active| active.manifest.version != version)
        }) {
            return Err(HostError::new(
                "invalid_request",
                "Only the installed version can be pinned.",
            ));
        }
        source.pinned_version = version.map(str::to_string);
        save(&self.inner.config, &stored)?;
        state.stored = stored;
        drop(state);
        self.inner.publish(id);
        self.snapshot(id)
    }
    pub async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
        stop_work: bool,
    ) -> HostResult<SourceSnapshot> {
        self.snapshot(id)?;
        let gate = self.inner.gate(id, true);
        let _guard = gate.lock().await;
        if enabled {
            let snapshot = self.snapshot(id)?;
            if snapshot.installed_version.is_none() || snapshot.state == SourceState::UpdateRequired
            {
                return Err(incompatible());
            }
        } else {
            self.stop_work(id, stop_work).await?;
        }
        {
            let mut state = self.inner.state.lock().unwrap();
            let mut stored = state.stored.clone();
            stored.sources.get_mut(id).ok_or_else(unknown)?.enabled = enabled;
            let result = save(&self.inner.config, &stored);
            state.sources.entry(id.into()).or_default().stopping = false;
            result?;
            state.stored = stored;
        }
        self.inner.publish(id);
        self.snapshot(id)
    }
    pub async fn uninstall(&self, id: &str, stop_work: bool) -> HostResult<SourceSnapshot> {
        self.snapshot(id)?;
        let gate = self.inner.gate(id, true);
        let _guard = gate.lock().await;
        self.stop_work(id, stop_work).await?;
        {
            let mut state = self.inner.state.lock().unwrap();
            let mut stored = state.stored.clone();
            let source = stored.sources.get_mut(id).ok_or_else(unknown)?;
            source.enabled = false;
            source.active = None;
            source.staged = None;
            source.pinned_version = None;
            let result = save(&self.inner.config, &stored);
            state.sources.entry(id.into()).or_default().stopping = false;
            result?;
            state.stored = stored;
        }
        self.inner.publish(id);
        self.snapshot(id)
    }
    pub async fn retry(&self, id: &str) -> HostResult<SourceSnapshot> {
        let snapshot = self.snapshot(id)?;
        let failed = {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.sources.entry(id.into()).or_default();
            live.failures = 0;
            live.retry_at = None;
            live.error = None;
            live.failed_install.clone()
        };
        if let Some(version) = failed {
            return self.install(id, Some(&version)).await;
        }
        if snapshot.installed_version.is_none() {
            return self.install(id, None).await;
        }
        if snapshot.enabled {
            self.running(id).await?;
        }
        self.snapshot(id)
    }
}
