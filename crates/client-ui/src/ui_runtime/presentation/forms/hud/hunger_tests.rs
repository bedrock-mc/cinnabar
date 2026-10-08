use super::*;
use crate::ui_runtime::SequencedLocalAttributes;

fn food_update(player: &mut player_state::PlayerState, runtime: &mut UiRuntime, sequence: u64) {
    runtime
        .apply_local_attributes(
            player,
            SequencedLocalAttributes {
                session_id: runtime.session_id(),
                fifo_sequence: sequence,
                local_millis: sequence * 10,
                server_tick: 0,
                attributes: Arc::from([
                    protocol::ActorAttribute {
                        name: Arc::from("minecraft:player.hunger"),
                        min: 0.0,
                        max: 20.0,
                        current: 18.0,
                        default: None,
                        modifiers: Arc::from([]),
                    },
                    protocol::ActorAttribute {
                        name: Arc::from("minecraft:player.saturation"),
                        min: 0.0,
                        max: 20.0,
                        current: 0.0,
                        default: None,
                        modifiers: Arc::from([]),
                    },
                ]),
            },
        )
        .unwrap();
}

#[test]
fn repeated_zero_tick_food_updates_do_not_restart_hunger_motion() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut screens = HudScreens::default();
    let mut frame = HudFrame::default();
    let options = Default::default();
    for update in 1..=110 {
        food_update(&mut player, &mut runtime, update);
        frame.now_millis = update * 10;
        let paint = screens.capture_status(false, &player, &runtime, &frame, None, &options);
        if update % 55 == 0 {
            assert!(paint.hunger.iter().any(|cell| cell.at[1] == -1.0));
            assert!(
                paint
                    .hunger
                    .iter()
                    .all(|cell| [-1.0, 0.0].contains(&cell.at[1]))
            );
        } else {
            assert!(
                paint.hunger.iter().all(|cell| cell.at[1] == 0.0),
                "food packets must not start another shake at update {update}"
            );
        }
        for layers in paint.hunger.chunks_exact(2) {
            assert_eq!(layers[0].at, layers[1].at);
        }
        screens.capture_status(true, &player, &runtime, &frame, None, &options);
    }
}

#[test]
fn a_new_session_starts_with_a_neutral_hunger_row() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut screens = HudScreens::default();
    let frame = HudFrame::default();
    let options = Default::default();
    food_update(&mut player, &mut runtime, 1);
    for _ in 0..54 {
        screens.capture_status(false, &player, &runtime, &frame, None, &options);
    }
    let mut player = player_state::PlayerState::new(2);
    let mut runtime = UiRuntime::new(2);
    food_update(&mut player, &mut runtime, 1);
    let paint = screens.capture_status(false, &player, &runtime, &frame, None, &options);
    assert!(!paint.hunger.is_empty());
    assert!(paint.hunger.iter().all(|cell| cell.at[1] == 0.0));
}
