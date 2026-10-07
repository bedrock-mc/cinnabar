use super::*;
use crate::queue_review_support as fixture;
use bevy::{
    core_pipeline::core_3d::Transparent3d, ecs::system::RunSystemOnce,
    render::render_phase::AddRenderCommand, render::view::ExtractedView,
};
use mod_render::{Decal, DecalStyle, Pass, Primitives};

const GRADE: &str = "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv) * param(1u); }";

fn pass(name: &str, revision: u64) -> Pass {
    Pass {
        name: name.into(),
        order: 0,
        depth: false,
        source: GRADE.into(),
        shader: mod_render::shader::compose(GRADE, false).unwrap().into(),
        revision,
        enabled: true,
        params: [0.0; 16],
    }
}

fn decal() -> Primitives {
    Primitives {
        decals: vec![Decal {
            center: [0.0, 64.0, 0.0],
            radius: 2.0,
            color: [1.0; 4],
            progress: 0.5,
            style: DecalStyle::Shockwave,
        }],
        ..Default::default()
    }
}

#[test]
fn applying_output_rebuilds_geometry_only_on_change() {
    let mut scene = ModRenderScene::default();
    let primitives = Arc::new(decal());
    let output = RenderOutput {
        passes: vec![pass("a", 7)],
        primitives: Arc::clone(&primitives),
    };
    scene.apply(&output, 1);
    assert_eq!(scene.vertex_count(), 6);
    let vertices = Arc::clone(&scene.vertices);
    let mut next = output.clone();
    next.passes.push(pass("b", 8));
    scene.apply(&next, 2);
    assert_eq!(scene.pass_count(), 2);
    assert!(Arc::ptr_eq(&vertices, &scene.vertices));
    next.primitives = Arc::new(Primitives::default());
    scene.apply(&next, 2);
    assert_eq!(scene.vertex_count(), 6, "an applied generation is ignored");
    scene.apply(&next, 3);
    assert_eq!(scene.vertex_count(), 0);
    scene.clear();
    assert_eq!(scene.pass_count(), 0);
}

#[test]
fn pass_layouts_cover_every_resource_the_composed_shader_reads() {
    for depth in [false, true] {
        let source = if depth {
            "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv) * depth(uv) * param(0u); }"
        } else {
            GRADE
        };
        let shader = mod_render::shader::compose(source, depth).unwrap();
        crate::shader_test_support::assert_binding_visibility(&shader, 0, &passes::layout(depth));
    }
}

#[test]
fn frame_uniform_packs_view_and_params() {
    let view = ExtractedView {
        retained_view_entity: bevy::render::view::RetainedViewEntity::new(
            Entity::PLACEHOLDER.into(),
            None,
            0,
        ),
        clip_from_view: Mat4::perspective_infinite_reverse_rh(1.0, 2.0, 0.1),
        world_from_view: GlobalTransform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        clip_from_world: None,
        hdr: false,
        viewport: UVec4::new(0, 0, 640, 360),
        color_grading: default(),
        invert_culling: false,
    };
    let mut params = [0.0; 16];
    params[15] = 4.0;
    let uniform = passes::frame_uniform(&view, 2.5, 0.016, 3, params);
    assert_eq!(
        std::mem::size_of_val(&uniform),
        mod_render::shader::FRAME_UNIFORM_BYTES
    );
    assert_eq!(uniform.eye, [1.0, 2.0, 3.0, 1.0]);
    assert_eq!(uniform.resolution, [640.0, 360.0, 1.0 / 640.0, 1.0 / 360.0]);
    assert_eq!(uniform.time, [2.5, 0.016, 3.0, 0.0]);
    assert_eq!(uniform.params[15], 4.0);
    let clip = Mat4::from_cols_array_2d(&uniform.clip_from_world);
    let world = Mat4::from_cols_array_2d(&uniform.world_from_clip);
    let point = clip.project_point3(Vec3::new(1.0, 2.0, -7.0));
    assert!(
        world
            .project_point3(point)
            .distance(Vec3::new(1.0, 2.0, -7.0))
            < 1e-3
    );
}

