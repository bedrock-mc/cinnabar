//! Local hotbar slot selection.
//!
//! Number keys, the mouse wheel and controller buttons announce the selected slot through
//! equipment packets. Selection-only input stays local until the next tick's announcement.

use crate::runtime::{
    network::resource_packs::PackApplication,
    shutdown::{FatalError, record_fatal_error},
};
use bevy::prelude::{Res, ResMut};
use client_runtime::network::NetworkHandle;
use client_session::PacketSendError;
use protocol::Packet;

pub(crate) mod ordered;

/// Announces a queued selection before unordered hand actions capture it.
/// Selection-only frames leave the announcement to the next tick.
pub(crate) fn flush_hotbar_selection(
    mut player_runtime: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    network: Res<NetworkHandle<PackApplication>>,
    mut fatal_error: ResMut<FatalError>,
    input: Option<Res<client_state::SemanticInputSnapshot>>,
) {
    let Some(input) = input.as_ref().and_then(|input| input.snapshot()) else {
        return;
    };
    let hand_action = [
        semantic_input::Action::Attack,
        semantic_input::Action::Use,
        semantic_input::Action::PickItem,
    ]
    .into_iter()
    .any(|action| {
        let phase = input.phases[action as usize];
        phase.pressed || phase.held || phase.released
    });
    if input.ordered.is_some() || !hand_action {
        return;
    }
    ordered::flush_existing_with(
        &mut player_runtime,
        &mut fatal_error.0,
        |packet| network.send_hotbar_packet(packet),
        || network.closed_command_has_pending_control(),
    );
}

