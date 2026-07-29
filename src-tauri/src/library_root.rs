use crate::commands::unix_timestamp;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MARKER_FILENAME: &str = ".openkara-library";
const DATABASE_FILENAME: &str = "openkara.db";
const MEDIA_DIRECTORY: &str = "media";
const MEDIA_G_DIRECTORY: &str = "media-g";
const STEMS_DIRECTORY: &str = "stems";
pub const ARTWORK_DIRECTORY: &str = "artwork";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LibraryMarker {
    version: u32,
    created_at: i64,
    identifier: String,
}

#[derive(Debug, Clone)]
pub struct LibraryRoot {
    root: PathBuf,
}

impl LibraryRoot {
    pub fn create(path: &Path) -> Result<Self> {
        if path.join(MARKER_FILENAME).exists() {
            bail!("a library already exists at {}", path.display());
        }

        fs::create_dir_all(path)
            .with_context(|| format!("failed to create library directory at {}", path.display()))?;

        let marker = LibraryMarker {
            version: 1,
            created_at: unix_timestamp(),
            identifier: "com.openkara.library".to_owned(),
        };
        let marker_json =
            serde_json::to_string_pretty(&marker).context("failed to serialize library marker")?;
        fs::write(path.join(MARKER_FILENAME), marker_json)
            .with_context(|| format!("failed to write library marker at {}", path.display()))?;

        fs::create_dir_all(path.join(MEDIA_DIRECTORY))
            .context("failed to create media directory")?;
        fs::create_dir_all(path.join(MEDIA_G_DIRECTORY))
            .context("failed to create media-g directory")?;
        fs::create_dir_all(path.join(STEMS_DIRECTORY))
            .context("failed to create stems directory")?;
        fs::create_dir_all(path.join(ARTWORK_DIRECTORY))
            .context("failed to create artwork directory")?;

        Ok(Self {
            root: path.to_owned(),
        })
    }

    pub fn open(path: &Path) -> Result<Self> {
        let marker_path = path.join(MARKER_FILENAME);
        if !marker_path.exists() {
            bail!(
                "{} is not a valid OpenKara library (missing {})",
                path.display(),
                MARKER_FILENAME
            );
        }

        fs::create_dir_all(path.join(MEDIA_DIRECTORY))
            .context("failed to ensure media directory exists")?;
        fs::create_dir_all(path.join(MEDIA_G_DIRECTORY))
            .context("failed to ensure media-g directory exists")?;
        fs::create_dir_all(path.join(STEMS_DIRECTORY))
            .context("failed to ensure stems directory exists")?;
        fs::create_dir_all(path.join(ARTWORK_DIRECTORY))
            .context("failed to ensure artwork directory exists")?;

        Ok(Self {
            root: path.to_owned(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn database_path(&self) -> PathBuf {
        self.root.join(DATABASE_FILENAME)
    }

    pub fn stems_dir(&self) -> PathBuf {
        self.root.join(STEMS_DIRECTORY)
    }

    pub fn artwork_dir(&self) -> PathBuf {
        self.root.join(ARTWORK_DIRECTORY)
    }

    pub fn media_path(&self, hash: &str, ext: &str) -> PathBuf {
        self.root
            .join(MEDIA_DIRECTORY)
            .join(format!("{}.{}", hash, ext))
    }

    pub fn media_g_audio_path(&self, hash: &str, ext: &str) -> PathBuf {
        self.root
            .join(MEDIA_G_DIRECTORY)
            .join(format!("{}.{}", hash, ext))
    }

    pub fn media_g_cdg_path(&self, hash: &str) -> PathBuf {
        self.root
            .join(MEDIA_G_DIRECTORY)
            .join(format!("{}.cdg", hash))
    }

    pub fn media_g_zip_path(&self, hash: &str) -> PathBuf {
        self.root
            .join(MEDIA_G_DIRECTORY)
            .join(format!("{}.zip", hash))
    }

    pub fn resolve(&self, relative: &str) -> PathBuf {
        let native = if cfg!(windows) {
            relative.replace('/', "\\")
        } else {
            relative.to_owned()
        };
        self.root.join(native)
    }

    pub fn to_relative(&self, absolute: &Path) -> Result<String> {
        let relative = absolute.strip_prefix(&self.root).with_context(|| {
            format!(
                "{} is not inside library root {}",
                absolute.display(),
                self.root.display()
            )
        })?;

        let normalised = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");

        Ok(normalised)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_open_library() {
        let tmp = tempfile::tempdir().unwrap();
        let lib_path = tmp.path().join("TestLibrary");

        let lib = LibraryRoot::create(&lib_path).unwrap();
        assert!(lib_path.join(MARKER_FILENAME).exists());
        assert!(lib_path.join(MEDIA_DIRECTORY).is_dir());
        assert!(lib_path.join(MEDIA_G_DIRECTORY).is_dir());
        assert!(lib_path.join(STEMS_DIRECTORY).is_dir());
        assert!(lib_path.join(ARTWORK_DIRECTORY).is_dir());
        assert_eq!(lib.database_path(), lib_path.join(DATABASE_FILENAME));

        let reopened = LibraryRoot::open(&lib_path).unwrap();
        assert_eq!(reopened.root(), lib.root());
    }

    #[test]
    fn create_rejects_existing_library() {
        let tmp = tempfile::tempdir().unwrap();
        let lib_path = tmp.path().join("Existing");
        LibraryRoot::create(&lib_path).unwrap();
        assert!(LibraryRoot::create(&lib_path).is_err());
    }

    #[test]
    fn open_rejects_non_library_directory() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(LibraryRoot::open(tmp.path()).is_err());
    }

    #[test]
    fn resolve_and_to_relative_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = LibraryRoot::create(tmp.path().join("Lib").as_path()).unwrap();

        let relative = "media/abc123.mp3";
        let absolute = lib.resolve(relative);
        assert!(absolute.is_absolute());
        assert_eq!(lib.to_relative(&absolute).unwrap(), relative);
    }

    #[test]
    fn to_relative_rejects_outside_path() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = LibraryRoot::create(tmp.path().join("Lib").as_path()).unwrap();
        assert!(lib.to_relative(Path::new("/some/other/path")).is_err());
    }

    #[test]
    fn media_path_builds_correct_path() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = LibraryRoot::create(tmp.path().join("Lib").as_path()).unwrap();
        let p = lib.media_path("deadbeef", "flac");
        assert_eq!(p, tmp.path().join("Lib/media/deadbeef.flac"));
    }
}
