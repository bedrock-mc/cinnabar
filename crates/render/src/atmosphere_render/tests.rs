use std::sync::Arc;

use assets::{
    AtmosphereRole, AtmosphereTexture, CompiledAtmosphereAssets, RuntimeAtmosphereAssets,
    encode_atmosphere_blob,
};
use bevy::{
    app::SubApp,
    asset::Assets,
    core_pipeline::core_3d::{Opaque3d, Transparent3d},
    ecs::{schedule::Schedule, system::RunSystemOnce},
    image::BevyDefault,
    prelude::{App, Shader},
    render::{
        ExtractSchedule, Render, RenderApp, RenderStartup,
        render_phase::DrawFunctions,
        renderer::{RenderDevice, RenderQueue, WgpuWrapper},
    },
};
use sha2::{Digest, Sha256};

use super::{AtmosphereGpu, AtmosphereRenderInstalled, prepare_atmosphere_textures};
use crate::{AtmosphereTextureAssets, ChunkRenderPlugin, RuntimeStageProfiler};

#[test]
fn star_specialization_uses_native_colour_blend_and_never_writes_alpha() {
    use bevy::render::render_resource::{
        ColorTargetState, FragmentState, RenderPipelineDescriptor, Specializer,
    };
    for stars in [false, true] {
        let mut descriptor = RenderPipelineDescriptor {
            fragment: Some(FragmentState {
                targets: vec![Some(ColorTargetState {
                    format: super::TextureFormat::bevy_default(),
                    blend: None,
                    write_mask: super::ColorWrites::ALL,
                })],
                ..Default::default()
            }),
            ..Default::default()
        };
        super::AtmospherePipelineSpecializer
            .specialize(
                super::AtmospherePipelineKey {
                    msaa: super::Msaa::Sample4,
                    hdr: false,
                    stars,
                    skybox: false,
                },
                &mut descriptor,
            )
            .unwrap();
        let target = descriptor.fragment.unwrap().targets.remove(0).unwrap();
        assert_eq!(target.blend, stars.then(super::native_star_blend));
        assert_eq!(
            target.write_mask,
            super::ColorWrites::RED | super::ColorWrites::GREEN | super::ColorWrites::BLUE
        );
        assert_eq!(descriptor.multisample.count, 4);
    }
}

fn app_with_noop_render_sub_app() -> App {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut render_app = SubApp::new();
    render_app
        .insert_resource(RenderDevice::from(device))
        .insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))))
        .insert_resource(DrawFunctions::<Opaque3d>::default())
        .insert_resource(DrawFunctions::<Transparent3d>::default())
        .add_schedule(Schedule::new(RenderStartup))
        .add_schedule(Render::base_schedule())
        .add_schedule(Schedule::new(ExtractSchedule));

    let mut app = App::new();
    app.insert_resource(Assets::<Shader>::default())
        .insert_sub_app(RenderApp, render_app);
    app
}

