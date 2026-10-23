//! Allocation faults reach preparation, queued draw commands and the next valid frame.

use super::super::*;
use super::*;
use bevy::{
    diagnostic::FrameCount,
    ecs::schedule::SingleThreadedExecutor,
    prelude::*,
    render::{
        batching::gpu_preprocessing::GpuPreprocessingMode,
        render_phase::{BinnedPhaseItem, ViewBinnedRenderPhases, ViewSortedRenderPhases},
        view::{ExtractedView, ViewUniforms, prepare_view_uniforms},
    },
};
use std::{cell::RefCell, rc::Rc};

#[derive(Resource, Default)]
struct Frames(u32);

#[derive(Resource, Default)]
struct ExecutedSpans(Vec<crate::actor::gpu::ActorDrawSpan>);

/// Resets the real phases and proves the render schedule keeps advancing after faults.
fn begin_frame(
    views: Query<&ExtractedView>,
    mut opaque: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    mut transparent: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut frames: ResMut<Frames>,
) {
    frames.0 += 1;
    for view in &views {
        opaque.prepare_for_new_frame(view.retained_view_entity, GpuPreprocessingMode::None);
        transparent.prepare_for_new_frame(view.retained_view_entity);
    }
}

/// Installs the production actor plugin, shaders, lighting and a headless validation device.
pub(crate) fn fixture() -> App {
    bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (mut app, _) = crate::queue_review_support::app();
    let mut render = SubApp::new();
    *render.world_mut() = std::mem::take(app.world_mut());
    render
        .add_schedule(Schedule::new(RenderStartup))
        .add_schedule(Render::base_schedule())
        .add_schedule(Schedule::new(ExtractSchedule))
        .init_resource::<DrawFunctions<Opaque3d>>()
        .init_resource::<ViewBinnedRenderPhases<Opaque3d>>()
        .init_resource::<ViewUniforms>()
        .init_resource::<FrameCount>()
        .init_resource::<Frames>()
        .init_resource::<ExecutedSpans>()
        .init_resource::<render_scene::WorldLighting>()
        .init_resource::<render_scene::WorldFullbright>()
        .init_resource::<render_scene::NametagSceneResource>()
        .add_systems(Render, begin_frame.in_set(RenderSystems::PrepareViews))
        .add_systems(
            Render,
            prepare_view_uniforms.in_set(RenderSystems::PrepareResources),
        )
        .add_systems(
            Render,
            execute_draws
                .in_set(RenderSystems::Render)
                .before(submit_actor_presented_frame),
        );
    app.insert_resource(Assets::<Shader>::default());
    app.insert_sub_app(RenderApp, render);
    app.add_plugins(ActorRenderPlugin);
    crate::lighting::install(&mut app);
    app.finish();
    let _view_shader =
        app.world_mut()
            .resource_mut::<Assets<Shader>>()
            .add(crate::shader_safety::from_wesl(
                crate::shader_source::VIEW,
                "embedded://bevy_render/view.wesl",
            ));
    let shaders = app
        .world()
        .resource::<Assets<Shader>>()
        .iter()
        .map(|(id, shader)| (id, shader.clone()))
        .collect::<Vec<_>>();
    let world = app.sub_app_mut(RenderApp).world_mut();
    for (id, shader) in shaders {
        world.resource_mut::<PipelineCache>().set_shader(id, shader);
    }
    world
        .resource_mut::<bevy::ecs::schedule::Schedules>()
        .get_mut(Render)
        .unwrap()
        .set_executor(SingleThreadedExecutor::new());
    world.run_schedule(RenderStartup);
    // Single-sample rendering keeps this witness small; MSAA allocation is covered separately.
    let view = world
        .query_filtered::<Entity, With<ExtractedView>>()
        .single(world)
        .unwrap();
    world.entity_mut(view).insert(Msaa::Off);
    app
}

