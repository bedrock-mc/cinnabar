//! Local-only harness: server stacks through the world stream's registry, the
//! inventory ledger, the icon carrier and the engine HUD's item renderers.
//! Skips when a gitignored carrier is absent.

use std::sync::Arc;

use assets::{RuntimeAssets, RuntimeEntityAssets, RuntimeIconCatalog};
use json_ui::Draw;
use protocol::{
    ContainerIdentity, InventoryContentEvent, InventoryEvent, ItemRegistryEvent, NetworkItemStack,
    PlayerGameMode, WorldBootstrap,
};

use super::*;
use crate::ui_runtime::presentation::refresh_hud_frame;

/// Reads an installed item fixture, skipping missing files and rejecting other read errors.
fn local(name: &str) -> Option<Vec<u8>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.local/assets/compiled")
        .join(name);
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping item pipeline fixture test: missing {}; make assets",
                path.display()
            );
            None
        }
        Err(error) => panic!("read item fixture {}: {error}", path.display()),
    }
}

struct Harness {
    presentation: UiPresentationRuntime,
    stream: client_world::WorldStream,
}

fn harness() -> Option<Harness> {
    let icons = Arc::new(
        RuntimeIconCatalog::decode(&local("vanilla-v1.mcbeico")?)
            .expect("decode installed icon fixture"),
    );
    let entities = Arc::new(
        RuntimeEntityAssets::decode(&local("vanilla-v1.mcbeent")?)
            .expect("decode installed entity fixture"),
    );
    let world = Arc::new(
        RuntimeAssets::decode(&local("vanilla-v2193.mcbea")?)
            .expect("decode installed world fixture"),
    );
    let carrier = super::super::forms::pack_harness::carrier()?;
    let mut presentation =
        UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), icons)
            .expect("build item fixture HUD");
    presentation
        .enable_json_ui(carrier)
        .expect("enable installed UI fixture");
    let bootstrap = WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0., 64., 0.],
        world_spawn_position: [0, 64, 0],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    };
    let stream = client_world::WorldStream::new_with_asset_sets(
        bootstrap,
        world,
        entities,
        [0., 64., 0.],
        None,
    );
    Some(Harness {
        presentation,
        stream,
    })
}

const CUSTOM_ID: i32 = 10_000;

fn network_id(identifier: &str) -> i32 {
    if identifier == "zeqa:item.training" {
        return CUSTOM_ID;
    }
    protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == identifier)
        .unwrap_or_else(|| panic!("{identifier} is not a retail item"))
        .network_id
}

/// A named stack with NBT, as servers send lobby and kit items.
fn stack(identifier: &str, metadata: u32, count: u16) -> NetworkItemStack {
    let extra_data: Arc<[u8]> = Arc::from(&b"\x0a\x00\x00\x00"[..]);
    NetworkItemStack {
        network_id: network_id(identifier),
        metadata,
        stack_network_id: 1,
        count,
        nbt_digest: <sha2::Sha256 as sha2::Digest>::digest(&extra_data).into(),
        block_runtime_id: 0,
        extra_data,
    }
}

fn registry_with_custom_item() -> ItemRegistryEvent {
    let mut entries = protocol::vanilla_item_registry().to_vec();
    let mut custom = entries[0].clone();
    custom.identifier = Arc::from("zeqa:item.training");
    custom.network_id = CUSTOM_ID;
    custom.component_based = true;
    entries.push(custom);
    ItemRegistryEvent {
        entries: entries.into(),
    }
}

