//! Publisher tooling for Cinnabar server experiences: seeds, `.cxb` bundles, client cache seeding
//! and the golden fixtures the Go server half is tested against. Every format, domain and limit
//! comes from `server-experience`, the client's own crate.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use server_experience::{cache, crypto};

pub mod bundle;
pub mod fixtures;
pub mod keys;
pub mod media;

/// Publishes a bundle into a client's cache under its digest, as a finished download would.
/// Returns the digest and the cache directory.
pub fn seed_cache(cxb: &Path, user_data_root: &Path) -> Result<(String, PathBuf)> {
    let bytes = std::fs::read(cxb).with_context(|| format!("reading {}", cxb.display()))?;
    let digest = crypto::digest(&bytes);
    let objects = cache::objects_dir(user_data_root);
    cache::BundleCache::open(&objects)
        .with_context(|| format!("opening the bundle cache {}", objects.display()))?
        .publish(&digest, &bytes)?;
    Ok((digest, objects))
}
