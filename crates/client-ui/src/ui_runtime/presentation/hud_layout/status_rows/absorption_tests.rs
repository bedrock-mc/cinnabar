use super::*;
use ui::native_hud::Cell;
use crate::ui_runtime::SequencedLocalAttributes;
use protocol::{ActorEffectAction, ActorEffectEvent};
use std::sync::Arc;

/// Supplies health and absorption as local-player attribute packets do.
fn absorbed(current: f32, maximum: f32) -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply_local_attributes(
            &mut player,
            SequencedLocalAttributes {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: 0,
                attributes: Arc::from([
                    protocol::ActorAttribute {
                        name: Arc::from("minecraft:health"),
                        min: 0.0,
                        max: 20.0,
                        current: 20.0,
                        default: None,
                        modifiers: Arc::from([]),
                    },
                    protocol::ActorAttribute {
                        name: Arc::from("minecraft:absorption"),
                        min: 0.0,
                        max: maximum,
                        current,
                        default: Some(0.0),
                        modifiers: Arc::from([]),
                    },
                ]),
            },
        )
        .unwrap();
    (player, runtime)
}

/// Captures sprites without requiring a local pack or graphics device.
fn painted(player: &player_state::PlayerState, runtime: &UiRuntime) -> HudPaint {
    capture(
        player,
        runtime,
        &HudFrame::default(),
        None,
        &Default::default(),
    )
}

#[test]
fn absorption_uses_current_points_and_wraps_after_health() {
    for (points, expected) in [(0.001, 1), (3.0, 2), (4.0, 2), (41.0, 21), (61.0, 31)] {
        let (player, runtime) = absorbed(points, f32::MAX);
        let cells = painted(&player, &runtime).hearts;
        let gold: Vec<_> = cells
            .iter()
            .filter(|cell| cell.texture.contains("absorption_heart"))
            .collect();
        assert_eq!(gold.len(), expected, "absorption {points}");
        assert_eq!(gold[0].at, [0.0, -10.0]);
        assert_eq!(
            gold.last().unwrap().at,
            [
                ((expected - 1) % 10) as f32 * 8.0,
                -(((10 + expected - 1) / 10) as f32) * 10.0
            ]
        );
        assert_eq!(
            gold.last().unwrap().texture,
            if points.ceil() as u32 % 2 == 1 {
                "textures/ui/absorption_heart_half"
            } else {
                "textures/ui/absorption_heart"
            }
        );
    }
    let (player, mut runtime) = absorbed(4.0, 20.0);
    runtime
        .hud
        .set_health(ui::BoundedStat::new_scaled(2000, 2050, 100));
    let paint = painted(&player, &runtime);
    let gold = paint
        .hearts
        .iter()
        .find(|cell| cell.texture == "textures/ui/absorption_heart")
        .unwrap();
    assert_eq!(
        gold.at,
        [8.0, -10.0],
        "fractional maximum health rounds up before absorption"
    );
}

#[test]
fn absorption_poison_wither_hardcore_and_damage_flash_match_vanilla() {
    for (effect_id, expected) in [
        (19, "absorption_heart"),
        (25, "absorption_heart"),
        (20, "wither_heart"),
    ] {
        let (player, mut runtime) = absorbed(3.0, 20.0);
        runtime.gameplay_hud.set_hardcore(true);
        runtime
            .apply_local_effect(
                1,
                2,
                ActorEffectEvent {
                    dimension: 0,
                    actor_runtime_id: 1,
                    action: ActorEffectAction::Add,
                    effect_id,
                    amplifier: 0,
                    particles: true,
                    ambient: false,
                    duration_ticks: -1,
                    tick: 0,
                },
                0,
            )
            .unwrap();
        runtime.last_health_drop_millis = Some(0);
        let cells = painted(&player, &runtime).hearts;
        assert_eq!(
            cells
                .iter()
                .filter(|cell| cell.texture == "textures/ui/heart_blink")
                .count(),
            12
        );
        let foreground: Vec<_> = cells
            .iter()
            .filter(|cell| {
                cell.at[1] == -10.0
                    && cell.texture != "textures/ui/heart_background"
                    && cell.texture != "textures/ui/heart_blink"
            })
            .collect();
        assert_eq!(foreground.len(), 2);
        assert_eq!(foreground[0].texture, format!("textures/ui/{expected}"));
        assert_eq!(
            foreground[1].texture,
            format!("textures/ui/{expected}_half")
        );
        assert_eq!(
            foreground[0].preferred,
            Some(format!("textures/ui/hardcore/{expected}").as_str())
        );
    }
}

#[test]
fn absorption_texture_dispatch_is_bounded_independently_of_offscreen_rows() {
    let (player, runtime) = absorbed(f32::from(u16::MAX), f32::MAX);
    let paint = painted(&player, &runtime);
    assert!(
        paint.textures().count() <= 100,
        "offscreen rows must not enumerate textures"
    );
}

#[test]
fn absorption_cell_generation_follows_moved_controls_and_clips_with_bounded_work() {
    let (player, runtime) = absorbed(f32::from(u16::MAX), f32::MAX);
    let paint = painted(&player, &runtime);
    for px in [0.5, 1.0, 4.0] {
        for y in [-30.0, 3.0, 10.0, 180.0, 300.0] {
            let origin = [0.0, y];
            let bounds = [0.0, 0.0, 100.0, 180.0];
            let intersects = |cell: &Cell| {
                let x = origin[0] + cell.at[0] * px;
                let y = origin[1] + cell.at[1] * px;
                x < bounds[2]
                    && x + cell.size[0] * px > bounds[0]
                    && y < bounds[3]
                    && y + cell.size[1] * px > bounds[1]
            };
            let visible: Vec<_> = paint.hearts.visible_cells(origin, px, bounds).collect();
            assert!(
                visible.len() <= 800,
                "work depends on viewport, not absorption"
            );
            assert_eq!(
                visible.into_iter().filter(intersects).collect::<Vec<_>>(),
                paint.hearts.iter().filter(intersects).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn absorption_invalid_gui_scale_generates_no_cells() {
    let (player, runtime) = absorbed(4.0, f32::MAX);
    let paint = painted(&player, &runtime);
    for px in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(
            paint
                .hearts
                .visible_cells([0.0, 180.0], px, [0.0, 0.0, 320.0, 180.0])
                .count(),
            0
        );
    }
}
