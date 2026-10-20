//! Source admission shared by mod imports, metadata reads and instance preparation.

use crate::{MAX_COMPONENT_SOURCE_BYTES, ModGrants, ModHost, local_files, packet_mod};
use anyhow::{Result, ensure};
use std::path::Path;

/// The binary forms accepted by the library; core reactors run on the packet host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Component,
    Packet,
}

impl SourceKind {
    /// Classifies core reactors without executing their code; validation checks their ABI later.
    pub fn of(bytes: &[u8]) -> Self {
        if bytes.starts_with(b"\0asm\x01\0\0\0") || bytes.starts_with(b"(module") {
            Self::Packet
        } else {
            Self::Component
        }
    }

    /// The existing source ceiling for this host, independent of filename or claimed grants.
    pub fn byte_limit(self) -> usize {
        match self {
            Self::Component => MAX_COMPONENT_SOURCE_BYTES,
            Self::Packet => packet_mod::MAX_PACKET_MOD_BYTES,
        }
    }
}

/// Reads one regular source, retaining the smaller component limit for ordinary mods.
pub fn read_source(path: &Path) -> Result<Vec<u8>> {
    let bytes = local_files::read(path, packet_mod::MAX_PACKET_MOD_BYTES)?;
    check_size(&bytes)?;
    Ok(bytes)
}

/// Validates a source without running it; larger reactors use the packet host's ABI and cache.
pub fn validate_source(bytes: &[u8]) -> Result<()> {
    check_size(bytes)?;
    match SourceKind::of(bytes) {
        SourceKind::Component => ModHost::validate_component(bytes).or_else(|component_error| {
            validate_packet_source(bytes).map_err(|packet_error| {
                component_error.context(format!("packet mod validation: {packet_error:#}"))
            })
        }),
        SourceKind::Packet => validate_packet_source(bytes),
    }
}

/// Checks the packet world's ABI without executing guest code or granting a live instance.
fn validate_packet_source(bytes: &[u8]) -> Result<()> {
    packet_mod::CompiledPacketMod::compile(
        bytes,
        &ModGrants {
            packets: true,
            wasi: true,
            ..Default::default()
        },
    )
    .map(|_| ())
}

/// Applies the owning host's source ceiling even when bytes came from a captured snapshot.
pub(crate) fn check_size(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() <= SourceKind::of(bytes).byte_limit(),
        "source exceeds byte limit"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_reactors_keep_their_own_limit_and_components_keep_the_smaller_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mod.wasm");
        let mut bytes = vec![0; MAX_COMPONENT_SOURCE_BYTES + 1];
        bytes[..8].copy_from_slice(b"\0asm\x01\0\0\0");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(read_source(&path).unwrap(), bytes);
        bytes[..8].copy_from_slice(b"\0asm\x0d\0\x01\0");
        std::fs::write(&path, &bytes).unwrap();
        assert!(read_source(&path).is_err());
        bytes[..8].copy_from_slice(b"\0asm\x01\0\0\0");
        assert!(
            validate_source(&bytes).is_err(),
            "size admission must not admit invalid reactors"
        );
    }
}
