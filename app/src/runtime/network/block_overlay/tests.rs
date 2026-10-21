use std::{io::Write, sync::Arc};

use assets::{BlockFace, NetworkIdMode, RuntimeAssets, VisualKind, VisualSupport};
use protocol::{
    CustomBlock, CustomBlockVisuals, CustomBlocks, CustomMaterialInstance, CustomPermutation,
    CustomStateAxis, CustomStateValue, CustomTransformation, CustomVisualComponents,
};
use resource_pack::LayeredPackView;

use super::OverlayGaps;

/// Compiles non-device fixtures against the full reference capacity.
fn compile_block_overlay(
    view: &LayeredPackView,
    blocks: &CustomBlocks,
    hashed: bool,
    keys: Option<&assets::material_keys::MaterialKeys>,
) -> Option<super::CompiledBlockOverlay> {
    super::compile_block_overlay(
        view,
        blocks,
        hashed,
        keys,
        assets::MAX_TEXTURE_LAYERS as u32,
        None,
    )
}

mod block_items;
mod budget;
mod builtins;
mod capacity;
mod connection;
mod empty_geometry;
mod flipbooks;
mod hd_textures;
mod legacy;
mod lighting;
mod mining;
mod partial_faces;
mod random_offset;
mod session;
mod texture_budget;
mod vanilla_terrain;

mod vanilla_fallback;
mod vines;

fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(pixel(x, y)));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode fixture png");
    bytes.into_inner()
}

const GEOMETRY: &str = r#"{"format_version": "1.12.0", "minecraft:geometry": [{
    "description": {"identifier": "geometry.gen", "texture_width": 32, "texture_height": 32},
    "bones": [{"name": "block", "pivot": [0, 0, 0], "cubes": [
        {"origin": [-8, 0, -8], "size": [16, 16, 16], "uv": {
            "north": {"uv": [0, 0], "uv_size": [16, 16]},
            "east": {"uv": [16, 0], "uv_size": [16, 16]},
            "south": {"uv": [16, 0], "uv_size": [16, 16]},
            "west": {"uv": [16, 0], "uv_size": [16, 16]},
            "up": {"uv": [16, 32], "uv_size": [16, -16]},
            "down": {"uv": [0, 32], "uv_size": [16, -16]}}},
        {"origin": [-4, 16, -4], "size": [8, 1, 8], "uv": {"up": {"uv": [20, 20], "uv_size": [8, 8]}}},
        {"origin": [0, 0, 0], "size": [1, 1, 1], "rotation": [0, 45, 0], "uv": [0, 0]}
    ]}]}]}"#;

fn view() -> LayeredPackView {
    view_with_geometry(GEOMETRY.as_bytes())
}

/// Builds the normal overlay fixture with an alternate geometry document.
fn view_with_geometry(geometry: &[u8]) -> LayeredPackView {
    view_with_catalog(
        geometry,
        r#"{"texture_data": {
        "lucky": {"textures": "textures/blocks/lucky"},
        "gen": {"textures": ["textures/blocks/gen"]}}}"#,
    )
}

fn view_with_catalog(geometry: &[u8], terrain: &str) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000001";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let flipbook = r#"[{"flipbook_texture": "textures/blocks/gen", "atlas_tile": "gen",
        "frames": [1, 0], "ticks_per_frame": 15}]"#;
    let lucky = png(16, 16, |x, _| [x as u8 * 16, 200, 0, 255]);
    let gen_strip = png(32, 64, |_, y| {
        if y < 32 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        }
    });
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in [
        ("manifest.json", manifest.as_bytes()),
        ("textures/terrain_texture.json", terrain.as_bytes()),
        ("textures/flipbook_textures.json", flipbook.as_bytes()),
        ("textures/blocks/lucky.png", &lucky),
        ("textures/blocks/gen.png", &gen_strip),
        ("models/blocks/gen.geo.json", geometry),
    ] {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    let archive = protocol::ResourcePackArchive::unencrypted(
        id.parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        writer.finish().unwrap().into_inner(),
    );
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![archive]),
    ))
}

