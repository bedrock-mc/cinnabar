//! Local hotbar slot selection.
//!
//! Bedrock owns hotbar-slot selection on the client: number keys, the mouse wheel, and the
//! controller cycle buttons change the held slot immediately (predicted locally so the HUD
//! highlight follows input without waiting for the server), and the choice is announced upstream
//! with a `PlayerHotbar` packet.

use bevy::{
    input::mouse::AccumulatedMouseScroll,
    prelude::{Res, ResMut},
};
use protocol::{HOTBAR_SLOT_COUNT, Packet};
use semantic_input::Action;

use crate::{
    runtime::{
        network::{NetworkHandle, PacketSendError},
        shutdown::record_fatal_error,
        world::ClientWorld,
    },
    semantic_controls::SemanticInputSnapshot,
};
use client_ui::ui_runtime::UiRuntime;

pub(crate) const HOTBAR_DIGIT_ACTIONS: [Action; HOTBAR_SLOT_COUNT as usize] = [
    Action::Hotbar1,
    Action::Hotbar2,
    Action::Hotbar3,
    Action::Hotbar4,
    Action::Hotbar5,
    Action::Hotbar6,
    Action::Hotbar7,
    Action::Hotbar8,
    Action::Hotbar9,
];

/// Applies number-key, mouse-wheel, and controller hotbar selection to the local prediction and
/// notifies the server. Runs after semantic input is finalized and before UI publication.
#[allow(
    clippy::too_many_arguments,
    reason = "Player authority is borrowed separately from UI state."
)]
pub(crate) fn select_hotbar_slot(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    input: Res<SemanticInputSnapshot>,
    scroll: Res<AccumulatedMouseScroll>,
    menu: Option<Res<crate::menu::MenuRuntime>>,
    presentation: Option<Res<client_ui::ui_runtime::presentation::UiPresentationRuntime>>,
    runtime: Res<UiRuntime>,
    network: Res<NetworkHandle>,
    mut client_world: ResMut<ClientWorld>,
) {
    // Direct number-key selection. The router only resolves Hotbar1..9 in the Gameplay context,
    // so digits typed while chat is focused never reach this snapshot.
    let mut requested: Option<u8> = None;
    for (index, action) in HOTBAR_DIGIT_ACTIONS.iter().enumerate() {
        if input.phase(*action).pressed {
            requested = Some(index as u8);
        }
    }

    // Controller actions are router-gated; raw wheel input needs the same screen ownership.
    let mut cycle: i32 = 0;
    if input.phase(Action::HotbarNext).pressed {
        cycle += 1;
    }
    if input.phase(Action::HotbarPrevious).pressed {
        cycle -= 1;
    }
    if !crate::screen_policy::absorbs_input(
        &player_runtime,
        Some(&runtime),
        menu.as_deref(),
        presentation.as_deref(),
    ) {
        // One slot per frame: scrolling up selects the previous slot.
        if scroll.delta.y > 0.0 {
            cycle -= 1;
        } else if scroll.delta.y < 0.0 {
            cycle += 1;
        }
    }

    if requested.is_none() && cycle != 0 {
        let current = i32::from(
            player_runtime
                .inventory
                .selected_hotbar_slot(player_runtime.facts.player_game_mode())
                .unwrap_or(0)
                .min(HOTBAR_SLOT_COUNT - 1),
        );
        let slots = i32::from(HOTBAR_SLOT_COUNT);
        requested = Some((((current + cycle) % slots + slots) % slots) as u8);
    }

    if let Some(target) = requested {
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(target, game_mode);
    }

    if network.closed_command_has_pending_control() {
        return;
    }

    flush_pending_hotbar_selection(
        &mut player_runtime,
        &mut client_world.fatal_error,
        |packet| match network.send_hotbar_packet(packet) {
            Err(PacketSendError::Closed(packet))
                if network.closed_command_has_pending_control() =>
            {
                Err(PacketSendError::Full(packet))
            }
            result => result,
        },
    );
}

