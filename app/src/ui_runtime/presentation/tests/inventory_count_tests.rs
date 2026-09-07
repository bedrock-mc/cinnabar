//! Regression coverage for authoritative stack counts whose item identity has
//! no packed sprite. Occupied cells keep their existing decorations without
//! fabricating artwork for the missing icon.

use std::sync::Arc;

use protocol::{
    ActorHandedness, ContainerIdentity, ContainerOpenEvent, EquipmentEvent, InventoryContentEvent,
    InventoryEvent, NetworkItemStack,
};
use sha2::{Digest, Sha256};

use super::{fixture_font, fixture_hud};
use crate::ui_runtime::presentation::{IconRef, UiPresentationRuntime};
use crate::ui_runtime::{UiRuntime, inventory_ledger::GENERIC_STORAGE_WINDOW_TYPE};

const COUNT_QUADS: usize = 4;
const VERTICES_PER_QUAD: usize = 4;

fn stack(count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id: 1,
        metadata: 0,
        stack_network_id: 1,
        count,
        nbt_digest: Sha256::digest([]).into(),
        block_runtime_id: 0,
        extra_data: Arc::from([]),
    }
}

fn empty_player_slots() -> Vec<NetworkItemStack> {
    vec![NetworkItemStack::empty(); 36]
}

fn player_content(slots: Vec<NetworkItemStack>) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    })
}

fn build_vertices(presentation: &mut UiPresentationRuntime, runtime: &UiRuntime) -> usize {
    presentation
        .build(runtime, 0, [1280, 720], ui::DpiScale::new(1.0).unwrap())
        .unwrap()
        .vertices
        .len()
}

fn personal_inventory(slot: Option<(usize, u16)>) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(protocol::InventoryAuthority::Server);
    runtime.publish_local_runtime_id(1, 42).unwrap();
    if let Some((index, count)) = slot {
        let mut slots = empty_player_slots();
        slots[index] = stack(count);
        runtime.inventory_ledger_mut().apply(&player_content(slots));
    }
    runtime.toggle_inventory();
    runtime
}

fn storage_inventory(
    slot_count: usize,
    storage_slot: Option<(usize, u16)>,
    player_slot: Option<(usize, u16)>,
    cursor_count: Option<u16>,
) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    if let Some((index, count)) = player_slot {
        let mut slots = empty_player_slots();
        slots[index] = stack(count);
        runtime.inventory_ledger_mut().apply(&player_content(slots));
    }
    let identity = ContainerIdentity {
        window_id: Some(7),
        slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
        dynamic_id: None,
    };
    runtime
        .enqueue_inventory_event(
            1,
            1,
            InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(7),
                window_type: GENERIC_STORAGE_WINDOW_TYPE,
                position: [0; 3],
                runtime_entity_id: 0,
            }),
        )
        .unwrap();
    let mut slots = vec![NetworkItemStack::empty(); slot_count];
    if let Some((index, count)) = storage_slot {
        slots[index] = stack(count);
    }
    runtime
        .enqueue_inventory_event(
            1,
            2,
            InventoryEvent::Content(InventoryContentEvent {
                container: identity,
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory();
    if let Some(count) = cursor_count {
        runtime
            .inventory_ledger_mut()
            .apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity {
                    window_id: None,
                    slot_type: Some(protocol::CONTAINER_NAME_CURSOR),
                    dynamic_id: None,
                },
                slots: Arc::from([stack(count)]),
                storage_item: NetworkItemStack::empty(),
            }));
        runtime.set_inventory_pointer_gui(Some([320.0, 180.0]));
    }
    runtime
}

fn assert_missing_icon_count_delta(baseline: usize, counted: usize) {
    assert_eq!(
        counted,
        baseline + COUNT_QUADS * VERTICES_PER_QUAD,
        "two count glyphs and their shadows render without an item sprite"
    );
}

