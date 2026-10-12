use super::test_support::{draw, source};
use super::*;
use std::sync::Arc;

/// Packs an original-art preview mesh for the existing offline UI rasterizer.
fn grip_snapshot(mesh: &ui::UiMesh) -> render_model::UiRenderInput {
    use render_model::{
        UI_BLEND_ALPHA, UiRenderBatch, UiRenderTextureArray, UiRenderVertex, UiScissor,
        UiTexturePage,
    };
    let size = [384, 576];
    let vertices = mesh
        .vertices()
        .iter()
        .map(|vertex| UiRenderVertex {
            position: [
                vertex.position[0] * size[0] as f32,
                vertex.position[1] * size[1] as f32,
            ],
            clip_z: vertex.clip_z,
            clip_w: vertex.clip_w,
            uv: vertex.uv,
            color: vertex.color,
            style_flags: u32::from(vertex.style_flags),
            alpha_cutoff: 0.5,
            model_light: vertex.model_light,
            overlay_color: vertex.overlay_color,
        })
        .collect::<Vec<_>>();
    let batches = mesh
        .batches()
        .iter()
        .map(|batch| {
            UiRenderBatch::new(
                u32::from(batch.texture_page),
                UiScissor::new(0, 0, size[0], size[1]),
                batch.index_range.start,
                batch.index_range.end - batch.index_range.start,
                UI_BLEND_ALPHA,
            )
        })
        .collect::<Vec<_>>();
    let pages = (0..9)
        .map(|index| {
            let mut rgba = vec![0; 64 * 64 * 4];
            for y in 0..64 {
                for x in 0..64 {
                    let pixel = if index == 8 {
                        if (16..20).contains(&x) && (20..36).contains(&y) {
                            [235, 177, 65, 255]
                        } else {
                            [0; 4]
                        }
                    } else {
                        [95, 125, 155, 255]
                    };
                    rgba[(y * 64 + x) * 4..(y * 64 + x + 1) * 4].copy_from_slice(&pixel);
                }
            }
            UiTexturePage::owned([64, 64], rgba.into()).unwrap()
        })
        .collect::<Vec<_>>();
    let fixed_pages = pages.len();
    render_model::UiRenderInput {
        revision: 1,
        viewport_size: size,
        safe_area: [0; 4],
        vertices: vertices.into(),
        indices: Arc::from(mesh.indices()),
        batches: batches.into(),
        textures: Arc::new(UiRenderTextureArray::new(pages, fixed_pages).unwrap()),
    }
}

#[test]
fn unresolved_grips_do_not_borrow_the_cached_upright_placement() {
    let mut rgba = vec![0; 16 * 16 * 4];
    for y in 0..16 {
        for x in 6..10 {
            rgba[(y * 16 + x) * 4..(y * 16 + x + 1) * 4].copy_from_slice(&[235, 177, 65, 255]);
        }
    }
    let cached = model(
        render_model::held_sprite_vertices(16, 16, &rgba, [0.0, 0.0, 1.0, 1.0]).unwrap(),
        PreviewHeldPlacement::Sprite {
            hand_equipped: true,
        },
    );
    let resolved = cached.with_grip(Some(assets::item::grip::builtin_item_grip(
        "custom:diamond_sword",
    )));
    let mesh = draw([Some(&resolved), None]);
    let input = grip_snapshot(&mesh);
    crate::ui_runtime::presentation::forms::snapshot::write(&input, "unknown-item-grip");
    assert!(matches!(
        resolved.placements[0],
        PreviewHeldPlacement::Sprite {
            hand_equipped: false
        }
    ));
    assert_ne!(
        placement(&cached, 0).unwrap().0.rotation,
        placement(&resolved, 0).unwrap().0.rotation
    );
    assert!(Arc::ptr_eq(&cached.vertices, &resolved.vertices));
}

