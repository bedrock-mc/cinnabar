//! Completes spear pose inputs when the player definition only supplies their consuming clips.

use assets::{MolangEaseCurve as Curve, MolangEaseMode as Mode, MolangFunction, MolangOp};

use {super::*, world::TICK_DURATION as ACTOR_TICK_DURATION};

type PoseValue = fn(&SpearPose) -> f32;
const MODEL_SCALE: f32 = 40.0;

/// Resolved slots keep the formulas independent from a carrier's symbol ordering.
#[derive(Debug, Default)]
pub(super) struct Slots(Vec<(usize, PoseValue)>);

#[derive(Default)]
struct ItemPose {
    position: [f32; 3],
    rotation_y: f32,
    rotation_z: f32,
    attachable_rotation_z: f32,
}

#[derive(Default)]
struct SpearPose {
    equipped: f32,
    base_arm_x: f32,
    fp_use: ItemPose,
    fp_attack: ItemPose,
    tp_use_arm: [f32; 3],
    tp_use_item_x: f32,
    tp_use_attachable_z: f32,
    tp_attack_arm_x: f32,
    tp_attack_item_x: f32,
    tp_attack_attachable_z: f32,
}

const VARIABLES: &[(&str, PoseValue)] = &[
    ("variable.melee_spear_equipped", |pose| pose.equipped),
    ("variable.tp_melee_spear_base_arm_rotation_x", |pose| {
        pose.base_arm_x
    }),
    ("variable.fp_melee_spear_use_item_position_x", |pose| {
        pose.fp_use.position[0]
    }),
    ("variable.fp_melee_spear_use_item_position_y", |pose| {
        pose.fp_use.position[1]
    }),
    ("variable.fp_melee_spear_use_item_position_z", |pose| {
        pose.fp_use.position[2]
    }),
    ("variable.fp_melee_spear_use_item_rotation_y", |pose| {
        pose.fp_use.rotation_y
    }),
    ("variable.fp_melee_spear_use_item_rotation_z", |pose| {
        pose.fp_use.rotation_z
    }),
    (
        "variable.fp_melee_spear_use_attachable_rotation_z",
        |pose| pose.fp_use.attachable_rotation_z,
    ),
    ("variable.fp_melee_spear_attack_item_position_x", |pose| {
        pose.fp_attack.position[0]
    }),
    ("variable.fp_melee_spear_attack_item_position_y", |pose| {
        pose.fp_attack.position[1]
    }),
    ("variable.fp_melee_spear_attack_item_position_z", |pose| {
        pose.fp_attack.position[2]
    }),
    ("variable.fp_melee_spear_attack_item_rotation_y", |pose| {
        pose.fp_attack.rotation_y
    }),
    ("variable.fp_melee_spear_attack_item_rotation_z", |pose| {
        pose.fp_attack.rotation_z
    }),
    (
        "variable.fp_melee_spear_attack_attachable_rotation_z",
        |pose| pose.fp_attack.attachable_rotation_z,
    ),
    ("variable.tp_melee_spear_use_arm_rotation_x", |pose| {
        pose.tp_use_arm[0]
    }),
    ("variable.tp_melee_spear_use_arm_rotation_y", |pose| {
        pose.tp_use_arm[1]
    }),
    ("variable.tp_melee_spear_use_arm_rotation_z", |pose| {
        pose.tp_use_arm[2]
    }),
    ("variable.tp_melee_spear_use_item_rotation_x", |pose| {
        pose.tp_use_item_x
    }),
    (
        "variable.tp_melee_spear_use_attachable_rotation_z",
        |pose| pose.tp_use_attachable_z,
    ),
    ("variable.tp_melee_spear_attack_arm_rotation_x", |pose| {
        pose.tp_attack_arm_x
    }),
    ("variable.tp_melee_spear_attack_item_rotation_x", |pose| {
        pose.tp_attack_item_x
    }),
    (
        "variable.tp_melee_spear_attack_attachable_position_z",
        |pose| pose.tp_attack_attachable_z,
    ),
];

impl Slots {
    /// Binds only pose variables actually consumed by the loaded carrier.
    pub(super) fn new(mut slot: impl FnMut(&str) -> Option<usize>) -> Self {
        Self(
            VARIABLES
                .iter()
                .filter_map(|(name, value)| slot(name).map(|slot| (slot, *value)))
                .collect(),
        )
    }

    /// Publishes one sampled pose without advancing any actor or item clocks.
    pub(super) fn apply(
        &self,
        variables: &mut MolangVariables,
        actor: &ActorSnapshot,
        context: &ActorTickContext,
        input: &ActorTickInput,
        attack_time: f32,
    ) {
        let base_arm_x = if query::actor_flag(actor, query::FLAG_SWIMMING)
            || query::actor_flag(actor, query::FLAG_GLIDING)
        {
            -115.0 - input.pitch
        } else if query::actor_flag(actor, crate::actor_store::ACTOR_FLAG_CRAWLING) {
            -115.0
        } else {
            -30.0
        };
        let mut pose = SpearPose {
            base_arm_x,
            ..Default::default()
        };
        if context.main_hand_is_spear {
            pose.equipped = 1.0;
            let elapsed = if input.item_use_ticks > 0 {
                input.item_use_ticks.saturating_sub(1) as f32 + context.frame_alpha
            } else {
                0.0
            };
            pose.sample_use(elapsed, context.main_hand_kinetic.unwrap_or_default());
            pose.sample_attack(
                attack_time,
                context
                    .main_hand_swing_seconds
                    .unwrap_or(ACTOR_SWING_TICKS as f32 * ACTOR_TICK_DURATION.as_secs_f32()),
            );
        }
        for &(slot, value) in &self.0 {
            variables.set(Some(slot), value(&pose));
        }
    }
}