#[test]
fn personal_main_and_hotbar_counts_do_not_depend_on_item_icons() {
    for slot in [0, 9] {
        let mut presentation =
            UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
        let unknown = build_vertices(&mut presentation, &personal_inventory(None));
        let empty = build_vertices(&mut presentation, &personal_inventory(Some((slot, 0))));
        let single = build_vertices(&mut presentation, &personal_inventory(Some((slot, 1))));
        let counted = build_vertices(&mut presentation, &personal_inventory(Some((slot, 22))));

        assert_eq!(empty, unknown, "empty cells add no item geometry");
        assert_eq!(
            single, empty,
            "one-count stacks add no count or phantom icon"
        );
        assert_missing_icon_count_delta(single, counted);

        presentation.hud_frame_mut().inventory_icons.0[slot] = Some(IconRef {
            page: 0,
            uv: [0, 0, 1, 1],
        });
        let with_icon = build_vertices(&mut presentation, &personal_inventory(Some((slot, 22))));
        assert_eq!(
            with_icon,
            counted + VERTICES_PER_QUAD,
            "a resolved icon still adds exactly its existing sprite quad"
        );
    }
}

#[test]
fn storage_27_and_54_counts_do_not_depend_on_item_icons() {
    for slot_count in [27, 54] {
        let mut presentation =
            UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
        let baseline = build_vertices(
            &mut presentation,
            &storage_inventory(slot_count, None, None, None),
        );
        let storage_count = build_vertices(
            &mut presentation,
            &storage_inventory(slot_count, Some((slot_count - 1, 22)), None, None),
        );
        assert_missing_icon_count_delta(baseline, storage_count);

        let player_count = build_vertices(
            &mut presentation,
            &storage_inventory(slot_count, None, Some((9, 22)), None),
        );
        assert_missing_icon_count_delta(baseline, player_count);
    }
}

#[test]
fn cursor_count_renders_without_an_icon_on_personal_and_storage_screens() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut personal_single = personal_inventory(None);
    personal_single
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: None,
                slot_type: Some(protocol::CONTAINER_NAME_CURSOR),
                dynamic_id: None,
            },
            slots: Arc::from([stack(1)]),
            storage_item: NetworkItemStack::empty(),
        }));
    personal_single.set_inventory_pointer_gui(Some([320.0, 180.0]));
    let single = build_vertices(&mut presentation, &personal_single);

    let mut personal_counted = personal_inventory(None);
    personal_counted
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: None,
                slot_type: Some(protocol::CONTAINER_NAME_CURSOR),
                dynamic_id: None,
            },
            slots: Arc::from([stack(22)]),
            storage_item: NetworkItemStack::empty(),
        }));
    personal_counted.set_inventory_pointer_gui(Some([320.0, 180.0]));
    let counted = build_vertices(&mut presentation, &personal_counted);
    assert_missing_icon_count_delta(single, counted);

    let storage_single = build_vertices(
        &mut presentation,
        &storage_inventory(27, None, None, Some(1)),
    );
    let storage_counted = build_vertices(
        &mut presentation,
        &storage_inventory(27, None, None, Some(22)),
    );
    assert_missing_icon_count_delta(storage_single, storage_counted);
}

#[test]
fn offhand_count_does_not_depend_on_item_icon() {
    let runtime = |count| {
        let mut runtime = UiRuntime::new(1);
        runtime.publish_inventory_authority(protocol::InventoryAuthority::Server);
        runtime.publish_local_runtime_id(1, 42).unwrap();
        runtime.retain_local_selected_equipment(
            1,
            EquipmentEvent {
                actor_runtime_id: 7,
                stack: stack(count),
                inventory_slot: 0,
                selected_slot: 0,
                window_id: protocol::OFFHAND_WINDOW_ID as u8,
                handedness: Some(ActorHandedness::Left),
            },
        );
        runtime.toggle_inventory();
        runtime
    };
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let single = build_vertices(&mut presentation, &runtime(1));
    let counted = build_vertices(&mut presentation, &runtime(22));
    assert_missing_icon_count_delta(single, counted);
}
