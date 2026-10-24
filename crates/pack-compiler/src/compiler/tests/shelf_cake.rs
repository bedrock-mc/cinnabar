use super::*;
use assets::{CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Loads the active registry and lights so fixtures exercise the runtime identities.
pub(super) fn active_records(
    names: &[&str],
) -> (Vec<RegistryRecord>, Box<[assets::LightProperties]>, u32) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            record.name.as_ref() == "minecraft:air"
                || names.is_empty()
                || names.contains(&record.name.as_ref())
        })
        .collect();
    (records, lights, protocol)
}

/// Writes independently colored cake sprites with transparent unused texels.
pub(super) fn cake_pack(root: &Path) {
    write(
        root.join("blocks.json"),
        r#"{"cake":{"textures":{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west"}}}"#,
    );
    write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
        "cake_bottom":{"textures":["textures/blocks/cake_bottom","textures/blocks/cake_bottom"]},
        "cake_side":{"textures":["textures/blocks/cake_side","textures/blocks/cake_side"]},
        "cake_top":{"textures":["textures/blocks/cake_top","textures/blocks/cake_top"]},
        "cake_west":{"textures":["textures/blocks/cake_side","textures/blocks/cake_inner"]}
    }}"#,
    );
    write(root.join("textures/flipbook_textures.json"), "[]");
    for (name, top, bottom, color) in [
        ("cake_bottom", 1, 14, [90, 40, 10, 255]),
        ("cake_side", 8, 15, [130, 70, 30, 255]),
        ("cake_top", 1, 14, [240, 210, 180, 255]),
        ("cake_inner", 8, 15, [180, 140, 70, 255]),
    ] {
        let mut pixels = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
        for y in top..=bottom {
            for x in 1..=14 {
                let offset = ((y * TILE_SIZE + x) * 4) as usize;
                pixels[offset..offset + 4].copy_from_slice(&color);
            }
        }
        write_png(
            root.join(format!("textures/blocks/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
}

#[test]
fn current_cakes_compile_all_bites_with_the_cut_face_texture() {
    let (records, lights, protocol) = active_records(&["minecraft:cake"]);
    let directory = tempfile::tempdir().unwrap();
    cake_pack(directory.path());
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:cake")
    {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            visual.kind(),
            VisualKind::Model,
            "{}",
            record.canonical_state
        );
        assert_eq!(visual.support(), VisualSupport::Exact);
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), 6);
        assert!(
            quads
                .iter()
                .all(|quad| quad.material != assets::DIAGNOSTIC_MATERIAL)
        );
        let bite: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let bite = bite["bite_counter"]["value"].as_u64().unwrap() as i16;
        let min_x = quads
            .iter()
            .flat_map(|quad| quad.positions)
            .map(|point| point[0])
            .min()
            .unwrap();
        assert_eq!(min_x, 16 + 32 * bite);
        assert_eq!(quads[0].material == quads[1].material, bite == 0);
    }
}

#[test]
fn poplar_shelves_keep_the_recess_and_powered_front_through_the_carrier() {
    let (records, lights, protocol) = active_records(&["minecraft:poplar_shelf"]);
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        r#"{"poplar_shelf":{"textures":"fixture_shelf"}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"fixture_shelf":{"textures":["textures/fixture/shelf"]}}}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let mut pixels = vec![0; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let offset = (y * 32 + x) * 4;
            pixels[offset..offset + 4].copy_from_slice(&[x as u8 * 7, y as u8 * 7, 40, 255]);
        }
    }
    write_png(
        directory.path().join("textures/fixture/shelf.png"),
        32,
        32,
        &pixels,
    );
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let shelves = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:poplar_shelf")
        .collect::<Vec<_>>();
    assert_eq!(shelves.len(), 32);
    let mut front_regions = std::collections::BTreeSet::new();
    for record in shelves {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            visual.kind(),
            VisualKind::Model,
            "{}",
            record.canonical_state
        );
        assert_eq!(visual.support(), VisualSupport::Exact);
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(
            quads.len(),
            16,
            "back, recessed front and two projecting ledges"
        );
        assert!(
            quads
                .iter()
                .all(|quad| quad.material != assets::DIAGNOSTIC_MATERIAL)
        );
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state["minecraft:cardinal_direction"]["value"]
            .as_str()
            .unwrap();
        let (axis, min, max) = match direction {
            "south" => (2, 0, 80),
            "west" => (0, 176, 256),
            "north" => (2, 176, 256),
            "east" => (0, 0, 80),
            _ => unreachable!(),
        };
        assert_eq!(
            quads
                .iter()
                .flat_map(|quad| quad.positions)
                .map(|point| point[axis])
                .min(),
            Some(min)
        );
        assert_eq!(
            quads
                .iter()
                .flat_map(|quad| quad.positions)
                .map(|point| point[axis])
                .max(),
            Some(max)
        );
        let recessed = quads
            .iter()
            .filter(|quad| {
                quad.positions
                    .iter()
                    .all(|point| point[1] == 64 || point[1] == 192)
                    && quad.positions.iter().any(|point| point[1] == 64)
                    && quad.positions.iter().any(|point| point[1] == 192)
            })
            .collect::<Vec<_>>();
        assert_eq!(recessed.len(), 1);
        let front = recessed[0];
        let plane = if min == 0 { 48 } else { 208 };
        assert!(front.positions.iter().all(|point| point[axis] == plane));
        let powered = state["powered_bit"]["value"].as_u64().unwrap();
        if direction == "south" {
            front_regions.insert(front.uvs);
        }
        if powered == 0 {
            assert!(front.uvs.iter().all(|uv| uv[0] < 2048 && uv[1] < 2048));
        } else {
            assert!(front.uvs.iter().all(|uv| uv[1] > 2048));
        }
    }
    assert_eq!(
        front_regions.len(),
        5,
        "unpowered and all four powered fronts"
    );
}

