//! One admitted packet program retained in host memory, without native artifact files.

use super::{CompiledPacketMod, MAX_PACKET_MOD_BYTES};
use crate::ModGrants;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::sync::Mutex;

struct Entry {
    source: [u8; 32],
    grants: ModGrants,
    program: CompiledPacketMod,
}

static CACHE: Mutex<Option<Entry>> = Mutex::new(None);

/// Reuses only a program compiled and admitted by this process under the same grants.
/// The retained native image cannot exceed the packet source byte ceiling.
pub(super) fn load(
    bytes: &[u8],
    grants: &ModGrants,
    compile: impl FnOnce() -> Result<(CompiledPacketMod, usize)>,
) -> Result<CompiledPacketMod> {
    load_cached(&CACHE, bytes, grants, compile)
}

/// Compiles outside the cache lock and publishes only successfully admitted, bounded programs.
fn load_cached(
    cache: &Mutex<Option<Entry>>,
    bytes: &[u8],
    grants: &ModGrants,
    compile: impl FnOnce() -> Result<(CompiledPacketMod, usize)>,
) -> Result<CompiledPacketMod> {
    let source = Sha256::digest(bytes).into();
    if let Some(entry) = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .filter(|entry| entry.source == source && &entry.grants == grants)
    {
        return Ok(entry.program.clone());
    }
    let (program, image_bytes) = compile()?;
    if image_bytes <= MAX_PACKET_MOD_BYTES {
        *cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Entry {
            source,
            grants: grants.clone(),
            program: program.clone(),
        });
    }
    Ok(program)
}

#[cfg(test)]
mod tests;
