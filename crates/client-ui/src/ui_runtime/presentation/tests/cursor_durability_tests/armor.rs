//! Real armor identities and NBT damage exercise inventory and equipment pickup paths.

use super::*;
use inventory::inventory_ledger::{CellGesture, InventoryTarget};

const ARMOR: &[&str] = &[
    "minecraft:diamond_helmet",
    "minecraft:diamond_chestplate",
    "minecraft:diamond_leggings",
    "minecraft:diamond_boots",
];
const POINTER: [f32; 2] = [300.0, 150.0];

/// Encodes retained little-endian item user data with an authoritative Damage integer.
fn damaged_armor(network_id: i32, damage: i32) -> NetworkItemStack {
    let mut extra = vec![0xff, 0xff, 1, 10, 0, 0, 3, 6, 0];
    extra.extend_from_slice(b"Damage");
    extra.extend_from_slice(&damage.to_le_bytes());
    extra.push(0);
    NetworkItemStack {
        network_id,
        stack_network_id: 9,
        count: 1,
        nbt_digest: Sha256::digest(&extra).into(),
        extra_data: extra.into(),
        ..NetworkItemStack::empty()
    }
}

/// Opens a survival inventory with half-worn armor in its equipment or player cell.
fn scenario(
    slot: u8,
    equipped: bool,
) -> (
    player_state::PlayerState,
    UiRuntime,
    chunk_pipeline::WorldStream,
    InventoryTarget,
) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    let registry = protocol::vanilla_item_registry();
    let item = registry
        .iter()
        .find(|item| item.identifier.as_ref() == ARMOR[usize::from(slot)])
        .unwrap();
    let maximum = stream
        .authority()
        .item_durability(item.network_id)
        .maximum()
        .unwrap();
    let stack = damaged_armor(item.network_id, (maximum / 2) as i32);
    let mut inventory = vec![NetworkItemStack::empty(); protocol::PLAYER_INVENTORY_SLOTS as usize];
    let mut armor = vec![NetworkItemStack::empty(); usize::from(protocol::ARMOR_SLOTS)];
    let target = if equipped {
        armor[usize::from(slot)] = stack;
        InventoryTarget::Armor(slot)
    } else {
        inventory[usize::from(protocol::HOTBAR_SLOT_COUNT)] = stack;
        InventoryTarget::Player(protocol::HOTBAR_SLOT_COUNT)
    };
    let ledger = runtime.inventory_ledger_mut(&mut player);
    for (window, slots) in [(0, inventory), (protocol::ARMOR_WINDOW_ID, armor)] {
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(window),
            slots: slots.into(),
            storage_item: NetworkItemStack::empty(),
        }));
    }
    runtime.toggle_inventory(&mut player, None);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0; 3],
        runtime_entity_id: -1,
    }));
    runtime.set_inventory_pointer_gui(Some(POINTER));
    (player, runtime, stream, target)
}

/// Publishes the cursor and equipment from one predicted inventory revision.
fn capture(
    player: &player_state::PlayerState,
    runtime: &mut UiRuntime,
    presentation: &mut UiPresentationRuntime,
    stream: &chunk_pipeline::WorldStream,
) {
    capture_hud_frame(
        player,
        runtime,
        presentation,
        Some(stream),
        semantic_input::PerspectiveMode::FirstPerson,
        0,
        Default::default(),
    );
}

#[test]
fn cursor_durability_survives_picking_up_all_four_armor_pieces() {
    for equipped in [false, true] {
        for slot in 0..ARMOR.len() as u8 {
            let (mut player, mut runtime, stream, target) = scenario(slot, equipped);
            let mut presentation =
                UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
            capture(&player, &mut runtime, &mut presentation, &stream);
            let before = if equipped {
                presentation.hud_frame().armor_durability[usize::from(slot)]
            } else {
                presentation.hud_frame().durability.player[usize::from(protocol::HOTBAR_SLOT_COUNT)]
            }
            .expect("NBT damage must produce the armor's initial durability bar");
            assert!(before > 0.0 && before < 1.0);
            runtime
                .inventory_ledger_mut(&mut player)
                .begin_target_gesture(target, CellGesture::TakeCount(1))
                .unwrap();
            capture(&player, &mut runtime, &mut presentation, &stream);
            assert!(
                runtime
                    .inventory_ledger(&player)
                    .target_stack(target)
                    .is_none()
            );
            assert_eq!(
                presentation.hud_frame().cursor_durability,
                Some(before),
                "{} equipped={equipped}",
                ARMOR[usize::from(slot)]
            );
        }
    }
}

#[test]
fn cursor_durability_renders_with_installed_armor_artwork() {
    let Some(carrier) = ui_engine::test_support::carrier() else {
        eprintln!("skipping cursor armor artwork: missing UI carrier; make assets");
        return;
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/assets/compiled")
        .join(assets::carriers::ICON.output);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping cursor armor artwork: missing {}; make assets",
                path.display()
            );
            return;
        }
        Err(error) => panic!("read armor icons: {error}"),
    };
    let icons = Arc::new(assets::RuntimeIconCatalog::decode(&bytes).unwrap());
    let font_path = path.with_file_name(assets::carriers::FONT.output);
    let font_bytes = match std::fs::read(&font_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping cursor armor artwork: missing {}; make assets",
                font_path.display()
            );
            return;
        }
        Err(error) => panic!("read armor capture font: {error}"),
    };
    let manifest = assets::canonical_source_manifest_sha256(
        assets::carriers::FONT.font_face.unwrap().manifest,
    );
    let font = Arc::new(assets::RuntimeFontCatalog::decode(&font_bytes, manifest).unwrap());
    for engine in [false, true] {
        for slot in 0..ARMOR.len() as u8 {
            let (mut player, mut runtime, stream, target) = scenario(slot, true);
            let mut presentation = UiPresentationRuntime::with_hud_and_icons(
                Arc::clone(&font),
                fixture_hud(),
                Arc::clone(&icons),
            )
            .unwrap();
            presentation.set_gui_scale_preference(Some(2));
            if engine {
                presentation.enable_json_ui(Arc::clone(&carrier)).unwrap();
            }
            capture(&player, &mut runtime, &mut presentation, &stream);
            let input = presentation
                .build(
                    &player,
                    &runtime,
                    1_000,
                    [1280, 720],
                    DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            super::super::super::forms::snapshot::write(
                &input,
                &format!("armor-{slot}-equipped-engine-{engine}"),
            );
            runtime
                .inventory_ledger_mut(&mut player)
                .begin_target_gesture(target, CellGesture::TakeCount(1))
                .unwrap();
            capture(&player, &mut runtime, &mut presentation, &stream);
            assert!(
                presentation.hud_frame().cursor_icon.is_some(),
                "armor artwork must resolve"
            );
            let input = presentation
                .build(
                    &player,
                    &runtime,
                    1_000,
                    [1280, 720],
                    DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            let image = super::super::super::forms::snapshot::rasterize(&input);
            // The bar's rendered area follows the pointer at GUI scale two.
            assert!(
                (310..314).any(|y| (588..616).any(|x| {
                    let color = image.get_pixel(x, y).0;
                    color[0] >= 250 && color[1] >= 250 && color[2] == 0 && color[3] == 255
                })),
                "{} cursor bar must render with engine={engine}",
                ARMOR[usize::from(slot)]
            );
            super::super::super::forms::snapshot::write_image(
                &image,
                &format!("armor-{slot}-cursor-engine-{engine}"),
            );
        }
    }
}
