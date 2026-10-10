use super::*;

/// The effect column runs in effect-id order, harmful effects included, and its
/// top band counts heart rows plus the armor row.
#[test]
fn effect_icons_follow_effect_id_order_and_heart_rows() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    full_stats(&mut player_runtime, &mut runtime, 1);
    for (sequence, id) in [(2, 5), (3, 2), (4, 1), (5, 11), (6, 10)] {
        runtime
            .apply_local_effect(1, sequence, effect(id), 0)
            .unwrap();
    }
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
    .map(|role| role.source_path().strip_suffix(".png").unwrap())
    .into();
    assert_eq!(icons, expected);
    assert_eq!(paint.effects.status_rows, 1, "one heart row, no armor");
    if let Some(mut real) =
        engine_presentation_with(super::super::super::forms::pack_harness::font())
    {
        *real.hud_frame_mut() = first_person();
        let input = build(&player_runtime, &mut real, &runtime, 0);
        super::super::super::forms::snapshot::write(&input, "hud_effects");
    }
}