#[test]
fn pass_pipelines_compile_for_each_view_format_without_blending() {
    let (mut app, _) = fixture::app();
    app.world_mut().run_system_once(passes::init_gpu).unwrap();
    bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let formats = [
        bevy::render::view::ViewTarget::TEXTURE_FORMAT_HDR,
        bevy::render::render_resource::TextureFormat::bevy_default(),
    ];
    let pass = pass("a", 1);
    app.world_mut()
        .resource_scope(|world, mut gpu: Mut<passes::PassGpu>| {
            let device = world
                .resource::<bevy::render::renderer::RenderDevice>()
                .clone();
            let cache = world.resource::<bevy::render::render_resource::PipelineCache>();
            for format in formats {
                for _ in 0..1000 {
                    gpu.ensure_pipeline(&device, cache, &pass, format);
                    if !matches!(
                        gpu.pipelines.get(&(1, format)),
                        Some(passes::PipelineState::Creating(_))
                    ) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                assert!(gpu.pipeline(1, format).is_some(), "{format:?}");
                assert!(passes::color_target(format).blend.is_none());
            }
        });
}

#[test]
fn primitives_queue_one_late_transparent_item_only_when_present() {
    let (mut app, view) = fixture::app();
    app.init_resource::<primitives::PrimitivePipeline>()
        .add_render_command::<Transparent3d, primitives::DrawPrimitiveCommands>();
    app.world_mut()
        .run_system_once(primitives::init_gpu)
        .unwrap();
    app.init_resource::<ModRenderScene>();
    app.world_mut().run_system_once(primitives::queue).unwrap();
    assert!(fixture::items(&app, view).is_empty());
    app.world_mut().resource_mut::<ModRenderScene>().apply(
        &RenderOutput {
            passes: Vec::new(),
            primitives: Arc::new(decal()),
        },
        1,
    );
    app.world_mut().run_system_once(primitives::queue).unwrap();
    let items = fixture::items(&app, view);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].distance, primitives::PRIMITIVE_DISTANCE);
    let id = items[0].pipeline;
    let mut cache = app
        .world_mut()
        .resource_mut::<bevy::render::render_resource::PipelineCache>();
    let descriptor = fixture::queued_descriptor(&mut cache, id);
    let depth = descriptor.depth_stencil.as_ref().unwrap();
    assert!(!depth.depth_write_enabled);
    assert_eq!(
        depth.depth_compare,
        bevy::render::render_resource::CompareFunction::GreaterEqual
    );
}

#[test]
fn primitive_shader_resources_match_the_layout() {
    let source = crate::shader_source::standalone(include_str!("primitives.wgsl"), &[]);
    let pipeline = primitives::PrimitivePipeline::from_world(&mut World::new());
    crate::shader_test_support::assert_binding_visibility(&source, 0, &pipeline.layout);
}

#[test]
fn sandbox_shaders_survive_bevy_shader_composition() {
    let source = "fn effect(uv: vec2<f32>) -> vec3<f32> { return bloom(uv, 4.0, 0.5) + vec3<f32>(depth(uv)); }";
    let shader = mod_render::shader::compose(source, true).unwrap();
    let composed = crate::shader_source::composed(&shader, &[]);
    assert!(composed.contains(mod_render::shader::FRAGMENT_ENTRY));
    assert!(composed.contains(mod_render::shader::VERTEX_ENTRY));
}

