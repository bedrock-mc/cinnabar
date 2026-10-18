//! Player game-mode and ability authority after ordered stream commits.
use super::*;

/// Decodes an ability update for a fixture actor and layer count.
fn ability_update(owner: i64, count: u32) -> protocol::AbilitiesUpdate {
    let mut body = owner.to_le_bytes().to_vec();
    body.extend_from_slice(&[0xff, 0xfe, count as u8]);
    body.resize(body.len() + count as usize * 22, 0);
    protocol::decode_abilities_update(&body).unwrap()
}

#[test]
fn targeted_game_mode_updates_reach_live_hud_and_input_authority_after_fifo_commit() {
    use protocol::{GameModeEvent, GameModeUpdate, PlayerGameMode};
    let (mut app, _) = fixture_app();
    app.world_mut()
        .resource_mut::<PlayerRuntime>()
        .facts
        .publish_bootstrap_game_modes(PlayerGameMode::Survival, PlayerGameMode::Adventure, false);
    let targeted = |actor_unique_id, update| {
        WorldEvent::Ui(UiEvent::PlayerGameMode {
            actor_unique_id,
            tick: 0,
            event: GameModeEvent { update },
        })
    };
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            2,
            targeted(1, GameModeUpdate::Explicit(PlayerGameMode::Creative)),
        )
        .unwrap();
    app.update();
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .player_game_mode(),
        Some(PlayerGameMode::Survival)
    );
    // An update addressed to the runtime ID rather than unique ID must not change the local UI.
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            1,
            targeted(42, GameModeUpdate::Explicit(PlayerGameMode::Spectator)),
        )
        .unwrap();
    app.update();
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .player_game_mode(),
        Some(PlayerGameMode::Creative)
    );
    assert!(
        !app.world()
            .resource::<PlayerRuntime>()
            .facts
            .survival_stats_visible()
    );
    assert!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .game_mode_capabilities()
            .unwrap()
            .creative_inventory
    );
    assert!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .game_mode_capabilities()
            .unwrap()
            .can_fly
    );
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(3, targeted(1, GameModeUpdate::Unknown(77)))
        .unwrap();
    app.update();
    let runtime = app.world().resource::<UiRuntimeResource>();
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .player_game_mode(),
        Some(PlayerGameMode::Creative)
    );
    assert_eq!(runtime.gameplay_hud().diagnostics().odd_hud_packets, 1);
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(4, targeted(1, GameModeUpdate::WorldDefault))
        .unwrap();
    app.update();
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .player_game_mode(),
        Some(PlayerGameMode::Adventure)
    );
    assert!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .survival_stats_visible()
    );
    assert!(
        !app.world()
            .resource::<PlayerRuntime>()
            .facts
            .game_mode_capabilities()
            .unwrap()
            .creative_inventory
    );
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            5,
            WorldEvent::Ui(UiEvent::DefaultGameMode(GameModeEvent {
                update: GameModeUpdate::Explicit(PlayerGameMode::Survival),
            })),
        )
        .unwrap();
    app.update();
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .player_game_mode(),
        Some(PlayerGameMode::Survival)
    );
    assert!(app.world().resource::<FatalError>().0.is_none());
}

/// Binds the live player resource to the fixture stream.
fn bind_ability_fixture(app: &mut App) {
    let stream = app
        .world()
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .biome_tint_identity()
        .stream();
    app.world_mut()
        .resource_mut::<PlayerRuntime>()
        .facts
        .bind_local_abilities(1, stream, 1, true);
}

fn prepare_ability_control_fixture(app: &mut App) {
    app.init_resource::<client_runtime::publication::PublicationController>()
        .init_resource::<render_scene::ChunkUploadAcknowledgements>()
        .init_resource::<client_presentation::local_player::LocalAvatarPresentation>()
        .init_resource::<client_interaction::movement::PhysicsAuthorityGate>()
        .insert_resource(crate::runtime::visibility::AppMetrics(
            diagnostics::metrics::MetricsCollector::new(),
        ))
        .insert_resource(client_presentation::camera::AutoFly::new(false));
}

#[test]
fn actual_schedule_commits_ability_fifo_once_without_changing_input_or_forms() {
    let (mut app, _) = fixture_app();
    bind_ability_fixture(&mut app);
    assert!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .local_abilities()
            .is_none()
    );
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(2, WorldEvent::Abilities(ability_update(1, 0)))
        .unwrap();
    app.update();
    assert!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .local_abilities()
            .is_none()
    );
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(1, WorldEvent::Abilities(ability_update(42, 33)))
        .unwrap();
    app.update();
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .local_abilities(),
        Some(&ability_update(1, 0))
    );
    let input = app.world().resource::<SemanticInputSnapshot>().clone();
    app.update();
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .local_abilities(),
        Some(&ability_update(1, 0))
    );
    assert_eq!(
        app.world().resource::<SemanticInputSnapshot>().movement(),
        input.movement()
    );
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .server_forms()
            .active()
            .is_none()
    );
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(3, WorldEvent::Abilities(ability_update(1, 33)))
        .unwrap();
    app.update();
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .local_abilities(),
        Some(&ability_update(1, 33))
    );
}

