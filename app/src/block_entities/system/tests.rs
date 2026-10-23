use super::*;

#[test]
fn review_first_bell_cue_starts_once() {
    let mut rings = HashMap::new();
    assert_eq!(bell_elapsed(&mut rings, [0; 3], Some(7), 100.0), 0.0);
    prune_bell_rings(&mut rings, 120.0, |_| Some(7));
    assert_eq!(bell_elapsed(&mut rings, [0; 3], Some(7), 120.0), 20.0);
    assert_eq!(bell_elapsed(&mut rings, [0; 3], Some(8), 121.0), 0.0);
}

#[test]
fn review_session_changes_reset_container_animation() {
    let mut runtime = BlockEntityRuntime::new();
    runtime.bind_session(Some((1, 0)));
    runtime
        .lids
        .advance([0; 3], ContainerKind::Chest, true, 1.0);
    runtime.bind_session(Some((2, 0)));
    assert_eq!(
        runtime
            .lids
            .advance([0; 3], ContainerKind::Chest, false, 0.05),
        0.0
    );
}

#[test]
fn light_follows_the_terrain_curve_and_night_transfer_floor() {
    assert!((light_factor(15, 0, 0.0) - 1.0).abs() < 1.0e-6);
    assert!((light_factor(0, 15, 1.0) - 1.0).abs() < 1.0e-6);
    // Full sky light at night is throttled to the transfer floor.
    assert!((light_factor(0, 15, 0.0) - NIGHT_SKY_TRANSFER_FLOOR).abs() < 1.0e-6);
    assert_eq!(light_factor(0, 0, 1.0), 0.0);
    assert!(light_factor(8, 0, 1.0) > light_factor(4, 0, 1.0));
}

#[test]
fn placed_player_skulls_keep_native_light_coordinates_for_the_environment_shader() {
    let kind = BlockEntityKind::Skull(render_model::SkullModel {
        kind: render_model::SkullKind::Player,
        mount: render_model::SkullMount::Floor {
            rotation_degrees: 0.0,
        },
    });
    for (block, sky) in [(0, 0), (0, 15), (12, 0)] {
        for daylight in [0.0, 1.0] {
            assert_eq!(
                model_light(&kind, block, sky, daylight),
                BlockEntityLight::Actor { block, sky }
            );
        }
    }
    assert_eq!(
        model_light(&BlockEntityKind::EndPortal, 0, 15, 1.0),
        1.0.into()
    );
}

#[test]
fn the_viewer_yaw_turns_the_model_front_toward_the_eye() {
    // An eye due north of the block needs no turn; due east needs a quarter turn.
    assert!(yaw_toward(Vec3::new(0.5, 0.0, -5.0), [0, 0, 0]).abs() < 1.0e-3);
    let east = yaw_toward(Vec3::new(5.5, 0.0, 0.5), [0, 0, 0]);
    let front = Mat4::from_rotation_y(east.to_radians()).transform_vector3(Vec3::NEG_Z);
    assert!(front.abs_diff_eq(Vec3::X, 1.0e-4));
}

#[test]
fn map_pixels_unpack_red_first_and_keys_track_revisions() {
    assert_eq!(map_canvas(&[0x4433_2211]), vec![0x11, 0x22, 0x33, 0x44]);
    assert_ne!(map_cache_key(1, 1), map_cache_key(1, 2));
    assert_ne!(map_cache_key(1, 1), map_cache_key(2, 1));
    assert_eq!(map_cache_key(5, 9), map_cache_key(5, 9));
}

/// Bell state must not accumulate for every bell ever seen.
#[test]
fn bell_ring_state_expires_once_the_swing_settles() {
    let mut rings = HashMap::from([
        ([0, 0, 0], (1, 100.0)),
        ([1, 0, 0], (2, f64::NEG_INFINITY)),
        ([2, 0, 0], (3, 80.0)),
    ]);
    prune_bell_rings(&mut rings, 101.0, |_| None);
    assert_eq!(rings.keys().copied().collect::<Vec<_>>(), vec![[0, 0, 0]]);
}

