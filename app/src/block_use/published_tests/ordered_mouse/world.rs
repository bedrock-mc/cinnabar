//! Retained placement evidence includes every world column read before transport refusal.
use super::*;

const CLICKED: [i32; 3] = [15, 2, 14];
const DESTINATION: [i32; 3] = [16, 2, 14];
const EYE: [f32; 3] = [18.5, 2.5, 16.5];
const NETWORK_POSITION: [f32; 3] = [18.5, 2.620_01, 16.5];

/// Resolves fixture blocks from the same pinned registry as the production collision owner.
fn block_id(name: &str) -> u32 {
    assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    .find(|record| record.name.as_ref() == name)
    .unwrap()
    .sequential_id
}

/// Commits authoritative terrain through the real stream ingress before returning.
fn world_event(world: &mut World, event: protocol::WorldEvent) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream.submit(sequence, event).unwrap();
    test_time::eventually_within(Duration::from_secs(5), "boundary terrain ingress", || {
        stream.poll(NETWORK_POSITION, 0);
        stream.committed_sequence() >= sequence
    });
}

/// Writes one authoritative cell without changing inventory, input or movement authority.
fn update(world: &mut World, position: [i32; 3], block: u32) {
    world_event(
        world,
        protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
            dimension: 0,
            position,
            layer: 0,
            network_id: block,
        }]),
    );
}

/// Reads the published primary cell through the same collision view used for placement.
fn cell(world: &World, position: [i32; 3]) -> Result<u32, sim::WorldQueryError> {
    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_ref().unwrap();
    sim::PaletteWorld::new(
        stream.collision_store(),
        world
            .resource::<PhysicsCollisionRegistries>()
            .registry(stream.network_id_mode()),
        stream.current_dimension(),
        Default::default(),
    )
    .primary_runtime_id(position)
}

/// Reads the destination column's terrain authority independently of mesh publication.
fn destination_revision(world: &World) -> Option<world::ChunkCollisionRevision> {
    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_ref().unwrap();
    stream
        .collision_store()
        .collision_revision(world::ChunkKey::new(
            stream.current_dimension(),
            DESTINATION[0].div_euclid(16),
            DESTINATION[2].div_euclid(16),
        ))
}

/// Reads the held-placement inventory owner, including only already committed local uses.
fn held_count(world: &World) -> u16 {
    let player = world.resource::<client_state::PlayerRuntime>();
    let ui = world.resource::<UiRuntimeResource>();
    let selection = verified_use_selection(player, ui).unwrap();
    let revision = ui
        .inventory_ledger(player)
        .authoritative_slot_revision(selection.slot)
        .unwrap_or(0);
    world
        .resource::<BlockUseRuntime>()
        .inventory
        .predicted_selection(&selection, revision)
        .unwrap_or(selection)
        .item
        .count()
}

/// Loads a corner-crossing ray whose clicked, destination and obstruction columns differ.
fn boundary_fixture() -> (App, client_session::CapturedPackets) {
    let (mut app, mut captured) = app_fixture(8);
    let world = app.world_mut();
    remove_actor(world);
    set_hotbar_stack(world, 1, Some("minecraft:stone"));
    for (x, z) in [(1, 0), (0, 1), (1, 1)] {
        let mut payload = vec![1, 2];
        payload.extend(std::iter::repeat_n(
            0xff,
            protocol::vanilla_dimension_range(0)
                .unwrap()
                .sub_chunk_count
                - 1,
        ));
        payload.push(0);
        world_event(
            world,
            protocol::WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                dimension: 0,
                x,
                z,
                mode: protocol::LevelChunkMode::LimitedRequests { highest: 0 },
                payload,
            }),
        );
    }
    update(world, CLICKED, block_id("minecraft:stone"));
    {
        let mut movement = world.resource_mut::<MovementTicker>();
        movement.reset(7, 101, NETWORK_POSITION);
        movement.set_source(gameplay::movement::MovementSource::Physics);
        let mut sample = gameplay::test_support::survival_mining::completed(102);
        sample.position = NETWORK_POSITION;
        movement.enqueue_completed_physics(sample).unwrap();
    }
    flush_movement(world);
    captured.drain();
    let direction = (Vec3::new(15.5, 2.5, 14.5) - Vec3::from_array(EYE)).normalize();
    let rotation = Quat::from_rotation_arc(Vec3::NEG_Z, direction);
    let origin = frame_origin(world, Vec3::from_array(EYE), rotation);
    let authority = world
        .resource::<MovementTicker>()
        .interaction_authority_identity();
    world
        .resource_mut::<BlockUseRuntime>()
        .retain_pick(&origin, authority);
    world.insert_resource(origin);
    world.insert_resource(LocalViewPose::new(Vec3::from_array(EYE), rotation));
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    assert_eq!(cell(world, DESTINATION).unwrap(), block_id("minecraft:air"));
    (app, captured)
}

