use sha2::{Digest, Sha256};

use super::{
    BlockPhysicsRecord, PHYSICS_REGISTRY_MAGIC, invalid, read_physics_registry_for_protocol,
};
use crate::{AssetError, RegistryRecord};

/// Encodes and validates a complete compiled registry bound to these exact identities.
pub fn encode_physics_registry(
    protocol: u32,
    breg: &[u8],
    identities: &[RegistryRecord],
    records: &[BlockPhysicsRecord],
) -> Result<Vec<u8>, AssetError> {
    if records.len() > super::MAX_RECORDS {
        return invalid("too many compiled records");
    }
    let mut bytes = PHYSICS_REGISTRY_MAGIC.to_vec();
    bytes.extend_from_slice(&protocol.to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(breg));
    for record in records {
        if record.boxes.len() > super::MAX_BOXES {
            return invalid("too many compiled collision boxes");
        }
        bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&record.network_hash.to_le_bytes());
        bytes.extend([
            record.boxes.len() as u8,
            record.flags.bits(),
            record.surface_response as u8,
            record.movement_semantics as u8,
        ]);
        for value in [
            record.friction_q1e8,
            record.horizontal_speed_q1e8,
            record.vertical_speed_q1e8,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&record.fluid_height_q1e8.to_le_bytes());
        for shape in &record.boxes {
            for value in [
                shape.min_x,
                shape.min_y,
                shape.min_z,
                shape.max_x,
                shape.max_y,
                shape.max_z,
            ] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let Some(shapes) = &record.shapes else {
            return invalid("missing compiled block shapes; run make assets");
        };
        super::shapes::write(&mut bytes, shapes)?;
    }
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    read_physics_registry_for_protocol(&bytes, breg, identities, protocol)?;
    Ok(bytes)
}