/// A runtime id cached in one session must not resolve blocks in the next.
#[test]
fn session_change_drops_runtime_id_keyed_caches() {
    let mut runtime = BlockEntityRuntime::new();
    runtime.bind_session(Some((1, 0)));
    runtime.blocks.insert(7, None);
    runtime.shapes.shape(
        &Arc::new(assets::RuntimeAssets::diagnostic()),
        assets::NetworkIdMode::Sequential,
        Some(7),
        [0; 3],
        None,
    );
    runtime.bell_rings.insert([0, 0, 0], (1, 0.0));
    runtime.bind_session(Some((1, 0)));
    assert_eq!(runtime.blocks.len(), 1);
    runtime.bind_session(Some((2, 0)));
    assert!(runtime.blocks.is_empty() && runtime.shapes.is_empty());
    assert!(runtime.bell_rings.is_empty());
}

#[test]
fn framed_map_wire_colors_reach_the_rgba_canvas() {
    let expected = [
        [0x80, 0x80, 0x50, 0xff],
        [0x11, 0x22, 0x33, 0x44],
        [0x12, 0x34, 0x56, 0],
    ];
    let protocol::WorldEvent::MapData(event) =
        protocol::test_support::packets::map_pixel_row(266, &expected)
    else {
        panic!("valid map pixels must reach the frame");
    };
    assert_eq!(
        map_canvas(&event.pixels),
        expected.concat(),
        "framed map must preserve wire colors and alpha"
    );
}

/// Supplies a world session without installing optional block-entity artwork or its renderer.
fn map_request_world() -> ClientWorld {
    ClientWorld {
        stream: Some(chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 42,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        })),
        ..ClientWorld::default()
    }
}

#[test]
fn accepted_map_requests_stay_known_until_the_session_changes() {
    let (network, mut packets) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    let mut runtime = BlockEntityRuntime::new();
    runtime.missing_maps.push(266);
    let mut app = App::new();
    app.insert_resource(runtime)
        .insert_resource(network)
        .init_resource::<Time<Real>>()
        .insert_resource(map_request_world())
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .add_systems(Update, request_missing_maps);
    app.update();
    assert_eq!(packets.drain().len(), 1);
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(std::time::Duration::from_secs(60));
    app.world_mut()
        .resource_mut::<BlockEntityRuntime>()
        .missing_maps
        .push(266);
    app.update();
    assert!(packets.drain().is_empty(), "pending maps do not time out");
    *app.world_mut().resource_mut::<ClientWorld>() = map_request_world();
    app.world_mut()
        .resource_mut::<BlockEntityRuntime>()
        .missing_maps
        .push(266);
    app.update();
    assert_eq!(
        packets.drain().len(),
        1,
        "a new session owns a new map table without block-entity artwork"
    );
    let session = app
        .world()
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_session_id();
    {
        let mut runtime = app.world_mut().resource_mut::<BlockEntityRuntime>();
        runtime.bind_session(Some((session, 1)));
        runtime.missing_maps.push(266);
    }
    app.update();
    assert!(
        packets.drain().is_empty(),
        "dimension changes retain known maps"
    );
}

#[test]
fn a_full_output_queue_does_not_mark_a_map_request_as_known() {
    let (network, mut packets) =
        NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(1);
    network
        .send_inventory_packet(protocol::map_info_request_packet(1))
        .unwrap();
    let mut runtime = BlockEntityRuntime::new();
    runtime.missing_maps.push(266);
    let mut app = App::new();
    app.insert_resource(runtime)
        .insert_resource(network)
        .insert_resource(map_request_world())
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .add_systems(Update, request_missing_maps);
    app.update();
    assert_eq!(
        packets.drain().len(),
        1,
        "only the earlier packet was admitted"
    );
    app.world_mut()
        .resource_mut::<BlockEntityRuntime>()
        .missing_maps
        .push(266);
    app.update();
    assert_eq!(
        packets.drain().len(),
        1,
        "the refused map can retry admission"
    );
    app.world_mut()
        .resource_mut::<BlockEntityRuntime>()
        .missing_maps
        .push(266);
    app.update();
    assert!(
        packets.drain().is_empty(),
        "accepted requests stay deduplicated"
    );
}