/// Admits a selection prefix, then refuses the real placement batch with a full FIFO.
fn refuse_placement(app: &mut App, captured: &mut client_session::CapturedPackets) {
    fill(app, 7);
    next_slot(app.world_mut());
    tap(app.world_mut(), MouseButton::Right);
    frame(app);
    let prefix = captured.drain();
    assert_eq!(
        prefix
            .iter()
            .filter_map(equipment)
            .map(|e| e.selected_slot)
            .collect::<Vec<_>>(),
        [1]
    );
    assert!(
        prefix
            .iter()
            .filter_map(observe_transaction)
            .all(|tx| tx.item_use().is_none())
    );
    assert!(crate::ordered_interactions::pending(app.world()));
    assert_eq!(held_count(app.world()), 64);
    assert_eq!(
        app.world()
            .resource::<Messages<client_audio::audio::LocalBlockCue>>()
            .len(),
        0
    );
}

/// Checks cancellation has no packet, prediction, inventory or cue effects after space returns.
fn assert_cancelled(app: &mut App, captured: &mut client_session::CapturedPackets) {
    let terrain_before_retry = destination_revision(app.world());
    for _ in 0..3 {
        frame(app);
        let packets = captured.drain();
        assert!(
            packets.iter().all(|p| equipment(p).is_none()),
            "accepted selection must not replay"
        );
        assert!(
            packets
                .iter()
                .filter_map(observe_transaction)
                .all(|tx| tx.item_use().is_none()),
            "obsolete placement must not be admitted or retargeted"
        );
        assert_eq!(
            destination_revision(app.world()),
            terrain_before_retry,
            "cancelled placement must not mutate destination terrain"
        );
    }
    assert!(!crate::ordered_interactions::pending(app.world()));
    assert_eq!(held_count(app.world()), 64);
    assert_eq!(
        app.world()
            .resource::<Messages<client_audio::audio::LocalBlockCue>>()
            .len(),
        0
    );
}

#[test]
fn ordered_mouse_world_unchanged_boundary_placement_retries_once_with_original_pose() {
    let (mut app, mut captured) = boundary_fixture();
    refuse_placement(&mut app, &mut captured);
    let mut sent = Vec::new();
    for _ in 0..3 {
        frame(&mut app);
        sent.extend(captured.drain());
    }
    let uses: Vec<_> = sent
        .iter()
        .filter_map(observe_transaction)
        .filter_map(|tx| tx.item_use().cloned())
        .collect();
    assert_eq!(
        uses.iter().map(|use_| use_.action).collect::<Vec<_>>(),
        [
            protocol::test_support::transactions::ItemUseAction::ClickBlock,
            protocol::test_support::transactions::ItemUseAction::ClickAir,
        ],
        "one placement and its existing air-use suffix must each admit once",
    );
    assert_eq!(uses[1].item.count, 63);
    assert_eq!(uses[0].position, CLICKED);
    assert_eq!(uses[0].face, 5);
    assert_eq!(uses[0].from_position, NETWORK_POSITION);
    assert!(sent.iter().all(|p| equipment(p).is_none()));
    assert_eq!(
        cell(app.world(), DESTINATION).unwrap(),
        block_id("minecraft:stone")
    );
    assert_eq!(held_count(app.world()), 63);
}

#[test]
fn ordered_mouse_world_full_placement_cannot_overwrite_a_filled_adjacent_destination() {
    let (mut app, mut captured) = boundary_fixture();
    refuse_placement(&mut app, &mut captured);
    let dirt = block_id("minecraft:dirt");
    update(app.world_mut(), DESTINATION, dirt);
    assert!(
        app.world()
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .prediction_generation(DESTINATION)
            .is_some(),
        "authoritative destination fill must still await mesh publication"
    );
    assert_cancelled(&mut app, &mut captured);
    assert_eq!(cell(app.world(), DESTINATION).unwrap(), dirt);
}

#[test]
fn ordered_mouse_world_full_placement_cancels_when_the_adjacent_column_unloads() {
    let (mut app, mut captured) = boundary_fixture();
    refuse_placement(&mut app, &mut captured);
    app.world_mut()
        .resource_mut::<crate::runtime::world::ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .evict_publication_fixture_column(world::ChunkKey::new(0, 1, 0));
    assert_cancelled(&mut app, &mut captured);
    assert!(matches!(
        cell(app.world(), DESTINATION),
        Err(sim::WorldQueryError::UnloadedChunk(_))
    ));
}

#[test]
fn ordered_mouse_world_full_placement_cancels_for_a_new_obstruction_column() {
    let (mut app, mut captured) = boundary_fixture();
    refuse_placement(&mut app, &mut captured);
    let obstruction = [17, 2, 16];
    let dirt = block_id("minecraft:dirt");
    update(app.world_mut(), obstruction, dirt);
    assert_cancelled(&mut app, &mut captured);
    assert_eq!(cell(app.world(), obstruction).unwrap(), dirt);
    assert_eq!(
        cell(app.world(), DESTINATION).unwrap(),
        block_id("minecraft:air")
    );
}
