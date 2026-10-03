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

fn build_vertices(
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    player_runtime: &crate::player_runtime::PlayerRuntime,
) -> usize {
    presentation
        .build(
            player_runtime,
            runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
        .vertices
        .len()
}

fn personal_inventory(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    slot: Option<(usize, u16)>,
) -> UiRuntime {
    *player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(player_runtime, protocol::InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(player_runtime, 1, 42)
        .unwrap();
    if let Some((index, count)) = slot {
        let mut slots = empty_player_slots();
        slots[index] = stack(count);
        runtime
            .inventory_ledger_mut(player_runtime)
            .apply(&player_content(slots));
    }
    runtime.toggle_inventory(player_runtime);
    runtime
}

fn storage_inventory(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    slot_count: usize,
    storage_slot: Option<(usize, u16)>,
    player_slot: Option<(usize, u16)>,
    cursor_count: Option<u16>,
) -> UiRuntime {
    *player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    if let Some((index, count)) = player_slot {
        let mut slots = empty_player_slots();
        slots[index] = stack(count);
        runtime
            .inventory_ledger_mut(player_runtime)
            .apply(&player_content(slots));
    }
    let identity = ContainerIdentity {
        window_id: Some(7),
        slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
        dynamic_id: None,
    };
    runtime
        .enqueue_inventory_event(
            player_runtime,
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
            player_runtime,
            1,
            2,
            InventoryEvent::Content(InventoryContentEvent {
                container: identity,
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(player_runtime);
    if let Some(count) = cursor_count {
        runtime
            .inventory_ledger_mut(player_runtime)
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
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    for slot in [0, 9] {
        let mut presentation =
            UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
        let unknown = build_vertices(
            &mut presentation,
            &personal_inventory(&mut player_runtime, None),
            &player_runtime,
        );
        let empty = build_vertices(
            &mut presentation,
            &personal_inventory(&mut player_runtime, Some((slot, 0))),
            &player_runtime,
        );
        let single = build_vertices(
            &mut presentation,
            &personal_inventory(&mut player_runtime, Some((slot, 1))),
            &player_runtime,
        );
        let counted = build_vertices(
            &mut presentation,
            &personal_inventory(&mut player_runtime, Some((slot, 22))),
            &player_runtime,
        );

        assert_eq!(empty, unknown, "empty cells add no item geometry");
        assert_eq!(
            single, empty,
            "one-count stacks add no count or phantom icon"
        );
        assert_missing_icon_count_delta(single, counted);

        presentation.hud_frame_mut().inventory_icons.0[slot] = Some(IconRef {
            page: 0,
            uv: [0, 0, 1, 1],
            glint: false,
        });
        let with_icon = build_vertices(
            &mut presentation,
            &personal_inventory(&mut player_runtime, Some((slot, 22))),
            &player_runtime,
        );
        assert_eq!(
            with_icon,
            counted + VERTICES_PER_QUAD,
            "a resolved icon still adds exactly its existing sprite quad"
        );
    }
}

#[test]
fn ordinary_block_icon_uses_existing_inventory_and_hotbar_quads() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    use crate::ui_runtime::presentation::refresh_hud_frame;
    let stream = client_world::WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    assert_eq!(
        stream
            .canonical_item_stack(&stack(22))
            .unwrap()
            .identifier
            .as_deref(),
        Some("minecraft:stone"),
        "the fixture must follow the retained real registry"
    );
    let sprite = assets::IconSprite {
        width: 16,
        height: 16,
        rgba8: vec![255; 16 * 16 * 4].into(),
    };
    let icon = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog(
                [5; 32],
                &[sprite],
                &[assets::IconEntry {
                    identifier: "minecraft:stone".into(),
                    metadata: 0,
                    sprite: 0,
                }],
            )
            .unwrap(),
        )
        .unwrap(),
    );
    for slot in [0, 9] {
        let mut presentation = UiPresentationRuntime::with_hud_and_icons(
            fixture_font(),
            fixture_hud(),
            Arc::clone(&icon),
        )
        .unwrap();
        let mut runtime = personal_inventory(&mut player_runtime, Some((slot, 22)));
        let mut without_icon =
            UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
        refresh_hud_frame(
            &player_runtime,
            &mut runtime,
            &mut without_icon,
            Some(&stream),
            &crate::camera::CameraSettingsAuthority::default(),
            1,
        );
        let baseline = build_vertices(&mut without_icon, &runtime, &player_runtime);
        let resolved = presentation.item_icon("minecraft:stone", 0).unwrap();
        refresh_hud_frame(
            &player_runtime,
            &mut runtime,
            &mut presentation,
            Some(&stream),
            &crate::camera::CameraSettingsAuthority::default(),
            1,
        );
        assert_eq!(
            presentation.hud_frame().inventory_icons.0[slot],
            Some(resolved)
        );
        if slot < 9 {
            assert_eq!(presentation.hud_frame().hotbar_icons[slot], Some(resolved));
        }
        let with_icon = build_vertices(&mut presentation, &runtime, &player_runtime);
        assert_eq!(with_icon, baseline + 4);
        assert_eq!(presentation.item_icon("minecraft:stone", 1), Some(resolved));
        assert!(presentation.item_icon("minecraft:unknown", 0).is_none());
    }
}

#[test]
fn storage_27_and_54_counts_do_not_depend_on_item_icons() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    for slot_count in [27, 54] {
        let mut presentation =
            UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
        let baseline = build_vertices(
            &mut presentation,
            &storage_inventory(&mut player_runtime, slot_count, None, None, None),
            &player_runtime,
        );
        let storage_count = build_vertices(
            &mut presentation,
            &storage_inventory(
                &mut player_runtime,
                slot_count,
                Some((slot_count - 1, 22)),
                None,
                None,
            ),
            &player_runtime,
        );
        assert_missing_icon_count_delta(baseline, storage_count);

        let player_count = build_vertices(
            &mut presentation,
            &storage_inventory(&mut player_runtime, slot_count, None, Some((9, 22)), None),
            &player_runtime,
        );
        assert_missing_icon_count_delta(baseline, player_count);
    }
}

#[test]
fn cursor_count_renders_without_an_icon_on_personal_and_storage_screens() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut personal_single = personal_inventory(&mut player_runtime, None);
    personal_single
        .inventory_ledger_mut(&mut player_runtime)
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
    let single = build_vertices(&mut presentation, &personal_single, &player_runtime);

    let mut personal_counted = personal_inventory(&mut player_runtime, None);
    personal_counted
        .inventory_ledger_mut(&mut player_runtime)
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
    let counted = build_vertices(&mut presentation, &personal_counted, &player_runtime);
    assert_missing_icon_count_delta(single, counted);

    let storage_single = build_vertices(
        &mut presentation,
        &storage_inventory(&mut player_runtime, 27, None, None, Some(1)),
        &player_runtime,
    );
    let storage_counted = build_vertices(
        &mut presentation,
        &storage_inventory(&mut player_runtime, 27, None, None, Some(22)),
        &player_runtime,
    );
    assert_missing_icon_count_delta(storage_single, storage_counted);
}

#[test]
fn offhand_count_does_not_depend_on_item_icon() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let runtime = |player_runtime: &mut crate::player_runtime::PlayerRuntime, count| {
        *player_runtime = crate::player_runtime::PlayerRuntime::new(1);
        let mut runtime = UiRuntime::new(1);
        runtime.publish_inventory_authority(player_runtime, protocol::InventoryAuthority::Server);
        runtime
            .publish_local_runtime_id(player_runtime, 1, 42)
            .unwrap();
        runtime.retain_local_selected_equipment(
            player_runtime,
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
        runtime.toggle_inventory(player_runtime);
        runtime
    };
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let single = build_vertices(
        &mut presentation,
        &runtime(&mut player_runtime, 1),
        &player_runtime,
    );
    let counted = build_vertices(
        &mut presentation,
        &runtime(&mut player_runtime, 22),
        &player_runtime,
    );
    assert_missing_icon_count_delta(single, counted);
}

#[test]
fn review_carried_item_draws_after_the_recipe_panel() {
    use crate::ui_runtime::presentation::hud_layout::{HudGeometry, HudLayout};
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = personal_inventory(&mut player_runtime, None);
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: None,
                slot_type: Some(protocol::CONTAINER_NAME_CURSOR),
                dynamic_id: None,
            },
            slots: Arc::from([stack(1)]),
            storage_item: NetworkItemStack::empty(),
        }));
    runtime.set_inventory_pointer_gui(Some([80.0, 120.0]));
    runtime.screen_state_mut().book_open = true;
    let presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
    let frame = super::super::hud_layout::HudFrame {
        cursor_icon: Some(IconRef {
            page: 77,
            uv: [0, 0, 16, 16],
            glint: false,
        }),
        ..Default::default()
    };
    let mut layout = HudLayout::new(
        &mut nodes,
        &mut next,
        presentation.hud_textures.as_ref().unwrap(),
        &mut layouts,
        &presentation.font,
        0,
        HudGeometry::new([1280, 720], 1.0, ui::SafeArea::ZERO, Some(2)).unwrap(),
    )
    .unwrap();
    layout
        .append(&player_runtime, &runtime, &frame, true)
        .unwrap();
    let cursor = nodes
        .iter()
        .position(|node| {
            matches!(
                node.visual(),
                ui::UiVisual::Sprite {
                    texture_page: 77,
                    ..
                }
            )
        })
        .unwrap();
    assert_eq!(
        cursor,
        nodes.len() - 1,
        "the recipe panel painted over the carried item"
    );
}
