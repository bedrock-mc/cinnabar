//! The backend pipeline cache a pack's load reuses across runs.

use super::pipelines::GpuContext;
use std::path::{Path, PathBuf};

/// A backend pipeline cache persisted in `cache_dir`, keyed by the adapter and the pack's
/// content key.
pub(super) struct PipelineCacheFile {
    pub(super) cache: wgpu::PipelineCache,
    path: PathBuf,
}

impl PipelineCacheFile {
    /// Opens the cache for `key`; `None` without a cache directory or device support.
    pub(super) fn open(gpu: &GpuContext, cache_dir: Option<&Path>, key: &str) -> Option<Self> {
        let dir = cache_dir?;
        if !gpu
            .device
            .features()
            .contains(wgpu::Features::PIPELINE_CACHE)
        {
            return None;
        }
        let adapter = wgpu::util::pipeline_cache_key(&gpu.adapter.get_info())?;
        let path = dir.join(format!("{adapter}-{key}.bin"));
        let data = std::fs::read(&path).ok();
        // SAFETY: the data was written by `get_data` of a cache for the same adapter key;
        // `fallback` replaces data the driver rejects.
        let cache = unsafe {
            gpu.device
                .create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                    label: Some("shader pack pipeline cache"),
                    data: data.as_deref(),
                    fallback: true,
                })
        };
        Some(Self { cache, path })
    }

    /// Writes the cache's current data atomically; failures are logged and ignored.
    pub(super) fn store(&self) {
        let Some(data) = self.cache.get_data() else {
            return;
        };
        let write = || -> std::io::Result<()> {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let temp = self.path.with_extension("tmp");
            std::fs::write(&temp, &data)?;
            std::fs::rename(&temp, &self.path)
        };
        if let Err(error) = write() {
            bevy::log::debug!(target: "cinnabar::shaderpack", %error, "pipeline cache not saved");
        }
    }
}
