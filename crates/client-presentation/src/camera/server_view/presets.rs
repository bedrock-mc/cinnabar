use std::sync::Arc;

use bevy::prelude::Quat;

use super::{
    runtime::Pose,
    target::{TargetFocus, TargetSettings},
};
use protocol::{CameraAimAssistPresetSettings, CameraPreset};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PresetKind {
    Free,
    FirstPerson,
    ThirdPerson,
    ThirdPersonFront,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct PresetOverrides {
    pub(super) stationary: Option<Pose>,
    pub(super) target_settings: TargetSettings,
    pub(super) focus: Option<TargetFocus>,
    pub(super) orbit_rotation: Option<Quat>,
    pub(super) position: Option<[f32; 3]>,
    pub(super) rotation: Option<Quat>,
    pub(super) view_offset: Option<[f32; 2]>,
    pub(super) entity_offset: Option<[f32; 3]>,
}

#[derive(Debug, Default, Clone)]
pub(super) struct ResolvedPreset {
    pub(super) base_name: Option<Arc<str>>,
    pub(super) kind: Option<PresetKind>,
    pub(super) position: [Option<f32>; 3],
    pub(super) rotation_degrees: [Option<f32>; 2],
    pub(super) radius: Option<f32>,
    pub(super) view_offset: Option<[f32; 2]>,
    pub(super) entity_offset: Option<[f32; 3]>,
    pub(super) aim_assist: Option<CameraAimAssistPresetSettings>,
    pub(super) control_scheme: Option<u8>,
    pub(super) listener: Option<u8>,
    pub(super) player_effects: Option<bool>,
    pub(super) rotation_speed: Option<f32>,
    pub(super) target_distance: Option<f32>,
    pub(super) snap_to_target: Option<bool>,
    pub(super) continue_targeting: Option<bool>,
    pub(super) horizontal_rotation_limit: Option<[f32; 2]>,
    pub(super) vertical_rotation_limit: Option<[f32; 2]>,
    pub(super) starting_rotation: Option<[f32; 2]>,
    pub(super) use_starting_rotation: bool,
    pub(super) yaw_limit_min: Option<f32>,
    pub(super) yaw_limit_max: Option<f32>,
}

impl ResolvedPreset {
    /// Fills only the fields no more-derived preset already declared.
    pub(super) fn absorb(&mut self, preset: &CameraPreset) {
        for (slot, value) in self.position.iter_mut().zip(preset.position) {
            *slot = slot.or(value);
        }
        for (slot, value) in self
            .rotation_degrees
            .iter_mut()
            .zip(preset.rotation_degrees)
        {
            *slot = slot.or(value);
        }
        self.radius = self.radius.or(preset.radius);
        self.view_offset = self.view_offset.or(preset.view_offset);
        self.entity_offset = self.entity_offset.or(preset.entity_offset);
        self.control_scheme = self.control_scheme.or(preset.control_scheme);
        self.listener = self.listener.or(preset.listener);
        self.player_effects = self.player_effects.or(preset.player_effects);
        self.rotation_speed = self.rotation_speed.or(preset.rotation_speed);
        self.target_distance = self.target_distance.or(preset.block_listening_radius);
        self.snap_to_target = self.snap_to_target.or(preset.snap_to_target);
        self.continue_targeting = self.continue_targeting.or(preset.continue_targeting);
        self.horizontal_rotation_limit = self
            .horizontal_rotation_limit
            .or(preset.horizontal_rotation_limit);
        self.vertical_rotation_limit = self
            .vertical_rotation_limit
            .or(preset.vertical_rotation_limit);
        self.yaw_limit_min = self.yaw_limit_min.or(preset.yaw_limit_min);
        self.yaw_limit_max = self.yaw_limit_max.or(preset.yaw_limit_max);

        if self.aim_assist.is_none() {
            self.aim_assist = preset.aim_assist.clone();
        }
    }
}

/// Maps vanilla base preset names to the supported camera rigs.
pub(super) fn preset_kind_from_name(name: &str) -> Option<PresetKind> {
    match name {
        "minecraft:free" => Some(PresetKind::Free),
        "minecraft:first_person" => Some(PresetKind::FirstPerson),
        "minecraft:third_person" | "minecraft:follow_orbit" | "minecraft:fixed_boom" => {
            Some(PresetKind::ThirdPerson)
        }
        "minecraft:third_person_front" => Some(PresetKind::ThirdPersonFront),
        _ => None,
    }
}
