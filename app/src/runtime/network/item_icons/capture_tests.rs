//! Replays a local packet capture through the inventory registry, ledger and HUD.

use super::*;
use assets::{RuntimeAssets, RuntimeEntityAssets, RuntimeIconCatalog};
use client_ui::ui_runtime::{
    UiRuntime,
    presentation::{UiPresentationRuntime, refresh_hud_frame},
};
use protocol::{InventoryEvent, PlayerGameMode, WorldBootstrap};
use ui::DpiScale;

use protocol::{BedrockSession, ItemActorEvent, WorldEvent};

/// Encodes the unsigned framing integers used by a Bedrock packet batch.
fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        out.push(byte | if value == 0 { 0 } else { 0x80 });
        if value == 0 {
            break;
        }
    }
}

#[test]
fn captured_hotbar_survives_network_registry_and_inventory_publication() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(path) = std::env::var_os("CINNABAR_LOBBY_CAPTURE") else {
        eprintln!(
            "skipping captured_hotbar_survives_network_registry_and_inventory_publication: fixture unavailable; requires installed local carriers (make assets) and CINNABAR_LOBBY_CAPTURE, CINNABAR_RENDER_PACK"
        );
        return;
    };
    let (mut presentation, mut stream) = harness().expect("installed UI, icon and entity carriers");
    let view = super::super::local_pack::local_pack_view("CINNABAR_RENDER_PACK")
        .expect("captured session resource pack");
    let bytes = std::fs::read(path).unwrap();
    let session = BedrockSession { shield_item_id: 0 };
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let (mut at, mut sequence, mut contents) = (0, 0, 0);
    while at + 8 <= bytes.len() {
        let id = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &bytes[at + 8..at + 8 + len];
        at += 8 + len;
        if !matches!(id, 31 | 49 | 50 | 162) {
            continue;
        }
        let mut header = Vec::new();
        varint(&mut header, u64::from(id));
        let mut batch = vec![0xfe];
        varint(&mut batch, (header.len() + body.len()) as u64);
        batch.extend(header);
        batch.extend(body);
        for packet in protocol::decode_batch(batch.into(), &session).unwrap() {
            sequence += 1;
            match protocol::into_world_event(packet, 0).unwrap() {
                Some(WorldEvent::Inventory(event)) => {
                    if matches!(&event, InventoryEvent::Content(_)) {
                        contents += 1;
                    }
                    runtime
                        .enqueue_inventory_event(&mut player_runtime, 1, sequence, event)
                        .unwrap();
                    runtime.drain_pending_inventory(&mut player_runtime);
                }
                Some(WorldEvent::ItemActor(ItemActorEvent::Registry(event))) => {
                    assert!(stream.seed_item_registry(event.clone()));
                    runtime
                        .enqueue_item_registry_event(&mut player_runtime, 1, sequence, event)
                        .unwrap();
                    runtime.drain_pending_inventory(&mut player_runtime);
                }
                _ => {}
            }
        }
    }
    assert!(contents > 0, "capture contains no inventory contents");
    runtime.set_session_icons(compile_session_icons(&view, &[], BlockIcons::default()));
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    refresh_hud_frame(
        &player_runtime,
        &mut runtime,
        &mut presentation,
        Some(&stream),
        crate::camera::CameraSettingsAuthority::default().perspective(),
        0,
    );
    let frame = presentation.hud_frame().clone();
    for (slot, stack) in frame.hotbar_stacks.iter().enumerate() {
        if let Some(stack) = stack {
            eprintln!(
                "captured slot {slot}: {:?}, icon={:?}",
                stream.authority().canonical_item_stack(stack),
                frame.hotbar_icons[slot]
            );
            assert!(
                frame.hotbar_icons[slot].is_some(),
                "captured slot {slot} has no icon"
            );
        }
    }
    assert!(frame.hotbar_stacks.iter().any(Option::is_some));
    if let Some(pack) = crate::ui_runtime::presentation::forms::pack_harness::env_pack() {
        presentation.set_server_ui_pack(&pack);
    }
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    client_ui::ui_runtime::presentation::forms::snapshot::write(&input, "captured-hotbar");
    let icon_pages: std::collections::BTreeSet<_> = frame
        .hotbar_icons
        .iter()
        .flatten()
        .map(|icon| u32::from(icon.page))
        .collect();
    assert!(
        input
            .batches
            .iter()
            .any(|batch| icon_pages.contains(&batch.texture_page)),
        "resolved item art must reach a published draw batch"
    );
}

/// Loads the installed carriers into the inventory and GUI model presentation paths.
fn harness() -> Option<(UiPresentationRuntime, chunk_pipeline::WorldStream)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local/assets/compiled");
    let icons = Arc::new(
        RuntimeIconCatalog::decode(&std::fs::read(root.join("vanilla-v1.mcbeico")).ok()?).ok()?,
    );
    let entities = Arc::new(
        RuntimeEntityAssets::decode(&std::fs::read(root.join("vanilla-v1.mcbeent")).ok()?).ok()?,
    );
    let world = Arc::new(RuntimeAssets::diagnostic());
    let fixtures = crate::ui_runtime::presentation::tests::fixture_font();
    let hud = crate::ui_runtime::presentation::tests::fixture_hud();
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(fixtures, hud, icons).ok()?;
    presentation
        .enable_json_ui(crate::ui_runtime::presentation::forms::pack_harness::carrier()?)
        .ok()?;
    presentation.set_gui_models(&world, &entities).ok()?;
    let stream = chunk_pipeline::WorldStream::new_with_asset_sets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0., 64., 0.],
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        world,
        entities,
        [0., 64., 0.],
        None,
    );
    Some((presentation, stream))
}
