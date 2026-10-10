use std::{collections::BTreeSet, fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use {super::*, assets::BlockFace};

struct Fixture {
    directory: tempfile::TempDir,
    records: Vec<RegistryRecord>,
    protocol: u32,
}

impl Fixture {
    fn new() -> Self {
        let target: Value =
            serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
                .unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
        let records = assets::read_registry_for_protocol(
            &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
            protocol,
        )
        .unwrap()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:fire" | "minecraft:soul_fire" | "minecraft:air" | "minecraft:stone"
            )
        })
        .collect();
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
        write_json(
            directory.path(),
            "blocks.json",
            json!({
                "fire":{"textures":{"up":"fire_0", "down":"fire_1", "side":"fire_0"}},
                "soul_fire":{"textures":{
                    "up":"soul_fire_0", "down":"soul_fire_1", "side":"soul_fire_0"
                }},
                "stone":{"textures":"stone"}
            }),
        );
        write_json(
            directory.path(),
            "textures/terrain_texture.json",
            json!({"texture_data":{
                "fire_0":{"textures":"textures/blocks/flame"},
                "fire_1":{"textures":"textures/blocks/flame"},
                "soul_fire_0":{"textures":"textures/blocks/soul_flame"},
                "soul_fire_1":{"textures":"textures/blocks/soul_flame"},
                "stone":{"textures":"textures/blocks/stone"}
            }}),
        );
        // Both bindings share physical pixels, but their timeline phases differ.
        // This catches flattening the two native sprites into one material.
        let flipbooks = ["fire", "soul_fire"]
            .into_iter()
            .flat_map(|family| {
                let path = if family == "fire" {
                    "flame"
                } else {
                    "soul_flame"
                };
                [
                    json!({
                        "flipbook_texture":format!("textures/blocks/{path}"),
                        "atlas_tile":format!("{family}_0"),
                        "frames":[1,0], "ticks_per_frame":1, "blend_frames":false
                    }),
                    json!({
                        "flipbook_texture":format!("textures/blocks/{path}"),
                        "atlas_tile":format!("{family}_1"),
                        "frames":[0,1], "ticks_per_frame":1, "blend_frames":false
                    }),
                ]
            })
            .collect::<Vec<_>>();
        write_json(
            directory.path(),
            "textures/flipbook_textures.json",
            json!(flipbooks),
        );
        write_strip(
            directory.path(),
            "flame",
            [[240, 30, 10, 255], [40, 180, 70, 255]],
        );
        write_strip(
            directory.path(),
            "soul_flame",
            [[20, 60, 210, 255], [120, 40, 220, 255]],
        );
        write_strip(directory.path(), "stone", [[90, 100, 110, 255]]);
        Self {
            directory,
            records,
            protocol,
        }
    }

    fn compile(&self) -> (CompiledAssets, MaterialKeys) {
        let lights = vec![
            LightProperties::default();
            self.records
                .iter()
                .map(|record| record.sequential_id as usize + 1)
                .max()
                .unwrap()
        ];
        compile_pack_inner(
            self.directory.path(),
            &self.records,
            &lights,
            CompiledBiomeAssets::diagnostic(),
            self.protocol,
        )
        .unwrap()
    }

    fn record(&self, name: &str) -> &RegistryRecord {
        self.records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
    }
}

fn write_json(root: &Path, path: &str, value: Value) {
    fs::write(root.join(path), serde_json::to_vec(&value).unwrap()).unwrap();
}

fn write_strip(root: &Path, name: &str, colours: impl AsRef<[[u8; 4]]>) {
    let colours = colours.as_ref();
    let pixels = colours
        .iter()
        .flat_map(|colour| {
            std::iter::repeat_n(*colour, (assets::TILE_SIZE * assets::TILE_SIZE) as usize)
        })
        .flatten()
        .collect::<Vec<_>>();
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(
            &pixels,
            assets::TILE_SIZE,
            assets::TILE_SIZE * colours.len() as u32,
            ExtendedColorType::Rgba8,
        )
        .unwrap();
    fs::write(root.join(format!("textures/blocks/{name}.png")), png).unwrap();
}

fn template_quads(compiled: &CompiledAssets, index: u32) -> &[ModelQuad] {
    let template = compiled.model_templates[index as usize];
    &compiled.model_quads
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
}

