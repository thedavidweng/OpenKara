//! Auth + registry binding seam for Remote Providers.
//!
//! File ops live on [`super::provider::RemoteProvider`]. Auth/registry work
//! happens *before* a stored secret exists, so this module owns the sibling
//! surface: create/resolve a remote root from an in-progress session, and bind
//! Repository Credentials into the system store (shared by Register and
//! Reauthorize).

use super::{
    dropbox::{
        dropbox_ensure_folder_with_token, normalize_dropbox_root_path, store_dropbox_secret,
    },
    google_drive::{
        google_drive_get_or_create_folder_with_token, store_google_drive_secret,
        GOOGLE_DRIVE_ROOT_ID,
    },
    types::{DropboxSecret, GoogleDriveSecret, ProviderSessionData, WebDavSessionData},
    webdav::{join_url, normalize_webdav_root_path, store_webdav_secret},
};
use crate::{
    commands::error::{CommandError, CommandResult},
    config::RemoteLibraryConnectionConfig,
    library::error::LibraryError,
};
use std::path::Path;

impl ProviderSessionData {
    /// Mutates session state when the provider needs to remember the new root
    /// (e.g. Google Drive folder id).
    pub(crate) fn create_remote_root(&mut self, display_name: &str) -> CommandResult<String> {
        match self {
            Self::GoogleDrive(google) => {
                let access_token = google.access_token.clone().ok_or_else(|| {
                    CommandError::from(LibraryError::Internal(
                        "Google Drive sign-in has not completed yet. Finish the browser flow first."
                            .to_owned(),
                    ))
                })?;
                let root = google_drive_get_or_create_folder_with_token(
                    &access_token,
                    GOOGLE_DRIVE_ROOT_ID,
                    display_name,
                )?;
                google.root_folder_id = Some(root.id.clone());
                Ok(root.id)
            }
            Self::Dropbox(dropbox) => {
                let access_token = dropbox.access_token.clone().ok_or_else(|| {
                    CommandError::from(LibraryError::Internal(
                        "Dropbox sign-in has not completed yet. Finish the browser flow first."
                            .to_owned(),
                    ))
                })?;
                let root_path = normalize_dropbox_root_path(None, display_name);
                dropbox_ensure_folder_with_token(&access_token, &root_path)?;
                Ok(root_path)
            }
            Self::WebDav(webdav) => {
                let root_path =
                    normalize_webdav_root_path(webdav.root_path.as_deref(), display_name);
                join_url(&webdav.server_url, &format!("{root_path}/"))
            }
        }
    }

    /// Does not create folders on the remote.
    pub(crate) fn resolve_remote_root(
        &self,
        existing_locator: Option<&str>,
        display_name: &str,
    ) -> CommandResult<String> {
        match self {
            Self::GoogleDrive(google) => google.root_folder_id.clone().ok_or_else(|| {
                CommandError::from(LibraryError::Internal(
                    "Google Drive reauthorization did not resolve a remote repository folder."
                        .to_owned(),
                ))
            }),
            Self::Dropbox(_) => Ok(existing_locator
                .map(str::to_owned)
                .unwrap_or_else(|| normalize_dropbox_root_path(None, display_name))),
            Self::WebDav(webdav) => {
                let root_path =
                    normalize_webdav_root_path(webdav.root_path.as_deref(), display_name);
                join_url(&webdav.server_url, &format!("{root_path}/"))
            }
        }
    }

    /// Shared by Register Repository and Reauthorize Repository so both paths
    /// bind secrets through one adapter-owned implementation.
    pub(crate) fn bind_repository_credentials(
        &self,
        app_data_dir: &Path,
        library_id: &str,
        context: BindContext,
    ) -> CommandResult<RemoteLibraryConnectionConfig> {
        match self {
            Self::GoogleDrive(google) => {
                let access_token = google.access_token.clone().ok_or_else(|| {
                    CommandError::from(LibraryError::Internal(
                        "Google Drive sign-in has not completed yet. Finish the browser flow first."
                            .to_owned(),
                    ))
                })?;
                let refresh_token = google.refresh_token.clone().ok_or_else(|| {
                    CommandError::from(LibraryError::Internal(match context {
                        BindContext::Register => {
                            "Google Drive did not return a refresh token. Reconnect and ensure consent was granted."
                                .to_owned()
                        }
                        BindContext::Reauthorize => {
                            "Google Drive did not return a refresh token. Reauthorize and ensure consent was granted."
                                .to_owned()
                        }
                    }))
                })?;
                let oauth_client_id = google.client_id.clone();
                store_google_drive_secret(
                    app_data_dir,
                    GoogleDriveSecret {
                        library_id: library_id.to_owned(),
                        client_id: google.client_id.clone(),
                        client_secret: google.client_secret.clone(),
                        access_token,
                        refresh_token,
                        access_token_expires_at_ms: google.access_token_expires_at_ms,
                    },
                )?;
                Ok(RemoteLibraryConnectionConfig::GoogleDrive { oauth_client_id })
            }
            Self::Dropbox(dropbox) => {
                let access_token = dropbox.access_token.clone().ok_or_else(|| {
                    CommandError::from(LibraryError::Internal(
                        "Dropbox sign-in has not completed yet. Finish the browser flow first."
                            .to_owned(),
                    ))
                })?;
                let refresh_token = dropbox.refresh_token.clone().ok_or_else(|| {
                    CommandError::from(LibraryError::Internal(match context {
                        BindContext::Register => {
                            "Dropbox did not return a refresh token. Reconnect and ensure consent was granted."
                                .to_owned()
                        }
                        BindContext::Reauthorize => {
                            "Dropbox did not return a refresh token. Reauthorize and ensure consent was granted."
                                .to_owned()
                        }
                    }))
                })?;
                let app_key = dropbox.app_key.clone();
                store_dropbox_secret(
                    app_data_dir,
                    DropboxSecret {
                        library_id: library_id.to_owned(),
                        app_key: dropbox.app_key.clone(),
                        app_secret: dropbox.app_secret.clone(),
                        access_token,
                        refresh_token,
                        access_token_expires_at_ms: dropbox.access_token_expires_at_ms,
                    },
                )?;
                Ok(RemoteLibraryConnectionConfig::Dropbox { app_key })
            }
            Self::WebDav(webdav) => {
                let WebDavSessionData {
                    server_url,
                    username,
                    password,
                    ..
                } = webdav;
                store_webdav_secret(app_data_dir, library_id, username.clone(), password.clone())?;
                Ok(RemoteLibraryConnectionConfig::WebDav {
                    server_url: server_url.clone(),
                })
            }
        }
    }
}

/// Only affects user-facing refresh-token error copy.
#[derive(Debug, Clone, Copy)]
pub(crate) enum BindContext {
    Register,
    Reauthorize,
}
