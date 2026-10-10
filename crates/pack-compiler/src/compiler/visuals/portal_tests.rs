use std::{fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::Value;

use {super::*, assets::BlockFace};

#[test]
fn current_portal_states_compile_to_animated_blended_native_cuboids() {
    let target: Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry_bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&registry_bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &registry_bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            record.name.as_ref() == assets::NETHER_PORTAL_IDENTIFIER
                || record.name.as_ref() == "minecraft:air"
        })
        .collect::<Vec<_>>();
    let sequential_span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let lights = lights[..sequential_span].to_vec().into_boxed_slice();
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
    fs::write(
        directory.path().join("blocks.json"),
        r#"{"portal":{"textures":"portal"}}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"portal":{"textures":"textures/blocks/portal"}}}"#,
    )
    .unwrap();
    fs::write(directory.path().join("textures/flipbook_textures.json"), r#"[{"flipbook_texture":"textures/blocks/portal","atlas_tile":"portal","ticks_per_frame":2}]"#).unwrap();
    // Original translucent fixture art; two distinct frames catch deferred/static routing.
    let pixels = (0..16 * 32)
        .flat_map(|pixel| [if pixel < 16 * 16 { 180 } else { 90 }, 30, 210, 160])
        .collect::<Vec<_>>();
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(&pixels, 16, 32, ExtendedColorType::Rgba8)
        .unwrap();
    fs::write(directory.path().join("textures/blocks/portal.png"), png).unwrap();
    let (compiled, _) = compile_pack_inner(
        directory.path(),
        &records,
        &lights,
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let fallback = super::super::fallback::inventory(protocol).unwrap();
    let mut axes = Vec::new();
    for record in records.iter().filter(|record| is_record(record)) {
        assert_eq!(fallback.material_flags(record), None);
        let axis = canonical_state_str(&record.canonical_state, "portal_axis").unwrap();
        axes.push(axis.clone());
        let visual = runtime.resolve(assets::NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model);
        assert_eq!(visual.support(), VisualSupport::Exact);
        // Vanilla nether portals emit light without opacity.
        assert_eq!(visual.light_properties().emission(), 11);
        assert_eq!(visual.light_properties().filter(), 0);
        assert!(
            !visual
                .flags()
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        let template = compiled.model_templates[visual.model_template().unwrap() as usize];
        assert_eq!(template.flags, assets::MODEL_TEMPLATE_FLAG_NETHER_PORTAL);
        let quads =
            &compiled.model_quads[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), 6);
        let (min, max) = if axis.as_ref() == "x" {
            ([0, 0, NEAR], [256, 256, FAR])
        } else {
            ([NEAR, 0, 0], [FAR, 256, 256])
        };
        for component in 0..3 {
            let coordinates = quads
                .iter()
                .flat_map(|quad| quad.positions.map(|p| p[component]))
                .collect::<Vec<_>>();
            assert_eq!(*coordinates.iter().min().unwrap(), min[component]);
            assert_eq!(*coordinates.iter().max().unwrap(), max[component]);
        }
        assert_eq!(
            visual.variant(),
            if axis.as_ref() == "unknown" {
                assets::BLOCK_VISUAL_VARIANT_PORTAL_UNKNOWN
            } else {
                0
            }
        );
        for quad in quads {
            let material = compiled.materials[quad.material as usize];
            assert_eq!(material.flags, MATERIAL_FLAG_ALPHA_BLEND);
            assert_ne!(material.animation, NO_ANIMATION);
            let animation = compiled.animations[material.animation as usize];
            assert_eq!(animation.frame_count, 2);
            assert_eq!(animation.ticks_per_frame, 2);
            assert_ne!(
                compiled.animation_frames[animation.frame_start as usize],
                compiled.animation_frames[animation.frame_start as usize + 1]
            );
        }
    }
    axes.sort();
    assert_eq!(
        axes.iter().map(|axis| axis.as_ref()).collect::<Vec<_>>(),
        ["unknown", "x", "z"]
    );
    let unknown = compiled
        .visuals
        .iter()
        .find(|visual| {
            visual.kind == VisualKind::Model
                && visual.variant == assets::BLOCK_VISUAL_VARIANT_PORTAL_UNKNOWN
        })
        .unwrap();
    let mut malformed = compiled.clone();
    malformed.model_templates[unknown.model_template as usize + 1].flags = 0;
    assert!(assets::encode_blob(&malformed).is_err());
}

#[test]
fn portal_uvs_and_boundary_faces_follow_the_native_cuboid_emitters() {
    for axis_x in [false, true] {
        for (face, quad) in BlockFace::ALL.into_iter().zip(portal_quads([1; 6], axis_x)) {
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
            let inset = matches!(
                (axis_x, face),
                (true, BlockFace::North | BlockFace::South)
                    | (false, BlockFace::West | BlockFace::East)
            );
            assert_eq!(
                quad.flags & assets::MODEL_QUAD_FLAG_CULL_FACE_MASK == 0,
                inset
            );
            for ([x, y, z], uv) in quad.positions.into_iter().zip(quad.uvs) {
                let [x, y, z] = [x, y, z].map(|v| v as u16 * 16);
                assert_eq!(
                    uv,
                    match face {
                        BlockFace::West => [z, 4096 - y],
                        BlockFace::East => [4096 - z, 4096 - y],
                        BlockFace::Down => [x, 4096 - z],
                        BlockFace::Up => [x, z],
                        BlockFace::North => [4096 - x, 4096 - y],
                        BlockFace::South => [x, 4096 - y],
                    }
                );
            }
        }
    }
}
