//! Actual Go-generated world packets, without a second JSON GameData mapping.

use super::invalid;
use bedrock_client::server::session::ServerWorld;
use bedrock_protocol_1_26_51::{packets, wire};
use bridge::host_world::contract::{
    MAX_CONTROL_BYTES, MAX_PACKET_BYTES, MAX_PACKET_COUNT, WorldPolicy,
};
use bytes::Bytes;
use std::io;

/// Original startup prelude and the actual selected world's typed initialization data.
pub struct NativeHostWorld {
    world: ServerWorld,
    prelude: Vec<packets::Packet>,
}

impl NativeHostWorld {
    /// Validates the private world record and decodes its original current-protocol packet roles.
    /// The caller still needs the actual ServerWorldReady and current application authorization.
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > MAX_PACKET_BYTES as usize {
            return Err(invalid());
        }
        let mut input = bytes;
        let policy_bytes = take(&mut input, MAX_CONTROL_BYTES as usize)?;
        let policy: WorldPolicy = serde_json::from_slice(policy_bytes).map_err(|_| invalid())?;
        if policy.packet_count > MAX_PACKET_COUNT
            || policy.max_radius == 0
            || policy.initial_radius > policy.max_radius
        {
            return Err(invalid());
        }
        let max_radius = i32::try_from(policy.max_radius).map_err(|_| invalid())?;
        let initial_radius = i32::try_from(policy.initial_radius).map_err(|_| invalid())?;
        let mut prelude = Vec::new();
        let mut start = None;
        let mut items = None;
        let mut creative = None;
        let mut jigsaw = false;
        let mut voxel = false;
        for _ in 0..policy.packet_count {
            let raw = Bytes::copy_from_slice(take(&mut input, MAX_PACKET_BYTES as usize)?);
            let packet = decode(&raw)?;
            match packet {
                packet @ packets::Packet::DimensionData(_) if prelude.is_empty() => {
                    prelude.push(packet)
                }
                packet @ packets::Packet::JigsawStructureData(_)
                    if !jigsaw && !voxel && start.is_none() =>
                {
                    jigsaw = true;
                    prelude.push(packet);
                }
                packet @ packets::Packet::VoxelShapes(_) if jigsaw && !voxel && start.is_none() => {
                    voxel = true;
                    prelude.push(packet);
                }
                packets::Packet::StartGame(packet) if voxel && start.is_none() => {
                    start = Some(packet)
                }
                packets::Packet::ItemRegistry(packet) if start.is_some() && items.is_none() => {
                    items = Some(packet)
                }
                packets::Packet::CreativeContent(packet)
                    if items.is_some() && creative.is_none() =>
                {
                    creative = Some(packet)
                }
                _ => return Err(invalid()),
            }
        }
        if !input.is_empty() {
            return Err(invalid());
        }
        Ok(Self {
            world: ServerWorld {
                start_game: start.ok_or_else(invalid)?,
                items: items.ok_or_else(invalid)?,
                creative: creative.ok_or_else(invalid)?,
                max_chunk_radius: max_radius,
                initial_chunk_radius: (initial_radius != 0).then_some(initial_radius),
            },
            prelude,
        })
    }

    /// Transfers original pre-StartGame output and selected-world data to the real typed consumer.
    pub fn into_parts(self) -> (Vec<packets::Packet>, ServerWorld) {
        (self.prelude, self.world)
    }
}

/// Takes one existing BE-u32 sized field without allocating from an unvalidated claimed size.
fn take<'a>(input: &mut &'a [u8], maximum: usize) -> io::Result<&'a [u8]> {
    let (prefix, rest) = input.split_at_checked(4).ok_or_else(invalid)?;
    let length = u32::from_be_bytes(prefix.try_into().map_err(|_| invalid())?) as usize;
    if length > maximum {
        return Err(invalid());
    }
    let (body, rest) = rest.split_at_checked(length).ok_or_else(invalid)?;
    *input = rest;
    Ok(body)
}

/// Reuses the protocol's existing primary-header and generated packet decoder.
fn decode(raw: &Bytes) -> io::Result<packets::Packet> {
    let mut reader = wire::Reader::from_shared(raw);
    let header = wire::PacketHeader::from_raw(reader.read_var_u32().map_err(|_| invalid())?);
    if header.sender_subclient != 0 || header.target_subclient != 0 {
        return Err(invalid());
    }
    let id = packets::PacketId::from_raw(header.id).ok_or_else(invalid)?;
    crate::codec::decode_body(id, &raw.slice(reader.position()..)).map_err(|_| invalid())
}
