//! Real ordered-stream commit-to-authority witnesses, not network-ingress tests.
use super::*;
use crate::player_runtime::PlayerRuntime;
use crate::{
    acceptance::{AcceptanceRun, model_witness::ModelWitnessFileSource},
    app::{
        ClientBlobCacheOwner, configure_client_authority_systems, configure_client_frame_schedule,
    },
    camera::CameraSettingsAuthority,
    environment::{WeatherState, bind_session_generation},
    local_player::{InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalViewPose},
    menu::{MenuClipboard, MenuRuntime},
    movement::{
        LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
        MovementTicker, PhysicsCollisionRegistries,
    },
    runtime::{
        network::{NetworkHandle, ResourcePackAdmissionState},
        phase3_evidence::Phase3EvidenceEmitter,
    },
    semantic_controls::{
        PendingDeviceFrame, SemanticInputRuntime, SemanticInputSnapshot, SemanticRouteState,
        SemanticTouchTargets,
    },
    settings_runtime::RuntimeSettings,
    ui_runtime::presentation::tests::fixture_font,
};
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
        mouse::AccumulatedMouseMotion,
    },
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use client_presentation::server_camera::ServerCameraInstructions;
use client_ui::ui_runtime::{
    LocalFormAction, flush_form_response, presentation::UiPresentationRuntime,
};
use protocol::{
    FormKind, FormRequestEvent, ServerFormModel, TextMenuForm, UiEvent, WorldBootstrap, WorldEvent,
};
use render::ChunkUploadBudget;
use semantic_input::Action;
use std::sync::Arc;

mod boss_lifetime;
mod credits_identity;

