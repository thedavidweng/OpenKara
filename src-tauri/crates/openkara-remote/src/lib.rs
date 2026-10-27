pub mod asset;
pub(crate) mod download;
pub mod error;
pub mod manifest;
pub mod net_policy;
pub mod reader;
pub mod storage;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod webdav;

pub use asset::{
    asset_operation_id, content_digest_from_asset_path, fetch_asset, plan_asset_fetch, AssetSource,
    FetchedAsset, RangeReader, ASSET_DOWNLOAD_CHUNK_SIZE,
};
pub use error::{
    kind_from_http_status, kind_from_http_status_code, kind_from_io_error,
    remote_error_from_status, verify_content_range, RemoteError, RemoteErrorKind,
    RemoteObjectMetadata, RemoteProviderCapabilities, RemoteResult,
};
pub use manifest::{
    database_directory_for_generation, database_path_for_generation, database_path_for_operation,
    staging_dir_for_operation, validate_manifest, RepositoryManifest, CURRENT_SCHEMA_VERSION,
    MANIFEST_PATH,
};
pub use reader::{
    plan_refresh, probe_remote_revision, pull_database_snapshot, refresh_repository,
    remote_revision_is_stale, snapshot_operation_id, RefreshOutcome, RefreshPlan, RefreshPolicy,
    RefreshedSnapshot, SnapshotSource, WorkingCopyDatabase, LEGACY_DATABASE_PATH,
    SNAPSHOT_DOWNLOAD_CHUNK_SIZE,
};
pub use storage::{
    is_under_prefix, validate_object_path, validate_prefix, ListedObject, ObjectMetadata,
    RepositoryStorage, StorageCapabilities,
};
pub use webdav::WebDavStorage;
