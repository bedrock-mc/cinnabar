use assets::{
    BlockFlags, ContributorRole, ModelFamily, read_light_registry_for_protocol, read_registry,
    read_registry_for_protocol,
};
use sha2::{Digest, Sha256};

const BREG: &[u8] = include_bytes!("../../data/block-registry-v2193.bin");
const LREG: &[u8] = include_bytes!("../../data/block-light-registry-v2193.bin");
const LEGACY_BREG: &[u8] = include_bytes!("../../data/block-registry-v1001.bin");

#[test]
fn checked_in_v2193_block_and_light_registries_are_exact_and_bound() {
    let records = read_registry_for_protocol(BREG, 2193).expect("decode v2193 BREG");
    assert!(!records.is_empty());
    assert!(
        records
            .iter()
            .enumerate()
            .all(|(index, record)| record.sequential_id == index as u32)
    );
    let lights = read_light_registry_for_protocol(LREG, BREG, records.len(), 2193)
        .expect("decode exact BREG-bound v2193 LREG1001");
    assert_eq!(lights.len(), records.len());
    assert_eq!(
        format!("{:x}", Sha256::digest(BREG)),
        serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../../assets/bedrock-target.json"
        ))
        .unwrap()["hashes"]["block_registry"]
            .as_str()
            .unwrap()
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(LREG)),
        serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../../assets/bedrock-target.json"
        ))
        .unwrap()["hashes"]["light_registry"]
            .as_str()
            .unwrap()
    );
}

#[test]
fn checked_in_v2193_registry_pins_the_unique_canonical_air_identity() {
    let records = read_registry_for_protocol(BREG, 2193).expect("decode v2193 BREG");
    let air = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:air")
        .collect::<Vec<_>>();
    assert_eq!(
        air.len(),
        1,
        "the checked-in v2193 registry must carry exactly one minecraft:air record"
    );
    let air = air[0];
    assert_eq!(air.sequential_id, 17_025);
    assert_eq!(air.canonical_state.as_ref(), "{}");
    assert_eq!(air.model_family, ModelFamily::Air);
    assert_eq!(air.contributor_role, ContributorRole::Air);
    assert!(air.flags.contains(BlockFlags::AIR));

    // The network hash is content-derived from the canonical identity, so it
    // must equal the legacy registry's air hash; only the sequential slot
    // moves between protocols.
    let legacy_air = read_registry(LEGACY_BREG)
        .expect("decode legacy BREG1003")
        .into_iter()
        .find(|record| record.name.as_ref() == "minecraft:air")
        .expect("legacy canonical air");
    assert_eq!(legacy_air.canonical_state.as_ref(), "{}");
    assert_eq!(
        air.network_hash, legacy_air.network_hash,
        "canonical air keeps its content-derived network hash across protocols"
    );
}

#[test]
fn protocol_aware_decoders_reject_cross_version_and_cross_hash_inputs() {
    assert!(read_registry(BREG).is_err());
    assert!(read_registry_for_protocol(LEGACY_BREG, 2193).is_err());
    assert!(read_registry_for_protocol(BREG, 999).is_err());
    let records = read_registry_for_protocol(BREG, 2193).expect("v2193 BREG");
    assert!(read_light_registry_for_protocol(LREG, LEGACY_BREG, records.len(), 2193).is_err());
    assert!(read_light_registry_for_protocol(LREG, BREG, records.len(), 1001).is_err());
    assert!(read_light_registry_for_protocol(LREG, BREG, records.len(), 999).is_err());
}

#[test]
fn v2193_decoders_reject_trailing_truncated_and_malformed_carriers() {
    let mut trailing = BREG.to_vec();
    trailing.push(0);
    assert!(read_registry_for_protocol(&trailing, 2193).is_err());
    assert!(read_registry_for_protocol(&BREG[..BREG.len() - 1], 2193).is_err());

    let mut malformed_light = LREG.to_vec();
    malformed_light[48] ^= 1;
    let records = read_registry_for_protocol(BREG, 2193).expect("v2193 BREG");
    assert!(read_light_registry_for_protocol(&malformed_light, BREG, records.len(), 2193).is_err());
}