#[derive(Clone, Copy, Debug)]
enum InputCase {
    Move,
    Attack,
    Use,
    Chat,
    Inventory,
    Pause,
}

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
    let runtime = app.world().resource::<UiRuntime>();
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
    assert!(app.world().resource::<ClientWorld>().fatal_error.is_none());
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
    app.init_resource::<crate::runtime::publication::PublicationController>()
        .init_resource::<render::ChunkUploadAcknowledgements>()
        .init_resource::<crate::local_player::LocalAvatarPresentation>()
        .init_resource::<crate::movement::PhysicsAuthorityGate>()
        .insert_resource(crate::runtime::visibility::AppMetrics(
            diagnostics::metrics::MetricsCollector::new(),
        ))
        .insert_resource(crate::camera::AutoFly::new(false));
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
            .resource::<UiRuntime>()
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
            0 => {
                app.world_mut().resource_mut::<ClientWorld>().fatal_error =
                    Some("fixture failure".into())
            }
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
            let mut world = app.world_mut().resource_mut::<ClientWorld>();
            world.fatal_error = None;
            world.transfer_notice = None;
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
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
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
    use crate::runtime::network::{
        NetworkControlEvent, NetworkFailureOrigin, SessionTransferTarget, receive_network_events,
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
        let (handle, sender) = NetworkHandle::stub_with_control_sender();
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
    let (old_handle, sender) = NetworkHandle::stub_with_control_sender();
    sender
        .try_send(crate::runtime::network::NetworkControlEvent::Stopped {
            decode_error_count: 0,
        })
        .unwrap();
    drop(old_handle);
    assert!(sender.is_closed());
    app.insert_resource(NetworkHandle::disconnected());
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
    use crate::runtime::network::{NetworkControlEvent, receive_network_events};
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
        let (handle, sender) = NetworkHandle::stub_with_control_sender();
        sender
            .try_send(NetworkControlEvent::Bootstrap {
                session_generation: generation,
                world: WorldBootstrap {
                    dimension: 0,
                    local_player_runtime_id: 42,
                    local_player_unique_id: 1,
                    player_position: [0.0, 70.0, 0.0],
                    world_spawn_position: [0, 70, 0],
                    air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
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
                custom_blocks: protocol::CustomBlocks::default(),
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
                packs: crate::runtime::network::PackApplication::default(),
                terrain_before_spawn: true,
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
            assert!(app.world().resource::<ClientWorld>().fatal_error.is_none());
        } else {
            assert!(
                app.world()
                    .resource::<PlayerRuntime>()
                    .facts
                    .local_abilities()
                    .is_none()
            );
            assert!(app.world().resource::<ClientWorld>().fatal_error.is_some());
        }
    }
}

/// Builds a complete authority schedule with independent player and UI owners.
fn fixture_app() -> (App, Entity) {
    let mut player_runtime = PlayerRuntime::new(1);
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    bind_session_generation(&mut clock, &mut weather, 1);
    let stream = chunk_pipeline::WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
        local_player_unique_id: 1,
        player_position: [0.0, 70.0, 0.0],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let breg = include_bytes!("../../../../../crates/assets/data/block-registry-v2193.bin");
    let preg = include_bytes!("../../../../../crates/assets/data/block-physics-v2193.bin");
    let records = assets::read_registry_for_protocol(breg, 2193).unwrap();
    let collisions = PhysicsCollisionRegistries::from_assets(breg, &records, preg, 2193).unwrap();
    let mut menu = MenuRuntime::new(false, 2, "Test".into());
    menu.set_visible(false);
    let mut app = App::new();
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(&mut player_runtime, protocol::InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(&mut player_runtime, 1, 42)
        .unwrap();
    configure_client_frame_schedule(&mut app);
    configure_client_authority_systems(&mut app);
    let (network, command_receiver) = NetworkHandle::with_command_capacity(64);
    app.insert_non_send_resource(command_receiver);
    app.add_message::<KeyboardInput>()
        .add_message::<AppExit>()
        .insert_resource(ClientWorld {
            stream: Some(stream),
            ..ClientWorld::default()
        })
        .insert_resource(clock)
        .insert_resource(weather)
        .insert_resource(collisions)
        .insert_resource(runtime)
        .insert_resource(player_runtime)
        .insert_resource(UiPresentationRuntime::new(fixture_font()).unwrap())
        .insert_resource(menu)
        .insert_resource(network)
        .insert_resource(AcceptanceRun::new(Some(900), None, false, false))
        .insert_resource(ModelWitnessFileSource::new(None))
        .init_resource::<MovementTicker>()
        .init_resource::<LocalPhysicsController>()
        .init_resource::<LocalMovementEffectTimeline>()
        .init_resource::<LocalMovementSpeedAuthority>()
        .init_resource::<Time<Real>>()
        .init_resource::<ChunkUploadBudget>()
        .init_resource::<CameraSettingsAuthority>()
        .init_resource::<LocalViewPose>()
        .init_resource::<LocalPlayerFrameCarrier>()
        .init_resource::<InteractionOriginSnapshot>()
        .init_resource::<Phase3EvidenceEmitter>()
        .init_resource::<ServerCameraInstructions>()
        .init_resource::<crate::session::SessionController>()
        .init_resource::<ClientBlobCacheOwner>()
        .init_resource::<ResourcePackAdmissionState>()
        .init_resource::<MenuClipboard>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticInputSnapshot>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticRouteState>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<RuntimeSettings>();
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions {
                visible: false,
                grab_mode: CursorGrabMode::Locked,
                ..Default::default()
            },
            PrimaryWindow,
        ))
        .id();
    app.update(); // Bind the real semantic authority before any test edge.
    (app, window)
}

fn submit_form(app: &mut App, sequence: u64) {
    submit_form_id(app, sequence, 7);
}

fn submit_form_id(app: &mut App, sequence: u64, form_id: u32) {
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            sequence,
            WorldEvent::Ui(UiEvent::Form(FormRequestEvent {
                form_id,
                kind: FormKind::Menu,
                title: Some(Arc::from("Choose 世界")),
                json: Arc::from("{}"),
                model: ServerFormModel::TextMenu(TextMenuForm {
                    title: Arc::from("Choose 世界"),
                    content: Arc::from("Pick one"),
                    buttons: vec![Arc::from("First ✓"), Arc::from("第二")].into(),
                    button_images: [].into(),
                    omitted_images: 0,
                }),
            })),
        )
        .unwrap();
}

fn submit_transition(app: &mut App, sequence: u64, dimension: i32) {
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            sequence,
            WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                dimension,
                position: [0.0, 70.0, 0.0],
                ..Default::default()
            }),
        )
        .unwrap();
}

