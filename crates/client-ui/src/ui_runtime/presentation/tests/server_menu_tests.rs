//! Server chest titles survive the HUD capture boundary after UI extraction.

use std::sync::Arc;

use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue,
    ActorSpawnEvent, ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent,
    NetworkItemStack, WorldBootstrap, WorldEvent,
};

use super::{UiPresentationRuntime, UiRuntime, fixture_font};
use crate::ui_runtime::presentation::publish::capture_hud_frame;

mod empty_ack;

const NAME: u32 = protocol_ids::ActorDataKey::Name.wire_value();
const FILTERED_NAME: u32 = protocol_ids::ActorDataKey::FilteredName.wire_value();

/// The local player, UI runtime, presentation and world stream, with a server chest menu
/// opened on a chest minecart whose unique and runtime IDs differ.
fn open_actor_menu() -> (
    player_state::PlayerState,
    UiRuntime,
    UiPresentationRuntime,
    chunk_pipeline::WorldStream,
) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut stream = chunk_pipeline::WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    // Distinct unique and runtime IDs catch use of the wrong actor identity.
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 70,
                runtime_id: 7,
                kind: ActorKind::Entity {
                    identifier: "minecraft:chest_minecart".into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    for (sequence, event) in [
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(4),
            window_type: protocol::WINDOW_TYPE_CONTAINER,
            position: [0; 3],
            runtime_entity_id: 70,
        }),
        InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(4),
            slots: vec![NetworkItemStack::empty(); 45].into(),
            storage_item: NetworkItemStack::empty(),
        }),
    ]
    .into_iter()
    .enumerate()
    {
        runtime
            .enqueue_inventory_event(&mut player, 1, sequence as u64 + 1, event)
            .unwrap();
    }
    runtime.drain_pending_inventory(&mut player);
    assert!(runtime.inventory_open());
    assert_eq!(
        runtime.inventory_ledger(&player).storage_slot_count(),
        Some(45)
    );
    (player, runtime, presentation, stream)
}

/// Syncs `names` as `(actor data key, value)` onto the menu's actor, then returns the
/// captured screen's custom and final titles.
fn titles_after(
    menu: &mut (
        player_state::PlayerState,
        UiRuntime,
        UiPresentationRuntime,
        chunk_pipeline::WorldStream,
    ),
    sequence: u64,
    names: &[(u32, &str)],
) -> (Option<String>, Option<String>) {
    let (player, runtime, presentation, stream) = menu;
    stream
        .submit(
            sequence,
            WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 7,
                metadata: names
                    .iter()
                    .map(|&(key, name)| ActorMetadata {
                        key,
                        value: ActorMetadataValue::String(name.into()),
                    })
                    .collect(),
                properties: Arc::from([]),
                tick: sequence,
            })),
        )
        .unwrap();
    capture_hud_frame(
        player,
        runtime,
        presentation,
        Some(stream),
        semantic_input::PerspectiveMode::FirstPerson,
        sequence,
        Default::default(),
    );
    let text = &presentation.hud_frame().window_text;
    (text.custom_title.clone(), text.title.clone())
}

#[test]
fn server_menu_title_keeps_actor_formatting_and_clears_empty_names() {
    let mut menu = open_actor_menu();
    for (sequence, title) in [(2, "§r§aServer menu\n§lPick a game"), (3, "")] {
        let expected = (!title.is_empty()).then(|| title.to_owned());
        assert_eq!(
            titles_after(&mut menu, sequence, &[(NAME, title)]),
            (expected.clone(), expected)
        );
    }
}

#[test]
fn server_menu_title_prefers_a_non_empty_filtered_actor_name() {
    let mut menu = open_actor_menu();
    let shown = |text: &str| (Some(text.to_owned()), Some(text.to_owned()));
    assert_eq!(
        titles_after(
            &mut menu,
            2,
            &[(NAME, "Raw title"), (FILTERED_NAME, "F****d title")]
        ),
        shown("F****d title")
    );
    // An empty filtered name falls back to the name tag.
    assert_eq!(
        titles_after(&mut menu, 3, &[(FILTERED_NAME, "")]),
        shown("Raw title")
    );
}

#[test]
fn server_menu_default_zero_descriptor_draws_chest_cells() {
    assert_rendered_chest(0, "server-chest-default-descriptor", false);
}

#[test]
fn repeated_chest_open_keeps_the_rendered_chest_screen() {
    assert_rendered_chest(20, "server-chest-retried-open", false);
}

#[test]
fn server_menu_delivery_ack_keeps_the_menu_open() {
    assert_rendered_chest(1, "server-chest-delivery-ack", true);
}

fn assert_rendered_chest(retries: u64, snapshot_name: &str, require_delivery_ack: bool) {
    use crate::ui_runtime::presentation::{forms, inventory_pointer::InventoryScreen};
    let Some(mut presentation) =
        crate::test_support::engine_presentation_with(forms::pack_harness::font())
    else {
        eprintln!(
            "skipping server_menu_default_zero_descriptor_draws_chest_cells: missing local UI carrier; make assets"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    let open = ContainerOpenEvent {
        container: ContainerIdentity::window(4),
        window_type: protocol::WINDOW_TYPE_CONTAINER,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    };
    for (sequence, event) in [
        InventoryEvent::Open(open),
        InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: Some(4),
                slot_type: Some(0),
                dynamic_id: Some(0),
            },
            slots: vec![
                NetworkItemStack::empty();
                inventory::inventory_ledger::LARGE_STORAGE_SLOT_COUNT
            ]
            .into(),
            storage_item: NetworkItemStack::empty(),
        }),
    ]
    .into_iter()
    .enumerate()
    {
        runtime
            .enqueue_inventory_event(&mut player, 1, sequence as u64 + 1, event)
            .unwrap();
    }
    runtime.drain_pending_inventory(&mut player);
    for retry in 0..retries {
        runtime
            .enqueue_inventory_event(&mut player, 1, retry + 3, InventoryEvent::Open(open))
            .unwrap();
    }
    runtime.drain_pending_inventory(&mut player);
    // A menu dispatcher closes a window whose delivery was never acknowledged.
    if require_delivery_ack
        && runtime.inventory_ledger(&player).pending_batch().unwrap()
            != Some((protocol::repeated_container_open_warning_packet(), 1))
    {
        runtime
            .enqueue_inventory_event(
                &mut player,
                1,
                retries + 3,
                InventoryEvent::Close(protocol::ContainerCloseEvent {
                    container: open.container,
                    window_type: open.window_type,
                    server_initiated: true,
                }),
            )
            .unwrap();
        runtime.drain_pending_inventory(&mut player);
    }
    for now in [0, 500] {
        presentation
            .build(
                &player,
                &runtime,
                now,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    let input = presentation
        .build(
            &player,
            &runtime,
            5_000,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    forms::snapshot::write(&input, snapshot_name);
    let screen = InventoryScreen::of_runtime(&player, &runtime);
    assert_eq!(
        screen,
        InventoryScreen::Storage(inventory::inventory_ledger::LARGE_STORAGE_SLOT_COUNT)
    );
    let frame = presentation.engine_container_frame().unwrap();
    let last = frame.hits.iter().find(|hit| {
        hit.collection.as_deref() == Some("container_items") && hit.collection_index == Some(53)
    });
    assert!(last.is_some(), "the last chest cell remains interactive");
}
