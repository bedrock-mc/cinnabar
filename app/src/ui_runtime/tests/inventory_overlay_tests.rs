//! Authoritative item-stack response overlays: accepted corrections retain
//! server-owned display names and durability damage per cell, every other
//! authoritative cell replacement clears or replaces them, and presentation
//! prefers them over locally derived facts.

use std::sync::Arc;

use protocol::{
    ContainerIdentity, InventoryAuthority, InventoryContentEvent, InventoryEvent,
    InventorySlotEvent, ItemStackResponseEvent, NetworkItemStack, SlotIdentity, StackResponse,
    StackResponseContainer, StackResponseSlot, StackResponseStatus, WorldBootstrap,
};
use sha2::{Digest, Sha256};

use super::*;
use crate::ui_runtime::inventory_ledger::{
    GENERIC_STORAGE_SLOT_TYPE, GENERIC_STORAGE_WINDOW_TYPE, INVENTORY_REQUEST_TIMEOUT_MILLIS,
    PLAYER_INVENTORY_SLOT_COUNT, SMALL_STORAGE_SLOT_COUNT,
};
use crate::{
    camera::CameraSettingsAuthority,
    ui_runtime::presentation::{UiPresentationRuntime, refresh_hud_frame, tests::fixture_font},
};

fn ledger_stack(network_id: i32, stack_network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id,
        count,
        nbt_digest: Sha256::digest([]).into(),
        block_runtime_id: 0,
        extra_data: Arc::from([]),
    }
}

fn publish_slot(runtime: &mut UiRuntime, slot: u8, stack: NetworkItemStack) {
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: u16::from(slot),
            },
            stack,
            storage_item: None,
        }));
}

fn correction(
    slot: u8,
    count: u8,
    item_stack_id: i32,
    name: &str,
    filtered_name: &str,
    durability: i32,
) -> StackResponseSlot {
    StackResponseSlot {
        slot,
        hotbar_slot: slot,
        count,
        item_stack_id,
        custom_name: Arc::from(name),
        filtered_custom_name: Arc::from(filtered_name),
        durability_correction: durability,
    }
}

fn accepted_response(
    request_id: i32,
    slot_type: Option<u8>,
    dynamic_id: Option<u32>,
    slots: Vec<StackResponseSlot>,
) -> InventoryEvent {
    InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from([StackResponseContainer {
                container: ContainerIdentity {
                    window_id: None,
                    slot_type,
                    dynamic_id,
                },
                slots: Arc::from(slots),
            }]),
        }]),
    })
}

/// Drives one take/place gesture pair so an accepted response corrects the
/// freshly placed stack in player-inventory slot 0.
fn corrected_sword_in_slot_zero() -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    publish_slot(&mut runtime, 1, ledger_stack(745, 13, 4));
    publish_slot(&mut runtime, 0, NetworkItemStack::empty());
    let take = runtime.inventory_ledger_mut().begin_click(1).unwrap();
    runtime
        .inventory_ledger_mut()
        .apply(&accepted_response(take, Some(12), None, Vec::new()));
    let place = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime.inventory_ledger_mut().apply(&accepted_response(
        place,
        Some(12),
        None,
        vec![correction(0, 2, 99, "Renamed Blade", "Filtered Blade", 125)],
    ));
    runtime
}

#[test]
fn accepted_corrections_retain_names_and_durability_on_the_corrected_cell() {
    let runtime = corrected_sword_in_slot_zero();

    let displayed = runtime.inventory_ledger().displayed_stack(0).unwrap();
    assert_eq!(displayed.network_id, 745);
    assert_eq!(displayed.count, 2);
    assert_eq!(displayed.stack_network_id, 99);
    let overlay = runtime
        .inventory_ledger()
        .slot_overlay(0)
        .expect("overlay retained");
    assert_eq!(overlay.custom_name.as_deref(), Some("Renamed Blade"));
    assert_eq!(
        overlay.filtered_custom_name.as_deref(),
        Some("Filtered Blade")
    );
    assert_eq!(overlay.durability_correction, Some(125));
    // Cells the server did not correct carry no overlay.
    assert_eq!(runtime.inventory_ledger().slot_overlay(1), None);
    assert_eq!(runtime.inventory_ledger().cursor_overlay(), None);
}