#[test]
fn rapid_dimension_return_skips_old_form_but_preserves_new_form_and_non_form_ui() {
    let (mut app, _) = fixture_app();
    submit_form(&mut app, 1);
    submit_transition(&mut app, 2, 1);
    submit_transition(&mut app, 3, 0);
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            4,
            WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health: 17 })),
        )
        .unwrap();
    submit_form_id(&mut app, 5, 8);
    app.update();
    let runtime = app.world().resource::<UiRuntime>();
    assert_eq!(runtime.server_forms().active().unwrap().form_id, 8);
    assert_eq!(runtime.server_forms().queued_busy_count(), 0);
    assert_eq!(runtime.hud().health(), ui::BoundedStat::new(17, 20));
    assert!(app.world().resource::<ClientWorld>().fatal_error.is_none());
}

#[test]
fn transition_retires_full_local_and_busy_answers_and_stale_same_id_actions() {
    use client_ui::ui_runtime::{FormRespondError, FormTransportError};
    let (mut app, _) = fixture_app();
    submit_form(&mut app, 1);
    submit_form_id(&mut app, 2, 8);
    app.update();
    let old = app
        .world()
        .resource::<UiRuntime>()
        .server_forms()
        .active()
        .unwrap()
        .identity;
    {
        let mut runtime = app.world_mut().resource_mut::<UiRuntime>();
        runtime.server_forms_mut().move_focus(1);
        runtime.server_forms_mut().set_scroll(3);
        runtime
            .respond_to_server_form(old, LocalFormAction::SubmitButton(1))
            .unwrap();
        assert_eq!(
            flush_form_response(&mut runtime, |_| Err(FormTransportError::Full)),
            Err(FormTransportError::Full)
        );
        assert_eq!(runtime.server_forms().queued_busy_count(), 1);
    }
    submit_transition(&mut app, 3, 1);
    submit_transition(&mut app, 4, 0);
    submit_form(&mut app, 5);
    app.update();
    let mut runtime = app.world_mut().resource_mut::<UiRuntime>();
    let current = runtime.server_forms().active().unwrap().identity;
    assert!(current.revision > old.revision);
    assert_eq!(runtime.server_forms().queued_busy_count(), 0);
    assert_eq!(runtime.server_forms().focus(), 0);
    assert_eq!(runtime.server_forms().scroll(), 0);
    assert_eq!(
        runtime.respond_to_server_form(old, LocalFormAction::Dismiss),
        Err(FormRespondError::StaleIdentity)
    );
    assert!(
        !flush_form_response(&mut runtime, |_| panic!("retired answers must not enqueue")).unwrap()
    );
    runtime
        .respond_to_server_form(current, LocalFormAction::SubmitButton(0))
        .unwrap();
    let mut packets = Vec::new();
    assert!(
        flush_form_response(&mut runtime, |packet| {
            packets.push(packet);
            Ok(())
        })
        .unwrap()
    );
    assert_eq!(packets.len(), 1);
    assert!(!flush_form_response(&mut runtime, |_| panic!("no duplicate answer")).unwrap());
}

#[test]
fn new_session_with_same_initial_epoch_retires_old_form_authority() {
    let (mut app, _) = fixture_app();
    submit_form(&mut app, 1);
    app.update();
    let old = app
        .world()
        .resource::<UiRuntime>()
        .server_forms()
        .active()
        .unwrap()
        .identity;
    app.world_mut()
        .resource_mut::<UiRuntime>()
        .respond_to_server_form(old, LocalFormAction::Dismiss)
        .unwrap();
    let replacement = fixture_app()
        .0
        .world_mut()
        .remove_resource::<ClientWorld>()
        .unwrap();
    app.insert_resource(replacement);
    let mut clock = app.world_mut().remove_resource::<WorldClock>().unwrap();
    let mut weather = app.world_mut().remove_resource::<WeatherState>().unwrap();
    bind_session_generation(&mut clock, &mut weather, 2);
    app.insert_resource(clock).insert_resource(weather);
    app.world_mut()
        .resource_scope(|world, mut player: Mut<PlayerRuntime>| {
            crate::session::begin_session(&mut world.resource_mut::<UiRuntime>(), &mut player, 2);
        });
    submit_form(&mut app, 1);
    app.update();
    let mut runtime = app.world_mut().resource_mut::<UiRuntime>();
    let current = runtime.server_forms().active().unwrap().identity;
    assert_eq!(current.session, 2);
    assert!(current.revision > old.revision);
    assert_eq!(
        runtime.respond_to_server_form(old, LocalFormAction::Dismiss),
        Err(client_ui::ui_runtime::FormRespondError::StaleIdentity)
    );
    assert!(
        !flush_form_response(&mut runtime, |_| panic!(
            "old-session pending response retired"
        ))
        .unwrap()
    );
}

