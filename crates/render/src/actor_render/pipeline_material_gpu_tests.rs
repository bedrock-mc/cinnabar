use crate::{self as render_api, gpu_snapshot, shader_source};
use assets::{EntityRenderMaterial, EntityRenderMaterialState};
use bevy::{prelude::Msaa, render::render_resource::Specializer, shader::ShaderDefVal};

use super::super::{
    ActorPipelineKey, ActorPipelineSpecializer, actor_bind_group_layout, actor_pipeline_descriptor,
};

#[path = "../../tests/it/support/actor_raster.rs"]
mod actor_raster;

fn center(frame: &[u8]) -> &[u8] {
    let pixel = (gpu_snapshot::SNAPSHOT_SIDE as usize / 2 * gpu_snapshot::SNAPSHOT_SIDE as usize
        + gpu_snapshot::SNAPSHOT_SIDE as usize / 2)
        * 4;
    &frame[pixel..pixel + 4]
}

#[test]
fn actor_ignoring_lightmap_keeps_directional_shading_and_authored_multiplier() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "actor_ignoring_lightmap_keeps_directional_shading_and_authored_multiplier",
    ) else {
        return;
    };
    let material = crate::ActorMaterial {
        state: Some(EntityRenderMaterialState {
            disable_overlay: true,
            ..Default::default()
        }),
        light_color_multiplier: 0.5,
        ..Default::default()
    };
    for normal in [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        let mut plane = actor_raster::cube([16, 0, 16], true, false);
        for vertex in &mut plane {
            vertex.normal = normal;
        }
        let frame = actor_raster::raster_material_with_overlay(
            &gpu,
            &plane,
            material,
            false,
            [[200, 120, 40, 255]; 2],
            false,
            true,
            crate::pack_actor_light_without_lightmap(),
            crate::pack_overlay_rgba8([1.0; 4]),
        );
        let shade = crate::fancy_actor_shade(normal, 0.0);
        for (actual, source) in center(&frame)[..3].iter().zip([200, 120, 40]) {
            let expected = (source as f32 * 0.5 * shade).round() as i32;
            assert!(
                (i32::from(*actual) - expected).abs() <= 1,
                "ignoring the lightmap preserves face shading: {normal:?}, {actual} != {expected}"
            );
        }
    }
}