/// Gives each page distinct original pixels and each instance its corresponding page.
pub(crate) fn publication(generation: u64, markers: &[u8], layers: u32) -> ActorRenderFrame {
    let mut frame = generic_frame();
    frame.rig.frame_generation = generation;
    frame.rig.geometry_revision = generation;
    let instance = frame.rig.instances[0];
    frame.rig.instances = vec![
        render_model::ActorGpuInstance {
            texture_layer: layers - 1,
            ..instance
        };
        markers.len()
    ]
    .into();
    let entry = frame.rig.manifest[0].clone();
    frame.rig.manifest = (0..markers.len())
        .map(|index| {
            let mut entry = entry.clone();
            entry.instance_index = index as u32;
            entry.identity.runtime_id += index as u64;
            entry.identity.pose_generation = PoseGeneration::new(generation);
            entry.completed_tick = generation;
            entry
        })
        .collect::<Vec<_>>()
        .into();
    frame.instance_pages = (1..=markers.len() as u16).collect::<Vec<_>>().into();
    let artwork = Arc::make_mut(&mut frame.artwork);
    artwork.entity_identity = [generation as u8; 32];
    artwork.pages = markers
        .iter()
        .map(|&marker| render_model::ActorTexturePage {
            width: 4,
            height: 4,
            layers,
            rgba8: [marker, 64, 128, 255].repeat(16 * layers as usize).into(),
            color_mask: false,
            multitexture: false,
        })
        .collect::<Vec<_>>()
        .into();
    frame
}

/// Runs real queued opaque commands and returns only spans that reached a GPU draw.
pub(crate) fn draw_frame(
    app: &mut App,
    frame: &ActorRenderFrame,
) -> Vec<crate::actor::gpu::ActorDrawSpan> {
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.insert_resource(frame.clone());
    world.run_schedule(Render);
    let spans = world.resource::<ExecutedSpans>().0.clone();
    assert!(app.should_exit().is_none());
    spans
}

