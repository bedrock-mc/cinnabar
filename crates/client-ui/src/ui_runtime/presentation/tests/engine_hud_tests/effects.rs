use super::*;
use crate::ui_runtime::presentation::forms::{ServerUiPack, pack_harness, snapshot};

const JAVA_RENDERER: &str = "java_mob_effects_renderer";
const VANILLA_RENDERER: &str = "mob_effects_renderer";

/// Survival with good (speed, strength, regeneration, resistance) and bad (slowness) effects.
fn session(player_runtime: &mut player_state::PlayerState) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    full_stats(player_runtime, &mut runtime, 1);
    for (sequence, id) in [(2, 5), (3, 2), (4, 1), (5, 11), (6, 10)] {
        runtime
            .apply_local_effect(1, sequence, effect(id), 0)
            .unwrap();
    }
    runtime
}

fn icon_path(role: assets::HudTextureRole) -> &'static str {
    role.source_path().strip_suffix(".png").unwrap()
}

/// The vanilla column runs in effect-id order, harmful effects included, and its
/// top band counts heart rows plus the armor row.
#[test]
fn effect_icons_follow_effect_id_order_and_heart_rows() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let runtime = session(&mut player_runtime);
    let paint = hud_layout::capture_hud_paint(
        &player_runtime,
        &runtime,
        &first_person(),
        None,
        &Default::default(),
    );
    let icons: Vec<_> = paint.effects.icons.iter().map(|icon| icon.icon).collect();
    let expected: Vec<_> = [
        assets::HudTextureRole::EffectIconSpeed,
        assets::HudTextureRole::EffectIconSlowness,
        assets::HudTextureRole::EffectIconStrength,
        assets::HudTextureRole::EffectIconRegeneration,
        assets::HudTextureRole::EffectIconResistance,
    ]
    .map(icon_path)
    .into();
    assert_eq!(icons, expected);
    assert_eq!(paint.effects.status_rows, 1, "one heart row, no armor");
}

/// The Java rows put beneficial effects on the first row and harmful ones below.
#[test]
fn java_rows_split_beneficial_and_harmful_effects() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let runtime = session(&mut player_runtime);
    let paint = hud_layout::capture_hud_paint(
        &player_runtime,
        &runtime,
        &first_person(),
        None,
        &Default::default(),
    );
    let row = |role| {
        paint
            .java_effects
            .iter()
            .find(|cell| cell.texture == icon_path(role))
            .map(|cell| cell.at)
            .unwrap()
    };
    let speed = row(assets::HudTextureRole::EffectIconSpeed);
    let slowness = row(assets::HudTextureRole::EffectIconSlowness);
    let strength = row(assets::HudTextureRole::EffectIconStrength);
    assert_eq!(speed[1], strength[1], "beneficial effects share a row");
    assert!(slowness[1] > speed[1], "harmful effects sit below");
    assert!(strength[0] < speed[0], "rows run leftward in id order");
}

/// The built-in Java pack keeps its rows; a pack placing vanilla's control gets the column.
#[test]
fn java_pack_draws_rows_and_the_vanilla_pack_draws_the_column() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let runtime = session(&mut player_runtime);
    let Some(mut java) = engine_presentation_with(pack_harness::font()) else {
        eprintln!("skipping effect pack layouts: missing vanilla-v1.mcbeui; make assets");
        return;
    };
    *java.hud_frame_mut() = first_person();
    let input = build(&player_runtime, &mut java, &runtime, 0);
    let nodes = java.hud_draw_nodes();
    assert_eq!(customs(nodes, JAVA_RENDERER).len(), 1);
    assert!(customs(nodes, VANILLA_RENDERER).is_empty());
    snapshot::write(&input, "hud_effects_java");

    let hud_screen = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(assets::vanilla_source().resource_pack_dir())
        .join("ui/hud_screen.json");
    let Ok(vanilla_hud) = std::fs::read(&hud_screen) else {
        eprintln!(
            "skipping vanilla effect column: missing {}; make assets",
            hud_screen.display()
        );
        return;
    };
    let mut vanilla = engine_presentation_with(pack_harness::font()).unwrap();
    vanilla.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
            ),
            ("ui/hud_screen.json".into(), vanilla_hud),
        ]],
        ..Default::default()
    });
    *vanilla.hud_frame_mut() = first_person();
    let input = build(&player_runtime, &mut vanilla, &runtime, 0);
    let nodes = vanilla.hud_draw_nodes();
    assert_eq!(customs(nodes, VANILLA_RENDERER).len(), 1);
    assert!(customs(nodes, JAVA_RENDERER).is_empty());
    snapshot::write(&input, "hud_effects_vanilla");
}
