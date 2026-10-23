use super::*;
use bevy::{
    ecs::system::RunSystemOnce,
    render::{render_phase::AddRenderCommand, render_resource::*},
};
use mod_render::retained::*;
use std::sync::Arc;

#[path = "raster_tests.rs"]
mod raster;

#[path = "timing_tests.rs"]
mod timing;

#[test]
fn unlit_shader_layout_and_stages_match_native_storage() {
    let source = materials::source(
        &mod_render::material::module(mod_render::material::BUILTIN),
        include_str!("drawing.wgsl"),
    );
    crate::shader_test_support::assert_binding_visibility(&source, 0, &pipeline::layout());
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    for (name, expected) in [
        ("Vertex", std::mem::size_of::<mesh::GpuVertex>()),
        ("DrawingView", std::mem::size_of::<gpu::CameraFrame>()),
        (
            "Instance",
            std::mem::size_of::<render_scene::mod_scene::retained::GpuInstance>(),
        ),
    ] {
        let (_, ty) = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some(name))
            .unwrap();
        let naga::TypeInner::Struct { span, .. } = &ty.inner else {
            panic!("storage record")
        };
        assert_eq!(*span as usize, expected);
    }
}

/// Builds one triangle referenced by two placements in a fenced drawing store.
pub(super) fn drawing() -> (Store, mod_api::drawing::DrawingHandle, Instance) {
    let mut store = Store::default();
    store.bind_session(1, 0);
    let geometry = store
        .create_geometry(GeometrySpec::Triangles {
            vertices: [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]
                .map(|position| Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            indices: vec![0, 1, 2],
        })
        .unwrap();
    let instance = Instance {
        geometry,
        material: None,
        params: mod_api::materials::Preset::default().0,
        anchor: Anchor::World {
            dimension: 0,
            origin: [0.; 3],
        },
        transform: mod_render::retained::Transform::default(),
        tint: [1.; 4],
        depth: Depth::Test,
        blend: Blend::Premultiplied,
        width: 1.,
        width_mode: WidthMode::World,
        join: Join::Bevel,
        cap: Cap::Butt,
    };
    let id = store
        .create(instance.clone(), mod_api::drawing::DrawingPermissions::ALL)
        .unwrap();
    store
        .create(instance.clone(), mod_api::drawing::DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    (store, id, instance)
}

#[test]
fn moving_instances_reuses_the_uploaded_mesh_and_session_end_releases_it() {
    let (mut app, _) = crate::queue_review_support::app();
    app.init_resource::<gpu::Gpu>()
        .init_resource::<materials::Materials>()
        .init_resource::<clock::AnimationClock>()
        .init_resource::<pipeline::Pipeline>()
        .insert_resource(DrawingFrame {
            connection: 1,
            ..default()
        })
        .init_resource::<render_scene::ModRenderScene>()
        .add_render_command::<bevy::core_pipeline::core_3d::Transparent3d, pipeline::Draw>();
    let (mut store, id, mut instance) = drawing();
    app.world_mut().resource_mut::<DrawingFrame>().live_owners = store
        .snapshot()
        .geometry
        .keys()
        .map(|h| (h.owner, h.generation))
        .collect();
    app.world_mut()
        .resource_mut::<render_scene::ModRenderScene>()
        .retained = vec![store.snapshot().clone()];
    app.world_mut().run_system_once(gpu::queue).unwrap();
    let buffer = app
        .world()
        .resource::<gpu::Gpu>()
        .meshes
        .values()
        .next()
        .unwrap()
        .buffer
        .id();
    assert_eq!(
        app.world()
            .resource::<gpu::Gpu>()
            .views
            .values()
            .next()
            .unwrap()
            .batches
            .len(),
        1
    );
    store.begin();
    instance.transform.translation[0] = 1.;
    store
        .update(id, instance, mod_api::drawing::DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    app.world_mut()
        .resource_mut::<render_scene::ModRenderScene>()
        .retained = vec![store.snapshot().clone()];
    app.world_mut().run_system_once(gpu::queue).unwrap();
    assert_eq!(
        buffer,
        app.world()
            .resource::<gpu::Gpu>()
            .meshes
            .values()
            .next()
            .unwrap()
            .buffer
            .id()
    );
    app.world_mut().resource_mut::<DrawingFrame>().connection = 2;
    app.world_mut().run_system_once(gpu::queue).unwrap();
    assert!(app.world().resource::<gpu::Gpu>().meshes.is_empty());
    assert!(
        app.world()
            .resource::<gpu::Gpu>()
            .views
            .values()
            .all(|view| view.uploaded.is_empty())
    );
}

#[test]
fn every_depth_and_blend_variant_keeps_depth_writes_disabled() {
    let mut pipeline = pipeline::Pipeline::from_world(&mut World::new());
    let (mut app, _) = crate::queue_review_support::app();
    for always in [false, true] {
        for additive in [false, true] {
            let id = pipeline
                .variants
                .specialize(
                    app.world().resource::<PipelineCache>(),
                    pipeline::Key {
                        msaa: Msaa::Off,
                        hdr: false,
                        always,
                        additive,
                    },
                )
                .unwrap();
            let mut cache = app.world_mut().resource_mut::<PipelineCache>();
            let descriptor = crate::queue_review_support::queued_descriptor(&mut cache, id);
            let depth = descriptor.depth_stencil.as_ref().unwrap();
            assert_eq!(depth.depth_write_enabled, Some(false));
            assert_eq!(
                depth.depth_compare,
                Some(if always {
                    CompareFunction::Always
                } else {
                    CompareFunction::GreaterEqual
                })
            );
        }
    }
}

#[test]
fn animation_changes_only_the_frame_uniform() {
    let (mut app, _) = crate::queue_review_support::app();
    app.init_resource::<gpu::Gpu>()
        .init_resource::<materials::Materials>()
        .init_resource::<clock::AnimationClock>()
        .init_resource::<pipeline::Pipeline>()
        .insert_resource(DrawingFrame {
            connection: 1,
            ..default()
        })
        .init_resource::<render_scene::ModRenderScene>()
        .add_render_command::<bevy::core_pipeline::core_3d::Transparent3d, pipeline::Draw>();
    let (store, _, _) = drawing();
    app.world_mut().resource_mut::<DrawingFrame>().live_owners = store
        .snapshot()
        .geometry
        .keys()
        .map(|h| (h.owner, h.generation))
        .collect();
    app.world_mut()
        .resource_mut::<render_scene::ModRenderScene>()
        .retained = vec![store.snapshot().clone()];
    app.world_mut().run_system_once(gpu::queue).unwrap();
    let gpu = app.world().resource::<gpu::Gpu>();
    let mesh_buffer = gpu.meshes.values().next().unwrap().buffer.id();
    let view = gpu.views.values().next().unwrap();
    let instances = view.uploaded.clone();
    let instance_buffer = view.buffer.id();
    app.world_mut()
        .resource_mut::<clock::AnimationClock>()
        .phase = mod_render::material::phase(1.);
    app.world_mut().run_system_once(gpu::queue).unwrap();
    let gpu = app.world().resource::<gpu::Gpu>();
    assert_eq!(mesh_buffer, gpu.meshes.values().next().unwrap().buffer.id());
    let view = gpu.views.values().next().unwrap();
    assert_eq!(instance_buffer, view.buffer.id());
    assert_eq!(instances, view.uploaded);
    assert_eq!(
        view.frame_uploaded.animation[0],
        mod_render::material::phase(1.)
    );
}

#[test]
fn native_presets_animate_without_replacing_geometry() {
    use crate::gpu_snapshot::{self, Draw, Gpu};
    use mod_api::materials::Preset;
    let Some(gpu) = Gpu::for_fixture("procedural drawing materials") else {
        return;
    };
    let mut store = Store::default();
    let geometry = store
        .create_geometry(GeometrySpec::Polyline {
            vertices: [[-0.8, 0., 0.5], [0.8, 0., 0.5]]
                .map(|position| Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            closed: false,
        })
        .unwrap();
    store.commit();
    let vertices = mesh::build(&store.snapshot().geometry[&geometry]);
    let storage = gpu.buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::STORAGE);
    let source = materials::source(
        &mod_render::material::compose(mod_render::material::BUILTIN).unwrap(),
        include_str!("drawing.wgsl"),
    );
    for (name, preset) in [
        ("solid", Preset::default()),
        ("rainbow", Preset::rainbow(1.)),
        ("pulse", Preset::pulse(1., 0.1)),
        ("dashes", Preset::dashed(0.4, 0.5, 1.)),
    ] {
        let instance = render_scene::mod_scene::retained::GpuInstance {
            transform: Mat4::IDENTITY.to_cols_array_2d(),
            tint: [1.; 4],
            line: [32., 1., 0., 0.],
            params: preset.0,
        };
        let instance_buffer = gpu.buffer(
            bytemuck::cast_slice(&[instance]),
            wgpu::BufferUsages::STORAGE,
        );
        let mut images = Vec::new();
        for (label, elapsed) in [("before", 0.), ("after", 0.375)] {
            let frame = gpu::CameraFrame {
                clip: Mat4::IDENTITY.to_cols_array_2d(),
                viewport: [
                    gpu_snapshot::SNAPSHOT_SIDE as f32,
                    gpu_snapshot::SNAPSHOT_SIDE as f32,
                    1.,
                    0.,
                ],
                animation: [mod_render::material::phase(elapsed), 0., 0., 0.],
            };
            let uniform = gpu.buffer(bytemuck::cast_slice(&[frame]), wgpu::BufferUsages::UNIFORM);
            let bindings = [
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: storage.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: instance_buffer.as_entire_binding(),
                },
            ];
            let pixels = gpu.render(
                &source,
                "vertex",
                &[Draw {
                    fragment: "material_fragment",
                    vertices: 0..vertices.len() as u32,
                    bindings: &bindings,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_depth: false,
                }],
            );
            gpu_snapshot::save(&format!("material-{name}-{label}"), &pixels);
            images.push(pixels);
        }
        if name == "solid" {
            assert_eq!(images[0], images[1]);
        } else {
            assert_ne!(images[0], images[1], "{name} must animate from the clock");
        }
    }
}

#[test]
fn hidden_world_reconciles_meshes_and_releases_view_state() {
    for disconnect in [false, true] {
        let (mut app, view) = crate::queue_review_support::app();
        app.add_schedule(Render::base_schedule());
        crate::device_poll::install(app.main_mut());
        app.init_resource::<gpu::Gpu>()
            .init_resource::<pipeline::Pipeline>()
            .init_resource::<materials::Materials>()
            .init_resource::<clock::AnimationClock>()
            .init_resource::<render_scene::panorama::PanoramaScene>()
            .init_resource::<render_scene::ModRenderScene>()
            .insert_resource(DrawingFrame {
                connection: 1,
                ..default()
            })
            .add_render_command::<bevy::core_pipeline::core_3d::Transparent3d, pipeline::Draw>();
        install_systems(app.main_mut());
        let (store, _, _) = drawing();
        let geometry = Arc::downgrade(store.snapshot().geometry.values().next().unwrap());
        app.world_mut().resource_mut::<DrawingFrame>().live_owners = store
            .snapshot()
            .geometry
            .keys()
            .map(|h| (h.owner, h.generation))
            .collect();
        app.world_mut()
            .resource_mut::<render_scene::ModRenderScene>()
            .retained = vec![store.snapshot().clone()];
        app.world_mut().run_schedule(Render);
        assert_eq!(app.world().resource::<gpu::Gpu>().meshes.len(), 1);
        assert!(!app.world().resource::<gpu::Gpu>().views.is_empty());

        app.world_mut()
            .resource_mut::<render_scene::panorama::PanoramaScene>()
            .set_game_visible(false);
        crate::queue_review_support::clear(&mut app, view);
        if disconnect {
            app.world_mut().resource_mut::<DrawingFrame>().connection = 0;
        } else {
            app.world_mut()
                .resource_mut::<DrawingFrame>()
                .live_owners
                .clear();
        }
        app.world_mut().run_schedule(Render);
        assert!(app.world().resource::<gpu::Gpu>().meshes.is_empty());
        assert!(app.world().resource::<gpu::Gpu>().views.is_empty());
        assert!(crate::queue_review_support::items(&app, view).is_empty());

        drop(store);
        app.world_mut()
            .resource_mut::<render_scene::ModRenderScene>()
            .retained
            .clear();
        app.world()
            .resource::<bevy::render::renderer::RenderDevice>()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        assert!(geometry.upgrade().is_none());
    }
}

/// Out of memory draws only the instances that fit and uploads the geometry on a later frame.
#[test]
fn out_of_memory_draws_what_fits_and_uploads_geometry_later() {
    let (mut app, _) = crate::queue_review_support::app();
    app.init_resource::<gpu::Gpu>()
        .init_resource::<materials::Materials>()
        .init_resource::<clock::AnimationClock>()
        .init_resource::<pipeline::Pipeline>()
        .insert_resource(DrawingFrame {
            connection: 1,
            ..default()
        })
        .init_resource::<render_scene::ModRenderScene>()
        .add_render_command::<bevy::core_pipeline::core_3d::Transparent3d, pipeline::Draw>();
    let (store, _, _) = drawing();
    app.world_mut().resource_mut::<DrawingFrame>().live_owners = store
        .snapshot()
        .geometry
        .keys()
        .map(|h| (h.owner, h.generation))
        .collect();
    app.world_mut()
        .resource_mut::<render_scene::ModRenderScene>()
        .retained = vec![store.snapshot().clone()];
    let geometry_attempts = std::rc::Rc::new(std::cell::Cell::new(0));
    {
        let attempts = std::rc::Rc::clone(&geometry_attempts);
        let _failure = crate::gpu_allocation::inject(move |label, _| {
            attempts.set(attempts.get() + usize::from(label == "mod retained geometry"));
            label.starts_with("mod retained")
        });
        app.world_mut().run_system_once(gpu::queue).unwrap();
    }
    // Both placements share one geometry, so its failed allocation is attempted once.
    assert_eq!(geometry_attempts.get(), 1);
    let gpu = app.world().resource::<gpu::Gpu>();
    assert!(gpu.meshes.is_empty());
    let view = gpu.views.values().next().unwrap();
    assert_eq!(view.uploaded.len(), 1);
    assert_eq!(view.batches[0].instances, 0..1);

    app.world_mut().run_system_once(gpu::queue).unwrap();
    let gpu = app.world().resource::<gpu::Gpu>();
    assert_eq!(gpu.meshes.len(), 1);
    let view = gpu.views.values().next().unwrap();
    assert_eq!(view.uploaded.len(), 2);
    assert_eq!(view.batches[0].instances, 0..2);
}

/// Both explicit revocation and disconnect remove session-owned GPU allocations.
#[test]
fn client_part_revoke_and_disconnect_release_uploaded_meshes() {
    for disconnect in [false, true] {
        let (mut app, _) = crate::queue_review_support::app();
        app.init_resource::<gpu::Gpu>()
            .init_resource::<materials::Materials>()
            .init_resource::<clock::AnimationClock>()
            .init_resource::<pipeline::Pipeline>()
            .insert_resource(DrawingFrame {
                connection: 1,
                ..default()
            })
            .init_resource::<render_scene::ModRenderScene>()
            .add_render_command::<bevy::core_pipeline::core_3d::Transparent3d, pipeline::Draw>();
        let (store, _, _) = drawing();
        app.world_mut().resource_mut::<DrawingFrame>().live_owners = store
            .snapshot()
            .geometry
            .keys()
            .map(|id| (id.owner, id.generation))
            .collect();
        let output = mod_render::RenderOutput {
            retained: vec![store.snapshot().clone()],
            ..default()
        };
        app.world_mut()
            .resource_mut::<render_scene::ModRenderScene>()
            .apply_server(&output, 1);
        app.world_mut().run_system_once(gpu::queue).unwrap();
        assert_eq!(app.world().resource::<gpu::Gpu>().meshes.len(), 1);
        if disconnect {
            app.world_mut().resource_mut::<DrawingFrame>().connection = 0;
        } else {
            app.world_mut()
                .resource_mut::<render_scene::ModRenderScene>()
                .apply_server(&default(), 0);
        }
        app.world_mut().run_system_once(gpu::queue).unwrap();
        assert!(app.world().resource::<gpu::Gpu>().meshes.is_empty());
        assert!(
            app.world()
                .resource::<gpu::Gpu>()
                .views
                .values()
                .all(|view| view.uploaded.is_empty())
        );
    }
}