fn model(
    vertices: Vec<render_model::ActorRigVertex>,
    placement: PreviewHeldPlacement,
) -> PreviewHeldModel {
    PreviewHeldModel {
        source: source(8),
        vertices: vertices.into(),
        placements: [placement; 2],
        // Pack humanoid pivots in native mirrored rig blocks; production reads
        // the actual geometry's named item bones rather than these fixture values.
        hand_pivots: [
            [6.0 / 16.0, 15.0 / 16.0, 1.0 / 16.0],
            [-6.0 / 16.0, 15.0 / 16.0, 1.0 / 16.0],
        ],
    }
}

#[test]
fn cube_has_six_real_faces_and_both_hands_use_model_source_pages() {
    let block = model(
        render_model::textured_cube_vertices([[0.0, 0.0, 1.0, 1.0]; 6]),
        PreviewHeldPlacement::Block,
    );
    let off = PreviewHeldModel {
        source: source(9),
        ..block.clone()
    };
    let mesh = draw([Some(&block), Some(&off)]);
    assert_eq!(mesh.batches().len(), 3);
    let right = &mesh.batches()[1];
    let left = &mesh.batches()[2];
    assert_eq!(right.index_range.end - right.index_range.start, 36);
    assert_eq!(left.index_range.end - left.index_range.start, 36);
    assert_eq!(right.texture_page, block.source.page);
    assert_eq!(left.texture_page, off.source.page);
    for batch in [right, left] {
        let vertices =
            &mesh.vertices()[batch.index_range.start as usize..batch.index_range.end as usize];
        let far = vertices
            .iter()
            .map(|vertex| vertex.clip_z)
            .fold(f32::INFINITY, f32::min);
        let near = vertices
            .iter()
            .map(|vertex| vertex.clip_z)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            near - far > 0.01,
            "cube must not collapse into a GUI-thumbnail plane"
        );
        assert!(
            vertices
                .windows(2)
                .any(|pair| pair[0].model_light != pair[1].model_light)
        );
    }
}

#[test]
fn sprite_extrusion_keeps_original_side_texel_centres() {
    let sprite = model(
        render_model::held_sprite_vertices(16, 16, &[255; 16 * 16 * 4], [0.0, 0.0, 1.0, 1.0])
            .unwrap(),
        PreviewHeldPlacement::Sprite {
            hand_equipped: false,
        },
    );
    let mesh = draw([Some(&sprite), None]);
    let batch = &mesh.batches()[1];
    assert!(batch.index_range.end - batch.index_range.start > 12);
    let emitted =
        &mesh.vertices()[batch.index_range.start as usize..batch.index_range.end as usize];
    for (vertex, native) in emitted.iter().zip(&*sprite.vertices) {
        assert_eq!(
            vertex.uv,
            [10.0 + native.uv[0] * 16.0, 20.0 + native.uv[1] * 16.0]
        );
    }
    assert!(
        emitted
            .iter()
            .skip(12)
            .any(|vertex| vertex.uv[0].fract() == 0.5)
    );
}

#[test]
fn authored_bind_pivot_is_removed_once_and_no_generic_grip_is_applied() {
    let pivot = [1.0, 2.0, 3.0];
    let bone = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [1.1, 2.2, 3.3, 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    };
    let authored = model(
        render_model::textured_cube_vertices([[0.0, 0.0, 1.0, 1.0]; 6]),
        PreviewHeldPlacement::Authored { bone, pivot },
    );
    let (placed, origin) = placement(&authored, 0).unwrap();
    assert_eq!(origin, Vec3::from_array(pivot));
    assert_eq!(placed.rotation, bone.rotation);
    assert_eq!(placed.axis_scale, bone.axis_scale);
    for axis in 0..3 {
        assert_eq!(
            placed.translation_scale[axis],
            bone.translation_scale[axis] + authored.hand_pivots[0][axis]
        );
    }
}