#[test]
fn terrain_replacement_applies_literal_atlas_tint_once_and_keeps_alpha() {
    let view = view_with_catalog(
        GEOMETRY.as_bytes(),
        r##"{"texture_data":{"lucky":{"textures":[{"path":"textures/blocks/lucky","tint_color":"#ff80ff"}]}}}"##,
    );
    let catalog = super::textures::TextureCatalog::new(&view, None);
    let image = catalog.decode_choice("lucky", 0, 0).unwrap();
    assert_eq!(&image.rgba8[..4], &[0, 100, 0, 255]);
}

#[test]
fn raster_only_replacement_retains_the_base_lily_tint() {
    let view = view_with_catalog(GEOMETRY.as_bytes(), r#"{"texture_data":{}}"#);
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "pad")])
        .with_aliases([("pad", "textures/blocks/lucky")])
        .with_fixed_tints([("pad", [32, 128, 48])]);
    let catalog = super::textures::TextureCatalog::new(&view, Some(&keys));
    let image = catalog.decode_choice("pad", 0, 0).unwrap();
    assert_eq!(&image.rgba8[..4], &[0, 100, 0, 255]);
    let compiled =
        compile_block_overlay(&view, &CustomBlocks::default(), false, Some(&keys)).unwrap();
    assert_eq!(compiled.overlay.material_overrides.len(), 1);
    let texture = compiled.overlay.material_overrides[0].texture;
    let page = compiled.overlay.textures.first().unwrap();
    let mip = &page.mips[0];
    let start = texture.layer() as usize * (mip.size * mip.size * 4) as usize;
    assert_eq!(&mip.rgba8[start..start + 4], &[0, 100, 0, 255]);
}

fn materials(texture: &str) -> Option<Box<[CustomMaterialInstance]>> {
    Some(Box::new([CustomMaterialInstance {
        name: "*".into(),
        texture: texture.into(),
        render_method: None,
        tint_method: None,
        ambient_occlusion: None,
        face_dimming: None,
    }]))
}

fn block(name: &str, state_count: u32, visual: CustomBlockVisuals) -> CustomBlock {
    CustomBlock {
        mining: Default::default(),
        state_physics_complete: false,
        state_physics: Default::default(),
        state_mining: Default::default(),
        player_interact: false,
        name: name.into(),
        tags: Default::default(),
        state_count,
        collides: true,
        collision_boxes: None,
        selection: Default::default(),
        friction: None,
        visual: Arc::new(visual),
    }
}

fn turn(quarters: i32) -> CustomVisualComponents {
    CustomVisualComponents {
        transformation: Some(CustomTransformation {
            rotation: [0, quarters, 0],
            scale: [1.0; 3],
            translation: [0.0; 3],
        }),
        ..CustomVisualComponents::default()
    }
}

fn generator() -> CustomBlock {
    let direction = |value: &str, quarters| CustomPermutation {
        physical: Default::default(),
        condition: format!("q.block_state('minecraft:cardinal_direction') == '{value}'").into(),
        components: turn(quarters),
    };
    block(
        "test:generator",
        4,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("geometry.gen".into()),
                materials: materials("gen"),
                ..CustomVisualComponents::default()
            },
            permutations: Box::new([direction("west", 1), direction("north", 0)]),
            state_axes: Box::new([CustomStateAxis {
                name: "minecraft:cardinal_direction".into(),
                values: ["south", "west", "north", "east"]
                    .map(|value| CustomStateValue::String(value.into()))
                    .into(),
            }]),
            ..CustomBlockVisuals::default()
        },
    )
}