/// Attempts the latest pending hotbar selection once and retains it when authority or transport
/// is not ready.
fn flush_pending_hotbar_selection(
    player_runtime: &mut client_state::PlayerRuntime,
    fatal_error: &mut Option<String>,
    mut send: impl FnMut(Packet) -> Result<(), PacketSendError>,
) {
    let (target, packet) = match player_runtime
        .inventory
        .pending_hotbar_packet(player_runtime.facts.player_game_mode())
    {
        Ok(Some(command)) => command,
        Ok(None) => return,
        Err(error) => {
            record_fatal_error(
                fatal_error,
                format!("hotbar selection packet validation failed: {error}"),
            );
            return;
        }
    };
    match send(packet) {
        Ok(()) => {
            player_runtime
                .inventory
                .clear_pending_hotbar_selection(target);
        }
        Err(PacketSendError::Full(_)) => {}
        Err(PacketSendError::Closed(_)) => record_fatal_error(
            fatal_error,
            "hotbar selection send failed because the network command channel closed".to_owned(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::mouse::AccumulatedMouseScroll;
    use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
    use client_state::SemanticInputSnapshot;
    use client_ui::ui_runtime::UiRuntime;
    use std::sync::Arc;

    use protocol::{
        ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryEvent,
        InventorySlotEvent, ItemStackResponseEvent, NetworkItemStack, SelectedSlotEvent,
        SlotIdentity, StackResponse, StackResponseContainer, StackResponseSlot,
        StackResponseStatus, select_hotbar_slot_packet,
    };
    use sha2::{Digest, Sha256};

    use super::*;
    use client_interaction::hotbar::queue_hotbar_selection;

    #[test]
    fn ordered_selection_rechecks_terminal_control_after_a_closed_send() {
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let mut ui = identified_runtime(&mut player);
        publish_slot(&mut player, &mut ui, 1, NetworkItemStack::empty());
        let mode = player.facts.player_game_mode();
        player.inventory.queue_local_hotbar_selection(1, mode);
        let pending = std::cell::Cell::new(false);
        let mut fatal = None;
        assert!(!ordered::flush_existing_with(
            &mut player,
            &mut fatal,
            |packet| {
                pending.set(true);
                Err(PacketSendError::Closed(packet))
            },
            || pending.get(),
        ));
        assert_eq!(fatal, None);
        assert_eq!(player.inventory.pending_hotbar_selection(), Some(1));
    }

    /// Publishes one authoritative player-inventory slot into a UI runtime.
    fn publish_slot(
        player_runtime: &mut client_state::PlayerRuntime,
        runtime: &mut UiRuntime,
        slot: u8,
        stack: NetworkItemStack,
    ) {
        if runtime.inventory_authority(player_runtime) == Some(InventoryAuthority::Server)
            && !runtime
                .inventory_ledger(player_runtime)
                .personal_inventory_desired_open()
        {
            open_personal_inventory(player_runtime, runtime);
        }
        runtime
            .inventory_ledger_mut(player_runtime)
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity::window(0),
                    slot: u16::from(slot),
                },
                stack,
                storage_item: None,
            }));
    }

    fn open_personal_inventory(
        player_runtime: &mut client_state::PlayerRuntime,
        runtime: &mut UiRuntime,
    ) {
        assert!(
            runtime
                .inventory_ledger_mut(player_runtime)
                .request_personal_open(42)
        );
        assert!(
            runtime
                .inventory_ledger_mut(player_runtime)
                .mark_transport_enqueued(0)
        );
        runtime
            .inventory_ledger_mut(player_runtime)
            .apply(&InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(2),
                window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }));
    }

    /// Creates a runtime with the local actor identity needed by MobEquipment.
    fn identified_runtime(player_runtime: &mut client_state::PlayerRuntime) -> UiRuntime {
        *player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let mut runtime = UiRuntime::new(1);
        runtime
            .publish_local_runtime_id(player_runtime, 1, 42)
            .unwrap();
        runtime
    }

    /// Builds one valid non-empty stack for outbound hotbar packet tests.
    fn present_stack() -> NetworkItemStack {
        NetworkItemStack {
            network_id: 7,
            metadata: 3,
            stack_network_id: 13,
            count: 4,
            nbt_digest: Sha256::digest([]).into(),
            block_runtime_id: 92,
            extra_data: Arc::from([]),
        }
    }

    /// A wheel frame passes through the production hotbar system.
    fn wheel_selection(
        player_runtime: &mut client_state::PlayerRuntime,
        menu: Option<launcher::menu::MenuScreen>,
    ) -> u8 {
        let runtime = UiRuntime::new(1);
        let mut menu_runtime = client_shell::menu::MenuRuntime::new(false, 2, "Tester".into());
        if let Some(screen) = menu {
            menu_runtime.activate(launcher::menu::MenuAction::Navigate(screen));
        }
        wheel_with_ui(player_runtime, runtime, menu_runtime)
    }

    /// Delivers the same raw wheel frame with an arbitrary focused UI screen.
    fn wheel_with_ui(
        player_runtime: &mut client_state::PlayerRuntime,
        runtime: UiRuntime,
        menu_runtime: client_shell::menu::MenuRuntime,
    ) -> u8 {
        use bevy::{ecs::system::RunSystemOnce, prelude::*};
        let mut app = App::new();
        player_runtime.inventory.set_local_selected_slot(0);
        app.insert_resource(UiRuntimeResource(runtime))
            .insert_resource(player_runtime.clone())
            .insert_resource(menu_runtime)
            .insert_resource(SemanticInputSnapshot::default())
            .insert_resource(AccumulatedMouseScroll {
                delta: Vec2::new(0.0, -1.0),
                ..Default::default()
            })
            .insert_resource(NetworkHandle::<PackApplication>::disconnected())
            .insert_resource(FatalError::default());
        app.init_resource::<client_state::ScreenFocus>();
        app.world_mut()
            .run_system_once(client_shell::menu::screen_focus::publish_screen_focus)
            .unwrap();
        app.world_mut()
            .run_system_once(queue_hotbar_selection)
            .unwrap();
        app.world_mut()
            .run_system_once(flush_hotbar_selection)
            .unwrap();
        *player_runtime = app
            .world_mut()
            .remove_resource::<client_state::PlayerRuntime>()
            .unwrap();
        player_runtime.selected_hotbar_slot().unwrap()
    }

    #[test]
    fn menu_input_leak_settings_wheel_never_selects_a_hotbar_slot() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        assert_eq!(
            wheel_selection(
                &mut player_runtime,
                Some(launcher::menu::MenuScreen::Settings)
            ),
            0
        );
    }

    #[test]
    fn menu_input_leak_hud_wheel_selects_the_next_hotbar_slot() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        assert_eq!(wheel_selection(&mut player_runtime, None), 1);
    }

    #[test]
    fn menu_input_leak_chat_inventory_forms_and_pause_absorb_the_wheel() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        assert_eq!(
            wheel_selection(&mut player_runtime, Some(launcher::menu::MenuScreen::Pause)),
            0
        );
        let hidden = || client_shell::menu::MenuRuntime::new(false, 2, "Tester".into());
        let mut chat = UiRuntime::new(1);
        chat.open_chat(&mut player_runtime);
        assert_eq!(wheel_with_ui(&mut player_runtime, chat, hidden()), 0);
        let mut inventory = identified_runtime(&mut player_runtime);
        inventory
            .publish_inventory_authority(&mut player_runtime, protocol::InventoryAuthority::Server);
        inventory.toggle_inventory(&mut player_runtime, None);
        assert!(inventory.inventory_open());
        assert_eq!(wheel_with_ui(&mut player_runtime, inventory, hidden()), 0);
        let form = client_ui::test_support::pack_harness::action_form(
            &mut player_runtime,
            "Form",
            &["OK"],
        );
        assert_eq!(wheel_with_ui(&mut player_runtime, form, hidden()), 0);
    }

    /// Delivers one raw wheel frame with the presentation behind the screen policy, and returns
    /// the selected slot and the presentation.
    fn wheel_with_presentation(
        player_runtime: &mut client_state::PlayerRuntime,
        presentation: client_ui::ui_runtime::presentation::UiPresentationRuntime,
    ) -> (
        u8,
        client_ui::ui_runtime::presentation::UiPresentationRuntime,
    ) {
        use bevy::{ecs::system::RunSystemOnce, prelude::*};
        let mut app = App::new();
        player_runtime.inventory.set_local_selected_slot(0);
        app.insert_resource(UiRuntimeResource(UiRuntime::new(1)))
            .insert_resource(player_runtime.clone())
            .insert_resource(client_shell::menu::MenuRuntime::new(
                false,
                2,
                "Tester".into(),
            ))
            .insert_resource(UiPresentationResource(presentation))
            .insert_resource(SemanticInputSnapshot::default())
            .insert_resource(AccumulatedMouseScroll {
                delta: Vec2::new(0.0, -1.0),
                ..Default::default()
            })
            .insert_resource(NetworkHandle::<PackApplication>::disconnected())
            .insert_resource(FatalError::default());
        app.init_resource::<client_state::ScreenFocus>();
        app.world_mut()
            .run_system_once(client_shell::menu::screen_focus::publish_screen_focus)
            .unwrap();
        app.world_mut()
            .run_system_once(queue_hotbar_selection)
            .unwrap();
        app.world_mut()
            .run_system_once(flush_hotbar_selection)
            .unwrap();
        *player_runtime = app
            .world_mut()
            .remove_resource::<client_state::PlayerRuntime>()
            .unwrap();
        let slot = player_runtime.selected_hotbar_slot().unwrap();
        let presentation = app
            .world_mut()
            .remove_resource::<UiPresentationResource>()
            .unwrap();
        (slot, presentation.0)
    }

    /// A client part's modal screen is a screen like any other: while it is open the wheel and
    /// every router-gated gameplay action stop reaching the player, and closing it (Escape or
    /// `ui.close-screen`, both of which clear its template) gives them back.
    #[test]
    fn menu_input_leak_client_part_modal_absorbs_gameplay_until_closed() {
        use client_ui::ui_runtime::presentation::UiPresentationRuntime;
        use mod_ui::experience_modal::ExperienceModal;
        use server_experience::screen::{Files, Modal};
        use ui_engine::test_support::fixture_font;

        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let files = Arc::new(Files::default());
        let mut modal = Modal::default();
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        let follow = |presentation: &mut UiPresentationRuntime, modal: &Modal| {
            presentation.set_experience_modal(Some(ExperienceModal {
                bundle: "benergistics",
                files: &files,
                modal,
            }));
        };
        let absorbs = |player_runtime: &client_state::PlayerRuntime,
                       presentation: &UiPresentationRuntime| {
            client_shell::screen_policy::absorbs_input(
                player_runtime,
                Some(&UiRuntime::new(1)),
                Some(&client_shell::menu::MenuRuntime::new(
                    false,
                    2,
                    "Tester".into(),
                )),
                Some(presentation),
            )
        };

        follow(&mut presentation, &modal);
        assert!(!absorbs(&player_runtime, &presentation), "before it opens");
        let (slot, mut presentation) = wheel_with_presentation(&mut player_runtime, presentation);
        assert_eq!(slot, 1);

        modal.open(Some("terminal.json".into()));
        follow(&mut presentation, &modal);
        assert!(absorbs(&player_runtime, &presentation), "while it is open");
        let (slot, mut presentation) = wheel_with_presentation(&mut player_runtime, presentation);
        assert_eq!(slot, 0);

        modal.open(None);
        follow(&mut presentation, &modal);
        assert!(!absorbs(&player_runtime, &presentation), "after it closed");
        let (slot, _) = wheel_with_presentation(&mut player_runtime, presentation);
        assert_eq!(slot, 1);
    }

    #[test]
    fn unknown_hotbar_cell_announces_empty_equipment_and_releases_hand_actions() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        identified_runtime(&mut player_runtime);
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(2, game_mode);
        let mut sends = 0;
        let mut fatal = None;

        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |_packet| {
            sends += 1;
            Ok::<(), PacketSendError>(())
        });

        assert_eq!(sends, 1);
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
        let hand = client_interaction::mining::hand_interaction_selection(&player_runtime)
            .expect("the admitted selection releases bare-hand actions");
        assert_eq!(hand.slot, 2);
        assert_eq!(hand.item.network_id(), 0);
        assert_eq!(fatal, None);
    }

    #[test]
    fn matching_equipment_bootstrap_sends_while_ledger_slot_is_unknown() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        let mut runtime = identified_runtime(&mut player_runtime);
        let equipment_stack = present_stack();
        let game_mode = player_runtime.facts.player_game_mode();
        runtime.retain_local_selected_equipment(
            &mut player_runtime,
            1,
            protocol::EquipmentEvent {
                actor_runtime_id: 42,
                stack: equipment_stack.clone(),
                inventory_slot: 2,
                selected_slot: 2,
                window_id: 0,
                handedness: Some(protocol::ActorHandedness::Right),
            },
        );
        player_runtime
            .inventory
            .queue_local_hotbar_selection(2, game_mode);
        let snapshot = player_runtime.selected_stack_snapshot().unwrap();
        assert_eq!(snapshot.slot, 2);
        assert_eq!(
            snapshot.state,
            inventory::inventory_ledger::PlayerInventorySlot::Present(&equipment_stack)
        );
        let mut sent = None;
        let mut fatal = None;

        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |packet| {
            sent = Some(packet);
            Ok(())
        });

        let expected = select_hotbar_slot_packet(42, 2, &equipment_stack).unwrap();
        assert_eq!(
            protocol::encode(&sent.unwrap()).unwrap(),
            protocol::encode(&expected).unwrap()
        );
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
        assert_eq!(fatal, None);
    }

    #[test]
    fn full_retry_rebuilds_packet_from_the_current_selected_snapshot() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        let mut runtime = identified_runtime(&mut player_runtime);
        let first = present_stack();
        publish_slot(&mut player_runtime, &mut runtime, 2, first);
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(2, game_mode);
        let mut fatal = None;

        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |packet| {
            Err(PacketSendError::Full(packet))
        });
        publish_slot(
            &mut player_runtime,
            &mut runtime,
            2,
            NetworkItemStack::empty(),
        );

        let mut retried = None;
        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |packet| {
            retried = Some(packet);
            Ok(())
        });

        let expected = select_hotbar_slot_packet(42, 2, &NetworkItemStack::empty()).unwrap();
        assert_eq!(
            protocol::encode(&retried.unwrap()).unwrap(),
            protocol::encode(&expected).unwrap()
        );
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
        assert_eq!(fatal, None);
    }

    #[test]
    fn full_send_retries_next_frame_without_new_input() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        let mut runtime = identified_runtime(&mut player_runtime);
        publish_slot(
            &mut player_runtime,
            &mut runtime,
            2,
            NetworkItemStack::empty(),
        );
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(2, game_mode);
        let mut attempts = 0;
        let mut fatal = None;

        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |packet| {
            attempts += 1;
            Err(PacketSendError::Full(packet))
        });
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), Some(2));

        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |_| {
            attempts += 1;
            Ok(())
        });

        assert_eq!(attempts, 2);
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
        assert_eq!(fatal, None);
    }

    #[test]
    fn newer_selection_supersedes_pending_selection() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(2, game_mode);
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(7, game_mode);

        assert_eq!(player_runtime.selected_hotbar_slot(), Some(7));
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), Some(7));
    }

    #[test]
    fn same_slot_input_does_not_suppress_an_unsent_pending_selection() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        let mut runtime = identified_runtime(&mut player_runtime);
        publish_slot(
            &mut player_runtime,
            &mut runtime,
            4,
            NetworkItemStack::empty(),
        );
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(4, game_mode);
        let mut fatal = None;
        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |packet| {
            Err(PacketSendError::Full(packet))
        });

        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(4, game_mode);
        let mut sent = false;
        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |_| {
            sent = true;
            Ok(())
        });

        assert!(sent);
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
    }

    #[test]
    fn begin_session_clears_pending_hotbar_selection() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        let mut runtime = UiRuntime::new(1);
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(5, game_mode);

        client_runtime::session::begin_session(&mut runtime, &mut player_runtime, 2);

        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
    }

    #[test]
    fn screen_prediction_changes_presentation_but_equipment_keeps_backing_stack() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        let mut runtime = identified_runtime(&mut player_runtime);
        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
        open_personal_inventory(&mut player_runtime, &mut runtime);
        let authoritative = present_stack();
        publish_slot(&mut player_runtime, &mut runtime, 0, authoritative.clone());
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(0, game_mode);
        let authoritative_snapshot = player_runtime.selected_stack_snapshot().unwrap();
        assert_eq!(authoritative_snapshot.slot, 0);
        assert_eq!(
            authoritative_snapshot.state,
            inventory::inventory_ledger::PlayerInventorySlot::Present(&authoritative)
        );
        let request_id = runtime
            .inventory_ledger_mut(&mut player_runtime)
            .begin_click(0)
            .unwrap();
        let predicted_snapshot = player_runtime.selected_stack_snapshot().unwrap();
        assert_eq!(predicted_snapshot.slot, 0);
        assert_eq!(
            predicted_snapshot.state,
            inventory::inventory_ledger::PlayerInventorySlot::Empty
        );

        let mut predicted_packet = None;
        let mut fatal = None;
        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |packet| {
            predicted_packet = Some(packet);
            Ok(())
        });
        let predicted_bytes = protocol::encode(&predicted_packet.unwrap()).unwrap();
        let authoritative_packet = select_hotbar_slot_packet(42, 0, &authoritative).unwrap();
        assert_eq!(
            predicted_bytes,
            protocol::encode(&authoritative_packet).unwrap()
        );

        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .apply(&InventoryEvent::Response(ItemStackResponseEvent {
                responses: Arc::from([StackResponse {
                    status: StackResponseStatus::Rejected,
                    request_id,
                    containers: Arc::from([]),
                }]),
            }));
        let restored_snapshot = player_runtime.selected_stack_snapshot().unwrap();
        assert_eq!(restored_snapshot.slot, 0);
        assert_eq!(
            restored_snapshot.state,
            inventory::inventory_ledger::PlayerInventorySlot::Present(&authoritative)
        );

        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(1, game_mode);
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(0, game_mode);
        let mut restored_packet = None;
        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |packet| {
            restored_packet = Some(packet);
            Ok(())
        });
        let restored_bytes = protocol::encode(&restored_packet.unwrap()).unwrap();
        assert_eq!(
            restored_bytes,
            protocol::encode(&authoritative_packet).unwrap()
        );
        assert_eq!(fatal, None);
    }

    /// A screen-predicted stack does not delay or replace the backing equipment packet.
    #[test]
    fn screen_predicted_selected_slot_sends_backing_without_waiting_for_correction() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let mut runtime = identified_runtime(&mut player_runtime);
        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
        open_personal_inventory(&mut player_runtime, &mut runtime);
        let original = present_stack();
        publish_slot(
            &mut player_runtime,
            &mut runtime,
            0,
            NetworkItemStack::empty(),
        );
        publish_slot(&mut player_runtime, &mut runtime, 1, original.clone());

        let take = runtime
            .inventory_ledger_mut(&mut player_runtime)
            .begin_click(1)
            .unwrap();
        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .apply(&InventoryEvent::Response(ItemStackResponseEvent {
                responses: Arc::from([StackResponse {
                    status: StackResponseStatus::Accepted,
                    request_id: take,
                    containers: Arc::from([
                        StackResponseContainer {
                            container: ContainerIdentity {
                                window_id: None,
                                slot_type: Some(
                                    protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                                ),
                                dynamic_id: None,
                            },
                            slots: Arc::from([StackResponseSlot {
                                slot: 1,
                                hotbar_slot: 1,
                                count: 0,
                                item_stack_id: 0,
                                custom_name: Arc::from(""),
                                filtered_custom_name: Arc::from(""),
                                durability_correction: 0,
                            }]),
                        },
                        StackResponseContainer {
                            container: ContainerIdentity {
                                window_id: None,
                                slot_type: Some(protocol::CONTAINER_NAME_CURSOR),
                                dynamic_id: None,
                            },
                            slots: Arc::from([StackResponseSlot {
                                slot: 0,
                                hotbar_slot: 0,
                                count: u8::try_from(original.count).unwrap(),
                                item_stack_id: original.stack_network_id,
                                custom_name: Arc::from(""),
                                filtered_custom_name: Arc::from(""),
                                durability_correction: 0,
                            }]),
                        },
                    ]),
                }]),
            }));
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(0, game_mode);
        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .begin_click(0)
            .unwrap();
        let predicted = match player_runtime.selected_stack_snapshot().unwrap().state {
            inventory::inventory_ledger::PlayerInventorySlot::Present(stack) => stack.clone(),
            state => panic!("the place predicts a present stack, got {state:?}"),
        };
        assert!(predicted.stack_network_id < -1);

        let mut fatal = None;
        let mut sent = Vec::new();
        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |packet| {
            sent.push(packet);
            Ok(())
        });
        assert_eq!(sent.len(), 1);
        let expected = select_hotbar_slot_packet(42, 0, &NetworkItemStack::empty()).unwrap();
        assert_eq!(
            protocol::encode(&sent[0]).unwrap(),
            protocol::encode(&expected).unwrap()
        );
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
        assert_eq!(fatal, None);
    }

    /// A server stack without network tracking sends at once.
    #[test]
    fn untracked_stack_identity_does_not_defer_hotbar_selection() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let mut runtime = identified_runtime(&mut player_runtime);
        let mut stack = present_stack();
        stack.stack_network_id = -1;
        publish_slot(&mut player_runtime, &mut runtime, 2, stack);
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(2, game_mode);
        let mut fatal = None;
        let mut sends = 0;
        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |_| {
            sends += 1;
            Ok(())
        });
        assert_eq!(sends, 1);
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
        assert_eq!(fatal, None);
    }

    #[test]
    fn server_forced_selection_cancels_pending_local_packet() {
        let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

        let mut runtime = identified_runtime(&mut player_runtime);
        publish_slot(
            &mut player_runtime,
            &mut runtime,
            2,
            NetworkItemStack::empty(),
        );
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(2, game_mode);
        runtime
            .enqueue_inventory_event(
                &mut player_runtime,
                1,
                1,
                InventoryEvent::SelectedSlot(SelectedSlotEvent {
                    container: ContainerIdentity::window(0),
                    slot: 5,
                    select_slot: true,
                }),
            )
            .unwrap();

        runtime.drain_pending_inventory(&mut player_runtime);
        let mut sends = 0;
        let mut fatal = None;
        flush_pending_hotbar_selection(&mut player_runtime, &mut fatal, |_| {
            sends += 1;
            Ok(())
        });

        assert_eq!(player_runtime.selected_hotbar_slot(), Some(5));
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
        assert_eq!(sends, 0);
        assert_eq!(fatal, None);
    }
}