#[test]
fn authoritative_slot_replacement_clears_the_response_overlay() {
    let mut runtime = corrected_sword_in_slot_zero();
    assert!(
        runtime
            .inventory_ledger()
            .slot_overlay(0)
            .is_some_and(|overlay| overlay.custom_name.as_deref() == Some("Renamed Blade"))
    );

    publish_slot(&mut runtime, 0, ledger_stack(745, 41, 3));

    assert_eq!(runtime.inventory_ledger().slot_overlay(0), None);
    let replaced = runtime.inventory_ledger().displayed_stack(0).unwrap();
    assert_eq!(replaced.count, 3);
    assert_eq!(replaced.stack_network_id, 41);
}

#[test]
fn full_inventory_content_replaces_overlays_of_every_rewritten_cell() {
    let mut runtime = corrected_sword_in_slot_zero();

    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: Arc::from(vec![ledger_stack(745, 5, 1); PLAYER_INVENTORY_SLOT_COUNT]),
            storage_item: NetworkItemStack::empty(),
        }));

    assert_eq!(runtime.inventory_ledger().slot_overlay(0), None);
    let replaced = runtime.inventory_ledger().displayed_stack(0).unwrap();
    assert_eq!(replaced.stack_network_id, 5);
}

#[test]
fn cursor_updates_clear_the_cursor_response_overlay() {
    let mut runtime = corrected_sword_in_slot_zero();
    // Taking the corrected sword moves it to the cursor; the same accepted
    // response carries a cursor correction with its own authoritative names.
    let take = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime.inventory_ledger_mut().apply(&accepted_response(
        take,
        Some(59),
        None,
        vec![correction(0, 2, 55, "Cursor Blade", "Cursor Blade", 60)],
    ));
    assert_eq!(
        runtime
            .inventory_ledger()
            .cursor_overlay()
            .and_then(|overlay| overlay.durability_correction),
        Some(60)
    );

    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(0),
                    slot_type: Some(59),
                    dynamic_id: None,
                },
                slot: 0,
            },
            stack: ledger_stack(746, 88, 1),
            storage_item: None,
        }));

    assert_eq!(runtime.inventory_ledger().cursor_overlay(), None);
}

#[test]
fn empty_cell_corrections_clear_the_overlay_with_the_stack() {
    let mut runtime = corrected_sword_in_slot_zero();
    assert!(runtime.inventory_ledger().slot_overlay(0).is_some());

    let take = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime.inventory_ledger_mut().apply(&accepted_response(
        take,
        Some(12),
        None,
        vec![correction(0, 0, -1, "", "", 0)],
    ));

    assert_eq!(runtime.inventory_ledger().displayed_stack(0), None);
    assert_eq!(runtime.inventory_ledger().slot_overlay(0), None);
}

#[test]
fn rejected_responses_rollback_without_writing_overlays() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    publish_slot(&mut runtime, 0, ledger_stack(745, 13, 4));

    let request_id = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Response(ItemStackResponseEvent {
            responses: Arc::from([StackResponse {
                status: StackResponseStatus::Rejected,
                request_id,
                containers: Arc::from([]),
            }]),
        }));

    let restored = runtime.inventory_ledger().displayed_stack(0).unwrap();
    assert_eq!(restored.count, 4);
    assert_eq!(restored.stack_network_id, 13);
    assert_eq!(runtime.inventory_ledger().slot_overlay(0), None);
}

#[test]
fn request_timeouts_clear_stale_overlays_through_recovery_marking() {
    let mut runtime = corrected_sword_in_slot_zero();
    assert!(runtime.inventory_ledger().slot_overlay(0).is_some());

    let _request_id = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    assert!(
        runtime
            .inventory_ledger_mut()
            .mark_transport_enqueued(1_000)
    );
    assert!(
        !runtime
            .inventory_ledger_mut()
            .poll_timeout(1_000 + INVENTORY_REQUEST_TIMEOUT_MILLIS)
    );

    assert!(runtime.inventory_ledger().resync_required());
    assert_eq!(runtime.inventory_ledger().slot_overlay(0), None);
    assert_eq!(runtime.inventory_ledger().cursor_overlay(), None);
}

#[test]
fn session_reset_discards_every_retained_overlay() {
    let mut runtime = corrected_sword_in_slot_zero();
    assert!(runtime.inventory_ledger().slot_overlay(0).is_some());

    runtime.begin_session(2);

    assert_eq!(runtime.inventory_ledger().slot_overlay(0), None);
    assert_eq!(runtime.inventory_ledger().cursor_overlay(), None);
}

