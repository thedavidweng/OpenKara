use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug)]
pub struct ModelCache<T> {
    cached_key: Option<String>,
    cached_model: Option<Arc<T>>,
}

impl<T> Default for ModelCache<T> {
    fn default() -> Self {
        Self {
            cached_key: None,
            cached_model: None,
        }
    }
}

impl<T> ModelCache<T> {
    pub fn get_or_load_with(
        &mut self,
        path: &Path,
        load: impl FnOnce(&Path) -> Result<T>,
    ) -> Result<Arc<T>> {
        self.get_or_load_with_key(path.display().to_string(), || load(path))
    }

    /// Returns `Arc<T>` so the caller can release the model cache lock
    /// before running inference.
    pub fn get_or_load_with_key(
        &mut self,
        key: impl Into<String>,
        load: impl FnOnce() -> Result<T>,
    ) -> Result<Arc<T>> {
        let key = key.into();

        if self.cached_key.as_deref() != Some(key.as_str()) {
            // Demucs model loads are large enough that re-reading from disk for every
            // song dominates batch separation time. The cache stays single-instance
            // on purpose: current separation is sequential, so reuse matters more
            // than parallelism here.
            self.cached_key = None;
            self.cached_model = None;
        }

        if self.cached_model.is_none() {
            self.cached_model = Some(Arc::new(load()?));
            self.cached_key = Some(key);
        }

        Ok(Arc::clone(self.cached_model.as_ref().expect(
            "model cache should hold a model after a successful load",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::ModelCache;

    #[test]
    fn reloads_when_cache_key_changes_for_same_model_path() {
        let mut cache = ModelCache::default();
        let mut loads = 0;

        let model = cache
            .get_or_load_with_key("/tmp/model.onnx::cpu", || {
                loads += 1;
                Ok::<_, anyhow::Error>(loads)
            })
            .expect("initial model load should succeed");
        assert_eq!(*model, 1);

        let model = cache
            .get_or_load_with_key("/tmp/model.onnx::cpu", || {
                loads += 1;
                Ok::<_, anyhow::Error>(loads)
            })
            .expect("same cache key should reuse the model");
        assert_eq!(*model, 1);

        let model = cache
            .get_or_load_with_key("/tmp/model.onnx::xnnpack", || {
                loads += 1;
                Ok::<_, anyhow::Error>(loads)
            })
            .expect("different provider key should force a reload");
        assert_eq!(*model, 2);
    }

    #[test]
    fn model_cache_returns_arc_allowing_lock_release() {
        let mut cache = ModelCache::default();

        let model1 = cache
            .get_or_load_with_key("key", || Ok::<_, anyhow::Error>(42))
            .expect("load should succeed");

        let model1_clone = std::sync::Arc::clone(&model1);
        drop(model1);

        assert_eq!(*model1_clone, 42);

        let model2 = cache
            .get_or_load_with_key("key", || Ok::<_, anyhow::Error>(99))
            .expect("load should succeed");
        assert!(std::sync::Arc::ptr_eq(&model1_clone, &model2));
    }
}
