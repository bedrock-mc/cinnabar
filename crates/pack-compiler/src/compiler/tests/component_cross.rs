use super::*;
use assets::{BlockFace, CompiledBiomeAssets, ModelFamily, NetworkIdMode, RuntimeAssets};

/// Builds an original sprite whose only binding is an authored behavior-pack material.
fn fixture(components: serde_json::Value) -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let protocol = assets::active_content_registry_protocol();
    let records =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
            .unwrap()
            .into_vec()
            .into_iter()
            .filter(|record| {
                matches!(
                    record.name.as_ref(),
                    "minecraft:air"
                        | "minecraft:red_shrub"
                        | "minecraft:reeds"
                        | "minecraft:short_grass"
                        | "minecraft:fern"
                )
            })
            .collect::<Vec<_>>();
    let root = tempfile::tempdir().unwrap();
    let resource = root.path().join("resource_pack");
    let behavior = root.path().join("behavior_pack");
    write(
        resource.join("blocks.json"),
        r#"{"red_shrub":{"sound":"red_shrub"},"reeds":{"textures":"shrub_art"},"short_grass":{"textures":"shrub_art"},"fern":{"textures":"shrub_art"}}"#,
    );
    write(
        resource.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"shrub_art":{"textures":"textures/shrub"}}}"#,
    );
    write(resource.join("textures/flipbook_textures.json"), "[]");
    write_png(
        resource.join("textures/shrub.png"),
        TILE_SIZE,
        TILE_SIZE,
        &[211, 39, 63, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
    );
    write(behavior.join("blocks/shrub.block.json"), serde_json::to_vec(&serde_json::json!({
        "minecraft:block": {"description":{"identifier":"minecraft:red_shrub"},"components": components}
    })).unwrap());
    let pack =
        pack_compile_input::pack::read_pack_with_behavior(&resource, &behavior, &records).unwrap();
    let count = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_sources(
        &resource,
        pack,
        &records,
        &vec![assets::LightProperties::default(); count],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

/// Declares the supported builtin cross with an untinted single-sided material and AO disabled.
fn components() -> serde_json::Value {
    serde_json::json!({
        "minecraft:geometry":{"identifier":"minecraft:geometry.cross"},
        "minecraft:material_instances":{"*":{"texture":"shrub_art","render_method":"alpha_test_single_sided","ambient_occlusion":0.0}}
    })
}

#[test]
fn behavior_cross_material_renders_red_shrub_art_with_four_inset_faces() {
    let (compiled, records) = fixture(components());
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let shrub = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:red_shrub")
        .unwrap();
    assert_eq!(shrub.model_family, ModelFamily::Cross);
    for (mode, id) in [
        (NetworkIdMode::Sequential, shrub.sequential_id),
        (NetworkIdMode::Hashed, shrub.network_hash),
    ] {
        let visual = runtime.resolve(mode, id);
        assert_eq!(visual.kind(), VisualKind::Cross);
        let material = runtime.material(visual.face(BlockFace::Down).material_id());
        assert_ne!(
            visual.face(BlockFace::Down).material_id(),
            assets::DIAGNOSTIC_MATERIAL
        );
        assert_ne!(material.flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT, 0);
        assert_ne!(material.flags & assets::MATERIAL_FLAG_DISABLE_AO, 0);
        assert_eq!(
            material.flags & (assets::MATERIAL_FLAG_TWO_SIDED | assets::MATERIAL_FLAG_TINT_MASK),
            0
        );
        let model = compiled.model_templates[visual.model_template().unwrap() as usize];
        let quads = &compiled.model_quads
            [model.quad_start as usize..(model.quad_start + model.quad_count) as usize];
        assert_eq!(quads.len(), 4);
        for pair in quads.as_chunks::<2>().0 {
            assert_eq!(
                pair[1].positions,
                [
                    pair[0].positions[1],
                    pair[0].positions[0],
                    pair[0].positions[3],
                    pair[0].positions[2]
                ]
            );
            assert_eq!(pair[0].uvs, pair[1].uvs);
        }
        for quad in quads {
            assert_eq!(quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED, 0);
            assert!(quad.positions.iter().all(|p| [13, 243].contains(&p[0])
                && [0, 256].contains(&p[1])
                && [13, 243].contains(&p[2])));
        }
        let texture = material.texture;
        let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
        let offset = (texture.layer() * mip.size * mip.size * 4) as usize;
        assert_eq!(&mip.rgba8[offset..offset + 4], &[211, 39, 63, 255]);
    }
}

#[test]
fn unsupported_component_cross_transforms_retain_the_diagnostic_model() {
    let mut unsupported = components();
    unsupported["minecraft:transformation"] = serde_json::json!({"rotation":[0,45,0]});
    let (compiled, records) = fixture(unsupported);
    let shrub = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:red_shrub")
        .unwrap();
    assert_eq!(
        compiled.visuals[shrub.sequential_id as usize].kind,
        VisualKind::Diagnostic
    );
}

#[test]
fn reed_growth_states_keep_narrow_planes_aligned_between_stacked_segments() {
    let (compiled, records) = fixture(components());
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let reeds = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:reeds")
        .collect::<Vec<_>>();
    assert!(!reeds.is_empty());
    let mut shared_template = None;
    for record in reeds {
        for (mode, id) in [
            (NetworkIdMode::Sequential, record.sequential_id),
            (NetworkIdMode::Hashed, record.network_hash),
        ] {
            let visual = runtime.resolve(mode, id);
            assert_eq!(visual.kind(), VisualKind::Cross);
            let material = runtime.material(visual.face(BlockFace::Down).material_id());
            assert_eq!(
                material.flags & assets::MATERIAL_FLAG_TINT_MASK,
                assets::MATERIAL_FLAG_GRASS_TINT,
                "reed state {} must receive its biome grass colour",
                record.canonical_state
            );
            let id = visual.model_template().unwrap();
            assert_eq!(*shared_template.get_or_insert(id), id);
            assert!(runtime.model_random_offset(id).is_none());
            let template = compiled.model_templates[id as usize];
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            assert_eq!(quads.len(), 2);
            for quad in quads {
                assert!(quad.positions.iter().all(|p| [36, 220].contains(&p[0])
                    && [0, 256].contains(&p[1])
                    && [36, 220].contains(&p[2])));
                for (p, uv) in quad.positions.iter().zip(quad.uvs) {
                    assert_eq!(uv[1], if p[1] == 0 { 4096 } else { 0 });
                }
            }
        }
    }
}

#[test]
fn short_grass_and_fern_use_narrow_sprites_and_deterministic_column_displacement() {
    let (compiled, records) = fixture(components());
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    for name in ["minecraft:short_grass", "minecraft:fern"] {
        let record = records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap();
        for (mode, id) in [
            (NetworkIdMode::Sequential, record.sequential_id),
            (NetworkIdMode::Hashed, record.network_hash),
        ] {
            let visual = runtime.resolve(mode, id);
            assert_eq!(visual.kind(), VisualKind::Cross);
            let template_id = visual.model_template().unwrap();
            let offset = runtime.model_random_offset(template_id).unwrap();
            assert_eq!(offset.axes.map(|axis| axis.steps), [16; 3]);
            assert_eq!(
                offset.axes.map(|axis| axis.range),
                [[-0.25, 0.25], [-0.2, 0.0], [-0.25, 0.25]]
            );
            for position in [[0, 64, 0], [-31, 90, 18], [517, 70, -263]] {
                let sample = offset.offset(position);
                assert_eq!(sample, offset.offset([position[0], -150, position[2]]));
                assert!((-0.25..=0.25).contains(&sample[0]));
                assert!((-0.2..=0.0).contains(&sample[1]));
                assert!((-0.25..=0.25).contains(&sample[2]));
            }
            assert_ne!(offset.offset([0; 3]), offset.offset([1, 0, 3]));
            let template = compiled.model_templates[template_id as usize];
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            assert_eq!(quads.len(), 2);
            assert!(
                quads
                    .iter()
                    .flat_map(|q| q.positions)
                    .all(|p| [36, 220].contains(&p[0])
                        && [0, 256].contains(&p[1])
                        && [36, 220].contains(&p[2]))
            );
            assert_eq!(
                runtime
                    .material(visual.face(BlockFace::Down).material_id())
                    .flags
                    & assets::MATERIAL_FLAG_TINT_MASK,
                assets::MATERIAL_FLAG_GRASS_TINT
            );
        }
    }
}