fn compiled() -> super::CompiledBlockOverlay {
    let lucky = block(
        "test:lucky",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("minecraft:geometry.full_block".into()),
                materials: materials("lucky"),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let missing = block(
        "test:missing",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                materials: materials("absent"),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: vec![lucky, generator(), missing].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    compile_block_overlay(&view(), &blocks, false, None).expect("overlay")
}

// Full blocks become cubes on page 1; missing textures paint their faces
// diagnostic while the cube still renders, and are counted.
#[test]
fn full_block_is_a_page_one_cube_and_missing_textures_degrade_per_face() {
    let compiled = compiled();
    let overlay = &compiled.overlay;
    assert_eq!(
        overlay.visuals.len(),
        6,
        "one lucky, four generator, one missing state"
    );
    let lucky = overlay.visuals[0];
    assert_eq!(lucky.kind, VisualKind::Cube);
    let material = overlay.materials[lucky.faces[BlockFace::Up as usize] as usize];
    assert_eq!((material.texture.page(), material.flags), (1, 0));
    let missing = overlay.visuals[5];
    assert_eq!(missing.kind, VisualKind::Cube);
    assert!(
        missing
            .faces
            .iter()
            .all(|&face| face == assets::DIAGNOSTIC_MATERIAL),
        "failed faces keep the diagnostic material instead of sinking the cube"
    );
    assert_eq!(
        compiled.gaps,
        OverlayGaps {
            missing_textures: 1,
            ..OverlayGaps::default()
        },
        "rotated cubes are rendered, not skipped"
    );
    let tile = overlay
        .textures
        .iter()
        .map(|shard| shard.mips[0].size)
        .max()
        .expect("page");
    assert_eq!(tile, 32, "largest source keeps its resolution");
}

// Flipbook frames become layers in the listed order behind one animation.
#[test]
fn flipbook_texture_animates_listed_frames() {
    let compiled = compiled();
    let overlay = &compiled.overlay;
    let generator = overlay.visuals[1];
    let material = overlay.materials[generator.faces[0] as usize];
    let animation = overlay.animations[material.animation as usize];
    assert_eq!((animation.frame_count, animation.ticks_per_frame), (2, 15));
    let first_frame = overlay.animation_frames[animation.frame_start as usize];
    let texture = overlay
        .textures
        .get(first_frame.page() as usize - 1)
        .expect("frame page");
    let layer_bytes = 32 * 32 * 4;
    let pixel = &texture.mips[0].rgba8[first_frame.layer() as usize * layer_bytes..][..4];
    assert_eq!(
        pixel,
        [0, 0, 255, 255],
        "frame 1 of the strip is listed first"
    );
}

// A server flipbook that omits `blend_frames` interpolates, matching the offline compiler.
#[test]
fn flipbook_without_blend_frames_blends() {
    let compiled = compiled();
    let overlay = &compiled.overlay;
    let material = overlay.materials[overlay.visuals[1].faces[0] as usize];
    let animation = overlay.animations[material.animation as usize];
    assert_ne!(animation.flags & assets::ANIMATION_FLAG_BLEND, 0);
}

// Geometry faces keep their pixel UVs and a quarter turn moves the front to the west.
#[test]
fn geometry_quads_follow_uvs_and_placement_rotation() {
    let compiled = compiled();
    let overlay = &compiled.overlay;
    let north_state = overlay.visuals[3];
    assert_eq!(north_state.kind, VisualKind::Model);
    let template = overlay.model_templates[north_state.model_template as usize];
    assert_eq!(
        template.quad_count, 13,
        "six body faces, the top plate, and the six faces of the turned cube"
    );
    let quads = &overlay.model_quads[template.quad_start as usize..][..13];
    let front = quads
        .iter()
        .find(|quad| quad.flags & 7 == 5)
        .expect("north face");
    assert!(front.positions.iter().all(|corner| corner[2] == 0));
    assert!(front.uvs.iter().all(|uv| uv[0] <= 2048 && uv[1] <= 2048));
    assert_eq!(
        front.flags >> 4 & 7,
        5,
        "boundary faces cull against neighbours"
    );

    let west_state = overlay.visuals[2];
    let west_template = overlay.model_templates[west_state.model_template as usize];
    let west_quads = &overlay.model_quads[west_template.quad_start as usize..][..13];
    let turned_front = west_quads
        .iter()
        .find(|quad| quad.uvs.iter().all(|uv| uv[0] <= 2048 && uv[1] <= 2048))
        .expect("front face");
    assert!(turned_front.positions.iter().all(|corner| corner[0] == 0));
    assert_eq!(turned_front.flags & 7, 3, "front now faces west");
}

// The overlay resolves at the ids after the base carrier and nowhere else.
#[test]
fn overlay_extends_runtime_assets_after_base_ids() {
    let compiled = compiled();
    let base = RuntimeAssets::diagnostic();
    let session = base
        .with_block_overlay(1, &compiled.overlay)
        .expect("overlay applies");
    assert_eq!(
        session.texture_pages().len(),
        1 + compiled.overlay.textures.len()
    );
    let lucky = session.resolve(NetworkIdMode::Sequential, 1);
    assert!(lucky.is_known());
    assert_eq!(lucky.kind(), VisualKind::Cube);
    let material = session.material(lucky.face(BlockFace::North).material_id());
    assert_eq!(material.texture.page(), 1);
    assert!(!session.resolve(NetworkIdMode::Sequential, 7).is_known());
    assert!(base.with_block_overlay(2, &compiled.overlay).is_err());
}

#[test]
fn sequential_overlay_retains_persistent_custom_identities() {
    let compiled = compiled();
    let assets = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .unwrap();
    let mut definitions = vec![("test:lucky", Vec::new(), 1)];
    for (offset, direction) in ["south", "west", "north", "east"].into_iter().enumerate() {
        let states = vec![(
            "minecraft:cardinal_direction",
            CustomStateValue::String(direction.into()),
        )];
        definitions.push(("test:generator", states, 2 + offset as u32));
    }
    definitions.push(("test:missing", Vec::new(), 6));
    for (name, states, expected_id) in definitions {
        let hash = protocol::block_state_network_hash(
            name,
            states.iter().map(|(name, value)| (*name, value)),
        );
        assert_eq!(
            assets.sequential_id_for_hash(hash),
            Some(expected_id),
            "persistent identity must use the existing sequential ID for {name}"
        );
    }
    let hash = protocol::block_state_network_hash("test:lucky", std::iter::empty());
    assert_eq!(assets.sequential_id_for_hash(hash), Some(1));
    assert_eq!(
        assets.resolve(NetworkIdMode::Sequential, 1).kind(),
        VisualKind::Cube
    );
}

#[test]
fn incomplete_state_identity_preserves_neighboring_custom_blocks() {
    let plain = |name| {
        block(
            name,
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some("minecraft:geometry.full_block".into()),
                    materials: materials("lucky"),
                    ..CustomVisualComponents::default()
                },
                ..CustomBlockVisuals::default()
            },
        )
    };
    let blocks = CustomBlocks {
        blocks: vec![
            plain("test:before"),
            block("test:unnamed_axis", 2, CustomBlockVisuals::default()),
            plain("test:after"),
        ]
        .into(),
        skipped: 0,
        ..Default::default()
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).unwrap();
    assert_eq!(compiled.gaps.incomplete_state_identities, 1);
    let assets = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .unwrap();
    for (name, id) in [("test:before", 1), ("test:after", 4)] {
        let hash = protocol::block_state_network_hash(name, std::iter::empty());
        assert_eq!(assets.sequential_id_for_hash(hash), Some(id));
        assert_eq!(
            assets.resolve(NetworkIdMode::Sequential, id).kind(),
            VisualKind::Cube
        );
    }
    let incomplete = protocol::block_state_network_hash("test:unnamed_axis", std::iter::empty());
    assert_eq!(assets.sequential_id_for_hash(incomplete), None);
    assert_eq!(compiled.overlay.visuals.len(), 4);
    for id in [2, 3] {
        assert_eq!(
            assets.resolve(NetworkIdMode::Sequential, id).kind(),
            VisualKind::Cube,
            "material-free states still render their cube"
        );
        assert!(
            compiled.overlay.visuals[id as usize - 1]
                .faces
                .iter()
                .all(|&face| face == assets::DIAGNOSTIC_MATERIAL),
            "their faces keep the diagnostic material"
        );
    }
}

