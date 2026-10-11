use super::*;
use world::BlockIds;

/// Declares enough binary properties to exhaust the state budget after one admitted state.
fn over_budget_definition() -> Vec<u8> {
    const AXES: u8 = 16;
    let mut bytes = vec![10, 0, 9, 10];
    bytes.extend(b"properties");
    bytes.extend([10, AXES * 2]);
    for axis in 0..AXES {
        let name = format!("test:axis_{axis}");
        bytes.extend([8, 4]);
        bytes.extend(b"name");
        bytes.push(name.len() as u8);
        bytes.extend(name.as_bytes());
        bytes.extend([9, 4]);
        bytes.extend(b"enum");
        bytes.extend([1, 4, 0, 1, 0]);
    }
    bytes.push(0);
    bytes
}

/// Builds a synthetic cube carrier with the pinned palette's size and one replaceable material.
fn base_carrier(count: usize) -> Arc<RuntimeAssets> {
    use assets::*;
    use sha2::{Digest, Sha256};

    let diagnostic = RuntimeAssets::diagnostic();
    let compiled = CompiledAssets {
        visuals: vec![
            BlockVisual {
                faces: [1; 6],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                kind: VisualKind::Cube,
                support: VisualSupport::Exact,
                contributor_role: ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            };
            count
        ]
        .into(),
        light_properties: vec![LightProperties::OPAQUE_DARK; count].into(),
        hashed: Box::new([]),
        materials: vec![diagnostic.material(0); 2].into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: diagnostic.texture_pages().to_vec().into(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: Sha256::digest(pinned_block_registry_bytes()).into(),
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    Arc::new(RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap())
}

#[test]
fn sequential_budget_rejection_keeps_admitted_visuals_overrides_and_wire_ids() {
    let breg = assets::pinned_block_registry_bytes();
    let version = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(breg, version).unwrap();
    let mut collisions = gameplay::movement::PhysicsCollisionRegistries::from_assets(
        breg,
        &records,
        include_bytes!("../../../../../../crates/assets/data/block-physics-v2193.bin"),
        version,
    )
    .unwrap();
    let mut names = ["test:first", "test:middle"];
    names.sort_by_key(|name| (protocol::block_name_sort_key(name), *name));
    let large = over_budget_definition();
    let mut blocks = CustomBlocks::from_definitions([
        (names[0], [10, 0, 0].as_slice()),
        (names[1], large.as_slice()),
    ]);
    assert_eq!(blocks.skipped, 1);
    assert!(blocks.blocks[1].visual.materialization_disabled);
    blocks.vanilla_blocks = assets::server_defined_blocks()
        .iter()
        .map(|block| Arc::from(block.name))
        .collect();
    Arc::make_mut(&mut Arc::make_mut(&mut blocks.blocks)[0].visual).base = CustomVisualComponents {
        geometry: Some("minecraft:geometry.full_block".into()),
        materials: materials("lucky"),
        ..Default::default()
    };
    let (range, remap) = collisions.begin_session_custom_blocks(&blocks).unwrap();
    let base = base_carrier(records.len());
    let keys = assets::MaterialKeys::from_entries([(1, "lucky")]);
    let compiled = Arc::new(compile_block_overlay(&view(), &blocks, false, Some(&keys)).unwrap());
    assert_eq!(compiled.overlay.visuals.len(), 1);
    assert_eq!(compiled.overlay.material_overrides.len(), 1);
    let empty_range = range.start..range.start;
    assert!(Arc::ptr_eq(
        &super::super::super::resource_packs::session_runtime_assets(
            &base,
            Some(&empty_range),
            Some(&compiled),
        ),
        &base,
    ));
    let session = super::super::super::resource_packs::session_runtime_assets(
        &base,
        Some(&range),
        Some(&compiled),
    );
    let admitted = session.resolve(NetworkIdMode::Sequential, range.start);
    assert!(
        admitted.is_known(),
        "the admitted custom visual must remain installed"
    );
    assert_eq!(admitted.kind(), VisualKind::Cube);
    assert_eq!(
        session
            .material(admitted.face(BlockFace::North).material_id())
            .texture
            .page(),
        1
    );
    assert_eq!(
        session.material(1).texture.page(),
        1,
        "the vanilla override must remain installed"
    );
    assert_eq!(session.visual_count(), base.visual_count() + 1);
    for id in [range.start + 1, range.end - 1] {
        let rejected = session.resolve(NetworkIdMode::Sequential, id);
        assert!(!rejected.is_known());
        assert_eq!(rejected.kind(), VisualKind::Diagnostic);
    }
    let bootstrap = protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    };
    let mut authority = client_world::WorldAuthority::new(bootstrap, session, None, [0.0; 3], None);
    authority.set_custom_block_ids(range.clone());
    authority.set_sequential_id_remap(remap);
    let ids = authority.decode_ids(0);
    assert_eq!(ids.resolve(ids.remap.to_wire(range.start)), range.start);
    assert_eq!(
        ids.resolve(ids.remap.to_wire(range.start + 1)),
        range.start + 1
    );
    let rejected_key = (protocol::block_name_sort_key(names[1]), names[1]);
    let later = records
        .iter()
        .find(|record| {
            record.sequential_id != ids.air
                && (
                    protocol::block_name_sort_key(&record.name),
                    record.name.as_ref(),
                ) > rejected_key
                && collisions
                    .block_identifier(NetworkIdMode::Sequential, record.sequential_id)
                    .is_some()
        })
        .expect("a vanilla block after the rejected run");
    let wire = later.sequential_id + blocks.total_states();
    assert_eq!(ids.resolve(wire), later.sequential_id);
    assert_eq!(ids.remap.to_wire(later.sequential_id), wire);
    assert_eq!(
        collisions.block_identifier(NetworkIdMode::Sequential, ids.resolve(wire)),
        Some(later.name.as_ref())
    );
}