/// Builds a shared original sheet for the named shelves, allowing alias routes.
fn shared_shelf_pack(root: &Path, records: &[RegistryRecord]) {
    let blocks: serde_json::Map<String, serde_json::Value> = records
        .iter()
        .filter(|record| super::super::visuals::shelf::is_record(record))
        .map(|record| {
            (
                record.name.strip_prefix("minecraft:").unwrap().to_owned(),
                serde_json::json!({"textures": "shared_shelf"}),
            )
        })
        .collect();
    write(
        root.join("blocks.json"),
        serde_json::to_string(&blocks).unwrap(),
    );
    write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"shared_shelf":{"textures":"textures/fixture/shelf"}}}"#,
    );
    write(root.join("textures/flipbook_textures.json"), "[]");
    write_png(
        root.join("textures/fixture/shelf.png"),
        32,
        32,
        &vec![255; 32 * 32 * 4],
    );
}

#[test]
fn shelf_models_cover_all_woods_and_reject_invalid_selectors() {
    let (all, lights, protocol) = active_records(&[]);
    let records = all
        .into_iter()
        .filter(|record| {
            record.name.as_ref() == "minecraft:air"
                || super::super::visuals::shelf::is_record(record)
        })
        .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    shared_shelf_pack(directory.path(), &records);
    let compile = |records: &[RegistryRecord]| {
        let span = records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap();
        super::super::compile_pack_inner(
            directory.path(),
            records,
            &lights[..span],
            CompiledBiomeAssets::diagnostic(),
            protocol,
        )
        .unwrap()
        .0
    };
    let compiled = compile(&records);
    let shelves = records
        .iter()
        .filter(|record| record.name.as_ref() != "minecraft:air")
        .collect::<Vec<_>>();
    assert_eq!(
        shelves
            .iter()
            .map(|record| &record.name)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        13
    );
    for record in &shelves {
        assert_eq!(
            compiled.visuals[record.sequential_id as usize].kind,
            VisualKind::Model,
            "{} {}",
            record.name,
            record.canonical_state
        );
    }
    for record in &shelves {
        let visual = &compiled.visuals[record.sequential_id as usize];
        let template = &compiled.model_templates[visual.model_template as usize];
        assert_ne!(template.flags & assets::MODEL_TEMPLATE_FLAG_SHELF, 0);
    }
    let valid = shelves[0];
    for (key, replacement) in [
        ("powered_bit", serde_json::json!({"type":"int","value":0})),
        ("powered_bit", serde_json::json!({"type":"byte","value":2})),
        (
            "powered_shelf_type",
            serde_json::json!({"type":"int","value":4}),
        ),
        (
            "minecraft:cardinal_direction",
            serde_json::json!({"type":"string","value":"up"}),
        ),
    ] {
        let mut invalid = valid.clone();
        let mut state: serde_json::Value = serde_json::from_str(&invalid.canonical_state).unwrap();
        state[key] = replacement;
        invalid.canonical_state = serde_json::to_string(&state).unwrap().into();
        let air = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .unwrap()
            .clone();
        let compiled = compile(&[air, invalid.clone()]);
        assert_eq!(
            compiled.visuals[invalid.sequential_id as usize].kind,
            VisualKind::Diagnostic,
            "{key}: malformed selectors must not use a fallback cube"
        );
    }
}

#[test]
fn shelf_texture_overrides_with_alpha_keep_their_model_materials() {
    let (records, lights, protocol) =
        active_records(&["minecraft:oak_shelf", "minecraft:poplar_shelf"]);
    let directory = tempfile::tempdir().unwrap();
    shared_shelf_pack(directory.path(), &records);
    let mut pixels = vec![255; 32 * 32 * 4];
    // Powered front texels are unused by an unpowered shelf, but remain in its atlas layer.
    for y in 16..32 {
        for x in 0..32 {
            pixels[(y * 32 + x) * 4 + 3] = 0;
        }
    }
    write_png(
        directory.path().join("textures/fixture/shelf.png"),
        32,
        32,
        &pixels,
    );
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() != "minecraft:air")
    {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            visual.kind(),
            VisualKind::Model,
            "{} {}",
            record.name,
            record.canonical_state
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        for quad in
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize]
        {
            assert_ne!(quad.material, assets::DIAGNOSTIC_MATERIAL);
            assert_ne!(
                runtime.material(quad.material).flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT,
                0
            );
        }
    }
}