#[test]
fn actual_drain_cannot_repopulate_after_fatal_transfer_or_missing_stream() {
    for case in 0..3 {
        let (mut app, _) = fixture_app();
        bind_ability_fixture(&mut app);
        app.world_mut()
            .resource_mut::<ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .submit(1, WorldEvent::Abilities(ability_update(1, 0)))
            .unwrap();
        app.update();
        assert!(
            app.world()
                .resource::<PlayerRuntime>()
                .facts
                .local_abilities()
                .is_some()
        );
        app.world_mut()
            .resource_mut::<ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .submit(2, WorldEvent::Abilities(ability_update(1, 33)))
            .unwrap();
        match case {
            0 => app.world_mut().resource_mut::<FatalError>().0 = Some("fixture failure".into()),
            1 => {
                app.world_mut()
                    .resource_mut::<ClientWorld>()
                    .transfer_notice = Some(crate::runtime::world::TransferNotice {
                    host: "127.0.0.1".into(),
                    port: 19132,
                })
            }
            _ => app.world_mut().resource_mut::<ClientWorld>().stream = None,
        }
        // Exercise the actual drain alone so the unrelated transfer follower cannot reconnect.
        let mut schedule = Schedule::default();
        schedule.add_systems(drain_committed_ui_before_authority);
        schedule.run(app.world_mut());
        assert!(
            app.world()
                .resource::<PlayerRuntime>()
                .facts
                .local_abilities()
                .is_none()
        );
        {
            app.world_mut().resource_mut::<FatalError>().0 = None;
            app.world_mut()
                .resource_mut::<ClientWorld>()
                .transfer_notice = None;
        }
        schedule.run(app.world_mut());
        assert!(
            app.world()
                .resource::<PlayerRuntime>()
                .facts
                .local_abilities()
                .is_none(),
            "retired binding cannot be reminted"
        );
    }
}

#[test]
fn actual_drain_retains_session_scoped_evidence_across_dimension_but_rejects_old_incarnation() {
    let (mut app, _) = fixture_app();
    bind_ability_fixture(&mut app);
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(1, WorldEvent::Abilities(ability_update(1, 0)))
        .unwrap();
    app.update();
    submit_transition(&mut app, 2, 1);
    app.update();
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .local_abilities(),
        Some(&ability_update(1, 0))
    );
    app.world_mut().resource_mut::<ClientWorld>().stream =
        Some(chunk_pipeline::WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 42,
            local_player_unique_id: 1,
            player_position: [0.0, 70.0, 0.0],
            world_spawn_position: [0, 70, 0],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        }));
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(1, WorldEvent::Abilities(ability_update(1, 33)))
        .unwrap();
    app.update();
    assert!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .local_abilities()
            .is_none()
    );
}

#[test]
fn actual_terminal_control_then_drain_retires_queued_abilities_without_rearming() {
    use {
        crate::runtime::network::{receive_network_events, session::NetworkControlEvent},
        client_session::{NetworkFailureOrigin, SessionTransferTarget},
    };
    let terminals = [
        NetworkControlEvent::Stopped {
            decode_error_count: 0,
        },
        NetworkControlEvent::Failed {
            message: "fixture failure".into(),
            decode_error_count: 0,
            server_disconnect: None,
            origin: NetworkFailureOrigin::Receive,
        },
        NetworkControlEvent::Transferred {
            target: SessionTransferTarget {
                host: "127.0.0.1".into(),
                port: 19132,
            },
            decode_error_count: 0,
        },
    ];
    for terminal in terminals {
        let (mut app, _) = fixture_app();
        prepare_ability_control_fixture(&mut app);
        bind_ability_fixture(&mut app);
        app.world_mut()
            .resource_mut::<ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .submit(1, WorldEvent::Abilities(ability_update(1, 0)))
            .unwrap();
        app.update();
        assert!(
            app.world()
                .resource::<PlayerRuntime>()
                .facts
                .local_abilities()
                .is_some()
        );
        app.world_mut()
            .resource_mut::<ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .submit(2, WorldEvent::Abilities(ability_update(1, 33)))
            .unwrap();
        let (handle, sender) = NetworkHandle::<PackApplication>::stub_with_control_sender();
        sender.try_send(terminal).unwrap();
        app.insert_resource(handle);
        // Use real control-before-drain functions; do not run the external transfer follower.
        let mut schedule = Schedule::default();
        schedule.add_systems((receive_network_events, drain_committed_ui_before_authority).chain());
        schedule.run(app.world_mut());
        assert!(
            app.world()
                .resource::<PlayerRuntime>()
                .facts
                .local_abilities()
                .is_none()
        );
        assert!(
            app.world_mut()
                .resource_mut::<ClientWorld>()
                .stream
                .as_mut()
                .unwrap()
                .take_committed_ui()
                .is_empty()
        );
    }
}

