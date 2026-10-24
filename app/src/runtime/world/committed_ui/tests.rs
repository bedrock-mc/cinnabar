//! Real ordered-stream commit-to-authority witnesses, not network-ingress tests.

use super::*;
#[cfg(not(feature = "acceptance"))]
use crate::acceptance::AcceptanceRun;
use crate::app::authority::configure_client_authority_systems;
use crate::game_frame::configure_client_frame_schedule;
use crate::runtime::network::resource_packs::PackApplication;
#[cfg(feature = "acceptance")]
use ::acceptance::AcceptanceRun;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
        mouse::AccumulatedMouseMotion,
    },
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use client_presentation::environment::{WeatherState, bind_session_generation};
use client_presentation::server_camera::ServerCameraInstructions;
use client_runtime::network::NetworkHandle;
use client_shell::ui_resources::UiPresentationResource;
use client_state::PlayerRuntime;
use client_ui::ui_runtime::{
    LocalFormAction, flush_form_response, presentation::UiPresentationRuntime,
};
use protocol::{
    FormKind, FormRequestEvent, ServerFormModel, TextMenuForm, UiEvent, WorldBootstrap, WorldEvent,
};
use render_scene::ChunkUploadBudget;
use semantic_input::Action;
use std::sync::Arc;
use {
    crate::{
        runtime::movement_evidence::MovementEvidenceEmitter,
        semantic_controls::{
            PendingDeviceFrame, SemanticInputRuntime, SemanticRouteState, SemanticTouchTargets,
        },
    },
    acceptance::model_witness::ModelWitnessFileSource,
    client_interaction::movement::{
        LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
        MovementTicker, PhysicsCollisionRegistries,
    },
    client_presentation::{
        camera::CameraSettingsAuthority,
        local_player::{InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalViewPose},
    },
    client_runtime::{
        blob_cache::ClientBlobCacheOwner, pack_admission::ResourcePackAdmissionState,
    },
    client_shell::menu::{MenuClipboard, MenuRuntime},
    client_state::{RuntimeSettings, SemanticInputSnapshot},
    ui_engine::test_support::fixture_font,
};

mod boss_lifetime;
mod credits_identity;
mod npc_identity;
mod player_authority;
mod respawn;

#[derive(Clone, Copy, Debug)]
enum InputCase {
    Move,
    Attack,
    Use,
    Chat,
    Inventory,
    Pause,
}

/// Builds a complete authority schedule with independent player and UI owners.
pub(crate) fn fixture_app() -> (App, Entity) {
    let mut player_runtime = PlayerRuntime(player_state::PlayerState::new(1));
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    bind_session_generation(&mut clock, &mut weather, 1);
    let stream = chunk_pipeline::WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
        local_player_unique_id: 1,
        player_position: [0.0, 70.0, 0.0],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
    let (network, command_receiver) = NetworkHandle::<PackApplication>::with_command_capacity(64);
    app.insert_non_send(command_receiver);
    app.add_message::<KeyboardInput>()
        .add_message::<AppExit>()
        .insert_resource(ClientWorld {
            stream: Some(stream),
            ..ClientWorld::default()
        })
        .init_resource::<FatalError>()
        .insert_resource(clock)
        .insert_resource(weather)
        .insert_resource(collisions)
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(player_runtime)
        .insert_resource(UiPresentationResource(
            UiPresentationRuntime::new(fixture_font()).unwrap(),
        ))
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
        .init_resource::<client_presentation::camera::look::LookSmoother>()
        .init_resource::<LocalViewPose>()
        .init_resource::<LocalPlayerFrameCarrier>()
        .init_resource::<InteractionOriginSnapshot>()
        .init_resource::<MovementEvidenceEmitter>()
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
                    title: "Choose 世界".into(),
                    content: "Pick one".into(),
                    buttons: vec!["First ✓".into(), "第二".into()].into(),
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
    let runtime = app.world().resource::<UiRuntimeResource>();
    assert_eq!(runtime.server_forms().active().unwrap().form_id, 8);
    assert_eq!(runtime.server_forms().queued_busy_count(), 0);
    assert_eq!(runtime.hud().health(), ui::BoundedStat::new(17, 20));
    assert!(app.world().resource::<FatalError>().0.is_none());
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
        .resource::<UiRuntimeResource>()
        .server_forms()
        .active()
        .unwrap()
        .identity;
    {
        let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
        runtime.server_forms_mut().move_focus(1);
        runtime.server_forms_mut().set_scroll(3);
        runtime
            .respond_to_server_form(old, LocalFormAction::SubmitButton(1))
            .unwrap();
        assert_eq!(
            flush_form_response(&mut runtime, |_| false, |_| Err(FormTransportError::Full)),
            Err(FormTransportError::Full)
        );
        assert_eq!(runtime.server_forms().queued_busy_count(), 1);
    }
    submit_transition(&mut app, 3, 1);
    submit_transition(&mut app, 4, 0);
    submit_form(&mut app, 5);
    app.update();
    let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
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
        !flush_form_response(
            &mut runtime,
            |_| false,
            |_| panic!("retired answers must not enqueue")
        )
        .unwrap()
    );
    runtime
        .respond_to_server_form(current, LocalFormAction::SubmitButton(0))
        .unwrap();
    let mut packets = Vec::new();
    assert!(
        flush_form_response(
            &mut runtime,
            |_| false,
            |packet| {
                packets.push(packet);
                Ok(())
            }
        )
        .unwrap()
    );
    assert_eq!(packets.len(), 1);
    assert!(
        !flush_form_response(&mut runtime, |_| false, |_| panic!("no duplicate answer")).unwrap()
    );
}

