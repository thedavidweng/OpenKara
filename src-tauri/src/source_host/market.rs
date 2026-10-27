use super::{registry, types::*, SourceHost, State};
use std::collections::BTreeSet;

impl SourceHost {
    /// Lists every package that the added Source Registries currently offer,
    /// with the install and update status of this device. A registry whose
    /// last fetch failed contributes its last good listing.
    pub fn market(&self) -> Vec<MarketPackage> {
        let state = self.inner.state.lock().unwrap();
        let mut market = Vec::new();
        for entry in &state.registries {
            let Some(document) = &entry.document else {
                continue;
            };
            for package in &document.packages {
                market.push(market_package(
                    &self.inner.config,
                    &state,
                    &document.registry_id,
                    package,
                ));
            }
        }
        market
    }
}

fn market_package(
    config: &HostConfig,
    state: &State,
    registry_id: &str,
    package: &registry::RegistryPackage,
) -> MarketPackage {
    let installed = state
        .stored
        .sources
        .get(&package.source_id)
        .filter(|stored| {
            stored.package.as_ref().is_some_and(|identity| {
                identity.registry_id == registry_id && identity.package_id == package.package_id
            })
        })
        .and_then(|stored| stored.active.as_ref().map(|active| (stored, active)));
    let update = installed.and_then(|(stored, active)| {
        if stored.pinned_version.is_some() {
            return None;
        }
        let candidate = registry::select(config, package, None).ok()?;
        let newer = semver::Version::parse(&candidate.version).ok()?
            > semver::Version::parse(&active.manifest.version).ok()?;
        newer.then(|| {
            let (added_origins, added_capabilities) = added_declarations(
                &active.manifest,
                &candidate.resource_origins,
                &candidate.media_origins,
                &candidate.capabilities,
            );
            MarketUpdate {
                version: candidate.version.clone(),
                requires_consent: !added_origins.is_empty() || !added_capabilities.is_empty(),
                added_origins,
                added_capabilities,
            }
        })
    });
    MarketPackage {
        registry_id: registry_id.into(),
        package_id: package.package_id.clone(),
        source_id: package.source_id.clone(),
        name: package.name.clone(),
        description: package.description.clone(),
        license: package.license.clone(),
        versions: package
            .versions
            .iter()
            .map(|version| MarketVersion {
                version: version.version.clone(),
                size: version.size,
                resource_origins: version.resource_origins.clone(),
                media_origins: version.media_origins.clone(),
                capabilities: version.capabilities.clone(),
            })
            .collect(),
        installed: installed.map(|(stored, active)| MarketInstall {
            version: active.manifest.version.clone(),
            pinned_version: stored.pinned_version.clone(),
        }),
        update,
    }
}

/// Returns the origins and capabilities that a candidate adds to the
/// declarations of the installed manifest. Removals never appear here and
/// never need consent.
pub(crate) fn added_declarations(
    current: &SourceManifest,
    resource_origins: &[String],
    media_origins: &[String],
    capabilities: &[String],
) -> (Vec<String>, Vec<String>) {
    let held: BTreeSet<&str> = current
        .resource_origins
        .iter()
        .chain(&current.media_origins)
        .map(String::as_str)
        .collect();
    let added_origins: Vec<String> = resource_origins
        .iter()
        .chain(media_origins)
        .filter(|origin| !held.contains(origin.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let held_capabilities: BTreeSet<&str> =
        current.capabilities.iter().map(String::as_str).collect();
    let added_capabilities: Vec<String> = capabilities
        .iter()
        .filter(|capability| !held_capabilities.contains(capability.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    (added_origins, added_capabilities)
}
