use super::{
    Result, check,
    format_generated::{self, REGISTRY_MAGIC},
};
use assets::{
    BlockPhysicsRecord,
    registry::{RegistryMetadata, RegistryProvenance, RegistryRecord},
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Encodes production block carriers with schema-generated field order and reader validation.
pub(super) fn blocks(protocol: u32, records: &[RegistryRecord]) -> Result<Vec<u8>> {
    check(
        records.len() <= format_generated::MAX_REGISTRY_RECORDS,
        "too many registry records",
    )?;
    let all: BTreeSet<_> = records.iter().map(|r| &r.name).collect();
    let valentine: BTreeSet<_> = records
        .iter()
        .filter(|r| r.provenance.contains(RegistryProvenance::VALENTINE))
        .map(|r| &r.name)
        .collect();
    let gap: BTreeSet<_> = records
        .iter()
        .filter(|r| !r.provenance.contains(RegistryProvenance::VALENTINE))
        .map(|r| &r.name)
        .collect();
    let states = records
        .iter()
        .filter(|r| r.provenance.contains(RegistryProvenance::VALENTINE))
        .count();
    let metadata = RegistryMetadata {
        protocol,
        canonical_names: all.len() as u32,
        canonical_states: records.len() as u32,
        valentine_names: valentine.len() as u32,
        valentine_states: states as u32,
        valentine_gap_names: gap.len() as u32,
        valentine_gap_states: (records.len() - states) as u32,
    };
    let mut bytes = REGISTRY_MAGIC.to_vec();
    format_generated::append_metadata(&mut bytes, &metadata);
    for record in records {
        check(
            u16::try_from(record.name.len()).is_ok()
                && u32::try_from(record.canonical_state.len()).is_ok(),
            "record string bound exceeded",
        )?;
        format_generated::append_header(&mut bytes, record);
        for b in &record.collision_seed.boxes {
            format_generated::append_box(&mut bytes, b);
        }
        bytes.extend_from_slice(record.name.as_bytes());
        bytes.extend_from_slice(record.canonical_state.as_bytes());
    }
    assets::read_registry_for_protocol(&bytes, protocol)?;
    Ok(bytes)
}

/// Encodes a light carrier bound to the newly compiled block bytes.
pub(super) fn lights(
    protocol: u32,
    breg: &[u8],
    records: &[RegistryRecord],
    properties: &[u8],
    magic: &[u8],
) -> Result<Vec<u8>> {
    check(
        records.len() == properties.len(),
        "light property count mismatch",
    )?;
    let mut bytes = magic.to_vec();
    bytes.extend_from_slice(&protocol.to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(breg));
    bytes.extend_from_slice(properties);
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    assets::read_light_registry_for_protocol(&bytes, breg, records.len(), protocol)?;
    Ok(bytes)
}

/// Encodes the movement seed, leaving final interaction shapes to the existing compiler pass.
pub(super) fn physics(
    protocol: u32,
    breg: &[u8],
    identities: &[RegistryRecord],
    records: &[BlockPhysicsRecord],
) -> Result<Vec<u8>> {
    let mut bytes = assets::registry::LEGACY_PHYSICS_REGISTRY_MAGIC.to_vec();
    bytes.extend_from_slice(&protocol.to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(breg));
    for r in records {
        bytes.extend_from_slice(&r.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&r.network_hash.to_le_bytes());
        bytes.extend([
            r.boxes.len() as u8,
            r.flags.bits(),
            r.surface_response as u8,
            r.movement_semantics as u8,
        ]);
        for value in [
            r.friction_q1e8,
            r.horizontal_speed_q1e8,
            r.vertical_speed_q1e8,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&r.fluid_height_q1e8.to_le_bytes());
        for b in &r.boxes {
            format_generated::append_box(&mut bytes, b);
        }
    }
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    assets::read_physics_registry_seed_for_protocol(&bytes, breg, identities, protocol)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_encoder_preserves_the_independent_wire_fixture() {
        let text = include_str!("../../../assets/tests/fixtures/registry-wire.hex");
        let hex: String = text.split_whitespace().collect();
        let bytes: Vec<_> = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let records =
            assets::read_registry_for_protocol(&bytes, assets::active_content_registry_protocol())
                .unwrap();
        assert_eq!(
            blocks(assets::active_content_registry_protocol(), &records).unwrap(),
            bytes
        );
    }
}