/// Authored player pre-animation owns the pose whenever it assigns the spear gate itself.
pub(super) fn needs_completion(assets: &RuntimeEntityAssets, binding: usize) -> bool {
    let Some(rig) = assets.rig_bindings().get(binding) else {
        return false;
    };
    if assets.symbols()[rig.entity_symbol as usize]
        .identifier
        .as_ref()
        != "minecraft:player"
    {
        return false;
    }
    let Some(script) = rig
        .pre_animation
        .and_then(|index| assets.molang_expressions().get(index as usize))
    else {
        return true;
    };
    !assets.molang_ops()
        [script.first_op as usize..script.first_op as usize + usize::from(script.op_count)]
        .iter()
        .any(|op| {
            matches!(op, MolangOp::StoreVariable(symbol)
            if assets.molang_symbols()[*symbol as usize].identifier.as_ref() == VARIABLES[0].0)
        })
}

impl SpearPose {
    /// Samples raise, sway, lower and return phases from the selected kinetic component's ticks.
    fn sample_use(&mut self, elapsed: f32, timing: protocol::KineticWeaponTiming) {
        let delay = timing.delay_ticks as f32;
        let raise = progress(0.0, delay, elapsed);
        let start = progress(0.0, 0.5, raise);
        let middle = progress(0.5, 0.8, raise);
        let end = progress(0.8, 1.0, raise);
        let sway_end = delay + timing.dismount_ticks as f32;
        let sway = progress(sway_end - 20.0, sway_end, elapsed);
        let lower_end = delay + timing.knockback_ticks as f32;
        let lower = ease(
            Curve::Elastic,
            Mode::InOut,
            progress(lower_end - 40.0, lower_end, elapsed - 20.0),
        );
        let return_end = delay + timing.damage_ticks as f32;
        let back = progress(return_end - 5.0, return_end, elapsed);
        let sway_intensity =
            2.0 * (ease(Curve::Circ, Mode::Out, sway) - ease(Curve::Circ, Mode::In, back));
        let slow = (elapsed * 19.0).to_radians().sin() * sway_intensity;
        let fast = (elapsed * 30.0).to_radians().sin() * sway_intensity;
        self.fp_use = ItemPose {
            position: [
                (start * 0.05 - end * 0.05 + slow * 0.005) * MODEL_SCALE,
                (-start * 0.075 + middle * 0.075 + fast * 0.01) * MODEL_SCALE,
                (start * 0.05 - end * 0.05 + slow * 0.005) * MODEL_SCALE,
            ],
            rotation_y: raise * 20.0 + lower * 20.0 + slow * 0.5 - back * 40.0,
            rotation_z: -60.0 * ease(Curve::Back, Mode::InOut, raise) - lower * 25.0
                + back * 85.0
                + fast * 0.5,
            attachable_rotation_z: progress(0.5, 0.55, raise) * -50.0 + sway * 90.0 - back * 40.0,
        };
        self.tp_use_arm = [
            -start * 40.0 + middle * 30.0 - end * 20.0 + lower * 20.0 + back * 10.0 + slow * 0.6,
            fast,
            slow * 0.5,
        ];
        self.tp_use_item_x = (raise - back) * 60.0;
        self.tp_use_attachable_z = (raise - sway) * 90.0;
    }

    /// Attack stages have fixed raise/thrust times and a component-length recovery.
    fn sample_attack(&mut self, attack_time: f32, seconds: f32) {
        if !seconds.is_finite() || seconds <= 0.0 {
            return;
        }
        let raise_end = 0.05 / seconds;
        let thrust_end = 0.2 / seconds;
        let raise = ease(
            Curve::Sine,
            Mode::InOut,
            progress(0.0, raise_end, attack_time),
        );
        let thrust = ease(
            Curve::Quad,
            Mode::In,
            progress(raise_end, thrust_end, attack_time),
        );
        let back = ease(
            Curve::Expo,
            Mode::InOut,
            progress(1.0 - 0.6 / seconds, 1.0, attack_time),
        );
        self.fp_attack = ItemPose {
            position: [
                (raise - thrust) * 0.1 * MODEL_SCALE,
                (back - raise) * 0.075 * MODEL_SCALE,
                (raise - thrust) * 0.65 * MODEL_SCALE,
            ],
            rotation_y: (raise - back) * 20.0,
            rotation_z: (back - raise) * 60.0,
            attachable_rotation_z: (raise - back) * 40.0,
        };
        self.tp_attack_arm_x = raise * 90.0 - thrust * 120.0 + back * 30.0;
        self.tp_attack_item_x = (thrust - back) * 60.0;
        self.tp_attack_attachable_z = (back - thrust) * 0.15 * MODEL_SCALE;
    }
}

/// Clamped inverse interpolation follows Molang's equal-endpoint handling.
fn progress(start: f32, end: f32, value: f32) -> f32 {
    assets::molang_call(
        MolangFunction::InverseLerp,
        &[start, end, value],
        &mut || 0.0,
    )
    .clamp(0.0, 1.0)
}

/// Uses the shared Molang easing implementation for the authored curve.
fn ease(curve: Curve, mode: Mode, progress: f32) -> f32 {
    assets::molang_call(
        MolangFunction::Ease(curve, mode),
        &[0.0, 1.0, progress],
        &mut || 0.0,
    )
}

#[cfg(test)]
mod tests;
