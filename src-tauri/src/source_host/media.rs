use super::{incompatible, network, types::*, MediaResponse, SourceHost};
use std::{fs, path::PathBuf};

impl SourceHost {
    pub async fn download_media(
        &self,
        id: &str,
        descriptor: ResourceDescriptor,
        destination: PathBuf,
        cancellation: Cancellation,
    ) -> HostResult<()> {
        use std::io::Write;
        let mut response = self
            .open_media(id, &descriptor, None, cancellation.clone())
            .await?;
        if !(200..300).contains(&response.status) {
            return Err(HostError::new(
                if matches!(response.status, 403 | 410) {
                    "media_expired"
                } else {
                    "network"
                },
                "Source media download failed. Resolve a fresh descriptor or retry.",
            ));
        }
        let temp = destination.with_extension(format!("{}.download", uuid::Uuid::new_v4()));
        let result: HostResult<()> = async {
            let mut file = fs::File::create(&temp)?;
            let mut size = 0u64;
            while let Some(chunk) = response.chunk().await? {
                size += chunk.len() as u64;
                if size > 4 * 1024 * 1024 * 1024 {
                    return Err(HostError::new(
                        "invalid_resource",
                        "Source media exceeds the download limit.",
                    ));
                }
                file.write_all(&chunk)?;
            }
            cancellation.check()?;
            file.sync_all()?;
            fs::rename(&temp, &destination)?;
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
    pub async fn open_media(
        &self,
        id: &str,
        descriptor: &ResourceDescriptor,
        range: Option<(u64, Option<u64>)>,
        cancellation: Cancellation,
    ) -> HostResult<MediaResponse> {
        let description = self.description(id)?;
        self.ensure_download(id)?;
        network::check_expiry(descriptor)?;
        #[cfg(test)]
        if let Some(response) = network::fixture_media(
            &self.inner.config,
            descriptor,
            &description.manifest.media_origins,
            range,
            cancellation.clone(),
        ) {
            return response;
        }
        let response = network::response_range(
            &descriptor.url,
            &descriptor.headers,
            Some(&description.manifest.media_origins),
            false,
            &cancellation,
            range,
        )
        .await?;
        Ok(MediaResponse::new(response, cancellation))
    }
    fn ensure_download(&self, id: &str) -> HostResult<()> {
        let snapshot = self.snapshot(id)?;
        if !snapshot.enabled {
            return Err(HostError::new("source_disabled", "Source is disabled."));
        }
        if snapshot.state == SourceState::UpdateRequired {
            return Err(incompatible());
        }
        Ok(())
    }
    pub async fn load_resource(
        &self,
        id: &str,
        descriptor: ResourceDescriptor,
        cancellation: Cancellation,
    ) -> HostResult<Vec<u8>> {
        let description = self.description(id)?;
        self.ensure_download(id)?;
        let cancel = cancellation;
        cancel.check()?;
        network::check_expiry(&descriptor)?;
        #[cfg(test)]
        if let Some(response) = network::fixture_media(
            &self.inner.config,
            &descriptor,
            &description.manifest.resource_origins,
            None,
            cancel.clone(),
        ) {
            let mut response = response?;
            if !(200..300).contains(&response.status) {
                return Err(HostError::new(
                    "network",
                    "Source artwork is unavailable. Retry later.",
                ));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
                    return Err(HostError::new(
                        "invalid_resource",
                        "Source artwork exceeds the download limit.",
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            return Ok(bytes);
        }
        let response = network::response(
            &descriptor.url,
            &descriptor.headers,
            Some(&description.manifest.resource_origins),
            false,
            &cancel,
        )
        .await?;
        network::read_bounded(response, 16 * 1024 * 1024, &cancel).await
    }
}
