use super::{
    incompatible, market, migration::replacement_required, registry, runtime::PreparedRuntime,
    save, types::*, unknown, SourceHost,
};
use std::path::PathBuf;

const SOURCE_ID_TAKEN: &str =
    "Another installed source package already uses this source id. Uninstall it before you install this package.";

fn not_listed() -> HostError {
    HostError::new(
        "not_found",
        "No added Source Registry lists this package. Add or refresh the registry.",
    )
}
fn consent_required(added_origins: &[String], added_capabilities: &[String]) -> HostError {
    let mut added = Vec::new();
    if !added_origins.is_empty() {
        added.push(format!("origins {}", added_origins.join(", ")));
    }
    if !added_capabilities.is_empty() {
        added.push(format!("capabilities {}", added_capabilities.join(", ")));
    }
    HostError::new(
        "consent_required",
        format!(
            "This update adds {}. Review the new access and confirm the update.",
            added.join(" and ")
        ),
    )
}

enum Plan {
    Index(Box<Release>),
    Registry(Box<registry::Candidate>),
}
impl Plan {
    fn version(&self) -> &str {
        match self {
            Self::Index(release) => &release.manifest.version,
            Self::Registry(candidate) => &candidate.version.version,
        }
    }
}

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
    /// Installs or updates an installed source from where it came from: its
    /// Source Registry package, or the signed index. An update that adds
    /// origins or capabilities needs `consent`.
    pub async fn install(
        &self,
        id: &str,
        version: Option<&str>,
        consent: bool,
    ) -> HostResult<SourceSnapshot> {
        self.inner.check()?;
        let gate = self.inner.gate(id, true);
        let _guard = gate
            .try_lock()
            .map_err(|_| HostError::new("busy", "A source installation is already active."))?;
        self.install_locked(id, version, consent).await
    }
    /// Installs a package that an added Source Registry lists. The package
    /// identity is the registry id and package id. When another package holds
    /// the source id, only a confirmed replacement (`replace`) takes it over:
    /// the previous package is removed and its credentials do not carry over.
    pub async fn install_package(
        &self,
        registry_id: &str,
        package_id: &str,
        version: Option<&str>,
        consent: bool,
        replace: bool,
    ) -> HostResult<SourceSnapshot> {
        self.inner.check()?;
        let identity = PackageIdentity {
            registry_id: registry_id.into(),
            package_id: package_id.into(),
        };
        let id = registry::package(&self.inner.state.lock().unwrap().registries, &identity)
            .map(|package| package.source_id.clone())
            .ok_or_else(not_listed)?;
        let gate = self.inner.gate(&id, true);
        let _guard = gate
            .try_lock()
            .map_err(|_| HostError::new("busy", "A source installation is already active."))?;
        self.claim(&id, &identity, replace).await?;
        self.install_locked(&id, version, consent).await
    }
    async fn claim(&self, id: &str, identity: &PackageIdentity, replace: bool) -> HostResult<()> {
        let owner = identity.owner_binding();
        {
            let state = self.inner.state.lock().unwrap();
            if state
                .stored
                .sources
                .get(id)
                .is_some_and(|source| source.owner_binding == owner)
            {
                return Ok(());
            }
            let taken = state
                .stored
                .sources
                .get(id)
                .is_some_and(|source| source.active.is_some() || source.staged.is_some());
            if !taken {
                drop(state);
                return self.claim_free(id, identity);
            }
            if !replace {
                return Err(HostError::new("source_id_taken", SOURCE_ID_TAKEN));
            }
        }
        // A confirmed replacement stops the current package's work before its
        // claim is cleared; it refuses while work is active.
        self.stop_work(id, false).await?;
        self.claim_free(id, identity)
    }
    fn claim_free(&self, id: &str, identity: &PackageIdentity) -> HostResult<()> {
        let mut state = self.inner.state.lock().unwrap();
        let mut stored = state.stored.clone();
        *stored.sources.entry(id.into()).or_default() = StoredSource {
            owner_binding: identity.owner_binding(),
            package: Some(identity.clone()),
            ..StoredSource::default()
        };
        save(&self.inner.config, &stored)?;
        state.stored = stored;
        let live = state.sources.entry(id.into()).or_default();
        live.error = None;
        live.stopping = false;
        drop(state);
        self.inner.publish(id);
        Ok(())
    }
    fn plan(
        &self,
        id: &str,
        version: Option<&str>,
        consent: bool,
    ) -> HostResult<(Plan, Cancellation)> {
        let mut state = self.inner.state.lock().unwrap();
        let stored = state.stored.sources.get(id).ok_or_else(unknown)?;
        if stored.retired.is_some() {
            return Err(replacement_required());
        }
        let wanted = version.or(stored.pinned_version.as_deref());
        let plan = if let Some(identity) = &stored.package {
            let package = registry::package(&state.registries, identity).ok_or_else(not_listed)?;
            if package.source_id != id {
                return Err(HostError::new(
                    "invalid_package",
                    "The Source Registry now lists this package under another source id. Ask the registry maintainer.",
                ));
            }
            let version = registry::select(&self.inner.config, package, wanted)?.clone();
            Plan::Registry(Box::new(registry::Candidate {
                identity: identity.clone(),
                package: package.clone(),
                version,
            }))
        } else {
            let source = state
                .index
                .as_ref()
                .and_then(|index| index.sources.iter().find(|source| source.source_id == id))
                .ok_or_else(unknown)?;
            Plan::Index(Box::new(
                source
                    .releases
                    .iter()
                    .filter(|release| {
                        !stored.revoked.contains(&release.manifest.version)
                            && wanted.is_none_or(|wanted| wanted == release.manifest.version)
                            && self.inner.compatible(release)
                    })
                    .max_by_key(|release| semver::Version::parse(&release.manifest.version).ok())
                    .cloned()
                    .ok_or_else(incompatible)?,
            ))
        };
        if !consent {
            if let Some(active) = &stored.active {
                if active.manifest.version != plan.version() {
                    let (added_origins, added_capabilities) = match &plan {
                        Plan::Index(release) => market::added_declarations(
                            &active.manifest,
                            &release.manifest.resource_origins,
                            &release.manifest.media_origins,
                            &release.manifest.capabilities,
                        ),
                        Plan::Registry(candidate) => market::added_declarations(
                            &active.manifest,
                            &candidate.version.resource_origins,
                            &candidate.version.media_origins,
                            &candidate.version.capabilities,
                        ),
                    };
                    if !added_origins.is_empty() || !added_capabilities.is_empty() {
                        return Err(consent_required(&added_origins, &added_capabilities));
                    }
                }
            }
        }
        let cancel = Cancellation::new();
        let live = state.sources.entry(id.into()).or_default();
        live.install = Some(cancel.clone());
        live.error = None;
        Ok((plan, cancel))
    }
    async fn install_locked(
        &self,
        id: &str,
        version: Option<&str>,
        consent: bool,
    ) -> HostResult<SourceSnapshot> {
        let (plan, cancel) = match self.plan(id, version, consent) {
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
        let planned_version = plan.version().to_string();
        let result = match plan {
            Plan::Index(mut release) => {
                async {
                    let config = &self.inner.config;
                    self.inner
                        .distribution
                        .choose_repair_slot(config, &mut release);
                    self.inner
                        .runtime
                        .choose_repair_slot(config, &mut release)?;
                    self.install_release(id, &release, &cancel).await
                }
                .await
            }
            Plan::Registry(candidate) => {
                self.install_registry_release(id, &candidate, &cancel).await
            }
        };
        {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.sources.entry(id.into()).or_default();
            live.install = None;
            live.progress = None;
            if let Err(error) = &result {
                live.error = Some(error.clone());
                live.failed_install = Some(planned_version);
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
        self.stage_candidate(id, release, runtime, package, cancel)
            .await
    }
    async fn install_registry_release(
        &self,
        id: &str,
        candidate: &registry::Candidate,
        cancel: &Cancellation,
    ) -> HostResult<()> {
        let config = &self.inner.config;
        let release = registry::download(config, candidate, cancel, &|done, total| {
            self.progress(id, "package", done, total)
        })
        .await?;
        let runtime = self
            .inner
            .runtime
            .prepare(config, &release, cancel, &|_, _| {})
            .await?;
        let package = self.inner.verified_package(&release)?;
        self.stage_candidate(id, &release, runtime, package, cancel)
            .await
    }
    /// Starts a downloaded candidate to pass its compatibility check, then
    /// stages it to replace the active release once the source is idle.
    async fn stage_candidate(
        &self,
        id: &str,
        release: &Release,
        runtime: Box<dyn PreparedRuntime>,
        package: PathBuf,
        cancel: &Cancellation,
    ) -> HostResult<()> {
        let config = &self.inner.config;
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
            return self.install(id, Some(&version), false).await;
        }
        if snapshot.installed_version.is_none() {
            return self.install(id, None, false).await;
        }
        if snapshot.enabled {
            self.running(id).await?;
        }
        self.snapshot(id)
    }
}
