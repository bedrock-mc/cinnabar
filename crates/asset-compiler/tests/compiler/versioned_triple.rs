use std::process::Command;

use assets::{NetworkIdMode, RuntimeAssets, VisualKind, VisualSupport};

use super::support::*;

/// One stone texture plus the biome fixture, the minimum pack surface the
/// compile CLI consumes for a two-record registry.
fn write_minimal_pack(resource_pack: &Path) {
    write_pack(
        resource_pack,
        r#"{"minecraft:stone":{"textures":"stone"}}"#,
        r#"{"texture_data":{"stone":{"textures":"textures/blocks/stone"}}}"#,
        "[]",
    );
    write_png(
        resource_pack,
        "textures/blocks/stone",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [120, 120, 120, 255]),
    );
    write_biome_fixture(resource_pack);
}

fn run_assetc_compile(
    directory: &Path,
    resource_pack: &Path,
    registry: &Path,
    light_registry: &Path,
) -> std::process::Output {
    let source_manifest = directory.join("vanilla-source.json");
    let biome_registry = directory.join("biome-registry.bin");
    let output_blob = directory.join("vanilla-out.mcbea");
    fs::write(&source_manifest, br#"{"schema":1}"#).expect("write source manifest fixture");
    fs::write(&biome_registry, biome_registry_bytes(0, "minecraft:plains"))
        .expect("write biome registry fixture");
    Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["compile", "--pack"])
        .arg(resource_pack)
        .arg("--source-manifest")
        .arg(&source_manifest)
        .arg("--registry")
        .arg(registry)
        .arg("--light-registry")
        .arg(light_registry)
        .arg("--biome-registry")
        .arg(&biome_registry)
        .arg("--out")
        .arg(&output_blob)
        .output()
        .expect("run assetc compile")
}

/// Builds one synthetic canonical-air record at an arbitrary wire identity.
fn arbitrary_air(sequential_id: u32, network_hash: u32) -> RegistryRecord {
    let mut record = canonical_air_record(sequential_id, network_hash);
    record.provenance = RegistryProvenance::PMMP;
    record
}

fn arbitrary_stone(sequential_id: u32, network_hash: u32) -> RegistryRecord {
    let mut record = record(
        sequential_id,
        network_hash,
        "minecraft:stone",
        "{}",
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
    );
    record.provenance = RegistryProvenance::PMMP;
    record
}

#[test]
fn assetc_compiles_a_v2168_triple_whose_air_sits_at_an_arbitrary_identity() {
    // Real registries number states densely from zero, which the light
    // registry's per-record binding requires; these identities are arbitrary
    // in the sense that they match no pinned protocol constant.
    const ARBITRARY_AIR_ID: u32 = 0;
    const ARBITRARY_AIR_HASH: u32 = 0x00a1_7e57;

    let directory = TempDir::new().expect("create versioned-triple fixture");
    let resource_pack = directory.path().join("pack");
    write_minimal_pack(&resource_pack);

    // The unique valid air carries identities that match no pinned constant.
    let records = vec![
        arbitrary_air(ARBITRARY_AIR_ID, ARBITRARY_AIR_HASH),
        arbitrary_stone(1, 0x00b1_6c7b),
    ];
    let registry_fixture = registry_bytes_for_protocol(2168, &records);
    let light_fixture = light_registry_bytes_for_protocol(2168, &registry_fixture, records.len());
    let registry = directory.path().join("block-registry.bin");
    let light_registry = directory.path().join("light-registry.bin");
    fs::write(&registry, &registry_fixture).expect("write v2168 block registry fixture");
    fs::write(&light_registry, &light_fixture).expect("write v2168 light registry fixture");

    let output = run_assetc_compile(directory.path(), &resource_pack, &registry, &light_registry);
    assert!(
        output.status.success(),
        "assetc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = fs::read(directory.path().join("vanilla-out.mcbea")).expect("read compiled blob");
    let runtime = RuntimeAssets::decode(&bytes).expect("decode compiled blob");

    // The exact Invisible route must follow the header-declared identity.
    assert_eq!(
        runtime.air_network_id(NetworkIdMode::Sequential),
        Some(ARBITRARY_AIR_ID)
    );
    assert_eq!(
        runtime.air_network_id(NetworkIdMode::Hashed),
        Some(ARBITRARY_AIR_HASH)
    );
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let value = match mode {
            NetworkIdMode::Sequential => ARBITRARY_AIR_ID,
            NetworkIdMode::Hashed => ARBITRARY_AIR_HASH,
        };
        let resolved = runtime.resolve(mode, value);
        assert!(resolved.is_known(), "{mode:?} air must resolve known");
        assert_eq!(resolved.kind(), VisualKind::Invisible, "{mode:?}");
        assert_eq!(resolved.support(), VisualSupport::Exact, "{mode:?}");
    }
}

#[test]
fn assetc_rejects_a_mixed_version_triple_naming_both_files() {
    let directory = TempDir::new().expect("create mixed-version fixture");
    let resource_pack = directory.path().join("pack");
    write_minimal_pack(&resource_pack);

    let records = vec![
        arbitrary_air(0, 0x00a1_7e57),
        arbitrary_stone(1, 0x00b1_6c7b),
    ];
    let block_registry_bytes = registry_bytes_for_protocol(2168, &records);
    // The light leg claims protocol 1001 while its BREG binding is 2168.
    let light_fixture =
        light_registry_bytes_for_protocol(1001, &block_registry_bytes, records.len());
    let registry = directory.path().join("block-registry-v2168.bin");
    let light_registry = directory.path().join("light-registry-v1001.bin");
    fs::write(&registry, &block_registry_bytes).expect("write block registry fixture");
    fs::write(&light_registry, &light_fixture).expect("write light registry fixture");

    let output = run_assetc_compile(directory.path(), &resource_pack, &registry, &light_registry);
    assert!(
        !output.status.success(),
        "a mixed-version triple must fail closed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("block-registry-v2168.bin"), "{stderr}");
    assert!(stderr.contains("light-registry-v1001.bin"), "{stderr}");
    assert!(
        stderr.contains("2168") && stderr.contains("1001"),
        "{stderr}"
    );
}