// Every hotbar stack with a resolvable icon reaches an item renderer; the
// report names the stage where each other stack stops.
#[test]
fn hotbar_stacks_resolve_icons_and_reach_the_engine_item_renderer() {
    let Some(Harness {
        mut presentation,
        mut stream,
    }) = harness()
    else {
        eprintln!(
            "skipping hotbar_stacks_resolve_icons_and_reach_the_engine_item_renderer: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    assert!(stream.seed_item_registry(registry_with_custom_item()));
    let hotbar = [
        ("minecraft:diamond", 0, 3),
        ("minecraft:diamond_sword", 0, 1),
        ("minecraft:compass", 0, 1),
        ("minecraft:golden_apple", 0, 8),
        ("minecraft:splash_potion", 22, 1),
        ("minecraft:ender_pearl", 0, 16),
        ("minecraft:book", 0, 1),
        ("minecraft:iron_helmet", 0, 1),
        ("zeqa:item.training", 0, 1),
    ];
    let mut slots = vec![NetworkItemStack::empty(); 36];
    for (slot, (identifier, metadata, count)) in hotbar.iter().enumerate() {
        slots[slot] = stack(identifier, *metadata, *count);
    }
    let mut runtime = UiRuntime::new(1);
    runtime.publish_local_runtime_id(1, 1).unwrap();
    runtime.publish_player_game_mode(PlayerGameMode::Survival);
    runtime
        .enqueue_inventory_event(
            1,
            1,
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity {
                    window_id: Some(0),
                    slot_type: None,
                    dynamic_id: None,
                },
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory();
    runtime.set_local_selected_slot(0);
    refresh_hud_frame(
        &mut runtime,
        &mut presentation,
        Some(&stream),
        &Default::default(),
        1_000,
    );
    let frame = presentation.hud_frame().clone();
    let report = hotbar
        .iter()
        .enumerate()
        .map(|(slot, (identifier, ..))| {
            let stage = if frame.hotbar_stacks[slot].is_none() {
                "no ledger stack"
            } else if stream
                .canonical_item_stack(frame.hotbar_stacks[slot].as_ref().unwrap())
                .and_then(|item| item.identifier)
                .is_none()
            {
                "no identifier"
            } else if frame.hotbar_icons[slot].is_none() {
                "no icon"
            } else {
                "icon"
            };
            format!("{identifier}: {stage}")
        })
        .collect::<Vec<_>>();
    eprintln!("{report:#?}");
    presentation
        .build(&runtime, 1_000, [1280, 720], DpiScale::new(1.0).unwrap())
        .unwrap();
    let rendered = presentation
        .hud_draw_nodes()
        .iter()
        .filter(|node| {
            matches!(&node.draw, Draw::Custom { renderer, data }
                if renderer == "inventory_item_renderer"
                    && data.get("#item_renderer_data").is_some_and(serde_json::Value::is_number))
        })
        .count();
    let resolved = frame.hotbar_icons.iter().flatten().count();
    assert_eq!(rendered, resolved, "{report:#?}");
    let cleared = presentation
        .hud_draw_nodes()
        .iter()
        .filter(|node| {
            matches!(&node.draw, Draw::Custom { renderer, data }
                if renderer == "inventory_item_renderer"
                    && data.get("#item_renderer_data").is_some_and(serde_json::Value::is_null))
        })
        .count();
    assert_eq!(cleared, hotbar.len() - resolved, "{report:#?}");
    assert!(
        frame.hotbar_stacks.iter().all(Option::is_some),
        "{report:#?}"
    );
}

// Armor sent on window 120 (named as zeqa.net names it) is the local player's:
// it fills the inventory's armor cells, the HUD armor points and icons, and the
// local rig's worn items.
#[test]
fn window_120_armor_dresses_the_hud_inventory_and_local_rig() {
    let Some(Harness {
        mut presentation,
        mut stream,
    }) = harness()
    else {
        eprintln!(
            "skipping window_120_armor_dresses_the_hud_inventory_and_local_rig: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    assert!(stream.seed_item_registry(registry_with_custom_item()));
    let mut runtime = UiRuntime::new(1);
    runtime.publish_local_runtime_id(1, 1).unwrap();
    runtime.publish_player_game_mode(PlayerGameMode::Survival);
    let worn = [
        "minecraft:diamond_helmet",
        "minecraft:diamond_chestplate",
        "minecraft:iron_leggings",
        "minecraft:iron_boots",
    ];
    runtime
        .enqueue_inventory_event(
            1,
            1,
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity {
                    window_id: Some(protocol::ARMOR_WINDOW_ID),
                    slot_type: Some(1),
                    dynamic_id: None,
                },
                slots: worn.map(|id| stack(id, 0, 1)).to_vec().into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory();
    assert_eq!(
        runtime.local_armor().chestplate.network_id,
        network_id("minecraft:diamond_chestplate")
    );
    refresh_hud_frame(
        &mut runtime,
        &mut presentation,
        Some(&stream),
        &Default::default(),
        1_000,
    );
    // Diamond helmet 3, chestplate 8, iron leggings 5, boots 2.
    assert_eq!(runtime.hud().armor().map(|armor| armor.current()), Some(18));
    assert!(
        presentation
            .hud_frame()
            .armor_icons
            .iter()
            .all(Option::is_some)
    );
    let rig = crate::presentation::equipment::local_input(&stream, Some(&runtime), 1);
    let rig_worn = rig
        .armor
        .map(|item| item.map(|item| item.identifier.to_string()));
    assert_eq!(rig_worn, worn.map(|id| Some(id.to_owned())));
}
