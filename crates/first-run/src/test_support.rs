use std::path::{Path, PathBuf};

/// Self-deleting scratch directory unique per process and label.
pub(super) struct Dir(PathBuf);

impl Dir {
    /// A fresh directory; a per-process counter keeps parallel tests with one label apart.
    pub(super) fn new(label: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "cinnabar-first-run-{}-{serial}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Writes a schema-complete vanilla manifest into the kit at `root` pinning `archive` at `sha256`.
pub(super) fn write_vanilla_manifest(root: &Path, url: &str, sha256: &str, archive: &str) {
    let manifest = serde_json::json!({
        "schema": 1,
        "tag": "test",
        "commit": "test",
        "archive": archive,
        "url": url,
        "sha256": sha256,
        "artifact_policy": "local-only",
        "cache_dir": ".local/assets/bedrock-samples/test/full",
    });
    let path =
        assets::carriers::Sources::Kit(root.into()).resolve(assets::carriers::VANILLA_MANIFEST);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, manifest.to_string()).unwrap();
}