// Conditions evaluate as block Molang; one that cannot evaluate is counted, not false.
#[test]
fn permutation_conditions_evaluate_as_block_molang() {
    let open = |condition: &str| CustomPermutation {
        physical: Default::default(),
        condition: condition.into(),
        components: turn(1),
    };
    let door = block(
        "test:door",
        2,
        CustomBlockVisuals {
            permutations: Box::new([
                open("q.block_state('test:open')"),
                open("q.block_state('test:open') == 1 || q.block_state('test:missing')"),
                open("math.random(0, 1) > 0.5"),
            ]),
            state_axes: Box::new([CustomStateAxis {
                name: "test:open".into(),
                values: Box::new([CustomStateValue::Bool(false), CustomStateValue::Bool(true)]),
            }]),
            ..CustomBlockVisuals::default()
        },
    );
    let mut gaps = OverlayGaps::default();
    let expressions = super::condition::BlockExpressions::new(&door);
    let mut state = |index| {
        let values = door.state_values(index);
        super::condition::state_visual(&door, &expressions, values.as_deref(), &mut gaps)
    };
    assert_eq!(state(0).components.transformation, None);
    let opened = state(1);
    assert_eq!(opened.components, turn(1));
    assert_eq!(
        gaps.unevaluated_permutations, 3,
        "the missing state when closed, and the random roll in both states"
    );
}

