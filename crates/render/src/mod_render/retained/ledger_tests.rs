//! Allocation lifetime regressions for signed client-part drawing.
use super::{gpu, pipeline, tests::drawing, *};
use bevy::{ecs::system::RunSystemOnce, render::render_phase::AddRenderCommand};
use mod_render::{GpuLedger, GpuReservation};

/// Installs the real allocation queue with one bundle's geometry and placements.
fn fixture(limit: u64) -> (App, mod_render::retained::Store, GpuLedger) {
    let (mut app, _) = crate::queue_review_support::app();
    let (store, _, _) = drawing();
    let ledger = GpuLedger::new(limit);
    app.init_resource::<gpu::Gpu>()
        .init_resource::<materials::Materials>()
        .init_resource::<clock::AnimationClock>()
        .init_resource::<pipeline::Pipeline>()
        .insert_resource(DrawingFrame {
            connection: 1,
            live_owners: store
                .snapshot()
                .geometry
                .keys()
                .map(|h| (h.owner, h.generation))
                .collect(),
            ..default()
        })
        .init_resource::<render_scene::ModRenderScene>()
        .add_render_command::<bevy::core_pipeline::core_3d::Transparent3d, pipeline::Draw>();
    let mut scene = app
        .world_mut()
        .resource_mut::<render_scene::ModRenderScene>();
    scene.retained = vec![store.snapshot().clone()];
    scene.gpu_reservations = vec![GpuReservation {
        owners: store.snapshot().geometry.keys().map(|h| h.owner).collect(),
        ledger: ledger.clone(),
        primitives: Default::default(),
        passes: Vec::new(),
    }];
    (app, store, ledger)
}

/// Completes all submitted work before inspecting released allocations on the NOOP device.
fn complete(app: &mut App) {
    app.world()
        .resource::<bevy::render::renderer::RenderQueue>()
        .submit([]);
    app.world_mut().run_system_once(gpu::retire).unwrap();
    app.world()
        .resource::<bevy::render::renderer::RenderDevice>()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
}

/// Measures real buffer descriptors rather than the current scene's instance count.
fn allocated(app: &App) -> u64 {
    let gpu = app.world().resource::<gpu::Gpu>();
    gpu.meshes
        .values()
        .map(|mesh| mesh.buffer.size())
        .sum::<u64>()
        + gpu
            .views
            .values()
            .map(|view| view.buffer.size() + view.frame.size())
            .sum::<u64>()
}

#[test]
fn drawing_and_media_admit_against_one_resident_allocation_limit() {
    let (mut app, _, ledger) = fixture(65536);
    app.world_mut().run_system_once(gpu::queue).unwrap();
    complete(&mut app);
    assert_eq!(ledger.used(), allocated(&app));
    let resident = ledger.used();
    assert!(resident > 0);
    let other_geometry = ledger
        .allocate(ledger.limit() - resident - 16, || Some(()))
        .unwrap();
    let device = app
        .world()
        .resource::<bevy::render::renderer::RenderDevice>();
    let media = crate::media::MediaTexture::new(device, [2, 2], 1, Some(&ledger)).unwrap();
    assert_eq!(ledger.used(), ledger.limit());
    assert!(crate::media::MediaTexture::new(device, [1, 1], 2, Some(&ledger)).is_none());
    drop(media);
    assert_eq!(ledger.used(), ledger.limit() - 16);
    drop(other_geometry);
    assert_eq!(ledger.used(), resident);
}

#[test]
fn shrinking_a_scene_retains_its_instance_capacity_and_every_view_is_charged() {
    let (mut app, mut store, ledger) = fixture(65536);
    app.world_mut().run_system_once(gpu::queue).unwrap();
    complete(&mut app);
    let first = ledger.used();
    let second = add_view(&mut app);
    app.world_mut().run_system_once(gpu::queue).unwrap();
    complete(&mut app);
    assert_eq!(app.world().resource::<gpu::Gpu>().views.len(), 2);
    assert_eq!(ledger.used(), allocated(&app));
    assert!(ledger.used() > first);
    let grown = ledger.used();
    store.begin();
    let ids: Vec<_> = store.snapshot().instances.keys().copied().collect();
    for id in ids {
        store.remove(id).unwrap();
    }
    store.commit();
    app.world_mut()
        .resource_mut::<render_scene::ModRenderScene>()
        .retained = vec![store.snapshot().clone()];
    app.world_mut().run_system_once(gpu::queue).unwrap();
    complete(&mut app);
    assert!(
        app.world()
            .resource::<gpu::Gpu>()
            .views
            .values()
            .all(|view| view.uploaded.is_empty())
    );
    assert_eq!(
        ledger.used(),
        grown,
        "a smaller instance count frees no resident capacity"
    );
    app.world_mut().despawn(second);
    app.world_mut().run_system_once(gpu::queue).unwrap();
    assert_eq!(
        ledger.used(),
        grown,
        "submitted view storage remains charged"
    );
    complete(&mut app);
    assert_eq!(ledger.used(), first);
    app.world_mut()
        .resource_mut::<render_scene::ModRenderScene>()
        .retained
        .clear();
    app.world_mut().run_system_once(gpu::queue).unwrap();
    assert!(ledger.used() > 0);
    complete(&mut app);
    assert_eq!(ledger.used(), 0);
}

/// Adds another independently allocated render view to the same bundle scene.
fn add_view(app: &mut App) -> Entity {
    use bevy::render::{
        render_phase::ViewSortedRenderPhases,
        sync_world::MainEntity,
        view::{ExtractedView, RetainedViewEntity},
    };
    let second = app.world_mut().spawn_empty().id();
    let retained = RetainedViewEntity::new(second.into(), None, 0);
    app.world_mut().entity_mut(second).insert((
        MainEntity::from(second),
        Msaa::Sample4,
        crate::render_test_support::camera(false),
        ExtractedView {
            retained_view_entity: retained,
            clip_from_view: Mat4::IDENTITY,
            world_from_view: GlobalTransform::IDENTITY,
            clip_from_world: None,
            target_format: render_scene::SCENE_COLOR_FORMAT,
            viewport: UVec4::new(0, 0, 1, 1),
            color_grading: default(),
            invert_culling: false,
        },
    ));
    app.world_mut()
        .resource_mut::<ViewSortedRenderPhases<bevy::core_pipeline::core_3d::Transparent3d>>()
        .prepare_for_new_frame(retained);
    second
}

#[test]
fn every_view_stays_within_the_signed_reservation() {
    let instance = std::mem::size_of::<render_scene::mod_scene::retained::GpuInstance>();
    let limit = (3 * std::mem::size_of::<mod_render::retained::mesh::GpuVertex>()
        + std::mem::size_of::<gpu::CameraFrame>()
        + instance
        + (2 * instance).next_power_of_two()) as u64;
    let (mut app, _, _) = fixture(limit);
    add_view(&mut app);
    app.world_mut().run_system_once(gpu::queue).unwrap();
    complete(&mut app);
    assert!(
        allocated(&app) <= limit,
        "all resident view allocations share the signed ceiling"
    );
}
