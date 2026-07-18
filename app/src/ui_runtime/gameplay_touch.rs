use bevy::{
    input::touch::Touches,
    prelude::{Res, ResMut, Single, With},
    window::{PrimaryWindow, Window},
};

use super::UiRuntime;
use crate::semantic_controls::SemanticTouchTargets;

const MOVEMENT_MAX_X: f32 = 0.5;
const MOVEMENT_MAX_Y: f32 = 0.5;
const ACTION_MIN_Y: f32 = 0.68;
const JUMP_MIN_X: f32 = 0.68;
const USE_MIN_X: f32 = 0.84;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GameplayTouchSample {
    contact_id: u64,
    position: [f32; 2],
    delta: [f32; 2],
}

impl GameplayTouchSample {
    #[must_use]
    pub(crate) const fn new(contact_id: u64, position: [f32; 2], delta: [f32; 2]) -> Self {
        Self {
            contact_id,
            position,
            delta,
        }
    }
}

pub(crate) fn reconcile_gameplay_touch_targets(
    targets: &mut SemanticTouchTargets,
    samples: &[GameplayTouchSample],
) {
    targets.retain_active_contacts(samples.iter().map(|sample| sample.contact_id));
    for sample in samples {
        if !sample.position.into_iter().all(f32::is_finite)
            || !sample.delta.into_iter().all(f32::is_finite)
            || sample
                .position
                .into_iter()
                .any(|coordinate| !(0.0..=1.0).contains(&coordinate))
        {
            targets.clear(sample.contact_id);
            continue;
        }

        let [x, y] = sample.position;
        if x <= MOVEMENT_MAX_X && y <= MOVEMENT_MAX_Y {
            targets.set_movement(sample.contact_id);
        } else if y >= ACTION_MIN_Y && (JUMP_MIN_X..USE_MIN_X).contains(&x) {
            targets.set(sample.contact_id, semantic_input::touch::JUMP);
        } else if y >= ACTION_MIN_Y && x >= USE_MIN_X {
            targets.set(sample.contact_id, semantic_input::touch::USE);
        } else if x > MOVEMENT_MAX_X && y < ACTION_MIN_Y {
            targets.set(sample.contact_id, look_target(sample.delta));
        } else {
            targets.clear(sample.contact_id);
        }
    }
}

fn look_target([x, y]: [f32; 2]) -> u16 {
    if x.abs() >= y.abs() {
        if x < 0.0 {
            semantic_input::touch::LOOK_LEFT
        } else {
            semantic_input::touch::LOOK_RIGHT
        }
    } else if y < 0.0 {
        semantic_input::touch::LOOK_UP
    } else {
        semantic_input::touch::LOOK_DOWN
    }
}

pub(crate) fn drive_gameplay_touch_targets(
    window: Single<&Window, With<PrimaryWindow>>,
    touches: Res<Touches>,
    ui: Res<UiRuntime>,
    mut targets: ResMut<SemanticTouchTargets>,
) {
    if ui.chat_focused() {
        return;
    }
    let width = window.width().max(1.0);
    let height = window.height().max(1.0);
    let samples = touches
        .iter()
        .take(semantic_input::MAX_TOUCH_CONTACTS)
        .map(|touch| {
            GameplayTouchSample::new(
                touch.id(),
                [
                    (touch.position().x / width).clamp(0.0, 1.0),
                    (touch.position().y / height).clamp(0.0, 1.0),
                ],
                [touch.delta().x / width, touch.delta().y / height],
            )
        })
        .collect::<Vec<_>>();
    reconcile_gameplay_touch_targets(&mut targets, &samples);
}