// Neighbor-dependent connection queries cannot resolve at compile time:
// every state keeps the base visual and each evaluation is counted.
#[test]
fn connection_permutations_fall_back_to_the_base_visual() {
    let arm = |direction| CustomPermutation {
        physical: Default::default(),
        condition: format!("q.block_state('minecraft:connection_{direction}')").into(),
        components: turn(1),
    };
    let fence = block(
        "test:fence",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("minecraft:geometry.full_block".into()),
                materials: materials("lucky"),
                ..CustomVisualComponents::default()
            },
            permutations: ["north", "south", "east", "west"].map(arm).into(),
            ..CustomBlockVisuals::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: vec![fence].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).expect("overlay");
    assert_eq!(
        compiled.gaps.unevaluated_permutations, 4,
        "one unevaluated connection per permutation"
    );
    assert_eq!(compiled.overlay.visuals.len(), 1);
    assert_eq!(
        compiled.overlay.visuals[0].kind,
        VisualKind::Cube,
        "unconnected states keep the base post"
    );
}

// A trait state and a property both vary: each palette index resolves its own permutation.
#[test]
fn sequential_states_with_several_axes_resolve_permutations() {
    let facing = |value: &str, quarters| CustomPermutation {
        physical: Default::default(),
        condition: format!("q.block_state('minecraft:cardinal_direction') == '{value}'").into(),
        components: turn(quarters),
    };
    let mut generator = generator();
    let visual = Arc::make_mut(&mut generator.visual);
    visual.permutations = Box::new([facing("west", 1), facing("north", 2)]);
    let mut axes = visual.state_axes.to_vec();
    axes.push(CustomStateAxis {
        name: "test:lit".into(),
        values: Box::new([CustomStateValue::Bool(false), CustomStateValue::Bool(true)]),
    });
    visual.state_axes = axes.into_boxed_slice();
    generator.state_count = 8;
    let blocks = CustomBlocks {
        blocks: vec![generator].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).expect("overlay");
    assert_eq!(compiled.gaps.unevaluated_permutations, 0);
    let template = |state: usize| compiled.overlay.visuals[state].model_template;
    // Palette order: cardinal south, west, north, east, then the same with `test:lit` set.
    assert_eq!(template(1), template(5), "west, lit or not");
    assert_eq!(template(2), template(6), "north, lit or not");
    assert_ne!(template(1), template(2));
    assert_eq!(template(0), template(3), "south and east keep the base");
}

const BONE_GEOMETRY: &str = r#"{"format_version": "1.12.0", "minecraft:geometry": [{
    "description": {"identifier": "geometry.bones", "texture_width": 16, "texture_height": 16},
    "bones": [
        {"name": "a", "cubes": [{"origin": [-8, 0, -8], "size": [16, 4, 16], "uv": [0, 0]}]},
        {"name": "b", "cubes": [{"origin": [-8, 4, -8], "size": [16, 4, 16], "uv": [0, 0]}]},
        {"name": "c", "cubes": [{"origin": [-8, 8, -8], "size": [16, 4, 16], "uv": [0, 0]}]},
        {"name": "d", "parent": "c", "cubes": [{"origin": [-8, 12, -8], "size": [16, 4, 16], "uv": [0, 0]}]}
    ]}]}"#;

