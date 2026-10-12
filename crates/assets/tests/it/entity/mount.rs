use assets::entity::mount::{
    MOUNT_EQUIPMENT_SOURCE_BYTES, MOUNT_EQUIPMENT_SOURCE_PATH, MountEquipment,
    builtin_mount_equipment,
};
use assets::{EntityAssetSource, RuntimeEntityAssets, encode_entity_blob};
use sha2::{Digest, Sha256};

#[test]
fn entity_carrier_round_trips_mount_equipment_dispositions() {
    for equipment in [
        MountEquipment::Unresolved,
        builtin_mount_equipment("minecraft:horse"),
    ] {
        let mut compiled = super::suite::fixture();
        let mut sources = compiled.sources.to_vec();
        sources.push(EntityAssetSource {
            path: MOUNT_EQUIPMENT_SOURCE_PATH.into(),
            source_bytes: MOUNT_EQUIPMENT_SOURCE_BYTES.len() as u32,
            source_sha256: Sha256::digest(MOUNT_EQUIPMENT_SOURCE_BYTES).into(),
        });
        compiled.sources = sources.into_boxed_slice();
        compiled.symbols[0].identifier = "minecraft:horse".into();
        compiled.symbols[0].mount_equipment = equipment;
        let blob = encode_entity_blob(&compiled).expect("encode mount catalog");
        let runtime = RuntimeEntityAssets::decode(&blob).expect("decode mount catalog");
        assert_eq!(runtime.symbols()[0].mount_equipment, equipment);
        assert_eq!(runtime.encode().unwrap(), blob);
    }
}