#[test]
fn transition_without_successor_clears_display_and_definitely_unsent_busy_reply() {
    use client_ui::ui_runtime::FormTransportError;
    let (mut app, window) = fixture_app();
    submit_form(&mut app, 1);
    submit_form_id(&mut app, 2, 8);
    app.update();
    {
        let mut runtime = app.world_mut().resource_mut::<UiRuntime>();
        assert_eq!(
            flush_form_response(&mut runtime, |_| Err(FormTransportError::Full)),
            Err(FormTransportError::Full)
        );
        assert!(runtime.server_forms().active().is_some());
        assert_eq!(runtime.server_forms().queued_busy_count(), 1);
    }
    submit_transition(&mut app, 3, 1);
    submit_transition(&mut app, 4, 0);
    app.update();
    {
        let mut runtime = app.world_mut().resource_mut::<UiRuntime>();
        assert!(runtime.server_forms().active().is_none());
        assert!(!runtime.server_forms().owns_input());
        assert_eq!(runtime.server_forms().queued_busy_count(), 0);
        assert!(
            !flush_form_response(&mut runtime, |_| panic!(
                "retired busy response must not enqueue"
            ))
            .unwrap()
        );
    }
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert!(!cursor.visible && cursor.grab_mode == CursorGrabMode::Locked);
}

