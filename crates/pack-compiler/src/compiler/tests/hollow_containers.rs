use super::*;
use assets::{BlockFace, BlockFlags, CompiledBiomeAssets, ModelQuad};

/// Compiles the active vessel states with distinct original texture-slot artwork.
fn compile_containers() -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    compile_with_state(None)
}

/// Replaces one canonical state to exercise malformed selector admission.
fn compile_with_state(
    replacement: Option<(&str, &str)>,
) -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let (mut records, lights, protocol) = super::shelf_cake::active_records(&[
        "minecraft:stone",
        "minecraft:composter",
        "minecraft:cauldron",
        "minecraft:lava_cauldron",
    ]);
    if let Some((name, state)) = replacement {
        records
            .iter_mut()
            .find(|r| r.name.as_ref() == name)
            .unwrap()
            .canonical_state = state.into();
    }
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write(
        root.join("blocks.json"),
        r#"{
        "stone":{"textures":"stone"},
        "composter":{"textures":{"down":"wood_bottom","side":"wood_side","up":"wood_top"}},
        "cauldron":{"textures":{"down":"metal_bottom","up":"metal_top","north":"metal_side","south":"metal_inner","west":"water","east":"grey_water"}},
        "lava_cauldron":{"textures":{"down":"metal_bottom","up":"metal_top","north":"metal_side","south":"metal_inner","west":"water","east":"still_lava"}}
    }"#,
    );
    let keys = [
        "stone",
        "wood_bottom",
        "wood_side",
        "wood_top",
        "compost",
        "ready",
        "metal_bottom",
        "metal_top",
        "metal_side",
        "metal_inner",
        "water",
        "grey_water",
        "still_lava",
        "powder_snow",
    ];
    let mut terrain = serde_json::Map::new();
    for (i, key) in keys.iter().enumerate() {
        terrain.insert(
            (*key).into(),
            serde_json::json!({"textures":format!("textures/fixture/{key}")}),
        );
        let mut rgba = Vec::new();
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let rim_hole = matches!(*key, "wood_top" | "metal_top")
                    && (2..14).contains(&x)
                    && (2..14).contains(&y);
                let foot_hole = *key == "metal_side" && y >= 13 && (4..12).contains(&x);
                rgba.extend([
                    40 + i as u8 * 12,
                    80 + x as u8 * 6,
                    60 + y as u8 * 6,
                    if rim_hole || foot_hole { 0 } else { 255 },
                ]);
            }
        }
        write_png(
            root.join(format!("textures/fixture/{key}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &rgba,
        );
    }
    terrain.insert("wood_top".into(), serde_json::json!({"textures":["textures/fixture/wood_top", "textures/fixture/compost", "textures/fixture/ready"]}));
    write(
        root.join("textures/terrain_texture.json"),
        serde_json::to_vec(&serde_json::json!({"texture_data":terrain})).unwrap(),
    );
    write(root.join("textures/flipbook_textures.json"), "[]");
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        root,
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    if let Some(path) =
        std::env::var_os("CINNABAR_HOLLOW_CARRIER").filter(|_| replacement.is_none())
    {
        fs::write(path, assets::encode_blob(&compiled).unwrap()).unwrap();
    }
    (compiled, records)
}

