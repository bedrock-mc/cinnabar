use crate::{self as render_api, shader_source};
use assets::{EntityRenderMaterial, EntityRenderMaterialState};
use bevy::{prelude::Msaa, render::render_resource::Specializer, shader::ShaderDefVal};

use super::super::{
    ActorPipelineKey, ActorPipelineSpecializer, actor_bind_group_layout, actor_pipeline_descriptor,
};

#[path = "../../tests/it/support/actor_raster.rs"]
mod actor_raster;
#[path = "../../tests/it/support/gpu_snapshot.rs"]
mod gpu_snapshot;

fn center(frame: &[u8]) -> &[u8] {
    let pixel = (gpu_snapshot::SNAPSHOT_SIDE as usize / 2 * gpu_snapshot::SNAPSHOT_SIDE as usize
        + gpu_snapshot::SNAPSHOT_SIDE as usize / 2)
        * 4;
    &frame[pixel..pixel + 4]
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