#[test]
fn replaced_passes_release_their_pipelines() {
    let (mut app, _) = fixture::app();
    app.world_mut().run_system_once(passes::init_gpu).unwrap();
    let format = bevy::render::view::ViewTarget::TEXTURE_FORMAT_HDR;
    let mut scene = ModRenderScene::default();
    for revision in 1..=20 {
        scene.apply(
            &RenderOutput {
                passes: vec![pass("a", revision)],
                primitives: Default::default(),
            },
            revision,
        );
        let world = app.world_mut();
        world.resource_scope(|world, mut gpu: Mut<passes::PassGpu>| {
            bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
            let device = world
                .resource::<bevy::render::renderer::RenderDevice>()
                .clone();
            let mut cache = world.resource_mut::<bevy::render::render_resource::PipelineCache>();
            gpu.retain_pipelines(&scene);
            gpu.ensure_pipeline(&device, &cache, &scene.passes[0], format);
            cache.process_queue();
        });
    }
    let cache = app
        .world()
        .resource::<bevy::render::render_resource::PipelineCache>();
    assert!(
        cache.pipelines().count() <= 1,
        "replacing a pass must not leave its pipeline behind ({} retained)",
        cache.pipelines().count()
    );
    assert_eq!(app.world().resource::<passes::PassGpu>().pipelines.len(), 1);
}

#[test]
fn primitives_queue_on_the_frame_they_first_arrive() {
    // Queue runs before resource preparation uploads the frame's vertices.
    let (mut app, view) = fixture::app();
    app.init_resource::<primitives::PrimitivePipeline>()
        .add_render_command::<Transparent3d, primitives::DrawPrimitiveCommands>();
    app.world_mut()
        .run_system_once(primitives::init_gpu)
        .unwrap();
    let mut scene = ModRenderScene::default();
    scene.apply(
        &RenderOutput {
            passes: Vec::new(),
            primitives: Arc::new(decal()),
        },
        1,
    );
    app.insert_resource(scene);
    app.world_mut().run_system_once(primitives::queue).unwrap();
    assert_eq!(fixture::items(&app, view).len(), 1);
    fixture::clear(&mut app, view);
    app.world_mut().resource_mut::<ModRenderScene>().clear();
    app.world_mut()
        .resource_mut::<primitives::PrimitiveGpu>()
        .vertex_count = 6;
    app.world_mut().run_system_once(primitives::queue).unwrap();
    assert!(
        fixture::items(&app, view).is_empty(),
        "stale uploads queue nothing"
    );
}

#[test]
fn solid_blocks_queue_separately_without_relaxing_guest_depth_tests() {
    let (mut app, view) = fixture::app();
    app.init_resource::<primitives::PrimitivePipeline>()
        .add_render_command::<Transparent3d, primitives::DrawPrimitiveCommands>()
        .add_render_command::<Transparent3d, primitives::DrawBlockHighlightCommands>();
    app.world_mut()
        .run_system_once(primitives::init_gpu)
        .unwrap();
    let mut scene = ModRenderScene::default();
    scene.apply(
        &RenderOutput {
            passes: Vec::new(),
            primitives: Arc::new(decal()),
        },
        1,
    );
    scene.set_block_highlights(&[[2, 3, 4]], [1.0, 0.1, 0.5, 1.0]);
    app.insert_resource(scene);
    app.world_mut().run_system_once(primitives::queue).unwrap();
    let pipelines: Vec<_> = fixture::items(&app, view)
        .iter()
        .map(|item| item.pipeline)
        .collect();
    assert_eq!(pipelines.len(), 2);
    let mut cache = app
        .world_mut()
        .resource_mut::<bevy::render::render_resource::PipelineCache>();
    let mut depths = Vec::new();
    for pipeline in pipelines {
        let descriptor = fixture::queued_descriptor(&mut cache, pipeline);
        let depth = descriptor.depth_stencil.as_ref().unwrap();
        assert!(!depth.depth_write_enabled);
        depths.push(depth.depth_compare);
    }
    assert!(depths.contains(&bevy::render::render_resource::CompareFunction::Always));
    assert!(depths.contains(&bevy::render::render_resource::CompareFunction::GreaterEqual));
    fixture::clear(&mut app, view);
    app.world_mut()
        .resource_mut::<ModRenderScene>()
        .set_block_highlights(&[], [1.0; 4]);
    app.world_mut().run_system_once(primitives::queue).unwrap();
    assert_eq!(
        fixture::items(&app, view).len(),
        1,
        "clearing highlights preserves the guest draw"
    );
}