fn press(app: &mut App, window: Entity, case: InputCase) {
    let key = match case {
        InputCase::Move => KeyCode::KeyW,
        InputCase::Chat => KeyCode::KeyT,
        InputCase::Inventory => KeyCode::KeyE,
        InputCase::Pause => KeyCode::Escape,
        InputCase::Attack | InputCase::Use => {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(if matches!(case, InputCase::Attack) {
                    MouseButton::Left
                } else {
                    MouseButton::Right
                });
            return;
        }
    };
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    app.world_mut().write_message(KeyboardInput {
        key_code: key,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
}

fn assert_positive_control(app: &App, case: InputCase) {
    let ui = app.world().resource::<UiRuntime>();
    let input = app.world().resource::<SemanticInputSnapshot>();
    match case {
        InputCase::Move => assert_ne!(
            input.movement(),
            [0.0; 2],
            "movement edge must normally reach semantic consumers"
        ),
        InputCase::Attack => assert!(
            input.phase(Action::Attack).pressed,
            "attack positive control"
        ),
        InputCase::Use => assert!(input.phase(Action::Use).pressed, "use positive control"),
        InputCase::Chat => assert!(ui.chat_focused(), "chat positive control"),
        InputCase::Inventory => assert!(ui.inventory_open(), "inventory positive control"),
        InputCase::Pause => assert!(
            app.world().resource::<MenuRuntime>().is_visible(),
            "pause positive control"
        ),
    }
}

#[test]
fn committed_form_owns_first_visible_frame_and_recovers_each_real_input_consumer() {
    for case in [
        InputCase::Move,
        InputCase::Attack,
        InputCase::Use,
        InputCase::Chat,
        InputCase::Inventory,
        InputCase::Pause,
    ] {
        let (mut control, control_window) = fixture_app();
        press(&mut control, control_window, case);
        control.update();
        assert_positive_control(&control, case);

        let (mut app, window) = fixture_app();
        submit_form(&mut app, 1); // Real ordered stream; never pre-admit UiRuntime.
        press(&mut app, window, case);
        app.update();
        let runtime = app.world().resource::<UiRuntime>();
        assert!(
            runtime.server_forms().owns_input(),
            "{case:?}: first-visible-frame authority"
        );
        assert!(!runtime.chat_focused() && !runtime.inventory_open());
        assert!(!app.world().resource::<MenuRuntime>().is_visible());
        let input = app.world().resource::<SemanticInputSnapshot>();
        assert_eq!(
            input.movement(),
            [0.0; 2],
            "{case:?}: admission-frame gameplay movement"
        );
        assert_eq!(input.phase(Action::Attack), Default::default());
        assert_eq!(input.phase(Action::Use), Default::default());
        let cursor = app.world().get::<CursorOptions>(window).unwrap();
        assert!(
            cursor.visible,
            "{case:?}: first-visible-frame cursor is released"
        );
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
        assert!(
            app.world().resource::<ClientWorld>().fatal_error.is_none(),
            "real stream drain must succeed"
        );
        let mut runtime = app.world_mut().remove_resource::<UiRuntime>().unwrap();
        let player_runtime = app.world().resource::<PlayerRuntime>().clone();
        app.world_mut()
            .resource_mut::<UiPresentationRuntime>()
            .build(
                &player_runtime,
                &runtime,
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        if let Some(form) = runtime.server_forms().active() {
            let identity = form.identity;
            assert_eq!(
                app.world()
                    .resource::<UiPresentationRuntime>()
                    .form_button_count(identity),
                Some(2)
            );
            runtime
                .respond_to_server_form(identity, LocalFormAction::Dismiss)
                .unwrap();
        }
        assert!(flush_form_response(&mut runtime, |_| Ok(())).unwrap());
        assert!(!flush_form_response(&mut runtime, |_| Ok(())).unwrap());
        app.insert_resource(runtime);
        app.update(); // Restore cursor/input without replaying the old edge.
        let cursor = app.world().get::<CursorOptions>(window).unwrap();
        assert!(!cursor.visible && cursor.grab_mode == CursorGrabMode::Locked);
        assert!(
            !app.world()
                .resource::<UiRuntime>()
                .ui_focused(app.world().resource::<PlayerRuntime>())
        );
        press(&mut app, window, case);
        app.update();
        assert_positive_control(&app, case);
    }
}

#[test]
fn committed_hunger_waits_for_fifo_and_rejected_updates_preserve_domain_facts() {
    let (mut app, _) = fixture_app();
    let attribute = |current| protocol::ActorAttribute {
        name: "minecraft:player.hunger".into(),
        min: 0.0,
        max: 20.0,
        current,
        default: Some(20.0),
        modifiers: Arc::from([]),
    };
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            2,
            WorldEvent::Actor(protocol::ActorEvent::Attributes(
                protocol::ActorAttributesUpdateEvent {
                    dimension: 0,
                    runtime_id: 42,
                    attributes: Arc::from([attribute(6.0)]),
                    tick: 0,
                },
            )),
        )
        .unwrap();
    app.update();
    assert!(
        app.world()
            .resource::<PlayerRuntime>()
            .facts
            .hunger()
            .is_none()
    );
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .commit(1)
        .unwrap();
    app.update();
    let accepted = app
        .world()
        .resource::<PlayerRuntime>()
        .facts
        .hunger()
        .unwrap();
    assert_eq!(accepted.current(), 600);
    assert_eq!(accepted.scale(), 100);
    for (session_id, fifo_sequence) in [(1, 2), (0, 3)] {
        crate::tests::with_ui_player(&mut app, |runtime, player| {
            assert!(
                runtime
                    .apply_local_attributes(
                        player,
                        client_ui::ui_runtime::SequencedLocalAttributes {
                            session_id,
                            fifo_sequence,
                            local_millis: 0,
                            server_tick: 0,
                            attributes: Arc::from([attribute(1.0)]),
                        }
                    )
                    .is_err()
            );
        });
        assert_eq!(
            app.world().resource::<PlayerRuntime>().facts.hunger(),
            Some(accepted)
        );
    }
    assert!(app.world().resource::<ClientWorld>().fatal_error.is_none());
}

/// Block cracks consume committed UI at the production dispatch point.
#[test]
fn block_crack_consumer_is_wired_to_the_production_committed_dispatch() {
    let source = include_str!("../../world.rs");
    let drive = source
        .split_once("pub(crate) fn drive_world_stream(")
        .unwrap()
        .1;
    let early = include_str!("../committed_ui.rs");
    let authority = include_str!("../../../app/authority.rs");
    assert!(early.contains("} => consume_committed_block_crack("));
    assert!(early.contains("stream.take_committed_ui()"));
    assert!(!drive.contains("stream.take_committed_ui()"));
    assert!(authority.contains("drain_committed_ui_before_authority"));
    assert!(authority.contains(".before(ClientFrameSet::UiAuthority)"));
    assert!(drive.contains("reconcile_world_block_cracks(&mut ui_runtime, stream)"));
    assert!(drive.contains("ui_runtime.clear_disconnected_block_cracks()"));
}
