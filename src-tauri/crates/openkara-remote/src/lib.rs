pub mod error;
pub mod manifest;
pub mod storage;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use error::{
    kind_from_http_status_code, kind_from_io_error, verify_content_range, RemoteError,
    RemoteErrorKind, RemoteObjectMetadata, RemoteProviderCapabilities, RemoteResult,
};
pub use manifest::{
    database_directory_for_generation, database_path_for_generation, database_path_for_operation,
    staging_dir_for_operation, validate_manifest, RepositoryManifest, CURRENT_SCHEMA_VERSION,
    MANIFEST_PATH,
};
pub use storage::{
    is_under_prefix, validate_object_path, validate_prefix, ListedObject, ObjectMetadata,
    RepositoryStorage, StorageCapabilities,
};
