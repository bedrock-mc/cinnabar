//! Carriers a browser installs in memory before the client starts. They are process-wide, so
//! these tests run in their own binary, apart from tests that read carriers from disk, and one
//! test installs every carrier it reads.

use std::path::Path;

use assets::{
    BlockEntityPlacement, ParticleTexture, RuntimeBlockEntityAssets, RuntimeParticleAssets,
    carriers,
    carriers::file::{self as carrier_file, MemoryCarriers},
    encode_block_entity_catalog, encode_particle_catalog,
};

#[test]
fn optional_carriers_load_from_preloaded_copies() {
    let compiled = Path::new("browser-install/compiled");
    let path = compiled.join(carriers::BLOCK_ENTITY.output);
    let missing = compiled.join("absent").join(carriers::BLOCK_ENTITY.output);
    let bytes = encode_block_entity_catalog(
        b"{}",
        2,
        1,
        &[255; 8],
        &[BlockEntityPlacement {
            name: "textures/entity/chest/normal".into(),
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        }],
    )
    .unwrap();
    let particles = compiled.join(carriers::PARTICLE.output);
    let texture = ParticleTexture {
        path: "textures/particle/particles".into(),
        width: 1,
        height: 1,
        rgba8: vec![255; 4].into(),
    };
    let particle_bytes = encode_particle_catalog([7; 32], &[texture], &[]).unwrap();
    assert!(!path.exists(), "the carriers must exist only in memory");
    let mut preloaded = MemoryCarriers::default();
    preloaded.insert(&path, bytes);
    preloaded.insert(&particles, particle_bytes);
    carrier_file::preload(preloaded);

    let read = |path: &Path| carrier_file::read_bounded(path, assets::MAX_PARTICLE_CARRIER_BYTES);
    let loaded = RuntimeParticleAssets::decode(&read(&particles).unwrap()).unwrap();
    assert!(loaded.texture("textures/particle/particles").is_some());
    assert!(read(&missing).is_err());

    let read =
        |path: &Path| carrier_file::read_bounded(path, assets::MAX_BLOCK_ENTITY_CARRIER_BYTES);
    let loaded = RuntimeBlockEntityAssets::decode(&read(&path).unwrap()).unwrap();
    assert_eq!(loaded.atlas_size(), [2, 1]);
    assert!(loaded.placement("textures/entity/chest/normal").is_some());
    assert!(read(&missing).is_err());
}
