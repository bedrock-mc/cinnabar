//! A real render app: once readiness is published, first use of each owner compiles nothing.

use crate::pipeline_warmup::PipelineWarmupReadiness;
use crate::{
    AimAssistHighlight, AimAssistHighlightPlugin, AimAssistHighlightScene, AimAssistTexture,
    AtmospherePlugin, ModRenderPlugin, ModRenderScene, PanoramaRenderPlugin, PanoramaScene,
    UiRenderPlugin, UiRenderSceneResource, UiRenderStatsResource,
};
use bevy::{
    asset::AssetPlugin,
    camera::{CameraPlugin, RenderTarget},
    core_pipeline::CorePipelinePlugin,
    image::ImagePlugin,
    mesh::MeshPlugin,
    prelude::*,
    render::{
        RenderApp,
        render_resource::{PipelineCache, TextureFormat, WgpuFeatures},
    },
    window::WindowPlugin,
};
use render_model::{
    PanoramaFaces, PanoramaView, UiRenderBatch, UiRenderInput, UiRenderTextureArray,
    UiRenderVertex, UiScissor, UiTexturePage,
};

const GRADE: &str = "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv) * param(1u); }";

/// Builds standalone custom renderers with an output format different from their scene pipelines.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..default()
        })
        .add_plugins((AssetPlugin::default(), TransformPlugin))
        .add_plugins(crate::chunk::noop_render_plugin(WgpuFeatures::empty()))
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
        ))
        .add_plugins((
            AtmospherePlugin,
            PanoramaRenderPlugin,
            AimAssistHighlightPlugin,
            UiRenderPlugin,
            ModRenderPlugin,
        ));
    // A window surface's format, distinct from the main texture, so the output composite counts.
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            64,
            48,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
    app.world_mut().spawn((
        Camera3d::default(),
        Camera::default(),
        RenderTarget::Image(image.into()),
        Msaa::Sample4,
        Transform::from_xyz(0.0, 70.0, 0.0),
    ));
    app.finish();
    app.cleanup();
    app
}

#[test]
fn standalone_renderers_keep_scene_format_independent_of_output() {
    let mut app = app();
    for _ in 0..4 {
        app.update();
    }
    let world = app.sub_app_mut(RenderApp).world_mut();
    let mut views = world.query::<&bevy::render::view::ViewTarget>();
    let target = views.single(world).unwrap();
    assert_eq!(target.main_texture_format(), crate::SCENE_COLOR_FORMAT);
    assert_eq!(
        target.out_texture_view_format(),
        Some(TextureFormat::Bgra8UnormSrgb)
    );
}

fn pass(revision: u64, enabled: bool) -> mod_render::Pass {
    mod_render::Pass {
        name: format!("grade-{revision}"),
        order: 0,
        depth: false,
        source: GRADE.into(),
        shader: mod_render::shader::compose(GRADE, false).unwrap().into(),
        revision,
        enabled,
        params: [0.0; 16],
    }
}

fn mods(app: &mut App, enabled: bool) {
    let output = mod_render::RenderOutput {
        passes: vec![pass(1, true), pass(2, enabled)],
        primitives: default(),
    };
    let mut scene = app.world_mut().resource_mut::<ModRenderScene>();
    let generation = scene.generation() + 1;
    scene.apply(&output, generation);
}

fn ui() -> UiRenderInput {
    UiRenderInput {
        revision: 1,
        viewport_size: [64, 48],
        safe_area: [0; 4],
        vertices: std::sync::Arc::from(
            [UiRenderVertex {
                position: [0.0; 2],
                clip_z: 0.0,
                clip_w: 1.0,
                uv: [0.0; 2],
                color: [255; 4],
                style_flags: 0,
                alpha_cutoff: -1.0,
                model_light: 1.0,
                overlay_color: [0.0; 4],
            }; 4],
        ),
        indices: std::sync::Arc::from([0, 1, 2, 0, 2, 3]),
        batches: std::sync::Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 64, 48),
            0,
            6,
            render_model::UI_BLEND_ALPHA,
        )]),
        textures: std::sync::Arc::new(
            UiRenderTextureArray::new(
                vec![UiTexturePage::owned([1, 1], std::sync::Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    }
}

/// Every pipeline the render world has created, including owned mod passes.
fn created(app: &App) -> (usize, usize) {
    let world = app.sub_app(RenderApp).world();
    let cached = world.resource::<PipelineCache>().pipelines().count();
    let passes = world
        .resource::<crate::mod_render::PassGpu>()
        .pipelines
        .len();
    (cached, passes)
}

#[test]
fn first_use_after_readiness_creates_no_pipelines() {
    let mut app = app();
    mods(&mut app, false);
    let readiness = app.world().resource::<PipelineWarmupReadiness>().clone();
    for _ in 0..64 {
        app.update();
        if readiness.is_ready() {
            break;
        }
    }
    assert!(readiness.is_ready(), "warmup never reported ready");
    for _ in 0..2 {
        app.update();
    }
    let warmed = created(&app);

    // Aim at a block, open a screen, enable a mod pass, then return to the menu panorama.
    let mut aim = app.world_mut().resource_mut::<AimAssistHighlightScene>();
    aim.textures[0] = Some(std::sync::Arc::new(
        AimAssistTexture::new([1, 1], std::sync::Arc::from([255; 4])).unwrap(),
    ));
    aim.target = AimAssistHighlight::block(Vec3::new(0.5, 64.5, 4.5), 2, Vec3::Z);
    let stats = app.world().resource::<UiRenderStatsResource>().clone();
    app.world_mut()
        .resource_mut::<UiRenderSceneResource>()
        .publish(ui(), &stats)
        .unwrap();
    mods(&mut app, true);
    for _ in 0..4 {
        app.update();
    }
    let mut panorama = app.world_mut().resource_mut::<PanoramaScene>();
    panorama.set_faces(Some(std::sync::Arc::new(
        PanoramaFaces::new(1, std::array::from_fn(|_| vec![255; 4])).unwrap(),
    )));
    panorama.show(Some(PanoramaView {
        yaw_radians: 0.0,
        pitch_radians: 0.0,
        vertical_fov_radians: 1.0,
        aspect: 1.0,
        tint: [0.0; 4],
    }));
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(created(&app), warmed);
}