fn material_timeline(compiled: &CompiledAssets, material: u32) -> &[TextureRef] {
    let animation = compiled.animations[compiled.materials[material as usize].animation as usize];
    &compiled.animation_frames
        [animation.frame_start as usize..(animation.frame_start + animation.frame_count) as usize]
}

fn pixel(compiled: &CompiledAssets, texture: TextureRef) -> [u8; 4] {
    let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
    let offset = texture.layer() as usize * (mip.size * mip.size * 4) as usize;
    mip.rgba8[offset..offset + 4].try_into().unwrap()
}

#[test]
fn current_registry_fire_preserves_both_phased_sprites_and_camera_down_face() {
    let fixture = Fixture::new();
    let (compiled, keys) = fixture.compile();
    let mut groups = BTreeSet::new();
    for (name, key, colours) in [
        (
            "minecraft:fire",
            "fire",
            [[240, 30, 10, 255], [40, 180, 70, 255]],
        ),
        (
            "minecraft:soul_fire",
            "soul_fire",
            [[20, 60, 210, 255], [120, 40, 220, 255]],
        ),
    ] {
        let records = fixture
            .records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect::<Vec<_>>();
        assert!(!records.is_empty(), "active registry must contain {name}");
        if name == "minecraft:fire" {
            assert!(records.len() > 1, "exercise every native fire age");
        }
        let first = compiled.visuals[records[0].sequential_id as usize];
        groups.insert(first.model_template);
        let up = first.faces[BlockFace::Up as usize];
        let down = first.faces[BlockFace::Down as usize];
        assert_ne!(up, down);
        assert!(keys.materials(&format!("{key}_0")).contains(&up));
        assert!(keys.materials(&format!("{key}_1")).contains(&down));
        assert_ne!(compiled.materials[up as usize].animation, NO_ANIMATION);
        assert_ne!(compiled.materials[down as usize].animation, NO_ANIMATION);
        let up_frames = material_timeline(&compiled, up);
        let down_frames = material_timeline(&compiled, down);
        assert_eq!(up_frames, [down_frames[1], down_frames[0]]);
        assert_eq!(down_frames.len(), 2);
        assert_eq!(pixel(&compiled, down_frames[0]), colours[0]);
        assert_eq!(pixel(&compiled, down_frames[1]), colours[1]);
        for material in [up, down] {
            assert_eq!(
                compiled.materials[material as usize].flags,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            let animation =
                compiled.animations[compiled.materials[material as usize].animation as usize];
            assert_eq!(animation.ticks_per_frame, 1);
            assert_eq!(animation.flags, 0);
        }
        for record in records {
            let visual = compiled.visuals[record.sequential_id as usize];
            assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
            assert_eq!(visual.support, VisualSupport::Exact);
            assert_eq!(
                visual.model_template, first.model_template,
                "ages share topology"
            );
            assert_eq!(visual.variant, 0);
            assert!(
                !visual
                    .flags
                    .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
            );
            for face in BlockFace::ALL {
                assert_eq!(
                    visual.faces[face as usize],
                    if face == BlockFace::Down { down } else { up }
                );
            }
        }
    }
    assert_eq!(
        groups.len(),
        2,
        "ordinary and soul flames use distinct sprite groups"
    );
    let encoded = assets::encode_blob(&compiled).unwrap();
    let decoded = assets::RuntimeAssets::decode(&encoded).unwrap();
    assert_eq!(decoded.model_templates(), compiled.model_templates.as_ref());
    assert_eq!(decoded.model_quads(), compiled.model_quads.as_ref());
    assert_eq!(decoded.animations(), compiled.animations.as_ref());
    assert_eq!(
        decoded.animation_frames(),
        compiled.animation_frames.as_ref()
    );
    let fire = fixture.record("minecraft:fire");
    let block = decoded.resolve(assets::NetworkIdMode::Sequential, fire.sequential_id);
    assert_eq!(
        block.face(BlockFace::Down).material_id(),
        compiled.visuals[fire.sequential_id as usize].faces[BlockFace::Down as usize]
    );
}

#[test]
fn supported_fire_has_eight_native_sloped_planes_instead_of_a_plant_cross() {
    let fixture = Fixture::new();
    let (compiled, _) = fixture.compile();
    let visual = compiled.visuals[fixture.record("minecraft:fire").sequential_id as usize];
    let quads = template_quads(&compiled, visual.model_template);
    assert_eq!(
        compiled.model_templates[visual.model_template as usize].flags,
        assets::MODEL_TEMPLATE_FLAG_FIRE
    );
    let up = visual.faces[BlockFace::Up as usize];
    let down = visual.faces[BlockFace::Down as usize];
    assert_eq!(quads.len(), 8);
    assert_eq!(
        quads.iter().map(|quad| quad.material).collect::<Vec<_>>(),
        [up, up, down, down, down, down, up, up]
    );
    // Interior and perimeter sheets pin slope, winding, height and texture U flip.
    assert_eq!(
        quads[0].positions,
        [[51, 358, 256], [179, 0, 256], [179, 0, 0], [51, 358, 0]]
    );
    assert_eq!(quads[0].uvs, [[4096, 0], [4096, 4096], [0, 4096], [0, 0]]);
    assert_eq!(
        quads[4].positions,
        [[26, 358, 0], [0, 0, 0], [0, 0, 256], [26, 358, 256]]
    );
    assert_eq!(quads[4].uvs, [[0, 0], [0, 4096], [4096, 4096], [4096, 0]]);
    let mut planes = BTreeSet::new();
    for quad in quads {
        let mut plane = quad.positions;
        plane.sort_unstable();
        assert!(planes.insert(plane), "each sheet has distinct geometry");
        assert_eq!(quad.flags, MODEL_QUAD_FLAG_TWO_SIDED);
        for (position, uv) in quad.positions.into_iter().zip(quad.uvs) {
            assert!((0..=256).contains(&position[0]));
            assert!((0..=256).contains(&position[2]));
            assert!(matches!(position[1], 0 | 358));
            assert_eq!(uv[1], if position[1] == 0 { 4096 } else { 0 });
        }
    }
}

#[test]
fn fire_topology_group_covers_every_attachment_phase_and_uv_orientation() {
    let fixture = Fixture::new();
    let (compiled, _) = fixture.compile();
    let visual = compiled.visuals[fixture.record("minecraft:fire").sequential_id as usize];
    let base = visual.model_template;
    let up = visual.faces[BlockFace::Up as usize];
    let down = visual.faces[BlockFace::Down as usize];
    assert_eq!(assets::FIRE_TEMPLATE_COUNT, 129);
    let mut offsets = BTreeSet::new();
    for alternate in [false, true] {
        for flip_u in [false, true] {
            for mask in 0..32_u8 {
                let offset = assets::fire_attachment_template_offset(mask, alternate, flip_u);
                assert!(offsets.insert(offset));
                let template = compiled.model_templates[(base + offset) as usize];
                assert_eq!(template.flags, assets::MODEL_TEMPLATE_FLAG_FIRE);
                assert_eq!(template.quad_count, mask.count_ones() * 2);
                assert!(template.quad_count <= 10, "bounded native attachment mesh");
                let quads = template_quads(&compiled, base + offset);
                assert!(
                    quads
                        .iter()
                        .all(|quad| quad.flags == MODEL_QUAD_FLAG_TWO_SIDED)
                );
                let side_count = (mask & 15).count_ones() as usize * 2;
                for pair in quads[..side_count].chunks_exact(2) {
                    assert_eq!(pair[0].material, if alternate { down } else { up });
                    assert_eq!(pair[1].material, pair[0].material);
                    for vertex in 0..4 {
                        assert_eq!(pair[1].positions[vertex], pair[0].positions[3 - vertex]);
                        assert_eq!(pair[1].uvs[vertex], pair[0].uvs[3 - vertex]);
                    }
                }
                if mask & 16 != 0 {
                    assert_eq!(quads[side_count].material, up);
                    assert_eq!(quads[side_count + 1].material, down);
                }
                let opposite_flip = template_quads(
                    &compiled,
                    base + assets::fire_attachment_template_offset(mask, alternate, !flip_u),
                );
                for (quad, flipped) in quads[..side_count].iter().zip(opposite_flip) {
                    assert_eq!(quad.positions, flipped.positions);
                    for (uv, flipped_uv) in quad.uvs.into_iter().zip(flipped.uvs) {
                        assert_eq!(uv[0] + flipped_uv[0], 4096);
                        assert_eq!(uv[1], flipped_uv[1]);
                    }
                }
            }
        }
    }
    assert_eq!(offsets.len() + 1, assets::FIRE_TEMPLATE_COUNT as usize);
    assert_eq!(offsets.first().copied(), Some(1));
    assert_eq!(
        offsets.last().copied(),
        Some(assets::FIRE_TEMPLATE_COUNT - 1)
    );
    let west = template_quads(
        &compiled,
        base + assets::fire_attachment_template_offset(1, false, false),
    );
    assert_eq!(west[0].positions[0], [51, 374, 256]);
    assert_eq!(west[0].positions[1], [0, 16, 256]);
    let east = template_quads(
        &compiled,
        base + assets::fire_attachment_template_offset(2, false, false),
    );
    assert_eq!(east[0].uvs[0][0], 0, "opposite walls reverse U orientation");
    assert_eq!(west[0].uvs[0][0], 4096);
}

#[test]
fn above_attached_fire_uses_two_different_slopes_and_changes_axis_with_parity() {
    let fixture = Fixture::new();
    let (compiled, _) = fixture.compile();
    let visual = compiled.visuals[fixture.record("minecraft:fire").sequential_id as usize];
    let normal = template_quads(
        &compiled,
        visual.model_template + assets::fire_attachment_template_offset(16, false, false),
    );
    let alternate = template_quads(
        &compiled,
        visual.model_template + assets::fire_attachment_template_offset(16, true, false),
    );
    assert_eq!(normal.len(), 2);
    assert_eq!(alternate.len(), 2);
    assert_ne!(normal[0].positions, normal[1].positions);
    assert_ne!(alternate[0].positions, alternate[1].positions);
    assert_eq!(normal[0].positions[0], [0, 205, 256]);
    assert_eq!(normal[0].positions[1], [0, 256, 0]);
    assert_eq!(alternate[0].positions[0], [0, 205, 0]);
    assert_eq!(alternate[0].positions[1], [256, 256, 0]);
    for quad in normal.iter().chain(alternate) {
        assert_eq!(
            quad.positions
                .iter()
                .filter(|position| position[1] == 205)
                .count(),
            2
        );
        assert_eq!(
            quad.positions
                .iter()
                .filter(|position| position[1] == 256)
                .count(),
            2
        );
    }
}

fn assert_invalid(compiled: &CompiledAssets, expected: &str) {
    let error = assets::encode_blob(compiled).expect_err("malformed fire group must not encode");
    assert!(
        matches!(error, AssetError::InvalidCompiledAssets { ref detail } if detail.contains(expected)),
        "{error}"
    );
}

#[test]
fn fire_carrier_rejects_truncated_groups_and_references_to_attachment_members() {
    let fixture = Fixture::new();
    let (compiled, _) = fixture.compile();
    let record = fixture.record("minecraft:fire");
    let base = compiled.visuals[record.sequential_id as usize].model_template;
    let mut truncated = compiled.clone();
    truncated.model_templates =
        truncated.model_templates[..(base + assets::FIRE_TEMPLATE_COUNT - 1) as usize].into();
    assert_invalid(&truncated, "fire template group is truncated");
    let mut member_reference = compiled.clone();
    member_reference.visuals[record.sequential_id as usize].model_template += 1;
    assert_invalid(&member_reference, "topology-group base");

    // Corrupt a semantic field and repair the envelope hash, proving runtime
    // admission checks the fire group itself rather than only the checksum.
    let table = compiled
        .model_templates
        .iter()
        .flat_map(|template| {
            [template.quad_start, template.quad_count, template.flags]
                .into_iter()
                .flat_map(u32::to_le_bytes)
        })
        .collect::<Vec<_>>();
    let mut encoded = assets::encode_blob(&compiled).unwrap().into_vec();
    let table_offset = encoded
        .windows(table.len())
        .position(|bytes| bytes == table)
        .unwrap();
    let flags_offset = table_offset
        + (base + assets::FIRE_TEMPLATE_COUNT - 1) as usize * std::mem::size_of::<ModelTemplate>()
        + std::mem::offset_of!(ModelTemplate, flags);
    encoded[flags_offset..flags_offset + std::mem::size_of::<u32>()]
        .copy_from_slice(&0_u32.to_le_bytes());
    let payload_len = encoded.len() - <Sha256 as Digest>::output_size();
    let digest = Sha256::digest(&encoded[..payload_len]);
    encoded[payload_len..].copy_from_slice(&digest);
    let error = match assets::RuntimeAssets::decode(&encoded) {
        Err(error) => error,
        Ok(_) => panic!("runtime must reject broken fire topology"),
    };
    assert!(
        matches!(error, AssetError::InvalidCompiledAssets { ref detail } if detail.contains("fire template group is noncanonical")),
        "{error}"
    );
}
