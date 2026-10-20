//! Facts from the original bounded archive verifier, without synthetic upstream wire metadata.

use bedrock_client::server::session::ServerPackContent;
use bedrock_packs::ResourcePackArchive;
use bedrock_protocol_1_26_51::types::PackInfoData;
use resource_pack::ArchiveKind;
use sha2::{Digest, Sha256};
use std::io;

/// Binds only one acquired archive; it carries no account, source URL or admission authority.
pub(in crate::login::native_network) struct ArchiveFacts {
    id: uuid::Uuid,
    version: String,
    size: u64,
    key_hash: [u8; 32],
    hash: [u8; 32],
    kind: ArchiveKind,
}

/// Uses the same manifest/index pass, then hashes ORIGINAL compressed bytes without rewriting them.
/// The caller runs this finite CPU work in its existing blocking acquisition worker.
pub(in crate::login::native_network) fn describe(
    bytes: &[u8],
    offer: &PackInfoData,
) -> io::Result<ArchiveFacts> {
    let id = offer.pack_id_version.pack_uuid;
    let version = offer
        .pack_id_version
        .pack_version
        .version
        .to_str()
        .map_err(|_| invalid())?;
    let kind = resource_pack::verify_archive_identity(bytes, id, version).map_err(|_| invalid())?;
    Ok(ArchiveFacts {
        id,
        version: version.into(),
        size: u64::try_from(bytes.len()).map_err(|_| invalid())?,
        key_hash: Sha256::digest(offer.content_key.as_bytes()).into(),
        hash: Sha256::digest(bytes).into(),
        kind,
    })
}

impl ArchiveFacts {
    /// Reoriginates a downstream descriptor from this same verified archive, never a DataInfo.
    pub(in crate::login::native_network) fn into_content(
        self,
        archive: &ResourcePackArchive,
    ) -> io::Result<ServerPackContent> {
        if self.id != archive.pack_id
            || self.version != archive.version
            || self.size != u64::try_from(archive.archive.len()).map_err(|_| invalid())?
            || self.key_hash != <[u8; 32]>::from(Sha256::digest(archive.content_key.expose()))
        {
            return Err(invalid());
        }
        let kind = match self.kind {
            ArchiveKind::Resources => bridge::contract::RESOURCE_PACK_TYPE_RESOURCES,
            ArchiveKind::Addon => bridge::contract::RESOURCE_PACK_TYPE_ADDON,
            ArchiveKind::Behavior => bridge::contract::RESOURCE_PACK_TYPE_BEHAVIOR,
            ArchiveKind::Skins => bridge::contract::RESOURCE_PACK_TYPE_SKINS,
            ArchiveKind::WorldTemplate => bridge::contract::RESOURCE_PACK_TYPE_WORLD_TEMPLATE,
        };
        // The selected Go delivery owner reoriginates these kinds and leaves Premium false.
        // Neither this value nor the computed hash claims absent upstream wire provenance.
        Ok(ServerPackContent::external(
            self.size, self.hash, kind, false,
        ))
    }
}

/// Omits archive identities, signed URLs and content keys from failed descriptor checks.
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid verified pack archive")
}