fn synthetic_runtime(seed: u8) -> Arc<RuntimeAtmosphereAssets> {
    let textures = [
        (AtmosphereRole::Sun, "textures/environment/sun.png", 32, 32),
        (
            AtmosphereRole::MoonPhases,
            "textures/environment/moon_phases.png",
            128,
            64,
        ),
        (
            AtmosphereRole::Clouds,
            "textures/environment/clouds.png",
            256,
            256,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (role, source_path, width, height))| {
        let rgba8 = vec![seed.wrapping_add(index as u8); (width * height * 4) as usize];
        AtmosphereTexture {
            role,
            source_path: source_path.into(),
            source_bytes: 1,
            source_sha256: [index as u8 + 1; 32],
            pixels_sha256: Sha256::digest(&rgba8).into(),
            width,
            height,
            rgba8: rgba8.into_boxed_slice(),
        }
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let blob = encode_atmosphere_blob(&CompiledAtmosphereAssets {
        source_manifest_sha256: [0x66; 32],
        textures,
        biome_profiles: Box::new([]),
        fog_profiles: Box::new([]),
    })
    .unwrap();
    Arc::new(RuntimeAtmosphereAssets::decode(&blob).unwrap())
}

#[test]
fn standalone_chunk_renderer_installs_and_starts_atmosphere_gpu_resources() {
    let mut app = app_with_noop_render_sub_app();
    app.add_plugins(ChunkRenderPlugin::new(1));
    app.finish();

    assert!(!app.world().contains_resource::<RuntimeStageProfiler>());
    let render_app = app.sub_app_mut(RenderApp);
    assert!(
        render_app
            .world()
            .contains_resource::<AtmosphereRenderInstalled>()
    );
    assert!(
        !render_app
            .world()
            .contains_resource::<RuntimeStageProfiler>()
    );
    render_app.world_mut().run_schedule(RenderStartup);
    assert!(render_app.world().contains_resource::<AtmosphereGpu>());
}

#[test]
fn enabled_stage_profiler_is_shared_with_the_render_world() {
    let mut app = app_with_noop_render_sub_app();
    app.insert_resource(RuntimeStageProfiler::new(true));
    app.add_plugins(ChunkRenderPlugin::new(1));

    assert!(
        app.sub_app(RenderApp)
            .world()
            .contains_resource::<RuntimeStageProfiler>()
    );
}

#[test]
fn gameplay_profiler_is_shared_without_enabling_aggregate_snapshots() {
    let mut app = app_with_noop_render_sub_app();
    app.insert_resource(RuntimeStageProfiler::for_gameplay(false, None));
    app.add_plugins(ChunkRenderPlugin::new(1));
    let profiler = app
        .sub_app(RenderApp)
        .world()
        .resource::<RuntimeStageProfiler>();
    assert!(!profiler.enabled());
    assert!(
        profiler
            .take_snapshot_if_due(std::time::Duration::ZERO)
            .is_none()
    );
}

#[test]
fn gpu_preparation_uploads_once_per_stable_asset_identity() {
    let mut app = app_with_noop_render_sub_app();
    app.add_plugins(ChunkRenderPlugin::new(1));
    app.finish();

    let render_app = app.sub_app_mut(RenderApp);
    render_app.world_mut().run_schedule(RenderStartup);
    let identity = [0x31; 32];
    render_app
        .world_mut()
        .insert_resource(AtmosphereTextureAssets::new(
            synthetic_runtime(0x10),
            identity,
        ));
    render_app
        .world_mut()
        .run_system_once(prepare_atmosphere_textures)
        .unwrap();
    assert_eq!(
        render_app.world().resource::<AtmosphereGpu>().upload_count,
        1
    );

    render_app
        .world_mut()
        .insert_resource(AtmosphereTextureAssets::new(
            synthetic_runtime(0x20),
            identity,
        ));
    render_app
        .world_mut()
        .run_system_once(prepare_atmosphere_textures)
        .unwrap();
    assert_eq!(
        render_app.world().resource::<AtmosphereGpu>().upload_count,
        1
    );

    render_app
        .world_mut()
        .insert_resource(AtmosphereTextureAssets::new(
            synthetic_runtime(0x20),
            [0x32; 32],
        ));
    render_app
        .world_mut()
        .run_system_once(prepare_atmosphere_textures)
        .unwrap();
    assert_eq!(
        render_app.world().resource::<AtmosphereGpu>().upload_count,
        2
    );
    for (kind, expected_uploads) in [
        (crate::SkyKind::End, 3),
        (crate::SkyKind::End, 3),
        (crate::SkyKind::Overworld, 4),
    ] {
        render_app
            .world_mut()
            .insert_resource(crate::AtmosphereFrame::default().with_sky_kind(kind));
        render_app
            .world_mut()
            .run_system_once(prepare_atmosphere_textures)
            .unwrap();
        assert_eq!(
            render_app.world().resource::<AtmosphereGpu>().upload_count,
            expected_uploads
        );
    }
}