#[test]
fn native_offhand_grip_is_not_a_main_hand_mirror() {
    let sprite = model(
        render_model::held_sprite_vertices(16, 16, &[255; 16 * 16 * 4], [0.0, 0.0, 1.0, 1.0])
            .unwrap(),
        PreviewHeldPlacement::Sprite {
            hand_equipped: true,
        },
    );
    let (main, _) = placement(&sprite, 0).unwrap();
    let (off, _) = placement(&sprite, 1).unwrap();
    let main_offset = main.translation_scale[0] - sprite.hand_pivots[0][0];
    let off_offset = off.translation_scale[0] - sprite.hand_pivots[1][0];
    // The world-rig conversion reverses reference X. The offhand reference
    // translation -.125 and hand-equipped translation difference -.1 total +.025 here.
    assert!((off_offset - main_offset - 0.025).abs() < 1e-6);
}

#[test]
fn offhand_raises_only_its_own_arm_in_the_live_controller() {
    let bare = Rig::new(Default::default(), PreviewView::default(), 0.0, [false; 2]);
    let off = Rig::new(
        Default::default(),
        PreviewView::default(),
        0.0,
        [false, true],
    );
    for vertex in render_model::standard_biped_vertices() {
        if vertex.part == 2 {
            assert_eq!(bare.project(vertex).world, off.project(vertex).world);
        }
    }
    assert!(
        render_model::standard_biped_vertices()
            .into_iter()
            .filter(|vertex| vertex.part == 3)
            .any(|vertex| bare.project(vertex).world != off.project(vertex).world)
    );
}

#[test]
fn bound_attachable_preview_preserves_the_resolved_hand_origin() {
    use render_model::equipment::{BoneChannels, attach};
    let pivot = [0.0, actor_animation::MODEL_PART_ORIGIN_Y / 16.0, 0.0];
    let identity = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [0.0, 0.0, 0.0, 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    };
    let bone = attach(identity, pivot, BoneChannels::default(), true).unwrap();
    let mut vertices = render_model::textured_cube_vertices([[0.0, 0.0, 1.0, 1.0]; 6]);
    for vertex in &mut vertices {
        vertex.position[1] += pivot[1];
    }
    let held = model(vertices, PreviewHeldPlacement::Authored { bone, pivot });
    let mesh = draw([Some(&held); 2]);
    let rig = Rig::new(Default::default(), PreviewView::default(), 0.0, [true; 2]);
    for (hand, batch) in mesh.batches().iter().skip(1).enumerate() {
        let origin = held.hand_pivots[hand];
        let expected = rig.project(ActorVertex {
            position: [-origin[0], origin[1], -origin[2]],
            uv: [0.0; 2],
            part: if hand == 0 { 2 } else { 3 },
        });
        let vertices =
            &mesh.vertices()[batch.index_range.start as usize..batch.index_range.end as usize];
        for (axis, size) in [PREVIEW_WIDTH, PREVIEW_HEIGHT].into_iter().enumerate() {
            let centre = vertices
                .iter()
                .map(|vertex| vertex.position[axis])
                .sum::<f32>()
                / vertices.len() as f32;
            assert!((centre * size as f32 - expected.screen[axis]).abs() < 1e-4);
        }
    }
}

use crate::ui_runtime::presentation::player_preview::PreviewView;

#[test]
fn held_glint_overrides_preserve_geometry_and_reach_both_hands() {
    let cached = model(
        render_model::textured_cube_vertices([[0.0, 0.0, 1.0, 1.0]; 6]),
        PreviewHeldPlacement::Block,
    )
    .with_glint(Some(true));
    let plain = cached.with_glint(Some(false));
    let retained = cached.with_glint(None);
    assert!(std::sync::Arc::ptr_eq(&cached.vertices, &plain.vertices));
    assert!(retained.source.glint);
    assert!(!plain.source.glint);
    let mesh = draw([Some(&retained), Some(&plain)]);
    for (batch, expected) in mesh.batches()[1..].iter().zip([true, false]) {
        let vertices =
            &mesh.vertices()[batch.index_range.start as usize..batch.index_range.end as usize];
        assert!(
            vertices
                .iter()
                .all(|vertex| (vertex.style_flags & ui::UI_STYLE_GLINT != 0) == expected)
        );
    }
}
