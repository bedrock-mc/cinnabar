//! The offhand frame and item stay aligned with the hotbar at every GUI scale.

use super::*;
use crate::ui_runtime::presentation::forms::{pack_harness, snapshot};

/// Captures a HUD after the frame and item have reached their physical pixel bounds.
fn capture_offhand(
    player: &player_state::PlayerState,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    physical: [u32; 2],
    scale: u8,
    right: bool,
    name: &str,
) {
    presentation.set_gui_scale_preference(Some(scale));
    let scale = f32::from(scale);
    let x = physical[0] as f32 / 2.0 + if right { 91.0 } else { -120.0 } * scale;
    let y = physical[1] as f32 - 23.0 * scale;
    let icon_x = x + if right { 10.0 } else { 3.0 } * scale;
    let expected = [
        [x, y, x + 29.0 * scale, y + 24.0 * scale],
        [
            icon_x,
            y + 4.0 * scale,
            icon_x + 16.0 * scale,
            y + 20.0 * scale,
        ],
    ];
    let mut captured = None;
    test_time::eventually("offhand frame and item become resident", || {
        let input = build_at(player, presentation, runtime, 0, physical, 1.0);
        let ready = expected.iter().all(|expected| {
            quad_bounds(&input).any(|actual| {
                actual
                    .into_iter()
                    .zip(expected)
                    .all(|(actual, expected)| (actual - expected).abs() < 1e-3)
            })
        });
        captured = Some(input);
        ready
    });
    snapshot::write(&captured.unwrap(), name);
}

#[test]
fn snapshot_offhand_hud() {
    let Some(carrier) = ui_engine::test_support::carrier() else {
        eprintln!("skipping snapshot_offhand_hud: missing UI carrier; make assets");
        return;
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/assets/compiled")
        .join(assets::carriers::HUD.output);
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("skipping snapshot_offhand_hud: missing HUD carrier; make assets");
        return;
    };
    let hud = Arc::new(assets::RuntimeHudCatalog::decode(&bytes).unwrap());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/assets/compiled")
        .join(assets::carriers::ICON.output);
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("skipping snapshot_offhand_hud: missing item icon carrier; make assets");
        return;
    };
    let icons = Arc::new(assets::RuntimeIconCatalog::decode(&bytes).unwrap());
    let mut presentation =
        UiPresentationRuntime::with_hud_and_icons(pack_harness::font(), hud, icons).unwrap();
    presentation.enable_json_ui(carrier).unwrap();
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    select_slot(&mut player, &mut runtime);
    full_stats(&mut player, &mut runtime, 1);
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            8,
            protocol::InventoryEvent::Content(protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
                slots: vec![item(1, 1)].into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player);
    presentation.hud_frame_mut().offhand_icon = Some(
        presentation
            .item_icon("minecraft:arrow", 0)
            .expect("arrow icon fixture"),
    );
    for right in [false, true] {
        presentation.set_server_ui_pack(&super::super::super::ServerUiPack {
            ui_layers: vec![vec![(
                "ui/_global_variables.json".into(),
                format!("{{\"$cinnabar_main_arm_left\":{right}}}").into_bytes(),
            )]],
            ..Default::default()
        });
        for scale in 1..=8 {
            capture_offhand(
                &player,
                &mut presentation,
                &runtime,
                [3840, 2160],
                scale,
                right,
                &format!(
                    "offhand-{}-scale-{scale}",
                    if right { "right" } else { "left" }
                ),
            );
        }
        capture_offhand(
            &player,
            &mut presentation,
            &runtime,
            [1920, 1080],
            3,
            right,
            &format!("offhand-{}-1080p", if right { "right" } else { "left" }),
        );
    }
}