#[test]
fn storage_corrections_retain_overrides_until_storage_replacement() {
    const STORAGE_WINDOW_ID: i32 = 4;
    const STORAGE_DYNAMIC_ID: u32 = 9;
    let mut runtime = UiRuntime::new(1);
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
            container: ContainerIdentity::window(STORAGE_WINDOW_ID),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            position: [0, 0, 0],
            runtime_entity_id: 1,
        }));
    let mut contents = vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT];
    contents[3] = ledger_stack(745, 13, 1);
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: Some(STORAGE_WINDOW_ID),
                slot_type: Some(GENERIC_STORAGE_SLOT_TYPE),
                dynamic_id: Some(STORAGE_DYNAMIC_ID),
            },
            slots: Arc::from(contents),
            storage_item: NetworkItemStack::empty(),
        }));

    let take = runtime
        .inventory_ledger_mut()
        .begin_storage_click(3)
        .unwrap();
    runtime.inventory_ledger_mut().apply(&accepted_response(
        take,
        Some(GENERIC_STORAGE_SLOT_TYPE),
        Some(STORAGE_DYNAMIC_ID),
        Vec::new(),
    ));
    // Place the stack back; the server's return correction carries the
    // authoritative names and damage for the restored storage cell.
    let place = runtime
        .inventory_ledger_mut()
        .begin_storage_click(3)
        .unwrap();
    runtime.inventory_ledger_mut().apply(&accepted_response(
        place,
        Some(GENERIC_STORAGE_SLOT_TYPE),
        Some(STORAGE_DYNAMIC_ID),
        vec![correction(3, 1, 77, "Stored Blade", "Stored Blade", 42)],
    ));
    assert_eq!(
        runtime
            .inventory_ledger()
            .storage_slot_overlay(3)
            .and_then(|overlay| overlay.durability_correction),
        Some(42)
    );

    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(STORAGE_WINDOW_ID),
                    slot_type: Some(GENERIC_STORAGE_SLOT_TYPE),
                    dynamic_id: Some(STORAGE_DYNAMIC_ID),
                },
                slot: 3,
            },
            stack: ledger_stack(745, 91, 1),
            storage_item: None,
        }));
    assert_eq!(runtime.inventory_ledger().storage_slot_overlay(3), None);
    let replaced = runtime.inventory_ledger().storage_stack(3).unwrap();
    assert_eq!(replaced.stack_network_id, 91);
}

#[test]
fn selected_item_name_prefers_the_authoritative_custom_name() {
    let mut runtime = corrected_sword_in_slot_zero();
    runtime.set_local_selected_slot(0);
    assert_eq!(
        runtime
            .selected_stack_custom_name()
            .map(|name| name.to_string()),
        Some("Renamed Blade".to_owned())
    );

    // Replacing the selected cell falls back to localized identifier naming.
    publish_slot(&mut runtime, 0, ledger_stack(745, 41, 3));
    assert_eq!(runtime.selected_stack_custom_name(), None);
}

#[test]
fn omitted_fields_retain_prior_overlays_and_affirmative_ones_replace_them() {
    let mut runtime = corrected_sword_in_slot_zero();

    // Cycling the corrected sword through the cursor and back, with a return
    // correction that states no names and no positive durability, must not
    // erase the retained overlay: the server restated only what changed.
    let take = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime
        .inventory_ledger_mut()
        .apply(&accepted_response(take, Some(59), None, Vec::new()));
    let place = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime.inventory_ledger_mut().apply(&accepted_response(
        place,
        Some(12),
        None,
        vec![correction(0, 2, 99, "", "", 0)],
    ));

    let overlay = runtime
        .inventory_ledger()
        .slot_overlay(0)
        .expect("overlay survives an omitting correction");
    assert_eq!(overlay.custom_name.as_deref(), Some("Renamed Blade"));
    assert_eq!(
        overlay.filtered_custom_name.as_deref(),
        Some("Filtered Blade")
    );
    assert_eq!(overlay.durability_correction, Some(125));

    // An affirmative restatement replaces every stated field.
    let take = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime
        .inventory_ledger_mut()
        .apply(&accepted_response(take, Some(59), None, Vec::new()));
    let place = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime.inventory_ledger_mut().apply(&accepted_response(
        place,
        Some(12),
        None,
        vec![correction(0, 2, 99, "New Name", "New Filtered", 60)],
    ));
    let overlay = runtime
        .inventory_ledger()
        .slot_overlay(0)
        .expect("overlay retained after the affirmative correction");
    assert_eq!(overlay.custom_name.as_deref(), Some("New Name"));
    assert_eq!(
        overlay.filtered_custom_name.as_deref(),
        Some("New Filtered")
    );
    assert_eq!(overlay.durability_correction, Some(60));
}