#[test]
fn additive_actor_without_overlay_keeps_authored_rgb_and_light_multiplier() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "additive_actor_without_overlay_keeps_authored_rgb_and_light_multiplier",
    ) else {
        return;
    };
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let source = [32, 100, 200, 255];
    for disable_overlay in [false, true] {
        let material = crate::ActorMaterial {
            state: Some(EntityRenderMaterialState {
                cull: false,
                blend: true,
                additive: true,
                disable_overlay,
                ..Default::default()
            }),
            light_color_multiplier: 0.5,
            ..Default::default()
        };
        for light in [
            0,
            crate::pack_actor_light(15, 15),
            crate::pack_actor_light_without_lightmap(),
        ] {
            let clear = actor_raster::raster_material_with_overlay(
                &gpu,
                &plane,
                material,
                false,
                [[0; 4]; 2],
                false,
                true,
                light,
                0,
            );
            for overlay in [0, crate::pack_overlay_rgba8([1.0; 4])] {
                let frame = actor_raster::raster_material_with_overlay(
                    &gpu,
                    &plane,
                    material,
                    false,
                    [source; 2],
                    false,
                    true,
                    light,
                    overlay,
                );
                let admitted = !disable_overlay && overlay != 0;
                let shade = if light == 0 {
                    1.0
                } else {
                    crate::fancy_actor_shade([0.0, 1.0, 0.0], if admitted { 1.0 } else { 0.0 })
                };
                for ((actual, destination), source) in center(&frame)[..3]
                    .iter()
                    .zip(&center(&clear)[..3])
                    .zip(source)
                {
                    let color = if admitted { 255 } else { source };
                    let expected = i32::from(*destination)
                        + (f32::from(color) * material.light_color_multiplier * shade).round()
                            as i32;
                    assert!(
                        (i32::from(*actual) - expected).abs() <= 1,
                        "overlay admission controls color and lighting: disabled={disable_overlay}, light={light}, overlay={overlay}, {actual} != {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn emissive_alpha_test_keeps_colored_zero_alpha_and_weights_only_lighting() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "emissive_alpha_test_keeps_colored_zero_alpha_and_weights_only_lighting",
    ) else {
        return;
    };
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let material = crate::ActorMaterial {
        state: Some(EntityRenderMaterialState {
            alpha_test: true,
            cull: false,
            emissive: true,
            ..Default::default()
        }),
        light_color_multiplier: 0.25,
        ..Default::default()
    };
    for alpha in [0, 255] {
        let frame = actor_raster::raster_material_lighting(
            &gpu,
            &plane,
            material,
            false,
            [[200, 120, 40, alpha]; 2],
            true,
            false,
            0,
        );
        let expected = if alpha == 0 {
            [200, 120, 40]
        } else {
            [50, 30, 10]
        };
        for (actual, expected) in center(&frame)[..3].iter().zip(expected) {
            assert!((i32::from(*actual) - expected).abs() <= 1);
        }
        assert_eq!(center(&frame)[3], alpha);
    }
    let clear = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [[0; 4]; 2],
        true,
        false,
    );
    let ordinary = actor_raster::raster_material_target(
        &gpu,
        &plane,
        crate::ActorMaterial {
            state: Some(EntityRenderMaterialState {
                alpha_test: true,
                ..Default::default()
            }),
            ..Default::default()
        },
        false,
        [[200, 120, 40, 0]; 2],
        true,
        false,
    );
    assert_eq!(center(&ordinary), center(&clear));
}

#[test]
fn additive_actor_material_adds_rgb_even_when_texture_alpha_is_zero() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "additive_actor_material_adds_rgb_even_when_texture_alpha_is_zero",
    ) else {
        return;
    };
    let material = crate::ActorMaterial {
        state: Some(EntityRenderMaterialState {
            alpha_test: true,
            cull: false,
            blend: true,
            additive: true,
            emissive: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    ActorPipelineSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Off,
                hdr: false,
                enhanced: false,
                material: material.gpu_word(),
            },
            &mut descriptor,
        )
        .unwrap();
    let fragment = descriptor.fragment.unwrap();
    let target = fragment.targets[0].as_ref().unwrap();
    let blend = target.blend.unwrap();
    assert_eq!(
        blend.color.src_factor,
        bevy::render::render_resource::BlendFactor::One
    );
    assert_eq!(
        blend.color.dst_factor,
        bevy::render::render_resource::BlendFactor::One
    );
    let srgb = target.format.is_srgb();
    let gamma = fragment.shader_defs.iter().any(
        |define| matches!(define, ShaderDefVal::Bool(name, true) if name == "ACTOR_GAMMA_BLEND"),
    );
    assert!(
        gamma && !srgb,
        "ordinary additive entities compose gamma RGB"
    );
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let clear = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [[0; 4]; 2],
        srgb,
        gamma,
    );
    let source = [32, 64, 96, 0];
    let result = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [source; 2],
        srgb,
        gamma,
    );
    for ((actual, destination), source) in center(&result)[..3]
        .iter()
        .zip(&center(&clear)[..3])
        .zip(source)
    {
        let expected = i32::from(*destination) + i32::from(source);
        assert!((i32::from(*actual) - expected).abs() <= 1);
    }
    assert_eq!(center(&result)[3], center(&clear)[3]);
}

#[test]
fn alpha_weighted_additive_actor_preserves_destination_and_fades_source() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "alpha_weighted_additive_actor_preserves_destination_and_fades_source",
    ) else {
        return;
    };
    let material = crate::ActorMaterial {
        state: Some(EntityRenderMaterialState {
            cull: false,
            blend: true,
            depth_write: false,
            additive: true,
            additive_alpha: true,
            emissive: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    ActorPipelineSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Off,
                hdr: false,
                enhanced: false,
                material: material.gpu_word(),
            },
            &mut descriptor,
        )
        .unwrap();
    assert!(!descriptor.depth_stencil.unwrap().depth_write_enabled);
    let fragment = descriptor.fragment.unwrap();
    let target = fragment.targets[0].as_ref().unwrap();
    let blend = target.blend.unwrap();
    assert_eq!(
        blend.color.src_factor,
        bevy::render::render_resource::BlendFactor::SrcAlpha
    );
    assert_eq!(
        blend.color.dst_factor,
        bevy::render::render_resource::BlendFactor::One
    );
    let srgb = target.format.is_srgb();
    let gamma = fragment.shader_defs.iter().any(
        |define| matches!(define, ShaderDefVal::Bool(name, true) if name == "ACTOR_GAMMA_BLEND"),
    );
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let clear = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [[0; 4]; 2],
        srgb,
        gamma,
    );
    for alpha in [0, 64, 255] {
        let source = [32, 64, 96, alpha];
        let result = actor_raster::raster_material_target(
            &gpu,
            &plane,
            material,
            false,
            [source; 2],
            srgb,
            gamma,
        );
        for ((actual, destination), source) in center(&result)[..3]
            .iter()
            .zip(&center(&clear)[..3])
            .zip(source)
        {
            let expected =
                i32::from(*destination) + (i32::from(source) * i32::from(alpha) + 127) / 255;
            assert!((i32::from(*actual) - expected).abs() <= 1);
        }
    }
}

