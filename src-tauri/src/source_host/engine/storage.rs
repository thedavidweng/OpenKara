use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub(super) const MAX_KEY_BYTES: usize = 256;
pub(super) const MAX_TOTAL_BYTES: usize = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StorageError {
    InvalidKey,
    Full,
    Unavailable,
}
impl StorageError {
    pub(super) fn message(&self) -> &'static str {
        match self {
            Self::InvalidKey => "storage keys must be 1 to 256 bytes",
            Self::Full => "source storage is full",
            Self::Unavailable => "source storage is unavailable",
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    owner_binding: String,
    entries: BTreeMap<String, String>,
}

/// A small key-value store that belongs to one installed package. Another
/// package that later takes the same source id starts with an empty store.
pub(super) struct Storage {
    path: PathBuf,
    owner_binding: String,
    entries: BTreeMap<String, String>,
}
impl Storage {
    pub(super) fn open(root: &Path, source_id: &str, owner_binding: &str) -> Self {
        let path = root
            .join("source-storage")
            .join(format!("{source_id}.json"));
        let entries = read(&path, owner_binding);
        Self {
            path,
            owner_binding: owner_binding.into(),
            entries,
        }
    }
    pub(super) fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }
    pub(super) fn keys(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }
    pub(super) fn set(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
        if key.is_empty() || key.len() > MAX_KEY_BYTES {
            return Err(StorageError::InvalidKey);
        }
        self.update(|entries| {
            entries.insert(key.into(), value.into());
        })
    }
    pub(super) fn remove(&mut self, key: &str) -> Result<(), StorageError> {
        self.update(|entries| {
            entries.remove(key);
        })
    }
    pub(super) fn clear(&mut self) -> Result<(), StorageError> {
        self.update(BTreeMap::clear)
    }
    /// Rereads the file first, so a second instance of the same package (the
    /// describe check of an update) cannot have its writes silently undone.
    fn update(
        &mut self,
        change: impl FnOnce(&mut BTreeMap<String, String>),
    ) -> Result<(), StorageError> {
        let mut entries = read(&self.path, &self.owner_binding);
        change(&mut entries);
        let total: usize = entries
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum();
        if total > MAX_TOTAL_BYTES {
            return Err(StorageError::Full);
        }
        let stored = Stored {
            owner_binding: self.owner_binding.clone(),
            entries,
        };
        write(&self.path, &stored).map_err(|_| StorageError::Unavailable)?;
        self.entries = stored.entries;
        Ok(())
    }
}

fn read(path: &Path, owner_binding: &str) -> BTreeMap<String, String> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        .filter(|stored| stored.owner_binding == owner_binding)
        .map(|stored| stored.entries)
        .unwrap_or_default()
}

fn write(path: &Path, stored: &Stored) -> std::io::Result<()> {
    let directory = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(directory)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, serde_json::to_vec(stored)?)?;
    fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_persist_per_package_and_stop_at_the_cap() {
        let root = tempfile::tempdir().unwrap();
        let mut storage = Storage::open(root.path(), "fixture", "publisher/package");
        storage.set("device", "abc").unwrap();
        assert_eq!(storage.get("device"), Some("abc"));

        let reopened = Storage::open(root.path(), "fixture", "publisher/package");
        assert_eq!(reopened.get("device"), Some("abc"));
        let other_package = Storage::open(root.path(), "fixture", "other/package");
        assert_eq!(other_package.get("device"), None);
        assert!(other_package.keys().is_empty());
        let other_source = Storage::open(root.path(), "second", "publisher/package");
        assert_eq!(other_source.get("device"), None);

        assert_eq!(storage.set("", "x"), Err(StorageError::InvalidKey));
        assert_eq!(
            storage.set(&"k".repeat(MAX_KEY_BYTES + 1), "x"),
            Err(StorageError::InvalidKey)
        );
        let large = "v".repeat(MAX_TOTAL_BYTES - 16);
        storage.set("large", &large).unwrap();
        assert_eq!(
            storage.set("more", &"v".repeat(64)),
            Err(StorageError::Full)
        );
        assert_eq!(storage.get("more"), None);
        storage.remove("large").unwrap();
        storage.set("more", &"v".repeat(64)).unwrap();
        assert_eq!(storage.keys(), ["device", "more"]);
        storage.clear().unwrap();
        assert!(Storage::open(root.path(), "fixture", "publisher/package")
            .keys()
            .is_empty());
    }
}