/// Executes the queued commands before render cleanup removes their phase.
fn execute_draws(world: &mut World) {
    let generation = world.resource::<ActorRenderFrame>().rig.frame_generation;
    let id = world
        .resource::<ActorPipeline>()
        .draw_variant(
            Msaa::Off,
            false,
            false,
            assets::EntityRenderMaterial::Default as u32,
        )
        .unwrap();
    world
        .resource_mut::<PipelineCache>()
        .block_on_render_pipeline(id);
    assert!(
        world
            .resource::<PipelineCache>()
            .get_render_pipeline(id)
            .is_some(),
        "fixture actor pipeline: {:?}",
        world
            .resource::<PipelineCache>()
            .get_render_pipeline_state(id)
    );
    let (view, retained) = world
        .query::<(Entity, &ExtractedView)>()
        .single(world)
        .map(|(entity, view)| (entity, view.retained_view_entity))
        .unwrap();
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let attachment = |format| {
        device
            .create_texture(&TextureDescriptor {
                label: Some("allocation fault draw witness"),
                size: Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    let color = attachment(render_scene::SCENE_COLOR_FORMAT);
    let depth = attachment(CORE_3D_DEPTH_FORMAT);
    let (_, commands) = crate::render_test_support::record(world, &device, |world, context| {
        let colors = [Some(RenderPassColorAttachment {
            view: &color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            color_attachments: &colors,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        let phases = world.resource::<ViewBinnedRenderPhases<Opaque3d>>();
        let phase = phases.get(&retained).unwrap();
        let mut functions = world.resource::<DrawFunctions<Opaque3d>>().write();
        functions.prepare(world);
        for ((batch, bin), entries) in &phase.non_mesh_items {
            for (main_entity, entity) in &entries.entities {
                let item = Opaque3d::new(
                    batch.clone(),
                    bin.clone(),
                    (*entity, *main_entity),
                    0..1,
                    PhaseItemExtraIndex::None,
                );
                functions
                    .get_mut(item.draw_function())
                    .unwrap()
                    .draw(world, &mut pass, view, &item)
                    .unwrap();
            }
        }
    });
    queue.submit(commands);
    let gpu = world.resource::<ActorGpu>();
    let tracker = world.resource::<ActorDrawTracker>();
    let recorded = tracker.recorded_spans();
    let spans = gpu
        .spans
        .iter()
        .copied()
        .filter(|span| recorded.contains(span))
        .collect::<Vec<_>>();
    assert_eq!(spans.len(), recorded.len());
    assert_eq!(
        gpu.executed_instances
            .load(std::sync::atomic::Ordering::Relaxed),
        spans.iter().map(|span| span.count).sum::<u32>()
    );
    if let Some(drawn) = tracker.take_drawn() {
        assert!(drawn.is_exact());
        assert_eq!(drawn.frame_generation, generation);
        assert_eq!(drawn.manifest.as_ref(), gpu.main_manifest.as_ref());
        assert_eq!(spans, gpu.spans);
    } else {
        assert!(spans.len() < gpu.spans.len() || spans.is_empty());
    }
    world.resource_mut::<ExecutedSpans>().0 = spans;
}

/// Records every checked boundary so a fault cannot silently miss its intended consumer.
fn fail_at(label: &'static str, occurrence: usize) -> (impl Drop, Rc<RefCell<Vec<String>>>) {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let observed = Rc::clone(&calls);
    let mut matched = 0;
    let hook = gpu_allocation::inject(move |owner, _| {
        observed.borrow_mut().push(owner.to_owned());
        if owner != label {
            return false;
        }
        matched += 1;
        matched >= occurrence
    });
    (hook, calls)
}

#[test]
fn gpu_fault_matrix_pages_retain_compatible_artwork_and_recover_draws() {
    for label in [
        "immutable actor page",
        "actor page upload",
        "actor page view",
    ] {
        for (first, layers, occurrence) in
            [(true, 1, 1), (false, 1, 1), (false, 2, 1), (false, 2, 2)]
        {
            let mut app = fixture();
            let control = publication(1, &[20, 40], 1);
            assert_eq!(draw_frame(&mut app, &control).len(), 2);
            let world = app.sub_app_mut(RenderApp).world_mut();
            let previous = world
                .resource::<ActorGpu>()
                .artwork
                .pages
                .iter()
                .flatten()
                .map(|page| page.view.id())
                .collect::<Vec<_>>();
            if first {
                let mut artwork = GpuArtwork::default();
                assert!(artwork.prepare(
                    &render_model::ActorArtworkPages::default(),
                    world.resource::<RenderDevice>(),
                    world.resource::<RenderQueue>(),
                ));
                world.resource_mut::<ActorGpu>().artwork = artwork;
            }
            let compatible = !first && layers == 1;
            let markers: &[u8] = if compatible || occurrence == 2 {
                &[60, 80]
            } else {
                &[60]
            };
            let next = publication(2, markers, layers);
            let (fault, calls) = fail_at(label, occurrence);
            let attempts = if compatible || label == "immutable actor page" {
                1
            } else {
                let page = &next.artwork.pages[0];
                page.width.max(page.height).ilog2() + 1
            };
            for attempt in 0..attempts {
                let previous_calls = calls.borrow().len();
                let drawn = draw_frame(&mut app, &next);
                assert!(
                    calls.borrow()[previous_calls..]
                        .iter()
                        .any(|owner| owner == label),
                    "{label}: each reduced size must reach the failing boundary"
                );
                let expected = if compatible {
                    vec![1, 2]
                } else if attempt + 1 < attempts {
                    // Earlier pages can draw while the failed page retries across frames.
                    (1..occurrence as u16).collect()
                } else {
                    Vec::new()
                };
                assert_eq!(
                    drawn.iter().map(|span| span.page).collect::<Vec<_>>(),
                    expected,
                    "{label}: incomplete pages cannot draw; exhausted demand releases every page"
                );
                if compatible {
                    let gpu = app.sub_app(RenderApp).world().resource::<ActorGpu>();
                    assert_eq!(
                        gpu.artwork
                            .pages
                            .iter()
                            .flatten()
                            .map(|page| page.view.id())
                            .collect::<Vec<_>>(),
                        previous
                    );
                }
            }
            assert!(
                calls
                    .borrow()
                    .iter()
                    .filter(|owner| owner.as_str() == label)
                    .count()
                    >= occurrence
            );
            if label == "immutable actor page" {
                let page = &next.artwork.pages[0];
                let expected = if compatible {
                    next.artwork.pages.len()
                } else {
                    occurrence + page.width.max(page.height).ilog2() as usize
                };
                assert_eq!(
                    calls
                        .borrow()
                        .iter()
                        .filter(|owner| owner.as_str() == label)
                        .count(),
                    expected,
                    "texture creation exhausts every size synchronously"
                );
            }
            if label == "immutable actor page" && occurrence == 1 {
                assert!(
                    !calls
                        .borrow()
                        .iter()
                        .any(|owner| owner == "actor page upload"),
                    "failed textures must never receive uploads"
                );
            }
            let attempts = calls.borrow().len();
            let released = !compatible && occurrence > 1;
            if released {
                // Exhaustion releases earlier completed pages; retirement permits a retry.
                app.sub_app(RenderApp)
                    .world()
                    .resource::<RenderDevice>()
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
            }
            let _ = draw_frame(&mut app, &next);
            if compatible || released {
                assert!(
                    calls.borrow().len() > attempts,
                    "demanded artwork retries after a fallback or released pages retire"
                );
            } else {
                assert_eq!(
                    calls.borrow().len(),
                    attempts,
                    "exhausted demand must back off without released pages"
                );
            }
            drop(fault);
            let recovered = publication(3, &[100, 120], layers);
            let drawn = draw_frame(&mut app, &recovered);
            assert_eq!(
                drawn
                    .iter()
                    .map(|span| (span.page, span.count))
                    .collect::<Vec<_>>(),
                [(1, 1), (2, 1)]
            );
            let gpu = app.sub_app(RenderApp).world().resource::<ActorGpu>();
            assert_eq!(
                gpu.artwork.pages[0]
                    .as_ref()
                    .unwrap()
                    .view
                    .texture()
                    .depth_or_array_layers(),
                layers
            );
            assert_ne!(gpu.artwork.page(0).unwrap().view.id(), previous[0]);
        }
    }
}

#[test]
fn demanded_artwork_burst_draws_complete_appearances_in_its_first_frame() {
    let mut app = fixture();
    let mut frame = publication(1, &[20, 40, 60, 80, 100], 1);
    let entries = Arc::make_mut(&mut frame.rig.manifest);
    entries[4].identity = entries[0].identity;
    entries[4].identity.layer = render_model::ACTOR_LAYER_BODY + 1;
    let first = draw_frame(&mut app, &frame);
    assert_eq!(
        first.iter().map(|span| span.page).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5]
    );
}

#[test]
fn one_cold_artwork_page_retains_existing_page_bindings() {
    let mut app = fixture();
    let control = publication(1, &[20], 1);
    assert_eq!(draw_frame(&mut app, &control).len(), 1);
    let previous = app
        .sub_app(RenderApp)
        .world()
        .resource::<ActorGpu>()
        .artwork
        .page(0)
        .unwrap()
        .bind_group
        .as_ref()
        .unwrap()
        .id();
    let mut next = publication(2, &[20, 40], 1);
    next.rig.geometry_revision = control.rig.geometry_revision;
    Arc::make_mut(&mut Arc::make_mut(&mut next.artwork).pages)[0] =
        control.artwork.pages[0].clone();
    assert_eq!(draw_frame(&mut app, &next).len(), 2);
    let gpu = app.sub_app(RenderApp).world().resource::<ActorGpu>();
    assert_eq!(
        gpu.artwork
            .page(0)
            .unwrap()
            .bind_group
            .as_ref()
            .unwrap()
            .id(),
        previous
    );
}

#[test]
fn gpu_fault_matrix_buffers_skip_partial_publications_and_recover_draws() {
    for label in [
        "shared actor rig vertices",
        "actor rig transfer",
        "shared actor rig geometry spans",
        "actor instance and bone upload",
        "actor bindings",
    ] {
        for first in [true, false] {
            let mut app = fixture();
            let control = publication(1, &[20], 1);
            assert_eq!(draw_frame(&mut app, &control).len(), 1);
            if first {
                let world = app.sub_app_mut(RenderApp).world_mut();
                let gpu = create_actor_gpu(
                    world.resource::<RenderDevice>(),
                    world.resource::<RenderQueue>(),
                )
                .unwrap();
                world.insert_resource(gpu);
            }
            let mut next = publication(2, &[60], 1);
            let previous = app
                .sub_app(RenderApp)
                .world()
                .resource::<ActorGpu>()
                .geometry_vertices
                .buffer()
                .map(|buffer| buffer.id());
            // Force replacement growth rather than rewriting the retained geometry allocation.
            next.rig.geometry_vertices = render_model::ActorRigVertexSegments::from_vertices(
                vec![render_model::ActorRigVertex::default(); 64],
            );
            let (fault, calls) = fail_at(label, 1);
            assert!(draw_frame(&mut app, &next).is_empty(), "{label}");
            assert!(calls.borrow().iter().any(|owner| owner == label), "{label}");
            if label == "shared actor rig vertices" {
                assert!(
                    !calls
                        .borrow()
                        .iter()
                        .any(|owner| owner == "actor rig transfer")
                );
                assert_eq!(
                    app.sub_app(RenderApp)
                        .world()
                        .resource::<ActorGpu>()
                        .geometry_vertices
                        .buffer()
                        .map(|buffer| buffer.id()),
                    previous,
                    "a failed replacement keeps the previous buffer"
                );
            }
            if label == "shared actor rig geometry spans" {
                assert_ne!(
                    app.sub_app(RenderApp)
                        .world()
                        .resource::<ActorGpu>()
                        .geometry_vertices
                        .buffer()
                        .map(|buffer| buffer.id()),
                    previous,
                    "the first buffer replacement succeeded before the second failed"
                );
            }
            drop(fault);
            let recovered = publication(3, &[100], 1);
            assert_eq!(draw_frame(&mut app, &recovered).len(), 1, "{label}");
            assert_eq!(app.sub_app(RenderApp).world().resource::<Frames>().0, 3);
        }
    }
}
