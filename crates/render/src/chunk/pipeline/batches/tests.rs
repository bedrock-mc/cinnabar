use bevy::camera::RenderTarget;

use super::*;
use crate::chunk::gpu_cull::app_tests::{
    KEYS, camera_transform, chunk_app, frame, insert_meshes, noop_render_plugin,
};

fn class(entities: &[Entity]) -> RenderVisibleEntitiesClass {
    RenderVisibleEntitiesClass {
        entities_cpu_culling: entities
            .iter()
            .map(|&entity| (entity, MainEntity::from(entity)))
            .collect(),
        ..default()
    }
}

/// Runs one queue of `view` over `visible` after `changed` allocations; returns how many
/// sub-chunks the view admitted again.
fn queue(
    retained: &mut RetainedTerrainBatches,
    view: Entity,
    visible: &RenderVisibleEntitiesClass,
    changed: &[Entity],
    reusable: bool,
) -> usize {
    for &entity in changed {
        retained
            .changes
            .record(entity, Some(MainEntity::from(entity)));
    }
    let changes = retained.begin_frame();
    let mut admitted = 0;
    retained
        .refresh_view(view, Some(visible), &changes, reusable)
        .drawable(|_, _| {
            admitted += 1;
            None
        });
    admitted
}

#[test]
fn a_still_view_reuses_its_lists_until_its_visible_terrain_changes() {
    let mut world = World::new();
    let [a, b, outside, view] = [(); 4].map(|_| world.spawn_empty().id());
    let mut retained = RetainedTerrainBatches::default();
    let visible = class(&[a, b]);
    assert_eq!(queue(&mut retained, view, &visible, &[], true), 2);
    assert_eq!(queue(&mut retained, view, &visible, &[], true), 0);
    assert_eq!(
        queue(&mut retained, view, &visible, &[outside], true),
        0,
        "an allocation the view does not see leaves its lists alone"
    );
    assert_eq!(queue(&mut retained, view, &visible, &[a], true), 2);
    let grown = class(&[a, b, outside]);
    assert_eq!(queue(&mut retained, view, &grown, &[], true), 3);
    retained.changes.invalidate_all();
    assert_eq!(queue(&mut retained, view, &grown, &[], true), 3);
    assert_eq!(
        retained.stats,
        TerrainBatchStats {
            lists_rebuilt: 4,
            lists_retained: 2,
            ..default()
        }
    );
}

#[test]
fn probe_frames_and_their_successors_admit_every_sub_chunk_again() {
    let mut world = World::new();
    let [a, b, view] = [(); 3].map(|_| world.spawn_empty().id());
    let mut retained = RetainedTerrainBatches::default();
    let visible = class(&[a, b]);
    assert_eq!(queue(&mut retained, view, &visible, &[], true), 2);
    assert_eq!(queue(&mut retained, view, &visible, &[], false), 2);
    assert_eq!(queue(&mut retained, view, &visible, &[], true), 2);
    assert_eq!(queue(&mut retained, view, &visible, &[], true), 0);
}

#[test]
fn views_not_queued_in_the_latest_run_are_forgotten() {
    let mut world = World::new();
    let [a, first, second] = [(); 3].map(|_| world.spawn_empty().id());
    let mut retained = RetainedTerrainBatches::default();
    let visible = class(&[a]);
    assert_eq!(queue(&mut retained, first, &visible, &[], true), 1);
    assert_eq!(queue(&mut retained, second, &visible, &[], true), 1);
    assert_eq!(
        queue(&mut retained, first, &visible, &[], true),
        1,
        "a skipped view may have missed changes"
    );
}

#[test]
fn indirect_cubes_sort_again_only_for_a_moved_view_and_requeue_only_for_a_new_order() {
    let mut world = World::new();
    let [near, far] = [(); 2].map(|_| world.spawn_empty().id());
    let terrain = |entity, x| DrawableTerrain {
        entity,
        main: MainEntity::from(entity),
        key: SubChunkKey::new(0, x, 0, 0),
        solid: false,
        cutout: false,
        model: false,
        depth_liquid: false,
    };
    let mut lists = ViewTerrainLists::default();
    lists
        .source
        .extend([near, far].map(|entity| (entity, MainEntity::from(entity))));
    lists.drawable(|entity, _| Some(terrain(entity, if entity == near { 0 } else { 2 })));
    let pose = |transform: Transform| {
        let affine = transform.compute_affine();
        (
            Mat4::from(affine),
            ViewRangefinder3d::from_world_from_view(&affine),
        )
    };
    let east = |x| Transform::from_xyz(x, 8.0, 8.0).looking_at(Vec3::new(64.0, 8.0, 8.0), Vec3::Y);

    let (matrix, rangefinder) = pose(east(-16.0));
    let first = lists.indirect_lists(matrix, &rangefinder, false);
    assert_eq!(first.visible.as_deref(), Some(&[near, far][..]));
    assert_eq!(first.cubes.as_deref(), Some(&[near, far][..]));
    let still = lists.indirect_lists(matrix, &rangefinder, false);
    assert!(still.visible.is_none() && still.cubes.is_none() && !still.sorted);

    let (matrix, rangefinder) = pose(east(-15.0));
    let moved = lists.indirect_lists(matrix, &rangefinder, false);
    assert!(moved.sorted, "a moved view sorts its cubes again");
    assert!(moved.cubes.is_none(), "an unchanged order keeps the batch");

    let west = Transform::from_xyz(64.0, 8.0, 8.0).looking_at(Vec3::new(-64.0, 8.0, 8.0), Vec3::Y);
    let (matrix, rangefinder) = pose(west);
    let turned = lists.indirect_lists(matrix, &rangefinder, false);
    assert_eq!(turned.cubes.as_deref(), Some(&[far, near][..]));
    assert!(turned.visible.is_none());

    let requeued = lists.indirect_lists(matrix, &rangefinder, true);
    assert!(requeued.visible.is_some() && requeued.cubes.is_some());
}

