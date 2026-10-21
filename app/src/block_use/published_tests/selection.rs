use super::*;

#[test]
fn world_backing_recovery_allows_a_real_block_use_transaction() {
    let (mut world, mut captured) = fixture();
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let ledger = ui.inventory_ledger_mut(&mut player);
        ledger.apply(&InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        ));
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(10));
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                slot: 28,
            },
            stack: protocol::NetworkItemStack {
                network_id: 6,
                count: 1,
                stack_network_id: 201,
                ..protocol::NetworkItemStack::empty()
            },
            storage_item: None,
        }));
        assert!(
            ledger.poll_timeout(10 + inventory::inventory_ledger::INVENTORY_REQUEST_TIMEOUT_MILLIS)
        );
        assert!(
            ledger.resync_required(),
            "closed input recovery remains independently retained"
        );
    });
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 2, 6]]);
}

#[test]
fn direct_block_outline_respects_the_nearer_actor() {
    for (actor_z, has_outline) in [(Some(7.5), false), (Some(5.5), true), (None, true)] {
        let (mut world, _) = fixture();
        if let Some(z) = actor_z {
            actor_use_tests::spawn(&mut world, z);
        }
        let origin = frame_origin(&world, Vec3::new(4.5, 2.5, 8.5), Quat::IDENTITY);
        world.insert_resource(origin);
        world.init_resource::<client_state::RuntimeSettings>();
        world.init_resource::<render_scene::BlockSelectionFrameResource>();
        world.init_resource::<crate::block_selection::CrosshairActor>();
        world
            .run_system_cached(crate::block_selection::publish)
            .unwrap();
        assert_eq!(
            world.resource::<crate::block_selection::CrosshairActor>().0,
            if has_outline { None } else { Some(99) }
        );
        let frame = world.resource::<render_scene::BlockSelectionFrameResource>();
        assert_eq!(
            !frame.outline.is_empty(),
            has_outline,
            "actor z={actor_z:?}"
        );
    }
}

#[test]
fn nametag_pick_reuses_eye_ray_reach_occlusion_and_mount_exclusion() {
    for (mode, creative, distance, mount, expected) in [
        (
            semantic_input::InputMode::KeyboardMouse,
            false,
            1.0,
            false,
            Some(99),
        ),
        (
            semantic_input::InputMode::KeyboardMouse,
            false,
            8.0,
            false,
            None,
        ),
        (
            semantic_input::InputMode::Touch,
            false,
            6.2,
            false,
            Some(99),
        ),
        (semantic_input::InputMode::Touch, false, 9.0, false, None),
        (semantic_input::InputMode::Touch, true, 9.0, false, Some(99)),
        (
            semantic_input::InputMode::KeyboardMouse,
            false,
            1.0,
            true,
            None,
        ),
    ] {
        let (mut world, _) = fixture();
        actor_use_tests::spawn(&mut world, 13.5 - distance);
        {
            let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
            let stream = client.stream.as_mut().unwrap();
            let sequence = stream.committed_sequence() + 1;
            stream.submit(sequence, empty_column(0, -1)).unwrap();
            test_time::eventually_within(
                Duration::from_secs(5),
                "creative pick ray terrain",
                || {
                    stream.poll([4.5, 2.5, 13.5], 0);
                    stream.committed_sequence() >= sequence
                },
            );
            stream.predict_block([4, 2, 6], 0, stream.air_block_id());
            if mount {
                stream
                    .submit(
                        stream.committed_sequence() + 1,
                        protocol::WorldEvent::ActorLink(protocol::ActorLinkEvent {
                            dimension: 0,
                            ridden_unique_id: 99,
                            rider_unique_id: stream.authority().local_player_unique_id(),
                            link_type: protocol::ActorLinkType::Rider,
                            immediate: true,
                            rider_initiated: false,
                        }),
                    )
                    .unwrap();
                stream.poll([4.5, 2.5, 13.5], 0);
            }
        }
        if mount {
            world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
                let mut player = world.resource_mut::<client_state::PlayerRuntime>();
                let session = ui.session_id();
                ui.apply_local_mount(&mut player, session, 1, Some(99))
                    .unwrap();
            });
        }
        let mut snapshot = world
            .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
            .route_and_finalize(semantic_input::DeviceFrame {
                keyboard_mouse: Some(semantic_input::KeyboardMouseFrame::default()),
                ..Default::default()
            })
            .unwrap();
        snapshot.input_mode = mode;
        world.insert_resource(client_state::SemanticInputSnapshot(
            semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
        ));
        world
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .publish_player_game_mode(if creative {
                PlayerGameMode::Creative
            } else {
                PlayerGameMode::Survival
            });
        let origin = frame_origin(&world, Vec3::new(4.5, 2.5, 13.5), Quat::IDENTITY);
        world.insert_resource(origin);
        // A third-person camera behind the player must not move the pick origin.
        world.spawn((Camera3d::default(), Transform::from_xyz(4.5, 2.5, 18.5)));
        world.init_resource::<client_state::RuntimeSettings>();
        world.init_resource::<render_scene::BlockSelectionFrameResource>();
        world.init_resource::<crate::block_selection::CrosshairActor>();
        world
            .run_system_cached(crate::block_selection::publish)
            .unwrap();
        assert_eq!(
            world.resource::<crate::block_selection::CrosshairActor>().0,
            expected,
            "mode={mode:?}, creative={creative}, distance={distance}, mount={mount}"
        );
        world
            .resource_mut::<client_state::ScreenFocus>()
            .menu_visible = true;
        world
            .run_system_cached(crate::block_selection::publish)
            .unwrap();
        assert_eq!(
            world.resource::<crate::block_selection::CrosshairActor>().0,
            None
        );
    }
}
