use super::*;
use assets::{BlockFace, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};
use serde_json::json;

/// Supplies original art and two original cuboids through state-dependent model components.
fn fixture() -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let protocol = assets::active_content_registry_protocol();
    let records =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
            .unwrap()
            .into_vec()
            .into_iter()
            .filter(|r| {
                matches!(
                    r.name.as_ref(),
                    "minecraft:air" | "minecraft:shelf_mushroom"
                )
            })
            .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .filter(|r| r.name.as_ref() == "minecraft:shelf_mushroom")
            .count(),
        8
    );
    let root = tempfile::tempdir().unwrap();
    let resource = root.path().join("resource_pack");
    let behavior = root.path().join("behavior_pack");
    write(resource.join("blocks.json"), "{}");
    write(resource.join("textures/flipbook_textures.json"), "[]");
    write(resource.join("textures/terrain_texture.json"), serde_json::to_vec(&json!({"texture_data":{"small_art":{"textures":"textures/small"},"large_art":{"textures":"textures/large"}}})).unwrap());
    for (name, color) in [("small", [190, 40, 80, 255]), ("large", [30, 180, 90, 255])] {
        write_png(
            resource.join(format!("textures/{name}.png")),
            16,
            16,
            &color.repeat(256),
        );
        let size = if name == "small" {
            [4, 2, 3]
        } else {
            [8, 3, 6]
        };
        write(resource.join(format!("models/blocks/{name}.geo.json")), serde_json::to_vec(&json!({"minecraft:geometry":[{"description":{"identifier":format!("geometry.{name}"),"texture_width":16,"texture_height":16},"bones":[{"name":"cap","pivot":[0,0,0],"cubes":[{"origin":[-2,7,1],"size":size,"uv":{
            "north":{"uv":[0,0],"uv_size":[4,2],"material_instance":"cap"},
            "south":{"uv":[4,0],"uv_size":[4,2],"material_instance":"cap"},
            "west":{"uv":[0,2],"uv_size":[3,2],"material_instance":"cap"},
            "east":{"uv":[3,2],"uv_size":[3,2],"material_instance":"cap"},
            "up":{"uv":[0,4],"uv_size":[4,3],"material_instance":"cap"},
            "down":{"uv":[4,7],"uv_size":[4,-3],"material_instance":"cap"}
        }}]}]}]})).unwrap());
    }
    let material = |texture: &str| json!({"cap":{"texture":texture,"render_method":"alpha_test_single_sided","ambient_occlusion":0.0}});
    let mut permutations = vec![
        json!({"condition":"query.block_state('growth') == 1","components":{"minecraft:geometry":{"identifier":"geometry.large"},"minecraft:material_instances":material("large_art")}}),
    ];
    for (facing, degrees) in [("north", 0), ("west", 90), ("south", 180), ("east", 270)] {
        permutations.push(json!({"condition":format!("query.block_state('minecraft:cardinal_direction') == '{facing}'"),"components":{"minecraft:transformation":{"rotation":[0,degrees,0]}}}));
    }
    write(behavior.join("blocks/shelf.json"), serde_json::to_vec(&json!({"minecraft:block":{"description":{"identifier":"minecraft:shelf_mushroom"},"components":{"minecraft:geometry":{"identifier":"geometry.small"},"minecraft:material_instances":material("small_art")},"permutations":permutations}})).unwrap());
    let pack =
        pack_compile_input::pack::read_pack_with_behavior(&resource, &behavior, &records).unwrap();
    let count = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
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

#[test]
fn authored_shelf_models_preserve_growth_facing_uvs_and_single_sided_materials() {
    let (compiled, records) = fixture();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut variants = std::collections::BTreeSet::new();
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:shelf_mushroom")
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let large = state["growth"]["value"] == 1;
        let direction = state["minecraft:cardinal_direction"]["value"]
            .as_str()
            .unwrap();
        let turns = match direction {
            "north" => 0,
            "west" => 1,
            "south" => 2,
            "east" => 3,
            _ => unreachable!(),
        };
        for (mode, id) in [
            (NetworkIdMode::Sequential, record.sequential_id),
            (NetworkIdMode::Hashed, record.network_hash),
        ] {
            let visual = runtime.resolve(mode, id);
            assert_eq!(visual.kind(), VisualKind::Model);
            let template_id = visual.model_template().unwrap();
            variants.insert(template_id);
            let template = compiled.model_templates[template_id as usize];
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            assert_eq!(quads.len(), 6);
            let mut bounds = [[i16::MAX; 3], [i16::MIN; 3]];
            for quad in quads {
                assert_eq!(quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED, 0);
                for point in quad.positions {
                    for axis in 0..3 {
                        bounds[0][axis] = bounds[0][axis].min(point[axis]);
                        bounds[1][axis] = bounds[1][axis].max(point[axis]);
                    }
                }
            }
            let mut expected = if large {
                [[32, 112, 144], [160, 160, 240]]
            } else {
                [[96, 112, 144], [160, 144, 192]]
            };
            for _ in 0..turns {
                expected = [
                    [expected[0][2], expected[0][1], 256 - expected[1][0]],
                    [expected[1][2], expected[1][1], 256 - expected[0][0]],
                ];
            }
            assert_eq!(bounds, expected, "{}", record.canonical_state);
            let material = runtime.material(visual.face(BlockFace::Up).material_id());
            assert_ne!(material.flags & assets::MATERIAL_FLAG_DISABLE_AO, 0);
            assert_ne!(material.flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT, 0);
            let mip = &compiled.texture_pages[material.texture.page() as usize]
                .texture
                .mips[0];
            let start = (material.texture.layer() * mip.size * mip.size * 4) as usize;
            assert_eq!(
                &mip.rgba8[start..start + 4],
                if large {
                    &[30, 180, 90, 255]
                } else {
                    &[190, 40, 80, 255]
                }
            );
            assert!(quads.iter().any(|q| q.uvs[0][1] != q.uvs[2][1]));
        }
    }
    assert_eq!(variants.len(), 8);
}
