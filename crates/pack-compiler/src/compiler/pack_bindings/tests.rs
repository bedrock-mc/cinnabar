use super::*;
use assets::{ModelFamily, ModelStateField};
use serde_json::json;

/// Supplies a scalar world binding and optionally tinted carried art.
fn pack(tinted_carried: bool) -> PackSources {
    pack_with_binding(tinted_carried, json!("world"))
}

/// Creates original catalogs while preserving the supplied world binding's shape.
fn pack_with_binding(tinted_carried: bool, textures: serde_json::Value) -> PackSources {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures")).unwrap();
    std::fs::write(
        root.path().join("blocks.json"),
        json!({"fixture":{"textures":textures,"carried_textures":"carry"}}).to_string(),
    )
    .unwrap();
    std::fs::write(root.path().join("textures/terrain_texture.json"), json!({"texture_data":{"world":{"textures":"textures/world"},"carry":{"textures":if tinted_carried { json!({"path":"textures/carry","tint_color":"#ff0000"}) } else { json!("textures/carry") }}}}).to_string()).unwrap();
    std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
    read_pack(root.path()).unwrap()
}

/// Minimal typed registry and visual facts for a plain full cube.
fn cube() -> (RegistryRecord, BlockVisual, Vec<Material>) {
    let record = RegistryRecord {
        sequential_id: 0,
        network_hash: 1,
        name: "minecraft:fixture".into(),
        canonical_state: "{}".into(),
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        model_family: ModelFamily::Cube,
        contributor_role: ContributorRole::Primary,
        model_state: Default::default(),
        face_coverage: 0x3f,
        collision_seed: Default::default(),
        provenance: assets::RegistryProvenance::PMMP,
        beacon_tint: assets::BeaconTint::Unresolved,
        traits: assets::registry::BlockTraits::empty(),
        survival: assets::registry::SurvivalRule::None,
        properties: Box::new([]),
    };
    let mut visual = BlockVisual::diagnostic(record.flags, record.contributor_role);
    visual.faces = [1; 6];
    visual.kind = VisualKind::Cube;
    visual.support = VisualSupport::Exact;
    (record, visual, vec![Material::unvaried(); 2])
}

/// Exercises the actual sidecar publisher so an excluded block cannot acquire a partial route.
fn retained_eligibility(
    pack: &PackSources,
    record: RegistryRecord,
    visual: BlockVisual,
    materials: &[Material],
) -> bool {
    let keys = retain(
        MaterialKeys::default(),
        (pack, Path::new("")),
        &BTreeMap::new(),
        materials,
        (&[record], &[visual]),
        &[],
        &[],
    )
    .unwrap();
    let restored =
        MaterialKeys::from_json(&keys.to_json(materials.len() as u32), materials.len()).unwrap();
    restored.block_sources()[0].cube_binding_eligible
}

#[test]
fn cube_binding_eligibility_requires_plain_faces_and_axis_only_orientation() {
    let pack = pack(false);
    let (record, visual, materials) = cube();
    assert!(retained_eligibility(
        &pack,
        record.clone(),
        visual,
        &materials
    ));
    for axis in ["x", "y", "z"] {
        let mut record = record.clone();
        record.canonical_state = json!({"pillar_axis":{"type":"string","value":axis}})
            .to_string()
            .into();
        record.model_state.set(ModelStateField::Orientation, 1);
        assert!(retained_eligibility(&pack, record, visual, &materials));
    }
    let mut malformed = record.clone();
    malformed.canonical_state = json!({"pillar_axis":{"type":"string","value":"invalid"}})
        .to_string()
        .into();
    assert!(!retained_eligibility(&pack, malformed, visual, &materials));
    let mut fallback = visual;
    fallback.support = VisualSupport::VanillaFallback;
    assert!(!retained_eligibility(&pack, record, fallback, &materials));
}

#[test]
fn pottable_plants_are_excluded_from_both_binding_routes() {
    let (mut record, mut visual, materials) = cube();
    record.model_family = ModelFamily::Cross;
    visual.kind = VisualKind::Cross;
    visual.model_template = 1;
    assert!(!retained_eligibility(
        &pack(false),
        record,
        visual,
        &materials
    ));
}

#[test]
fn directional_cubes_are_excluded_even_at_zero_rotation() {
    for direction in 0..4 {
        let (mut record, visual, materials) = cube();
        record.canonical_state = json!({"direction":{"type":"int","value":direction}})
            .to_string()
            .into();
        record
            .model_state
            .set(ModelStateField::Orientation, direction);
        assert!(!retained_eligibility(
            &pack(false),
            record,
            visual,
            &materials
        ));
    }
    let (mut record, visual, materials) = cube();
    record.model_state.set(ModelStateField::Orientation, 0);
    assert!(!retained_eligibility(
        &pack(false),
        record,
        visual,
        &materials
    ));
}

#[test]
fn internal_material_and_container_models_are_excluded() {
    let (mut record, mut visual, materials) = cube();
    record.model_family = ModelFamily::Cuboid;
    visual.kind = VisualKind::Model;
    visual.model_template = 0;
    assert!(!retained_eligibility(
        &pack(false),
        record,
        visual,
        &materials
    ));
}

