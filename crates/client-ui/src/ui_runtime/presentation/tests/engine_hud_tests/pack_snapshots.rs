//! Optional local pack frames; raw pack files and captures stay outside Git.

use super::*;
use crate::ui_runtime::presentation::forms::{pack_harness, snapshot};

#[test]
fn snapshot_local_pack_hud() {
    let Some(root) = std::env::var_os("CINNABAR_FORM_PACK_DIR") else {
        eprintln!("skipping snapshot_local_pack_hud: missing CINNABAR_FORM_PACK_DIR fixture");
        return;
    };
    let Some(mut presentation) = engine_presentation_with(pack_harness::font()) else {
        eprintln!("skipping snapshot_local_pack_hud: missing local carriers (make assets)");
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    full_stats(&mut player, &mut runtime, 1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    presentation.hud_frame_mut().first_person = true;
    runtime.set_server_ui(Some(Arc::new(pack_harness::dir_pack([
        std::path::PathBuf::from(root),
    ]))));
    for frame in 0..24 {
        let input = build(&player, &mut presentation, &runtime, frame * 100);
        if frame == 23 {
            snapshot::write(&input, "local-pack-hud");
        }
    }
    assert!(
        !customs(presentation.hud_draw_nodes(), "hotbar_renderer").is_empty(),
        "pack must preserve the hotbar"
    );
}

#[test]
fn snapshot_recent_effects() {
    let path = std::env::var_os("PINNED_HUD_CARRIER")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(assets::carriers::COMPILED_DIR)
                .join(assets::carriers::HUD.output)
        });
    if !path.is_file() {
        eprintln!(
            "skipping snapshot_recent_effects: missing PINNED_HUD_CARRIER fixture at {}",
            path.display()
        );
        return;
    }
    let Some(ui) = pack_harness::carrier() else {
        return;
    };
    let hud = Arc::new(assets::RuntimeHudCatalog::decode(&std::fs::read(path).unwrap()).unwrap());
    let mut presentation = UiPresentationRuntime::with_hud(pack_harness::font(), hud).unwrap();
    presentation.enable_json_ui(ui).unwrap();
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    full_stats(&mut player, &mut runtime, 1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    for id in 31..=37 {
        runtime
            .apply_local_effect(1, id as u64, effect(id), 0)
            .unwrap();
    }
    for frame in 0..24 {
        let input = build(&player, &mut presentation, &runtime, frame * 100);
        if frame == 23 {
            snapshot::write(&input, "recent-effects");
        }
    }
    assert_eq!(runtime.gameplay_hud().effects().len(), 7);
}