#[test]
fn swap_moves_each_stacks_overlay_to_the_opposite_cell() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    publish_slot(&mut runtime, 0, ledger_stack(745, 13, 2));
    publish_slot(&mut runtime, 1, ledger_stack(846, 24, 3));

    // One take whose accepted response affirms distinct authoritative facts
    // for both touched cells: alpha travels to the cursor with the stack it
    // describes, and beta stays attached to its slot.
    let take = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Response(ItemStackResponseEvent {
            responses: Arc::from([StackResponse {
                status: StackResponseStatus::Accepted,
                request_id: take,
                containers: Arc::from([
                    StackResponseContainer {
                        container: ContainerIdentity {
                            window_id: None,
                            slot_type: Some(59),
                            dynamic_id: None,
                        },
                        slots: Arc::from([correction(
                            0,
                            2,
                            13,
                            "Alpha Blade",
                            "Filtered Alpha",
                            100,
                        )]),
                    },
                    StackResponseContainer {
                        container: ContainerIdentity {
                            window_id: None,
                            slot_type: Some(12),
                            dynamic_id: None,
                        },
                        slots: Arc::from([correction(
                            1,
                            3,
                            24,
                            "Beta Blade",
                            "Filtered Beta",
                            200,
                        )]),
                    },
                ]),
            }]),
        }));
    assert_eq!(
        runtime
            .inventory_ledger()
            .cursor_stack()
            .map(|stack| stack.network_id),
        Some(745)
    );
    assert_eq!(
        runtime
            .inventory_ledger()
            .cursor_overlay()
            .and_then(|overlay| overlay.durability_correction),
        Some(100)
    );
    assert_eq!(
        runtime
            .inventory_ledger()
            .slot_overlay(1)
            .and_then(|overlay| overlay.durability_correction),
        Some(200)
    );

    // Swapping the occupied slot with the cursor must move each retained
    // overlay with its own stack: the cursor receives beta's facts and slot 1
    // receives alpha's, while the vacated source cell keeps none.
    let swap = runtime.inventory_ledger_mut().begin_click(1).unwrap();
    runtime
        .inventory_ledger_mut()
        .apply(&accepted_response(swap, Some(12), None, Vec::new()));

    assert_eq!(
        runtime
            .inventory_ledger()
            .cursor_stack()
            .map(|stack| stack.network_id),
        Some(846)
    );
    let beta = runtime
        .inventory_ledger()
        .cursor_overlay()
        .expect("beta's overlay travelled with beta");
    assert_eq!(beta.custom_name.as_deref(), Some("Beta Blade"));
    assert_eq!(beta.filtered_custom_name.as_deref(), Some("Filtered Beta"));
    assert_eq!(beta.durability_correction, Some(200));

    let landed = runtime
        .inventory_ledger()
        .displayed_stack(1)
        .map(|s| s.network_id);
    assert_eq!(landed, Some(745));
    let alpha = runtime
        .inventory_ledger()
        .slot_overlay(1)
        .expect("alpha's overlay travelled with alpha");
    assert_eq!(alpha.custom_name.as_deref(), Some("Alpha Blade"));
    assert_eq!(
        alpha.filtered_custom_name.as_deref(),
        Some("Filtered Alpha")
    );
    assert_eq!(alpha.durability_correction, Some(100));
    assert_eq!(runtime.inventory_ledger().slot_overlay(0), None);
}

const IRON_SWORD_NETWORK_ID: i32 = 309;

