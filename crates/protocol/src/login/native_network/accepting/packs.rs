//! Exact configured Go pack metadata and original external-archive ranges.

use super::invalid;
use bedrock_client::{
    packs::{PackTransferLimits, builtin_packs},
    server::session::{ServerPackContent, ServerPackSnapshot},
};
use bedrock_protocol_1_26_51::{
    packets,
    types::{PackInfoData, PackInstanceId},
    wire,
};
use bridge::host_world::contract::Snapshot;
use std::io;

/// Projects the actual final Go resource lease; this performs no archive/CDN/cache acquisition.
/// The accepting owner performs its full wire/hash/identity validation before advertising it.
pub fn native_host_packs(snapshot: &Snapshot) -> io::Result<ServerPackSnapshot> {
    let limits = PackTransferLimits::default();
    let builtins = builtin_packs();
    if i32::try_from(snapshot.protocol_version).ok() != Some(crate::PROTOCOL_VERSION)
        || snapshot
            .packs
            .len()
            .checked_add(builtins.len())
            .is_none_or(|n| limits.max_packs.is_some_and(|limit| n > limit))
    {
        return Err(invalid());
    }
    let mut infos = Vec::with_capacity(snapshot.packs.len());
    let mut contents = Vec::with_capacity(snapshot.packs.len());
    let mut stack = Vec::with_capacity(snapshot.packs.len() + builtins.len());
    let mut total = 0_u64;
    for pack in &snapshot.packs {
        total = total.checked_add(pack.size).ok_or_else(invalid)?;
        if pack.size == 0
            || limits
                .max_archive_bytes
                .is_some_and(|limit| pack.size > limit)
            || limits.max_stack_bytes.is_some_and(|limit| total > limit)
            || pack.version.is_empty()
            || pack.version.len() > limits.max_version_bytes
        {
            return Err(invalid());
        }
        let id = uuid::Uuid::parse_str(&pack.id).map_err(|_| invalid())?;
        let pack_type = u8::try_from(pack.pack_type).map_err(|_| invalid())?;
        let hash = checksum(&pack.sha256)?;
        let mut info = PackInfoData::default();
        info.pack_id_version.pack_uuid = id;
        info.pack_id_version.pack_version.version = pack.version.clone().into();
        info.pack_size = wire::U64LE(pack.size);
        info.content_key = pack.content_key.clone().into();
        info.content_identity.identity = pack.content_identity.clone().into();
        info.cdn_url = pack.download_url.clone().into();
        infos.push(info);
        stack.push(PackInstanceId {
            pack_id: id.to_string().into(),
            version: pack.version.clone().into(),
            ..Default::default()
        });
        contents.push(ServerPackContent::external(
            pack.size,
            hash,
            pack_type,
            pack.premium,
        ));
    }
    stack.extend(builtins.map(|(id, version)| PackInstanceId {
        pack_id: id.to_string().into(),
        version: version.into(),
        ..Default::default()
    }));
    Ok(ServerPackSnapshot {
        offer: packets::ResourcePacksInfo {
            resource_pack_required: snapshot.required,
            resource_packs: infos,
            ..Default::default()
        },
        stack: packets::ResourcePackStack {
            texture_pack_required: snapshot.required,
            texture_pack_list: stack,
            // Pinned Go's default constructed stack uses this exact wildcard.
            base_game_version: "*".into(),
            ..Default::default()
        },
        contents,
    })
}

/// Decodes only the immutable lease's exact SHA-256, without another pack-content hash owner.
fn checksum(value: &str) -> io::Result<[u8; 32]> {
    let mut hash = [0_u8; 32];
    if value.len() != hash.len() * 2 {
        return Err(invalid());
    }
    for (byte, digits) in hash.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
        let high = char::from(digits[0]).to_digit(16).ok_or_else(invalid)?;
        let low = char::from(digits[1]).to_digit(16).ok_or_else(invalid)?;
        *byte = ((high << 4) | low) as u8;
    }
    Ok(hash)
}
