//! Material sidedness through emitted cube geometry and the production actor shader.
use crate::{gpu_snapshot, shader_source};
use render as render_api;

use assets::EntityRenderMaterial;
use gpu_snapshot::{Gpu, SNAPSHOT_SIDE};

#[path = "support/actor_raster.rs"]
mod actor_raster;
use actor_raster::{cube, raster, raster_material};

pub(super) fn source() -> String {
    actor_raster::source(false)
}

#[test]
fn actor_material_states_keep_authored_one_sided_border_hidden_from_behind() {
    let Some(gpu) =
        Gpu::for_fixture("actor_material_states_keep_authored_one_sided_border_hidden_from_behind")
    else {
        return;
    };
    let plane = cube([16, 0, 16], true, false);
    let mut material = render::ActorMaterial {
        state: Some(assets::EntityRenderMaterialState {
            alpha_test: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let clear = center(&raster(
        &gpu,
        &plane,
        EntityRenderMaterial::Dragon,
        false,
        [[0; 4]; 2],
    ))
    .to_vec();
    let front = raster_material(&gpu, &plane, material, false, [[255; 4]; 2]);
    let back = raster_material(&gpu, &plane, material, true, [[255; 4]; 2]);
    assert!(center(&front)[0] > 200);
    assert_eq!(center(&back), clear);
    material.state.as_mut().unwrap().cull = false;
    let nocull_back = raster_material(&gpu, &plane, material, true, [[255; 4]; 2]);
    assert_ne!(center(&nocull_back), clear);
}

#[test]
fn actor_material_states_alpha_test_discards_below_threshold_and_blends_retained_pixels() {
    let Some(gpu) = Gpu::for_fixture(
        "actor_material_states_alpha_test_discards_below_threshold_and_blends_retained_pixels",
    ) else {
        return;
    };
    let plane = cube([16, 0, 16], true, false);
    let mut material = render::ActorMaterial {
        state: Some(assets::EntityRenderMaterialState {
            alpha_test: true,
            cull: false,
            ..Default::default()
        }),
        ..Default::default()
    };
    let clear = center(&raster(
        &gpu,
        &plane,
        EntityRenderMaterial::Dragon,
        false,
        [[0; 4]; 2],
    ))
    .to_vec();
    let below = raster_material(&gpu, &plane, material, false, [[255, 255, 255, 127]; 2]);
    assert_eq!(center(&below), clear);
    let retained = raster_material(&gpu, &plane, material, false, [[255, 255, 255, 128]; 2]);
    assert!(center(&retained)[0] > 200);
    material.state.as_mut().unwrap().blend = true;
    let blended = raster_material(&gpu, &plane, material, false, [[255, 255, 255, 128]; 2]);
    let alpha = f32::from(center(&retained)[3]) / f32::from(u8::MAX);
    for (channel, background) in clear.iter().take(3).enumerate() {
        let expected = (f32::from(center(&retained)[channel]) * alpha
            + f32::from(*background) * (1.0 - alpha))
            .round() as i32;
        assert!((i32::from(center(&blended)[channel]) - expected).abs() <= 1);
    }
}

fn center(frame: &[u8]) -> &[u8] {
    let index =
        (SNAPSHOT_SIDE as usize / 2 * SNAPSHOT_SIDE as usize + SNAPSHOT_SIDE as usize / 2) * 4;
    &frame[index..index + 4]
}

#[test]
fn controller_light_multiplier_scales_lit_and_unlit_rgb_without_changing_alpha() {
    let Some(gpu) = Gpu::for_fixture(
        "controller_light_multiplier_scales_lit_and_unlit_rgb_without_changing_alpha",
    ) else {
        return;
    };
    let plane = cube([16, 0, 16], true, false);
    for light in [0, render::pack_actor_light(15, 15)] {
        for multiplier in [0.0, 0.5, 1.0] {
            let material = render::ActorMaterial {
                light_color_multiplier: multiplier,
                ..Default::default()
            };
            let frame = actor_raster::raster_material_lighting(
                &gpu,
                &plane,
                material,
                false,
                [[255; 4]; 2],
                true,
                false,
                light,
            );
            let expected = (multiplier * f32::from(u8::MAX)).round() as i32;
            for channel in &center(&frame)[..3] {
                assert!((i32::from(*channel) - expected).abs() <= 1);
            }
            assert_eq!(center(&frame)[3], u8::MAX);
        }
    }
}

#[test]
fn dragon_and_dissolve_materials_reject_volume_backfaces() {
    let Some(gpu) = Gpu::for_fixture("dragon_and_dissolve_materials_reject_volume_backfaces")
    else {
        return;
    };
    let cube = cube([16; 3], true, false);
    assert_eq!(cube.len(), 6);
    let clear = center(&raster(
        &gpu,
        &cube,
        EntityRenderMaterial::Dragon,
        false,
        [[0; 4]; 2],
    ))
    .to_vec();
    for material in [
        EntityRenderMaterial::Dragon,
        EntityRenderMaterial::DissolveDepth,
        EntityRenderMaterial::DissolveColor,
    ] {
        let front = raster(&gpu, &cube, material, false, [[255; 4]; 2]);
        let back = raster(&gpu, &cube, material, true, [[255; 4]; 2]);
        assert_ne!(center(&front), clear, "{material:?} admits its outer face");
        assert_eq!(center(&back), clear, "{material:?} rejects its inner face");
    }
}

#[test]
fn collapsed_membrane_keeps_both_authored_uv_faces_in_one_sided_materials() {
    let Some(gpu) =
        Gpu::for_fixture("collapsed_membrane_keeps_both_authored_uv_faces_in_one_sided_materials")
    else {
        return;
    };
    let plane = cube([16, 0, 16], true, true);
    assert_eq!(plane.len(), 12);
    for material in [
        EntityRenderMaterial::Dragon,
        EntityRenderMaterial::DissolveColor,
    ] {
        let front = raster(
            &gpu,
            &plane,
            material,
            false,
            [[255, 0, 0, 255], [0, 255, 0, 255]],
        );
        let back = raster(
            &gpu,
            &plane,
            material,
            true,
            [[255, 0, 0, 255], [0, 255, 0, 255]],
        );
        assert!(
            center(&front)[0] > 200 && center(&front)[1] < 2,
            "front UV survives"
        );
        assert!(
            center(&back)[1] > 20 && center(&back)[0] < 2,
            "opposing UV survives"
        );
    }
}

#[test]
fn collapsed_membrane_back_uses_the_authored_opposing_face_normal() {
    let Some(gpu) =
        Gpu::for_fixture("collapsed_membrane_back_uses_the_authored_opposing_face_normal")
    else {
        return;
    };
    let plane = cube([16, 0, 16], true, true);
    let opposing = cube([16, 0, 16], false, true);
    for material in [
        EntityRenderMaterial::Dragon,
        EntityRenderMaterial::DissolveColor,
    ] {
        let collapsed = raster(&gpu, &plane, material, true, [[255; 4]; 2]);
        let authored = raster(&gpu, &opposing, material, true, [[255; 4]; 2]);
        assert_eq!(
            center(&collapsed),
            center(&authored),
            "{material:?}: collapsing two faces must preserve their independent shading"
        );
    }
}

#[test]
fn nocull_planes_keep_the_visible_authored_face_from_either_side() {
    let Some(gpu) =
        Gpu::for_fixture("nocull_planes_keep_the_visible_authored_face_from_either_side")
    else {
        return;
    };
    let plane = cube([16, 0, 16], true, true);
    for texels in [[[255; 4], [0; 4]], [[0; 4], [255; 4]]] {
        let clear = raster(
            &gpu,
            &plane,
            EntityRenderMaterial::Default,
            false,
            [[0; 4]; 2],
        );
        for below in [false, true] {
            let frame = raster(&gpu, &plane, EntityRenderMaterial::Default, below, texels);
            assert_ne!(
                center(&frame),
                center(&clear),
                "a nocull authored face must remain visible through the transparent opposing face: below {below}, texels {texels:?}"
            );
        }
    }
}