/// Builds a damaged vanilla iron sword whose retained user data carries the
/// fixed little-endian root `Damage` integer, digest-bound like the wire.
fn damaged_sword(damage: i32) -> NetworkItemStack {
    let mut extra = Vec::new();
    extra.extend_from_slice(&(-1_i16).to_le_bytes());
    extra.push(1);
    extra.push(10);
    extra.extend_from_slice(&0_u16.to_le_bytes());
    extra.push(3);
    extra.extend_from_slice(&6_u16.to_le_bytes());
    extra.extend_from_slice(b"Damage");
    extra.extend_from_slice(&damage.to_le_bytes());
    extra.push(0);
    NetworkItemStack {
        network_id: IRON_SWORD_NETWORK_ID,
        metadata: 0,
        stack_network_id: -1,
        count: 1,
        nbt_digest: Sha256::digest(&extra).into(),
        block_runtime_id: 0,
        extra_data: Arc::from(extra),
    }
}

fn world_stream() -> client_world::WorldStream {
    client_world::WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    })
}

/// Publishes one HUD frame and reads the selected hotbar cell's durability.
fn presented_selected_durability(
    runtime: &mut UiRuntime,
    stream: &client_world::WorldStream,
) -> Option<f32> {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    refresh_hud_frame(
        runtime,
        &mut presentation,
        Some(stream),
        &CameraSettingsAuthority::default(),
        1_000,
    );
    presentation.hud_frame().hotbar_durability[0]
}

/// Takes the selected sword onto the cursor and places it back through one
/// accepted response whose only restatement is the given final correction.
fn round_tripped_selected_sword(final_correction: StackResponseSlot) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    publish_slot(&mut runtime, 0, damaged_sword(125));
    runtime.set_local_selected_slot(0);
    let take = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime
        .inventory_ledger_mut()
        .apply(&accepted_response(take, Some(12), None, Vec::new()));
    let place = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime.inventory_ledger_mut().apply(&accepted_response(
        place,
        Some(12),
        None,
        vec![final_correction],
    ));
    runtime
}

#[test]
fn count_only_corrections_keep_the_locally_derived_durability_bar() {
    let mut runtime = round_tripped_selected_sword(correction(0, 1, -1, "", "", 0));
    let stream = world_stream();

    let overlay = runtime
        .inventory_ledger()
        .slot_overlay(0)
        .expect("the corrected cell retains its response overlay");
    assert_eq!(
        overlay.durability_correction, None,
        "unstated durability must stay absent instead of defaulting"
    );
    assert_eq!(overlay.custom_name.as_deref(), None);
    assert_eq!(overlay.filtered_custom_name.as_deref(), None);

    let presented = presented_selected_durability(&mut runtime, &stream);
    let derived = item_facts::durability_fraction(
        runtime.inventory_ledger().displayed_stack(0).unwrap(),
        Some("minecraft:iron_sword"),
    );
    assert!(
        derived.is_some_and(|fraction| (fraction - 0.5).abs() < 0.01),
        "the local damage tag alone must still derive the bar: {derived:?}"
    );
    assert_eq!(
        presented, derived,
        "a count-only correction must not silence locally derived durability"
    );
}

#[test]
fn stated_durability_corrections_override_the_local_damage_tag() {
    // The local tag reads half-worn (125/250), but the accepted correction
    // restates fully damaged (250): presentation must follow the server.
    let mut runtime = round_tripped_selected_sword(correction(0, 1, -1, "", "", 250));
    let stream = world_stream();

    let presented = presented_selected_durability(&mut runtime, &stream);
    assert_eq!(
        presented,
        Some(0.0),
        "the stated correction must override local derivation"
    );
}

#[test]
fn prior_retained_overlays_survive_a_rejected_response() {
    let mut runtime = corrected_sword_in_slot_zero();
    let request_id = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Response(ItemStackResponseEvent {
            responses: Arc::from([StackResponse {
                status: StackResponseStatus::Rejected,
                request_id,
                containers: Arc::from([]),
            }]),
        }));

    let restored = runtime.inventory_ledger().displayed_stack(0).unwrap();
    assert_eq!(restored.network_id, 745);
    assert_eq!(restored.count, 2);
    let overlay = runtime
        .inventory_ledger()
        .slot_overlay(0)
        .expect("the committed overlay survives rejection");
    assert_eq!(overlay.custom_name.as_deref(), Some("Renamed Blade"));
    assert_eq!(
        overlay.filtered_custom_name.as_deref(),
        Some("Filtered Blade")
    );
    assert_eq!(overlay.durability_correction, Some(125));
}