#[test]
fn prebinding_queue_waits_for_identity_and_old_terminal_receiver_cannot_clear_new_evidence() {
    let (mut app, _) = fixture_app();
    // This models deferred play ingress already queued after a completed bootstrap,
    // not a claim that StartGame contains an ability snapshot.
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(1, WorldEvent::Abilities(ability_update(1, 0)))
        .unwrap();
    bind_ability_fixture(&mut app);
    app.update();
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .local_abilities(),
        Some(&ability_update(1, 0))
    );
    let (old_handle, sender) = NetworkHandle::<PackApplication>::stub_with_control_sender();
    sender
        .try_send(
            crate::runtime::network::session::NetworkControlEvent::Stopped {
                decode_error_count: 0,
            },
        )
        .unwrap();
    drop(old_handle);
    assert!(sender.is_closed());
    app.insert_resource(NetworkHandle::<PackApplication>::disconnected());
    app.update();
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .local_abilities(),
        Some(&ability_update(1, 0))
    );
}

#[test]
fn actual_stale_bootstrap_is_noop_but_current_failed_setup_retires_ability_evidence() {
    use crate::runtime::network::{receive_network_events, session::NetworkControlEvent};
    for generation in [0, 1, 2] {
        let (mut app, _) = fixture_app();
        prepare_ability_control_fixture(&mut app);
        bind_ability_fixture(&mut app);
        app.world_mut()
            .resource_mut::<ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .submit(1, WorldEvent::Abilities(ability_update(1, 0)))
            .unwrap();
        app.update();
        let (handle, sender) = NetworkHandle::<PackApplication>::stub_with_control_sender();
        let custom_blocks = protocol::CustomBlocks::default();
        let base = crate::runtime::network::session_registries::RegistryBase::new(
            &app.world().resource::<ClientWorld>().runtime_assets,
            app.world().resource::<PhysicsCollisionRegistries>(),
        );
        let packs = crate::runtime::network::session_registries::tests::prepared_packs(
            base,
            simulation_clock::identity::SessionGeneration::new(generation),
            &custom_blocks,
            false,
        );
        sender
            .try_send(NetworkControlEvent::Bootstrap {
                session_generation: simulation_clock::identity::SessionGeneration::new(generation),
                world: WorldBootstrap {
                    dimension: 0,
                    local_player_runtime_id: 42,
                    local_player_unique_id: 1,
                    player_position: [0.0, 70.0, 0.0],
                    world_spawn_position: [0, 70, 0],
                    air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
                    block_network_ids_are_hashes: false,
                },
                environment: protocol::WorldEnvironmentBootstrap {
                    initial_time: 0,
                    day_cycle_lock_time: -1,
                    daylight_cycle_enabled: true,
                    weather_cycle_enabled: true,
                    rain_level: 0.0,
                    lightning_level: 0.0,
                },
                custom_blocks,
                inventory: protocol::InventoryEvent::SelectedSlot(protocol::SelectedSlotEvent {
                    container: protocol::ContainerIdentity::window(0),
                    slot: 0,
                    select_slot: true,
                }),
                item_registry: None,
                player_game_mode: protocol::PlayerGameMode::Survival,
                world_default_game_mode: protocol::GameModeUpdate::Explicit(
                    protocol::PlayerGameMode::Survival,
                ),
                player_game_mode_uses_world_default: false,
                server_authoritative_block_breaking: true,
                rewind_history_size: 20,
                hardcore: false,
                hud_rules: protocol::HudRules::default(),
                death_rules: protocol::DeathRules::default(),
                crafting_rules: Default::default(),
                packs,
                terrain_before_spawn: true,
                level_identity: None,
            })
            .unwrap();
        app.insert_resource(handle);
        let mut schedule = Schedule::default();
        schedule.add_systems((receive_network_events, drain_committed_ui_before_authority).chain());
        schedule.run(app.world_mut());
        if generation < 2 {
            assert_eq!(
                app.world()
                    .resource::<PlayerRuntime>()
                    .facts
                    .local_abilities(),
                Some(&ability_update(1, 0))
            );
            assert!(app.world().resource::<FatalError>().0.is_none());
        } else {
            assert!(
                app.world()
                    .resource::<PlayerRuntime>()
                    .facts
                    .local_abilities()
                    .is_none()
            );
            assert!(app.world().resource::<FatalError>().0.is_some());
        }
    }
}