/// Attempts the latest pending hotbar selection once and retains it when authority or transport
/// is not ready.
fn flush_pending_hotbar_selection(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
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
    use std::sync::Arc;

    use protocol::{
        ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryEvent,
        InventorySlotEvent, ItemStackResponseEvent, NetworkItemStack, SelectedSlotEvent,
        SlotIdentity, StackResponse, StackResponseContainer, StackResponseSlot,
        StackResponseStatus, select_hotbar_slot_packet,
    };
    use sha2::{Digest, Sha256};

    use super::*;

    /// Publishes one authoritative player-inventory slot into a UI runtime.
    fn publish_slot(
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
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
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
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
    fn identified_runtime(player_runtime: &mut crate::player_runtime::PlayerRuntime) -> UiRuntime {
        *player_runtime = crate::player_runtime::PlayerRuntime::new(1);
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
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        menu: Option<launcher::menu::MenuScreen>,
    ) -> u8 {
        let runtime = UiRuntime::new(1);
        let mut menu_runtime = crate::menu::MenuRuntime::new(false, 2, "Tester".into());
        if let Some(screen) = menu {
            menu_runtime.activate(launcher::menu::MenuAction::Navigate(screen));
        }
        wheel_with_ui(player_runtime, runtime, menu_runtime)
    }

    /// Delivers the same raw wheel frame with an arbitrary focused UI screen.
    fn wheel_with_ui(
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        runtime: UiRuntime,
        menu_runtime: crate::menu::MenuRuntime,
    ) -> u8 {
        use bevy::{ecs::system::RunSystemOnce, prelude::*};
        let mut app = App::new();
        player_runtime.inventory.set_local_selected_slot(0);
        app.insert_resource(runtime)
            .insert_resource(player_runtime.clone())
            .insert_resource(menu_runtime)
            .insert_resource(SemanticInputSnapshot::default())
            .insert_resource(AccumulatedMouseScroll {
                delta: Vec2::new(0.0, -1.0),
                ..Default::default()
            })
            .insert_resource(NetworkHandle::disconnected())
            .insert_resource(ClientWorld::default());
        app.world_mut().run_system_once(select_hotbar_slot).unwrap();
        *player_runtime = app
            .world_mut()
            .remove_resource::<crate::player_runtime::PlayerRuntime>()
            .unwrap();
        player_runtime.selected_hotbar_slot().unwrap()
    }

    #[test]
    fn menu_input_leak_settings_wheel_never_selects_a_hotbar_slot() {
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

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
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

        assert_eq!(wheel_selection(&mut player_runtime, None), 1);
    }

    #[test]
    fn menu_input_leak_chat_inventory_forms_and_pause_absorb_the_wheel() {
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

        assert_eq!(
            wheel_selection(&mut player_runtime, Some(launcher::menu::MenuScreen::Pause)),
            0
        );
        let hidden = || crate::menu::MenuRuntime::new(false, 2, "Tester".into());
        let mut chat = UiRuntime::new(1);
        chat.open_chat(&mut player_runtime);
        assert_eq!(wheel_with_ui(&mut player_runtime, chat, hidden()), 0);
        let mut inventory = identified_runtime(&mut player_runtime);
        inventory
            .publish_inventory_authority(&mut player_runtime, protocol::InventoryAuthority::Server);
        inventory.toggle_inventory(&mut player_runtime);
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
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        presentation: client_ui::ui_runtime::presentation::UiPresentationRuntime,
    ) -> (
        u8,
        client_ui::ui_runtime::presentation::UiPresentationRuntime,
    ) {
        use bevy::{ecs::system::RunSystemOnce, prelude::*};
        let mut app = App::new();
        player_runtime.inventory.set_local_selected_slot(0);
        app.insert_resource(UiRuntime::new(1))
            .insert_resource(player_runtime.clone())
            .insert_resource(crate::menu::MenuRuntime::new(false, 2, "Tester".into()))
            .insert_resource(presentation)
            .insert_resource(SemanticInputSnapshot::default())
            .insert_resource(AccumulatedMouseScroll {
                delta: Vec2::new(0.0, -1.0),
                ..Default::default()
            })
            .insert_resource(NetworkHandle::disconnected())
            .insert_resource(ClientWorld::default());
        app.world_mut().run_system_once(select_hotbar_slot).unwrap();
        *player_runtime = app
            .world_mut()
            .remove_resource::<crate::player_runtime::PlayerRuntime>()
            .unwrap();
        let slot = player_runtime.selected_hotbar_slot().unwrap();
        (slot, app.world_mut().remove_resource().unwrap())
    }

    /// A client part's modal screen is a screen like any other: while it is open the wheel and
    /// every router-gated gameplay action stop reaching the player, and closing it (Escape or
    /// `ui.close-screen`, both of which clear its template) gives them back.
    #[test]
    fn menu_input_leak_client_part_modal_absorbs_gameplay_until_closed() {
        use client_ui::test_support::fixture_font;
        use client_ui::ui_runtime::presentation::{ExperienceModal, UiPresentationRuntime};
        use server_experience::screen::{Files, Modal};

        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
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
        let absorbs = |player_runtime: &crate::player_runtime::PlayerRuntime,
                       presentation: &UiPresentationRuntime| {
            crate::screen_policy::absorbs_input(
                player_runtime,
                Some(&UiRuntime::new(1)),
                Some(&crate::menu::MenuRuntime::new(false, 2, "Tester".into())),
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
    fn unknown_slot_retains_pending_selection_without_sending() {
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

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

        assert_eq!(sends, 0);
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), Some(2));
        assert_eq!(fatal, None);
    }

    #[test]
    fn matching_equipment_bootstrap_sends_while_ledger_slot_is_unknown() {
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

        let mut runtime = identified_runtime(&mut player_runtime);
        let equipment_stack = present_stack();
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(2, game_mode);
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

        let session = protocol::BedrockSession { shield_item_id: 0 };
        let expected = select_hotbar_slot_packet(42, 2, &equipment_stack).unwrap();
        assert_eq!(
            protocol::encode(&sent.unwrap(), &session).unwrap(),
            protocol::encode(&expected, &session).unwrap()
        );
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
        assert_eq!(fatal, None);
    }

    #[test]
    fn full_retry_rebuilds_packet_from_the_current_selected_snapshot() {
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

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

        let session = protocol::BedrockSession { shield_item_id: 0 };
        let expected = select_hotbar_slot_packet(42, 2, &NetworkItemStack::empty()).unwrap();
        assert_eq!(
            protocol::encode(&retried.unwrap(), &session).unwrap(),
            protocol::encode(&expected, &session).unwrap()
        );
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
        assert_eq!(fatal, None);
    }

    #[test]
    fn full_send_retries_next_frame_without_new_input() {
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

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
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

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
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

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
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

        let mut runtime = UiRuntime::new(1);
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(5, game_mode);

        crate::session::begin_session(&mut runtime, &mut player_runtime, 2);

        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
    }

    #[test]
    fn predicted_slot_state_drives_packet_and_rollback_restores_authority() {
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

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
        let session = protocol::BedrockSession { shield_item_id: 0 };
        let predicted_bytes = protocol::encode(&predicted_packet.unwrap(), &session).unwrap();
        let empty_packet = select_hotbar_slot_packet(42, 0, &NetworkItemStack::empty()).unwrap();
        assert_eq!(
            predicted_bytes,
            protocol::encode(&empty_packet, &session).unwrap()
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
        let restored_bytes = protocol::encode(&restored_packet.unwrap(), &session).unwrap();
        let authoritative_packet = select_hotbar_slot_packet(42, 0, &authoritative).unwrap();
        assert_eq!(
            restored_bytes,
            protocol::encode(&authoritative_packet, &session).unwrap()
        );
        assert_eq!(fatal, None);
    }

    /// A slot still holding a predicted stack id is announced in the frame it is selected.
    #[test]
    fn predicted_selected_slot_sends_without_waiting_for_correction() {
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
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
        let session = protocol::BedrockSession { shield_item_id: 0 };
        let expected = select_hotbar_slot_packet(42, 0, &predicted).unwrap();
        assert_eq!(
            protocol::encode(&sent[0], &session).unwrap(),
            protocol::encode(&expected, &session).unwrap()
        );
        assert_eq!(player_runtime.inventory.pending_hotbar_selection(), None);
        assert_eq!(fatal, None);
    }

    /// A server stack without network tracking sends at once.
    #[test]
    fn untracked_stack_identity_does_not_defer_hotbar_selection() {
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
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
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

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