fn stats(app: &App) -> TerrainBatchStats {
    app.sub_app(RenderApp)
        .world()
        .resource::<RetainedTerrainBatches>()
        .stats
}

#[derive(Resource, Default)]
struct QueuedTerrain(usize);

/// Counts the terrain draws queued into every opaque phase this frame.
fn capture_queued_terrain(
    phases: Res<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    mut queued: ResMut<QueuedTerrain>,
) {
    let functions = draw_functions.read();
    queued.0 = phases
        .0
        .values()
        .flat_map(|phase| phase.non_mesh_items.iter())
        .filter(|((batch, _), _)| {
            super::super::opaque::timing_category(&functions, batch.draw_function)
                != RuntimeStage::GpuOpaqueOther
        })
        .map(|(_, items)| items.entities.len())
        .sum();
}

fn queued_terrain(app: &App) -> usize {
    app.sub_app(RenderApp).world().resource::<QueuedTerrain>().0
}

/// The direct-draw path of Apple GPUs: a settled still view queues its retained lists without
/// admitting a sub-chunk again, and only a change it can see rebuilds them.
#[test]
fn a_still_direct_view_rebuilds_its_lists_only_for_visible_changes() {
    let (mut app, _) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Sample4,
        camera_transform(),
    );
    app.sub_app_mut(RenderApp)
        .init_resource::<QueuedTerrain>()
        .add_systems(
            Render,
            capture_queued_terrain
                .in_set(RenderSystems::Queue)
                .after(crate::chunk::draw::queue_chunks),
        );
    insert_meshes(&mut app, &KEYS);
    for _ in 0..16 {
        frame(&mut app);
    }
    let settled = stats(&app);
    let queued = queued_terrain(&app);
    assert!(queued > 0);
    for _ in 0..4 {
        frame(&mut app);
        assert_eq!(
            queued_terrain(&app),
            queued,
            "retained lists queue the same draws"
        );
    }
    let still = stats(&app);
    assert_eq!(still.lists_rebuilt, settled.lists_rebuilt, "{still:?}");
    assert_eq!(still.lists_retained - settled.lists_retained, 4);

    // Behind the camera: it uploads, but the view never sees it.
    insert_meshes(&mut app, &[SubChunkKey::new(0, -4, 0, -4)]);
    for _ in 0..4 {
        frame(&mut app);
    }
    assert_eq!(stats(&app).lists_rebuilt, still.lists_rebuilt);

    insert_meshes(&mut app, &[SubChunkKey::new(0, 0, 0, 1)]);
    for _ in 0..16 {
        frame(&mut app);
    }
    assert!(stats(&app).lists_rebuilt > still.lists_rebuilt);
    assert!(queued_terrain(&app) > queued);
}

fn spawn_camera(app: &mut App, transform: Transform, order: isize) -> Entity {
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            64,
            48,
            TextureFormat::Rgba8Unorm,
            None,
        ));
    app.world_mut()
        .spawn((
            Camera3d::default(),
            Camera { order, ..default() },
            RenderTarget::Image(image.into()),
            Msaa::Off,
            transform,
        ))
        .id()
}

/// The CPU-planned indirect path: a still view rebuilds no pass, a move inside one facing cell
/// only sorts its cubes again, and crossing a sub-chunk face plane rebuilds only the solid pass.
#[test]
fn a_still_indirect_view_keeps_its_commands_and_a_plane_crossing_rebuilds_only_solid_runs() {
    let (mut app, _) = chunk_app(
        noop_render_plugin(
            WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT | WgpuFeatures::INDIRECT_FIRST_INSTANCE,
        ),
        Msaa::Off,
        camera_transform(),
    );
    // The lowest camera takes the GPU cull, so this one plans its batches on the CPU.
    let secondary = spawn_camera(&mut app, camera_transform(), 1);
    insert_meshes(&mut app, &KEYS);
    for _ in 0..8 {
        frame(&mut app);
    }
    let render_world = app.sub_app(RenderApp).world();
    let solid = render_world.resource::<pipeline::solid::ChunkSolidIndirectBatches>();
    let cutout = render_world.resource::<ChunkIndirectBatches>();
    assert_eq!((solid.0.len(), cutout.0.len()), (1, 1));
    let commands = solid
        .0
        .values()
        .map(|batch| batch.cubes.command_count)
        .sum::<u32>()
        + cutout
            .0
            .values()
            .map(|batch| batch.command_count)
            .sum::<u32>();
    assert!(commands > 0, "the secondary view draws through its batches");

    let settled = stats(&app);
    for _ in 0..4 {
        frame(&mut app);
    }
    let still = stats(&app);
    assert_eq!(still.batches_rebuilt, settled.batches_rebuilt, "{still:?}");
    assert_eq!(still.batches_retained - settled.batches_retained, 16);

    let translate = |app: &mut App, x: f32| {
        app.world_mut()
            .get_mut::<Transform>(secondary)
            .unwrap()
            .translation
            .x = x;
        frame(app);
        stats(app)
    };
    let inside = translate(&mut app, -5.5);
    assert_eq!(inside.batches_rebuilt, still.batches_rebuilt, "{inside:?}");
    assert!(inside.cube_sorts > still.cube_sorts);
    let crossed = translate(&mut app, 1.0);
    assert_eq!(
        crossed.batches_rebuilt - inside.batches_rebuilt,
        1,
        "{crossed:?}"
    );
    assert_eq!(crossed.lists_rebuilt, inside.lists_rebuilt);
}