/// Drains one full window-0 content event through the production queue so
/// both retained stores agree, then selects hotbar slot 0 so slot 3 stays a
/// nonselected cell for every witness below.
fn drained_inventory_runtime() -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(InventoryAuthority::Server);
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = ledger_stack(745, 13, 4);
    slots[3] = ledger_stack(846, 24, 1);
    runtime
        .enqueue_inventory_event(
            1,
            1,
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: Arc::from(slots),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory();
    runtime.set_local_selected_slot(0);
    runtime
}

/// Publishes one HUD frame and returns its presented hotbar stacks.
fn presented_hotbar_stacks(runtime: &mut UiRuntime) -> [Option<protocol::NetworkItemStack>; 9] {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    refresh_hud_frame(
        runtime,
        &mut presentation,
        Some(&world_stream()),
        &CameraSettingsAuthority::default(),
        1_000,
    );
    presentation.hud_frame().hotbar_stacks.clone()
}

fn cell_facts(stack: &protocol::NetworkItemStack) -> (u16, i32) {
    (stack.count, stack.stack_network_id)
}

#[test]
fn accepted_sparse_corrections_refresh_every_presented_nonselected_hotbar_consumer() {
    let mut runtime = drained_inventory_runtime();

    // Round-trip the nonselected sword through the cursor; the accepted place
    // response restates only slot 3: a server-side count change plus its
    // authoritative identity correction.
    let take = runtime.inventory_ledger_mut().begin_click(3).unwrap();
    runtime
        .inventory_ledger_mut()
        .apply(&accepted_response(take, Some(12), None, Vec::new()));
    let place = runtime.inventory_ledger_mut().begin_click(3).unwrap();
    runtime.inventory_ledger_mut().apply(&accepted_response(
        place,
        Some(12),
        None,
        vec![correction(3, 2, 99, "Corrected Blade", "", 250)],
    ));
    let corrected = runtime
        .inventory_ledger()
        .displayed_stack(3)
        .expect("the corrected cell stays present in the ledger");
    assert_eq!(cell_facts(corrected), (2, 99));

    // The presented nonselected cell and the HUD frame must show that exact
    // ledger revision, not the stale pre-correction mirror.
    assert_eq!(
        runtime
            .presented_hotbar_stack(3)
            .map(cell_facts)
            .expect("the corrected nonselected cell stays presented"),
        (2, 99),
        "the presented nonselected hotbar cell must follow the accepted correction",
    );
    let frame_stacks = presented_hotbar_stacks(&mut runtime);
    let presented = frame_stacks[3]
        .as_ref()
        .expect("the corrected nonselected cell is presented");
    assert_eq!(
        cell_facts(presented),
        (2, 99),
        "the HUD frame must present the corrected ledger snapshot",
    );
    // The selected cell keeps presenting its own untouched authority.
    assert_eq!(
        frame_stacks[0].as_ref().map(cell_facts),
        Some((4, 13)),
        "an unrelated accepted correction must not disturb the selected cell",
    );
}

#[test]
fn rejected_nonselected_gestures_present_the_pre_gesture_cells_again() {
    let mut runtime = drained_inventory_runtime();

    // Mid-flight, the nonselected cell presents its predicted half exactly
    // like the selected-cell authority already does.
    let request_id = runtime.inventory_ledger_mut().begin_click(3).unwrap();
    assert_eq!(runtime.presented_hotbar_stack(3), None);

    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Response(ItemStackResponseEvent {
            responses: Arc::from([StackResponse {
                status: StackResponseStatus::Rejected,
                request_id,
                containers: Arc::from([]),
            }]),
        }));

    // Rejection restores every presented consumer to the exact pre-gesture
    // facts; nothing else about the presented row moves.
    let restored = presented_hotbar_stacks(&mut runtime);
    assert_eq!(restored[3].as_ref().map(cell_facts), Some((1, 24)));
    assert_eq!(restored[0].as_ref().map(cell_facts), Some((4, 13)));
}

#[test]
fn session_reset_presents_no_hotbar_cells_from_either_store() {
    let mut runtime = drained_inventory_runtime();
    assert!(presented_hotbar_stacks(&mut runtime)[3].is_some());

    runtime.begin_session(2);

    let reset = presented_hotbar_stacks(&mut runtime);
    assert!(
        reset.iter().all(Option::is_none),
        "a session reset must clear every presented hotbar cell"
    );
    for slot in 0..9u8 {
        assert_eq!(runtime.presented_hotbar_stack(slot), None);
    }
}
