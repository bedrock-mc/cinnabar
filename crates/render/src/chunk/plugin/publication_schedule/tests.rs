use super::*;
use bevy::render::Render;

#[derive(Resource, Default)]
struct FramePublication {
    generation: u32,
    stages: Vec<&'static str>,
    water_ranges: Vec<std::ops::Range<u32>>,
    queued: Vec<std::ops::Range<u32>>,
    queued_generation: u32,
    drawn_frames: u32,
}

#[derive(Component)]
struct PublishedAllocation(u32);

fn attributes(mut frame: ResMut<FramePublication>) {
    frame.stages.clear();
    frame.stages.push("attributes");
}

fn geometry(mut commands: Commands, mut frame: ResMut<FramePublication>) {
    assert_eq!(frame.stages, ["attributes"]);
    frame.generation += 1;
    frame.stages.push("geometry");
    commands.spawn(PublishedAllocation(frame.generation));
}

fn liquids(allocations: Query<&PublishedAllocation>, mut frame: ResMut<FramePublication>) {
    assert_eq!(frame.stages, ["attributes", "geometry"]);
    assert!(
        allocations
            .iter()
            .any(|allocation| allocation.0 == frame.generation)
    );
    // Successive snapshots deliberately have different partitions. Queuing the
    // old ranges before this publication would address another chunk's faces.
    let split = frame.generation;
    frame.water_ranges = vec![0..split, split..split + 3];
    frame.stages.push("liquids");
}

fn models(mut frame: ResMut<FramePublication>) {
    assert_eq!(frame.stages, ["attributes", "geometry", "liquids"]);
    frame.stages.push("models");
}

fn queue(mut frame: ResMut<FramePublication>) {
    assert_eq!(
        frame.stages,
        ["attributes", "geometry", "liquids", "models"]
    );
    frame.queued = frame.water_ranges.clone();
    frame.queued_generation = frame.generation;
}

fn draw(mut frame: ResMut<FramePublication>) {
    assert_eq!(frame.queued_generation, frame.generation);
    assert_eq!(frame.queued, frame.water_ranges);
    frame.drawn_frames += 1;
}

#[test]
fn chunk_publication_and_deferred_allocations_are_stable_from_queue_through_draw() {
    let mut schedule = Render::base_schedule();
    configure_chunk_publication(&mut schedule);
    schedule.add_systems((
        attributes.in_set(ChunkPublicationStage::Attributes),
        geometry.in_set(ChunkPublicationStage::Geometry),
        liquids.in_set(ChunkPublicationStage::LiquidSort),
        models.in_set(ChunkPublicationStage::ModelSort),
        queue.in_set(RenderSystems::Queue),
        draw.in_set(RenderSystems::Render),
    ));
    let mut world = World::new();
    world.init_resource::<FramePublication>();
    for _ in 0..4 {
        schedule.run(&mut world);
    }
    assert_eq!(world.resource::<FramePublication>().drawn_frames, 4);
}