/// Returns the geometric normal without depending on a carrier's face metadata.
fn normal(quad: &ModelQuad) -> [i32; 3] {
    let [a, b, c, _] = quad.positions;
    let u = std::array::from_fn::<_, 3, _>(|i| i32::from(b[i] - a[i]));
    let v = std::array::from_fn::<_, 3, _>(|i| i32::from(c[i] - a[i]));
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

/// Reads one original-art texel from the material's serialized atlas layer.
fn texel(runtime: &assets::RuntimeAssets, material: u32, x: usize, y: usize) -> [u8; 4] {
    let texture = runtime.material(material).texture;
    let mip = &runtime.texture_pages()[texture.page() as usize]
        .texture
        .mips[0];
    let side = mip.size as usize;
    let start = (texture.layer() as usize * side * side + y * side + x) * 4;
    mip.rgba8[start..start + 4].try_into().unwrap()
}

#[test]
fn hollow_containers_have_visible_inward_walls_and_an_inset_floor() {
    let (compiled, records) = compile_containers();
    let runtime = assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut composter_states = 0;
    let mut cauldron_states = 0;
    for record in records.iter().filter(|r| {
        matches!(
            r.name.as_ref(),
            "minecraft:composter" | "minecraft:cauldron" | "minecraft:lava_cauldron"
        )
    }) {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(
            runtime
                .resolve(assets::NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            Some(visual.model_template)
        );
        if record.name.as_ref() == "minecraft:composter" {
            composter_states += 1;
        } else {
            cauldron_states += 1;
        }
        assert_eq!(
            visual.kind,
            VisualKind::Model,
            "{} {}",
            record.name,
            record.canonical_state
        );
        assert!(
            !visual
                .flags
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        let parts =
            assets::model_template_parts(&compiled.model_templates, visual.model_template).unwrap();
        let quads = parts
            .iter()
            .flat_map(|part| {
                &compiled.model_quads
                    [part.quad_start as usize..(part.quad_start + part.quad_count) as usize]
            })
            .collect::<Vec<_>>();
        for (axis, coordinate, sign) in [(0, 32, 1), (0, 224, -1), (2, 32, 1), (2, 224, -1)] {
            assert!(
                quads
                    .iter()
                    .any(|q| q.positions.iter().all(|p| p[axis] == coordinate)
                        && normal(q)[axis] * sign > 0),
                "{} needs an inward wall on axis {axis} at {coordinate}",
                record.name
            );
        }
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let level = state["composter_fill_level"]["value"].as_u64().unwrap_or(0);
        let floor_height = if record.name.as_ref() == "minecraft:composter" {
            if level == 0 {
                32
            } else if level == 8 {
                240
            } else {
                16 + level as i16 * 32
            }
        } else {
            64
        };
        assert!(
            quads
                .iter()
                .any(|q| q.positions.iter().all(|p| p[1] == floor_height
                    && (32..=224).contains(&p[0])
                    && (32..=224).contains(&p[2]))
                    && normal(q)[1] > 0),
            "{} {} needs its cavity floor at {floor_height}",
            record.name,
            record.canonical_state
        );
        assert!(
            quads
                .iter()
                .all(|q| q.flags & (assets::MODEL_QUAD_FLAG_FACE_MASK << 4) == 0),
            "neighbours cannot remove the interior"
        );
        assert!(visual.faces[BlockFace::Down as usize] != assets::DIAGNOSTIC_MATERIAL);
        let inner_floor = quads
            .iter()
            .find(|q| q.positions.iter().all(|p| p[1] == floor_height) && normal(q)[1] > 0)
            .unwrap();
        let floor_art = if record.name.as_ref() == "minecraft:composter" {
            if level == 0 {
                1
            } else if level == 8 {
                5
            } else {
                4
            }
        } else {
            9
        };
        assert_eq!(
            texel(&runtime, inner_floor.material, 8, 8),
            [40 + floor_art * 12, 128, 108, 255],
            "{} {} floor texture",
            record.name,
            record.canonical_state
        );
        if record.name.as_ref() == "minecraft:cauldron" {
            let level = state["fill_level"]["value"].as_u64().unwrap();
            let liquid = state["cauldron_liquid"]["value"].as_str().unwrap();
            let fill_height = 48 + level as i16 * 32;
            let fill = quads
                .iter()
                .filter(|q| q.positions.iter().all(|p| p[1] == fill_height) && normal(q)[1] > 0)
                .collect::<Vec<_>>();
            if level == 0 {
                assert!(fill.is_empty());
            } else {
                assert_eq!(fill.len(), 1, "{liquid} {level}");
                let material = runtime.material(fill[0].material);
                assert_eq!(
                    material.flags
                        & (assets::MATERIAL_FLAG_ALPHA_BLEND | assets::MATERIAL_FLAG_ALPHA_CUTOUT),
                    if liquid == "water" {
                        assets::MATERIAL_FLAG_ALPHA_BLEND
                    } else {
                        assets::MATERIAL_FLAG_ALPHA_CUTOUT
                    }
                );
                assert_eq!(
                    material.flags & assets::MATERIAL_FLAG_CAULDRON_WATER != 0,
                    liquid == "water"
                );
                let fill_art = match liquid {
                    "water" => 11,
                    "lava" => 12,
                    "powder_snow" => 13,
                    _ => unreachable!(),
                };
                assert_eq!(
                    texel(&runtime, fill[0].material, 8, 8),
                    [40 + fill_art * 12, 128, 108, 255],
                    "{liquid} {level} fill texture"
                );
            }
            assert!(
                quads
                    .iter()
                    .filter(|q| q.positions.iter().all(|p| p[1] == 256))
                    .all(|q| q.positions.iter().all(|p| p[0] <= 32)
                        || q.positions.iter().all(|p| p[0] >= 224)
                        || q.positions.iter().all(|p| p[2] <= 32)
                        || q.positions.iter().all(|p| p[2] >= 224)),
                "the cauldron rim must leave its center open even with an opaque texture override"
            );
            assert!(
                quads
                    .iter()
                    .any(|q| q.positions.iter().all(|p| p[1] == 0) && normal(q)[1] < 0),
                "feet need bottom surfaces"
            );
        } else {
            let floor = quads
                .iter()
                .find(|q| q.positions.iter().all(|p| p[1] == floor_height) && normal(q)[1] > 0)
                .unwrap();
            let texture = runtime.material(floor.material).texture;
            let rim = runtime
                .material(visual.faces[BlockFace::Up as usize])
                .texture;
            assert_eq!(
                texture == rim,
                false,
                "floor must select bottom/compost/ready instead of the transparent rim"
            );
        }
    }
    assert_eq!((composter_states, cauldron_states), (9, 21));
}

#[test]
fn hollow_containers_reject_unknown_and_mistyped_fill_states() {
    for (name, state) in [
        ("minecraft:composter", r#"{}"#),
        (
            "minecraft:composter",
            r#"{"composter_fill_level":{"type":"int","value":9}}"#,
        ),
        (
            "minecraft:composter",
            r#"{"composter_fill_level":{"type":"byte","value":0}}"#,
        ),
        (
            "minecraft:cauldron",
            r#"{"cauldron_liquid":{"type":"string","value":"water"},"fill_level":{"type":"int","value":7}}"#,
        ),
        (
            "minecraft:cauldron",
            r#"{"cauldron_liquid":{"type":"string","value":"potion"},"fill_level":{"type":"int","value":3}}"#,
        ),
        (
            "minecraft:cauldron",
            r#"{"cauldron_liquid":{"type":"string","value":"water"},"fill_level":{"type":"byte","value":3}}"#,
        ),
    ] {
        let (compiled, records) = compile_with_state(Some((name, state)));
        let record = records.iter().find(|r| r.name.as_ref() == name).unwrap();
        assert_eq!(
            compiled.visuals[record.sequential_id as usize].kind,
            VisualKind::Diagnostic,
            "{name} {state}"
        );
    }
}
