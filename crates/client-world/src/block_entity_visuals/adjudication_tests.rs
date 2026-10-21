use assets::{
    BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, Material, ModelQuad,
    ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, NetworkIdMode, TextureArray, TextureMip,
    TexturePage, TextureRef, VisualKind, encode_blob,
};

use super::{BackingBlockIdentity, BlockEntityVisualRoute, adjudicate_block_entity_visual};

/// Encodes a retained entity identity and bounded root fields.
fn block_entity_nbt(id: Option<&str>, fields: &[(u8, &str, u8)]) -> world::BlockEntityNbt {
    let mut bytes = vec![10, 0];
    if let Some(id) = id {
        bytes.extend([8, 2, b'i', b'd', id.len() as u8]);
        bytes.extend_from_slice(id.as_bytes());
    }
    for &(tag, name, value) in fields {
        bytes.extend([tag, name.len() as u8]);
        bytes.extend_from_slice(name.as_bytes());
        match tag {
            1 => bytes.push(value),
            3 => bytes.push(value << 1),
            _ => panic!("unsupported test tag"),
        }
    }
    bytes.push(0);
    world::BlockEntityNbt::decode_prefix(&bytes).unwrap().0
}

/// Resolves names from the real pinned registry, without copied ids or hashes.
fn records() -> &'static [assets::RegistryRecord] {
    static RECORDS: std::sync::OnceLock<Box<[assets::RegistryRecord]>> = std::sync::OnceLock::new();
    RECORDS.get_or_init(|| {
        assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap()
    })
}

/// Selects an actual registry record for a named backing block.
fn record(name: &str) -> &'static assets::RegistryRecord {
    records().iter().find(|r| r.name.as_ref() == name).unwrap()
}

/// Supplies the real identity with the visual kind being tested.
fn backing(name: &str, kind: VisualKind) -> BackingBlockIdentity {
    let record = record(name);
    let runtime = runtime_with_block_identity(record.sequential_id, record.network_hash, kind);
    BackingBlockIdentity::from_runtime(record.network_hash, NetworkIdMode::Hashed, &runtime)
}

/// Decodes the synthetic identity through the same checked carrier boundary as runtime.
fn runtime_with_block_identity(
    sequential_id: u32,
    network_hash: u32,
    kind: VisualKind,
) -> assets::RuntimeAssets {
    let compiled = compiled_block_identity(sequential_id, network_hash, kind);
    assets::RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

/// Builds bounded synthetic carrier data for block-entity route checks.
fn compiled_block_identity(
    sequential_id: u32,
    network_hash: u32,
    kind: VisualKind,
) -> CompiledAssets {
    let flags = if kind == VisualKind::Cube {
        BlockFlags::CUBE_GEOMETRY
    } else {
        BlockFlags::empty()
    };
    let visual = BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        support: if kind == VisualKind::Diagnostic {
            assets::VisualSupport::Diagnostic
        } else {
            assets::VisualSupport::Exact
        },
        faces: [0; 6],
        flags,
        kind,
        contributor_role: assets::ContributorRole::Primary,
        model_template: if kind == VisualKind::Model {
            0
        } else {
            NO_MODEL_TEMPLATE
        },
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    let mips = [16_u32, 8, 4, 2, 1]
        .map(|size| TextureMip {
            size,
            rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
        })
        .into();
    let count = sequential_id as usize + 1;
    CompiledAssets {
        visuals: vec![visual; count].into_boxed_slice(),
        light_properties: vec![assets::LightProperties::default(); count].into_boxed_slice(),
        hashed: vec![(network_hash, sequential_id)].into_boxed_slice(),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
            ..assets::Material::unvaried()
        }]
        .into_boxed_slice(),
        model_templates: if kind == VisualKind::Model {
            vec![ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: 0,
            }]
            .into_boxed_slice()
        } else {
            Box::new([])
        },
        model_quads: if kind == VisualKind::Model {
            vec![ModelQuad {
                positions: [[0; 3]; 4],
                uvs: [[0; 2]; 4],
                material: 0,
                flags: 0,
            }]
            .into_boxed_slice()
        } else {
            Box::new([])
        },
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray { layers: 1, mips })].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: *assets::pinned_world_provenance(),
    }
}

