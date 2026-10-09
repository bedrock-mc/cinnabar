use super::*;

/// Adds unit hitboxes to the fixture actor in server order.
fn add_hitboxes(world: &mut client_world::WorldAuthority, pivots: &[[f32; 3]]) {
    let mut root = world::NbtCompound::default();
    root.insert(
        "Hitboxes",
        world::NbtValue::List(
            pivots
                .iter()
                .map(|pivot| {
                    let mut entry = world::NbtCompound::default();
                    for (prefix, values) in
                        [("Min", [-0.5; 3]), ("Max", [0.5; 3]), ("Pivot", *pivot)]
                    {
                        for (axis, value) in ["X", "Y", "Z"].into_iter().zip(values) {
                            entry.insert(format!("{prefix}{axis}"), world::NbtValue::Float(value));
                        }
                    }
                    world::NbtValue::Compound(entry)
                })
                .collect(),
        ),
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 2,
                metadata: Arc::from([ActorMetadata {
                    key: client_world::HITBOX_METADATA_KEY,
                    value: ActorMetadataValue::Compound(root.encode_root().unwrap().into()),
                }]),
                properties: Arc::from([]),
                tick: 0,
            })),
            Some(2),
        )
        .unwrap();
}

/// Completes one two-tick search, with block targeting disabled.
fn search(
    frame: &mut AimAssistFrame,
    world: &client_world::WorldAuthority,
    store: &world::ChunkStore,
    registry: &sim::CollisionRegistry,
    first_tick: u64,
) {
    let blocks = sim::PaletteWorld::new(store, registry, 0);
    let mut state = ServerAimAssist::default();
    state.apply(
        1,
        &CameraEvent::AimAssist(settings(CameraAimAssistTargetMode::Angle)),
    );
    for tick in first_tick..first_tick + 2 {
        frame.evaluate(
            &state,
            world,
            &blocks,
            tick,
            Vec3::new(0.25, 0.5, 0.0),
            Vec3::Z,
            None,
            None,
            |_| None,
            |_| &[],
        );
    }
}

#[test]
fn disjoint_hitboxes_never_target_the_gap_between_them() {
    let (mut world, store, registry) = world_fixture();
    add_hitboxes(&mut world, &[[-1.0, 0.5, 0.0], [1.0, 0.5, 0.0]]);
    let mut frame = AimAssistFrame::default();
    search(&mut frame, &world, &store, &registry, 1);
    let target = frame.target.unwrap();
    assert_eq!(target.kind, TargetKind::Actor(2));
    assert!((target.point.x.abs() - 1.0).abs() < 1e-5, "{target:?}");
}

#[test]
fn one_occluded_box_does_not_hide_another_box_on_the_actor() {
    let (mut world, mut store, registry) = world_fixture();
    add_hitboxes(&mut world, &[[0.0, 0.5, 0.0], [-1.5, 0.5, 0.0]]);
    store
        .update_block(
            world::SubChunkKey::new(0, 0, 0, 0),
            world::BlockUpdate::new(0, 0, 1, 0, 1),
            0,
        )
        .unwrap();
    let mut frame = AimAssistFrame::default();
    search(&mut frame, &world, &store, &registry, 1);
    assert_eq!(frame.target.unwrap().point, Vec3::new(-1.5, 0.5, 3.0));
}

#[test]
fn admitted_actor_keeps_all_its_boxes_past_the_candidate_threshold() {
    let (mut world, store, registry) = world_fixture();
    let mut pivots = vec![[1.0, 0.5, 0.0]; 129];
    pivots.push([0.0, 0.5, 0.0]);
    add_hitboxes(&mut world, &pivots);
    let mut frame = AimAssistFrame::default();
    search(&mut frame, &world, &store, &registry, 1);
    assert_eq!(frame.target.unwrap().point, Vec3::new(0.0, 0.5, 3.0));
    let blocks = sim::PaletteWorld::new(&store, &registry, 0);
    let mut state = ServerAimAssist::default();
    state.apply(
        1,
        &CameraEvent::AimAssist(settings(CameraAimAssistTargetMode::Angle)),
    );
    let before = crate::test_allocations::count();
    for tick in 3..5 {
        frame.evaluate(
            &state,
            &world,
            &blocks,
            tick,
            Vec3::new(0.25, 0.5, 0.0),
            Vec3::Z,
            None,
            None,
            |_| None,
            |_| &[],
        );
    }
    assert_eq!(crate::test_allocations::count() - before, 0);
}