#[test]
fn tinted_world_or_carried_faces_are_excluded_as_a_whole() {
    let (record, visual, mut materials) = cube();
    materials[1].flags = assets::MATERIAL_FLAG_GRASS_TINT;
    assert!(!retained_eligibility(
        &pack(false),
        record.clone(),
        visual,
        &materials
    ));
    materials[1].flags = 0;
    let pack = pack(true);
    assert!(!retained_eligibility(&pack, record, visual, &materials));
}

#[test]
fn wall_fence_and_other_inventory_models_are_excluded() {
    for family in [
        ModelFamily::Wall,
        ModelFamily::Fence,
        ModelFamily::Stair,
        ModelFamily::Slab,
    ] {
        let (mut record, mut visual, materials) = cube();
        record.model_family = family;
        visual.kind = VisualKind::Model;
        visual.model_template = 0;
        assert!(!retained_eligibility(
            &pack(false),
            record,
            visual,
            &materials
        ));
    }
}

#[test]
fn pillar_mapping_retains_uv_rotation_independently_of_the_original_scalar_binding() {
    let pack = pack(false);
    let per_face = pack_with_binding(
        false,
        json!({"west":"world","east":"world","down":"world","up":"world","north":"world","south":"world"}),
    );
    for axis in ["x", "y", "z"] {
        let (mut record, visual, materials) = cube();
        record.canonical_state = json!({"pillar_axis":{"type":"string","value":axis}})
            .to_string()
            .into();
        let keys = retain(
            MaterialKeys::default(),
            (&pack, Path::new("")),
            &BTreeMap::new(),
            &materials,
            (&[record.clone()], &[visual]),
            &[],
            &[],
        )
        .unwrap();
        let source = &keys.block_sources()[0];
        for face in BlockFace::ALL {
            assert!(
                !pack_compile_input::pack::resolve_texture_key(&pack.blocks, &record, face)
                    .rotate_uv
            );
            assert_eq!(
                source.source_rotations[face as usize],
                pack_compile_input::pack::resolve_texture_key(&per_face.blocks, &record, face)
                    .rotate_uv
            );
        }
        assert_eq!(
            source.source_rotations.iter().any(|&rotate| rotate),
            axis != "y"
        );
    }
}

#[test]
fn fixed_atlas_tint_is_retained_only_on_the_baked_material_copy() {
    let pack = pack(false);
    let tint = [128, 64, 255];
    let descriptors = BTreeMap::from([(
        Descriptor {
            path: "textures/world".into(),
            texture_key: "world".into(),
            flags: 0,
            state_variant: 0,
            flipbook_selector: None,
        },
        1,
    )]);
    let keys = retain(
        MaterialKeys::from_entries([(1, "world"), (2, "world")])
            .with_fixed_tints([("world", tint)]),
        (&pack, Path::new("")),
        &descriptors,
        &[Material::unvaried(); 4],
        (&[], &[]),
        &[(2, "world".into())],
        &[(3, 2)],
    )
    .unwrap();
    let keys = MaterialKeys::from_json(&keys.to_json(4), 4).unwrap();
    assert_eq!(keys.material_source(1).unwrap().baked_tint, None);
    assert_eq!(keys.material_source(2).unwrap().baked_tint, Some(tint));
    assert_eq!(keys.material_source(3).unwrap().baked_tint, Some(tint));
}

#[test]
fn carrier_source_identity_retains_the_filename_selected_by_the_image_loader() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures")).unwrap();
    std::fs::write(root.path().join("textures/fixture.tga"), []).unwrap();
    assert_eq!(
        raster_identity(root.path(), "textures/fixture")
            .unwrap()
            .as_ref(),
        "textures/fixture.tga"
    );
    std::fs::write(root.path().join("textures/fixture.png"), []).unwrap();
    let preferred = format!(
        "textures/fixture.{}",
        pack_compile_input::texture_path::IMAGE_EXTENSIONS[0].0
    );
    assert_eq!(
        raster_identity(root.path(), "textures/fixture")
            .unwrap()
            .as_ref(),
        preferred.as_str()
    );
    for path in ["textures/fixture.png", "./textures/fixture.png"] {
        assert_eq!(
            raster_identity(root.path(), path).unwrap().as_ref(),
            "textures/fixture.png"
        );
    }
}

#[test]
fn unordered_registry_records_publish_sources_in_visual_order() {
    let pack = pack(false);
    let (first, visual, materials) = cube();
    let mut second = first.clone();
    second.sequential_id = 1;
    second.network_hash = 2;
    second.name = "minecraft:other".into();
    let keys = retain(
        MaterialKeys::default(),
        (&pack, Path::new("")),
        &BTreeMap::new(),
        &materials,
        (&[second, first], &[visual; 2]),
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(
        keys.block_sources()
            .iter()
            .map(|source| source.visual)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    let restored =
        MaterialKeys::from_json(&keys.to_json(materials.len() as u32), materials.len()).unwrap();
    for visual in [0, 1] {
        assert!(
            restored
                .block_sources()
                .binary_search_by_key(&visual, |source| source.visual)
                .is_ok()
        );
    }
}