#[test]
fn block_entity_visuals_classify_static_logical_deferred_and_unknown_routes() {
    for (id, name) in [
        ("Barrel", "minecraft:barrel"),
        ("BlastFurnace", "minecraft:blast_furnace"),
        ("Furnace", "minecraft:lit_furnace"),
        ("Smoker", "minecraft:smoker"),
    ] {
        assert!(matches!(
            adjudicate_block_entity_visual(
                &block_entity_nbt(Some(id), &[]),
                backing(name, VisualKind::Cube)
            ),
            BlockEntityVisualRoute::ExistingBlockState {
                additional_refs: 0,
                ..
            }
        ));
    }
    assert!(matches!(
        adjudicate_block_entity_visual(
            &block_entity_nbt(Some("Jukebox"), &[]),
            backing("minecraft:jukebox", VisualKind::Cube)
        ),
        BlockEntityVisualRoute::LogicalNoAdditionalDraw { .. }
    ));
    assert!(matches!(
        adjudicate_block_entity_visual(
            &block_entity_nbt(None, &[(1, "note", 24), (1, "powered", 1)]),
            backing("minecraft:noteblock", VisualKind::Cube)
        ),
        BlockEntityVisualRoute::LogicalNoAdditionalDraw { .. }
    ));
    for (id, name, kind) in [
        ("Chest", "minecraft:chest", VisualKind::Invisible),
        (
            "Chest",
            "minecraft:waxed_exposed_copper_chest",
            VisualKind::Invisible,
        ),
        (
            "ShulkerBox",
            "minecraft:red_shulker_box",
            VisualKind::Invisible,
        ),
        (
            "HangingSign",
            "minecraft:oak_hanging_sign",
            VisualKind::Model,
        ),
        ("Sign", "minecraft:standing_sign", VisualKind::Model),
    ] {
        assert!(
            matches!(
                adjudicate_block_entity_visual(
                    &block_entity_nbt(Some(id), &[]),
                    backing(name, kind)
                ),
                BlockEntityVisualRoute::Deferred { .. }
            ),
            "{name}"
        );
        assert!(matches!(
            adjudicate_block_entity_visual(
                &block_entity_nbt(Some(id), &[]),
                backing("minecraft:stone", kind)
            ),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }
}

#[test]
fn block_entity_visuals_require_exact_backing_identity_and_geometry() {
    let beacon = block_entity_nbt(Some("Beacon"), &[]);
    assert!(matches!(
        adjudicate_block_entity_visual(&beacon, backing("minecraft:beacon", VisualKind::Model)),
        BlockEntityVisualRoute::Deferred { .. }
    ));
    assert!(matches!(
        adjudicate_block_entity_visual(&beacon, backing("minecraft:beacon", VisualKind::Invisible)),
        BlockEntityVisualRoute::Unknown { .. }
    ));
    let record = record("minecraft:beacon");
    let runtime = runtime_with_block_identity(
        record.sequential_id,
        record.network_hash.wrapping_add(1),
        VisualKind::Cube,
    );
    for (mode, value) in [
        (NetworkIdMode::Sequential, record.sequential_id),
        (NetworkIdMode::Hashed, record.network_hash.wrapping_add(1)),
    ] {
        assert!(matches!(
            adjudicate_block_entity_visual(
                &beacon,
                BackingBlockIdentity::from_runtime(value, mode, &runtime)
            ),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }
}

#[test]
fn block_entity_visuals_fail_closed_at_every_note_identity_boundary() {
    let valid_backing = backing("minecraft:noteblock", VisualKind::Cube);
    for fields in [
        vec![],
        vec![(1, "note", 0)],
        vec![(1, "powered", 0)],
        vec![(1, "note", 25), (1, "powered", 0)],
        vec![(1, "note", 24), (1, "powered", 2)],
        vec![(3, "note", 24), (1, "powered", 1)],
        vec![(1, "note", 24), (3, "powered", 1)],
        vec![(1, "note", 24), (1, "note", 24), (1, "powered", 1)],
        vec![(1, "note", 24), (1, "powered", 1), (1, "powered", 1)],
    ] {
        assert!(matches!(
            adjudicate_block_entity_visual(&block_entity_nbt(None, &fields), valid_backing),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }
    assert!(matches!(
        adjudicate_block_entity_visual(
            &block_entity_nbt(None, &[(1, "note", 0), (1, "powered", 0)]),
            backing("minecraft:stone", VisualKind::Cube)
        ),
        BlockEntityVisualRoute::Unknown { .. }
    ));
}

#[test]
fn block_entity_visuals_route_digest_is_stable_and_identity_bound() {
    let source = block_entity_nbt(Some("Barrel"), &[]);
    let first =
        adjudicate_block_entity_visual(&source, backing("minecraft:barrel", VisualKind::Cube));
    let repeated = adjudicate_block_entity_visual(
        &block_entity_nbt(Some("Barrel"), &[(1, "irrelevant", 9)]),
        backing("minecraft:barrel", VisualKind::Cube),
    );
    assert_eq!(first.route_digest(), repeated.route_digest());
    let first_record = record("minecraft:barrel");
    let other = records()
        .iter()
        .find(|r| r.name == first_record.name && r.sequential_id != first_record.sequential_id)
        .unwrap();
    let runtime =
        runtime_with_block_identity(other.sequential_id, other.network_hash, VisualKind::Cube);
    assert_ne!(
        first.route_digest(),
        adjudicate_block_entity_visual(
            &source,
            BackingBlockIdentity::from_runtime(
                other.sequential_id,
                NetworkIdMode::Sequential,
                &runtime
            )
        )
        .route_digest()
    );
    let record = record("minecraft:chest");
    let source = block_entity_nbt(Some("Chest"), &[]);
    let runtime = runtime_with_block_identity(
        record.sequential_id,
        record.network_hash,
        VisualKind::Invisible,
    );
    let sequential = adjudicate_block_entity_visual(
        &source,
        BackingBlockIdentity::from_runtime(
            record.sequential_id,
            NetworkIdMode::Sequential,
            &runtime,
        ),
    );
    let hashed = adjudicate_block_entity_visual(
        &source,
        BackingBlockIdentity::from_runtime(record.network_hash, NetworkIdMode::Hashed, &runtime),
    );
    assert_eq!(sequential.route_digest(), hashed.route_digest());
}

#[test]
fn block_entity_visuals_route_digest_distinguishes_unresolved_hashed_states() {
    let source = block_entity_nbt(Some("Barrel"), &[]);
    let record = record("minecraft:barrel");
    let runtime =
        runtime_with_block_identity(record.sequential_id, record.network_hash, VisualKind::Cube);
    let first = adjudicate_block_entity_visual(
        &source,
        BackingBlockIdentity::from_runtime(1, NetworkIdMode::Hashed, &runtime),
    );
    let second = adjudicate_block_entity_visual(
        &source,
        BackingBlockIdentity::from_runtime(2, NetworkIdMode::Hashed, &runtime),
    );
    assert!(matches!(first, BlockEntityVisualRoute::Unknown { .. }));
    assert!(matches!(second, BlockEntityVisualRoute::Unknown { .. }));
    assert_ne!(first.route_digest(), second.route_digest());
}

/// Encodes full-cube model geometry for the adjudicator's retained barrel identities.
fn compiled_barrels_with_retained_identities()
-> (CompiledAssets, Vec<&'static assets::RegistryRecord>) {
    let records = records()
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:barrel")
        .collect::<Vec<_>>();
    let last = records.last().unwrap();
    let mut compiled =
        compiled_block_identity(last.sequential_id, last.network_hash, VisualKind::Model);
    compiled.hashed = records
        .iter()
        .map(|r| (r.network_hash, r.sequential_id))
        .collect();
    compiled.hashed.sort_unstable();
    for visual in &mut compiled.visuals {
        visual.faces = [1; 6];
        visual.flags = BlockFlags::OCCLUDES_FULL_FACE;
    }
    compiled.materials = vec![
        Material::unvaried(),
        Material {
            texture: TextureRef::new(0, 1).unwrap(),
            ..Material::unvaried()
        },
    ]
    .into();
    compiled.texture_pages[0].texture.layers = 2;
    for mip in &mut compiled.texture_pages[0].texture.mips {
        mip.rgba8 = mip.rgba8.repeat(2).into();
    }
    compiled.model_quads = assets::BlockFace::ALL
        .into_iter()
        .map(|face| {
            let (corners, uvs) = assets::gui_item::cube_face(face);
            let positions =
                corners.map(|point| point.map(|coordinate| (coordinate * 256.0) as i16));
            let first = (0..4).min_by_key(|&corner| positions[corner]).unwrap();
            ModelQuad {
                positions: std::array::from_fn(|corner| positions[(first + corner) % 4]),
                uvs: std::array::from_fn(|corner| {
                    uvs[(first + corner) % 4].map(|coordinate| (coordinate * 4096.0) as u16)
                }),
                material: 1,
                flags: face.model_quad_face_id(),
            }
        })
        .collect();
    compiled.model_templates[0].quad_count = compiled.model_quads.len() as u32;
    (compiled, records)
}

#[test]
fn compiled_barrel_models_keep_existing_block_entity_routes() {
    let (compiled, records) = compiled_barrels_with_retained_identities();
    let runtime = assets::RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    let source = block_entity_nbt(Some("Barrel"), &[]);
    let mut count = 0;
    for record in records.iter() {
        assert!(
            assets::gui_item::full_cube_model_quads(
                &runtime,
                assets::BlockVisualId(record.sequential_id)
            )
            .is_some()
        );
        for (mode, value) in [
            (NetworkIdMode::Sequential, record.sequential_id),
            (NetworkIdMode::Hashed, record.network_hash),
        ] {
            let identity = BackingBlockIdentity::from_runtime(value, mode, &runtime);
            assert!(
                matches!(
                    adjudicate_block_entity_visual(&source, identity),
                    BlockEntityVisualRoute::ExistingBlockState {
                        additional_refs: 0,
                        ..
                    }
                ),
                "compiled barrel models retain their backed NBT route"
            );
            assert!(matches!(
                adjudicate_block_entity_visual(&block_entity_nbt(Some("Furnace"), &[]), identity),
                BlockEntityVisualRoute::Unknown { .. }
            ));
        }
        count += 1;
    }
    assert_eq!(count, 12);
}

#[test]
fn barrel_backing_rejects_models_without_full_cube_geometry() {
    let (mut compiled, records) = compiled_barrels_with_retained_identities();
    let record = records.first().unwrap();
    let template = compiled.model_templates
        [compiled.visuals[record.sequential_id as usize].model_template as usize];
    for point in &mut compiled.model_quads[template.quad_start as usize].positions {
        point[0] = 16;
    }
    let runtime = assets::RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    for (mode, value) in [
        (NetworkIdMode::Sequential, record.sequential_id),
        (NetworkIdMode::Hashed, record.network_hash),
    ] {
        assert!(matches!(
            adjudicate_block_entity_visual(
                &block_entity_nbt(Some("Barrel"), &[]),
                BackingBlockIdentity::from_runtime(value, mode, &runtime)
            ),
            BlockEntityVisualRoute::Unknown { .. }
        ));
    }
}