fn bone_components(bones: &[(&str, &str)]) -> CustomVisualComponents {
    CustomVisualComponents {
        geometry: Some("geometry.bones".into()),
        bone_visibility: bones
            .iter()
            .map(|&(bone, expression)| (bone.into(), expression.into()))
            .collect(),
        materials: materials("lucky"),
        ..CustomVisualComponents::default()
    }
}

/// The four-pixel slabs a state draws, numbered from the bottom by their north faces.
fn shown_slabs(compiled: &super::CompiledBlockOverlay, state: usize) -> Vec<i16> {
    let visual = compiled.overlay.visuals[state];
    if visual.kind == VisualKind::Invisible {
        return Vec::new();
    }
    let template = compiled.overlay.model_templates[visual.model_template as usize];
    let mut bottoms = compiled.overlay.model_quads[template.quad_start as usize..]
        [..template.quad_count as usize]
        .iter()
        .filter(|quad| quad.flags & 7 == 5)
        .map(|quad| {
            quad.positions
                .iter()
                .map(|corner| corner[1])
                .min()
                .unwrap_or(0)
                / 64
        })
        .collect::<Vec<_>>();
    bottoms.sort_unstable();
    bottoms.dedup();
    bottoms
}

// Mirrors a vanilla server's definition: string constants and per-state Molang, applied to the
// named bone's own cubes only, with a permutation's geometry bringing its own visibility.
#[test]
fn bone_visibility_hides_bones_per_state() {
    let cardinal = CustomStateAxis {
        name: "minecraft:cardinal_direction".into(),
        values: ["south", "west", "north", "east"]
            .map(|value| CustomStateValue::String(value.into()))
            .into(),
    };
    let directed = block(
        "test:directed",
        4,
        CustomBlockVisuals {
            base: bone_components(&[
                (
                    "a",
                    "q.block_state('minecraft:cardinal_direction') == 'north'",
                ),
                ("b", "1.000000"),
                ("c", "0.000000"),
                ("missing", "0.000000"),
            ]),
            state_axes: Box::new([cardinal]),
            ..CustomBlockVisuals::default()
        },
    );
    let switched = block(
        "test:switched",
        2,
        CustomBlockVisuals {
            base: bone_components(&[]),
            permutations: Box::new([CustomPermutation {
                physical: Default::default(),
                condition: "q.block_state('test:s')".into(),
                components: bone_components(&[("a", "0.000000"), ("b", "0.4"), ("d", "-0.5")]),
            }]),
            state_axes: Box::new([CustomStateAxis {
                name: "test:s".into(),
                values: Box::new([CustomStateValue::Bool(false), CustomStateValue::Bool(true)]),
            }]),
            ..CustomBlockVisuals::default()
        },
    );
    let hidden = block(
        "test:hidden",
        1,
        CustomBlockVisuals {
            base: bone_components(&[("a", "0"), ("b", "0"), ("c", "0"), ("d", "false")]),
            ..CustomBlockVisuals::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: vec![directed, switched, hidden].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let compiled = compile_block_overlay(
        &view_with_geometry(BONE_GEOMETRY.as_bytes()),
        &blocks,
        false,
        None,
    )
    .expect("overlay");
    assert_eq!(shown_slabs(&compiled, 0), [1, 3], "south hides a and c");
    assert_eq!(
        shown_slabs(&compiled, 2),
        [0, 1, 3],
        "north shows a; d is not hidden with c"
    );
    assert_eq!(shown_slabs(&compiled, 4), [0, 1, 2, 3]);
    assert_eq!(
        shown_slabs(&compiled, 5),
        [2, 3],
        "0.4 rounds to zero, -0.5 away from it"
    );
    assert_eq!(compiled.overlay.visuals[6].kind, VisualKind::Invisible);
}

// Flipbook framing is bounded before copies are cut and each frame is shrunk.
#[test]
fn flipbook_frames_are_capped_and_shrunk() {
    use super::textures::{flipbook_frames, shrink_to_max};
    use assets::texture::flipbook::FlipbookSource;
    use pack_runtime::pack_textures::DecodedTexture;
    let strip = DecodedTexture {
        width: 64,
        height: 64 * 8,
        rgba8: (0..64 * 64 * 8)
            .flat_map(|pixel| [(pixel / (64 * 64)) as u8; 4])
            .collect(),
    };
    let flipbook: FlipbookSource = serde_json::from_value(serde_json::json!({
        "flipbook_texture": "textures/fixture", "atlas_tile": "fixture"
    }))
    .unwrap();
    let frames = flipbook_frames(&strip, &flipbook, 3, 32);
    assert_eq!(
        frames.frames.len(),
        3,
        "physical frame count capped to the budget"
    );
    assert_eq!(frames.timeline.len(), 3);
    assert!(
        frames
            .frames
            .iter()
            .all(|f| f.width == 32 && f.height == 32),
        "frames shrunk to max side"
    );
    assert!(flipbook_frames(&strip, &flipbook, 0, 32).frames.is_empty());

    let big = DecodedTexture {
        width: 256,
        height: 128,
        rgba8: vec![9; 256 * 128 * 4].into_boxed_slice(),
    };
    let shrunk = shrink_to_max(&big, 64);
    assert_eq!(
        (shrunk.width, shrunk.height),
        (64, 32),
        "aspect preserved under the cap"
    );
    let small = DecodedTexture {
        width: 16,
        height: 16,
        rgba8: vec![1; 16 * 16 * 4].into_boxed_slice(),
    };
    assert_eq!(
        shrink_to_max(&small, 64).width,
        16,
        "small textures are untouched"
    );
}

// Explicit light components override geometry absorption and the no-emission default.
#[test]
fn light_components_drive_state_light() {
    use protocol::{CustomBlockVisuals, CustomVisualComponents};
    let lit = block(
        "test:lamp",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("minecraft:geometry.full_block".into()),
                materials: materials("lucky"),
                light_emission: Some(13),
                light_dampening: Some(0),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let plain = block(
        "test:plain",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("minecraft:geometry.full_block".into()),
                materials: materials("lucky"),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: vec![lit, plain].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).expect("overlay");
    let light = &compiled.overlay.light_properties;
    assert_eq!((light[0].emission(), light[0].filter()), (13, 0));
    assert_eq!(
        (light[1].emission(), light[1].filter()),
        (0, 0),
        "modern geometry defaults to no absorption"
    );
}

// Hashed sessions get one visual and one distinct hash per state combination.
#[test]
fn hashed_mode_emits_a_visual_and_hash_per_state() {
    let blocks = CustomBlocks {
        blocks: vec![generator()].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let compiled = compile_block_overlay(&view(), &blocks, true, None).expect("overlay");
    assert_eq!(compiled.overlay.visuals.len(), 4);
    assert_eq!(compiled.overlay.hashes.len(), 4);
    let unique: std::collections::HashSet<_> = compiled.overlay.hashes.iter().collect();
    assert_eq!(unique.len(), 4);
    let base = RuntimeAssets::diagnostic();
    let session = base
        .with_block_overlay(1, &compiled.overlay)
        .expect("session assets");
    let hash = compiled.overlay.hashes[2].unwrap();
    assert_eq!(session.sequential_id_for_hash(hash), Some(3));
}

// A pack redefining a vanilla terrain key repoints that key's base materials; unknown keys are ignored.
#[test]
fn vanilla_terrain_keys_override_base_materials() {
    let keys = assets::material_keys::MaterialKeys::from_entries([
        (5, "lucky"),
        (6, "gen"),
        (7, "not_in_pack"),
    ]);
    let empty = CustomBlocks::default();
    let compiled = compile_block_overlay(&view(), &empty, false, Some(&keys)).expect("overrides");
    let mut materials = compiled
        .overlay
        .material_overrides
        .iter()
        .map(|replacement| replacement.material)
        .collect::<Vec<_>>();
    materials.sort_unstable();
    assert_eq!(materials, [5, 6]);
    let gen_override = compiled
        .overlay
        .material_overrides
        .iter()
        .find(|replacement| replacement.material == 6)
        .unwrap();
    assert_ne!(
        gen_override.animation,
        assets::NO_ANIMATION,
        "flipbook carries over"
    );
    assert!(compile_block_overlay(&view(), &empty, false, None).is_none());
}

#[test]
fn review_geometry_skips_overflowing_cube_bounds() {
    let mut document: serde_json::Value = serde_json::from_str(GEOMETRY).unwrap();
    document["minecraft:geometry"][0]["bones"][0]["cubes"] = serde_json::json!([
        {"origin": [1e38, 0, 0], "size": [3e38, 1, 1], "uv": [0, 0]},
        {"origin": [0, 0, 0], "size": [1, 1, 1], "uv": [0, 0]}
    ]);
    let parsed = super::geometry::parse_geometry_file(&serde_json::to_vec(&document).unwrap());
    assert_eq!(parsed[0].1.cubes.len(), 1);
    assert_eq!(parsed[0].1.skipped_cubes, 1);
    assert!(parsed[0].1.cubes.iter().all(|cube| {
        cube.min
            .iter()
            .chain(&cube.max)
            .all(|value| value.is_finite())
    }));
}

#[test]
fn review_geometry_catalog_accepts_json_escaped_identifiers() {
    let escaped = GEOMETRY.replace("geometry.gen", r"geometry\u002egen");
    let view = view_with_geometry(escaped.as_bytes());
    let wanted = std::collections::HashSet::from(["geometry.gen"]);
    let catalog = super::geometry_inputs::geometry_catalog(&view, &wanted);
    assert!(catalog.contains_key("geometry.gen"));
}

// A scaled or translated full block must draw at its transformed size, not as a whole cube.
#[test]
fn scaled_and_translated_full_blocks_draw_their_transformed_bounds() {
    let transformed = |scale: [f32; 3], translation: [f32; 3]| CustomVisualComponents {
        materials: materials("lucky"),
        transformation: Some(CustomTransformation {
            rotation: [0, 1, 0],
            scale,
            translation,
        }),
        ..CustomVisualComponents::default()
    };
    let blocks = CustomBlocks {
        blocks: vec![
            block(
                "test:turned",
                1,
                CustomBlockVisuals {
                    base: transformed([1.0; 3], [0.0; 3]),
                    ..CustomBlockVisuals::default()
                },
            ),
            block(
                "test:half",
                1,
                CustomBlockVisuals {
                    base: transformed([1.0, 0.5, 1.0], [0.0, -0.25, 0.0]),
                    ..CustomBlockVisuals::default()
                },
            ),
            block(
                "test:mirrored",
                1,
                CustomBlockVisuals {
                    base: transformed([-1.0, 1.0, 1.0], [0.0; 3]),
                    ..CustomBlockVisuals::default()
                },
            ),
        ]
        .into(),
        ..Default::default()
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).unwrap();
    assert_eq!(compiled.overlay.visuals[0].kind, VisualKind::Cube);
    assert_eq!(
        compiled.overlay.visuals[2].kind,
        VisualKind::Cube,
        "a reflecting scale keeps the cube instead of an inside-out model"
    );
    let half = compiled.overlay.visuals[1];
    assert_eq!(half.kind, VisualKind::Model);
    let template = compiled.overlay.model_templates[half.model_template as usize];
    let heights = compiled.overlay.model_quads[template.quad_start as usize..]
        [..template.quad_count as usize]
        .iter()
        .flat_map(|quad| quad.positions.iter().map(|corner| corner[1]))
        .collect::<Vec<_>>();
    assert_eq!(heights.iter().min(), Some(&0));
    assert_eq!(
        heights.iter().max(),
        Some(&128),
        "half a block in 1/256 units"
    );
}
