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
    let source = crate::shader_source::standalone(include_str!("drawing.wesl"), &[]);
    crate::shader_test_support::assert_binding_visibility(&source, 0, &pipeline::layout());
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    for (name, expected) in [
        ("Vertex", std::mem::size_of::<mesh::GpuVertex>()),
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
fn drawing() -> (Store, mod_api::drawing::DrawingHandle, Instance) {
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
fn hidden_world_reconciles_meshes_and_releases_view_state() {
    for disconnect in [false, true] {
        let (mut app, view) = crate::queue_review_support::app();
        app.add_schedule(Render::base_schedule());
        crate::device_poll::install(app.main_mut());
        app.init_resource::<gpu::Gpu>()
            .init_resource::<pipeline::Pipeline>()
            .init_resource::<bevy::render::view::ViewUniforms>()
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