#[test]
fn new_session_with_same_initial_epoch_retires_old_form_authority() {
    let (mut app, _) = fixture_app();
    submit_form(&mut app, 1);
    app.update();
    let old = app
        .world()
        .resource::<UiRuntimeResource>()
        .server_forms()
        .active()
        .unwrap()
        .identity;
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
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
            client_runtime::session::begin_session(
                &mut world.resource_mut::<UiRuntimeResource>(),
                &mut player,
                2,
            );
        });
    submit_form(&mut app, 1);
    app.update();
    let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
    let current = runtime.server_forms().active().unwrap().identity;
    assert_eq!(current.session, 2);
    assert!(current.revision > old.revision);
    assert_eq!(
        runtime.respond_to_server_form(old, LocalFormAction::Dismiss),
        Err(client_ui::ui_runtime::FormRespondError::StaleIdentity)
    );
    assert!(
        !flush_form_response(
            &mut runtime,
            |_| false,
            |_| panic!("old-session pending response retired")
        )
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
        let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
        assert_eq!(
            flush_form_response(&mut runtime, |_| false, |_| Err(FormTransportError::Full)),
            Err(FormTransportError::Full)
        );
        assert!(runtime.server_forms().active().is_some());
        assert_eq!(runtime.server_forms().queued_busy_count(), 1);
    }
    submit_transition(&mut app, 3, 1);
    submit_transition(&mut app, 4, 0);
    app.update();
    {
        let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
        assert!(runtime.server_forms().active().is_none());
        assert!(!runtime.server_forms().owns_input());
        assert_eq!(runtime.server_forms().queued_busy_count(), 0);
        assert!(
            !flush_form_response(
                &mut runtime,
                |_| false,
                |_| panic!("retired busy response must not enqueue")
            )
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
    let ui = app.world().resource::<UiRuntimeResource>();
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
        let runtime = app.world().resource::<UiRuntimeResource>();
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
            app.world().resource::<FatalError>().0.is_none(),
            "real stream drain must succeed"
        );
        let mut runtime = app
            .world_mut()
            .remove_resource::<UiRuntimeResource>()
            .unwrap();
        let player_runtime = app.world().resource::<PlayerRuntime>().clone();
        app.world_mut()
            .resource_mut::<UiPresentationResource>()
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
                    .resource::<UiPresentationResource>()
                    .form_button_count(identity),
                Some(2)
            );
            runtime
                .respond_to_server_form(identity, LocalFormAction::Dismiss)
                .unwrap();
        }
        assert!(flush_form_response(&mut runtime, |_| false, |_| Ok(())).unwrap());
        assert!(!flush_form_response(&mut runtime, |_| false, |_| Ok(())).unwrap());
        app.insert_resource(runtime);
        app.update(); // Restore cursor/input without replaying the old edge.
        let cursor = app.world().get::<CursorOptions>(window).unwrap();
        assert!(!cursor.visible && cursor.grab_mode == CursorGrabMode::Locked);
        assert!(
            !app.world()
                .resource::<UiRuntimeResource>()
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
    assert!(app.world().resource::<FatalError>().0.is_none());
}

/// Committed crack sequences reach frame authority before its consumers run.
#[test]
fn committed_block_cracks_reach_frame_authority_once_in_fifo_order() {
    use client_ui::ui_runtime::{SequencedBlockCrackEvent, UiRuntimeError};
    use protocol::{BlockCrackAction, BlockCrackEvent};

    let (mut app, _) = fixture_app();
    let event = BlockCrackEvent {
        position: [0; 3],
        action: BlockCrackAction::Stop,
    };
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(2, WorldEvent::BlockCrack(event))
        .unwrap();
    app.update();
    assert!(
        app.world_mut()
            .resource_mut::<ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .take_committed_ui()
            .is_empty()
    );

    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(1, WorldEvent::BlockCrack(event))
        .unwrap();
    app.add_systems(
        Update,
        (move |mut runtime: ResMut<UiRuntimeResource>| {
            assert!(matches!(
                runtime.retain_block_crack(SequencedBlockCrackEvent {
                    session_id: 1,
                    fifo_sequence: 2,
                    dimension: 0,
                    event,
                }),
                Err(UiRuntimeError::StaleBlockCrackSequence {
                    previous: 2,
                    actual: 2,
                })
            ));
        })
        .in_set(client_state::ClientFrameSet::UiAuthority),
    );
    app.update();
    app.update();
    assert!(app.world().resource::<FatalError>().0.is_none());
    let mut world = app.world_mut().resource_mut::<ClientWorld>();
    assert!(
        world
            .stream
            .as_mut()
            .unwrap()
            .take_committed_ui()
            .is_empty()
    );
}