#[test]
fn actor_material_black_plate_blends_encoded_destination_channels() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "actor_material_black_plate_blends_encoded_destination_channels",
    ) else {
        return;
    };
    let material = crate::ActorMaterial {
        state: Some(EntityRenderMaterialState {
            alpha_test: true,
            cull: false,
            blend: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    ActorPipelineSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Off,
                hdr: false,
                enhanced: false,
                material: material.gpu_word(),
            },
            &mut descriptor,
        )
        .unwrap();
    let fragment = descriptor.fragment.unwrap();
    let srgb = fragment.targets[0].as_ref().unwrap().format.is_srgb();
    let gamma = fragment.shader_defs.iter().any(
        |define| matches!(define, ShaderDefVal::Bool(name, true) if name == "ACTOR_GAMMA_BLEND"),
    );
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let clear = actor_raster::raster_material_target(
        &gpu,
        &plane,
        crate::ActorMaterial {
            kind: EntityRenderMaterial::Dragon,
            ..Default::default()
        },
        false,
        [[0; 4]; 2],
        srgb,
        gamma,
    );
    let plate_alpha = 153;
    let plate = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [[0, 0, 0, plate_alpha]; 2],
        srgb,
        gamma,
    );
    let pixel = (gpu_snapshot::SNAPSHOT_SIDE as usize / 2 * gpu_snapshot::SNAPSHOT_SIDE as usize
        + gpu_snapshot::SNAPSHOT_SIDE as usize / 2)
        * 4;
    for channel in 0..3 {
        let background = u32::from(clear[pixel + channel]);
        let expected = (background * u32::from(u8::MAX - plate_alpha) + u32::from(u8::MAX) / 2)
            / u32::from(u8::MAX);
        assert!(
            (i32::from(plate[pixel + channel]) - expected as i32).abs() <= 1,
            "black plate channel {channel}: background={background}, expected={expected}, actual={}",
            plate[pixel + channel],
        );
    }
}

#[test]
fn always_passing_depth_materials_ignore_occluders_and_draw_after_opaque_geometry() {
    for depth_always in [false, true] {
        let material = crate::ActorMaterial {
            state: Some(EntityRenderMaterialState {
                alpha_test: true,
                depth_always,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
        ActorPipelineSpecializer
            .specialize(
                ActorPipelineKey {
                    msaa: Msaa::Off,
                    hdr: false,
                    enhanced: false,
                    material: material.gpu_word(),
                },
                &mut descriptor,
            )
            .unwrap();
        // Sorted-pass spans render into the transparent pass's gamma-encoded target.
        let fragment = descriptor.fragment.as_ref().unwrap();
        let gamma = fragment.shader_defs.iter().any(
            |define| matches!(define, ShaderDefVal::Bool(name, true) if name == "ACTOR_GAMMA_BLEND"),
        );
        let srgb = fragment.targets[0].as_ref().unwrap().format.is_srgb();
        assert_eq!((gamma, srgb), (depth_always, !depth_always));
        let depth = descriptor.depth_stencil.unwrap();
        assert!(depth.depth_write_enabled);
        assert_eq!(
            depth.depth_compare == bevy::render::render_resource::CompareFunction::Always,
            depth_always
        );
        assert_eq!(
            super::super::phase::sorted(material.gpu_word()),
            depth_always
        );
    }
}
