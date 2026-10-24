//! Names of variables the actor presentation engine writes into authored Molang programs.

macro_rules! engine_variables {
    ($($variant:ident => $name:literal,)+) => {
        /// An engine-written variable; authored variables keep their own symbol slots.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum EngineVariable { $($variant,)+ }
        impl EngineVariable {
            /// Every fixed variable managed by the presentation engine.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            /// The exact symbol spelling used during carrier admission.
            pub const fn name(self) -> &'static str { match self { $(Self::$variant => $name,)+ } }
        }
    };
}

engine_variables! {
    AnimationFrames128x128 => "variable.animation_frames_128x128",
    AnimationFrames32x32 => "variable.animation_frames_32x32",
    AnimationFramesFace => "variable.animation_frames_face",
    Animationamount => "variable.animationamount",
    Animationamountprev => "variable.animationamountprev",
    AttackTime => "variable.attack_time",
    BobAnimation => "variable.bob_animation",
    ChargeAmount => "variable.charge_amount",
    ChestLayerVisible => "variable.chest_layer_visible",
    DamageNearbyMobs => "variable.damage_nearby_mobs",
    FirstPersonItemRotationFactor => "variable.first_person_item_rotation_factor",
    FirstPersonRotationFactor => "variable.first_person_rotation_factor",
    FpMeleeSpearAttackAttachableRotationZ => "variable.fp_melee_spear_attack_attachable_rotation_z",
    FpMeleeSpearAttackItemPositionX => "variable.fp_melee_spear_attack_item_position_x",
    FpMeleeSpearAttackItemPositionY => "variable.fp_melee_spear_attack_item_position_y",
    FpMeleeSpearAttackItemPositionZ => "variable.fp_melee_spear_attack_item_position_z",
    FpMeleeSpearAttackItemRotationY => "variable.fp_melee_spear_attack_item_rotation_y",
    FpMeleeSpearAttackItemRotationZ => "variable.fp_melee_spear_attack_item_rotation_z",
    FpMeleeSpearUseAttachableRotationZ => "variable.fp_melee_spear_use_attachable_rotation_z",
    FpMeleeSpearUseItemPositionX => "variable.fp_melee_spear_use_item_position_x",
    FpMeleeSpearUseItemPositionY => "variable.fp_melee_spear_use_item_position_y",
    FpMeleeSpearUseItemPositionZ => "variable.fp_melee_spear_use_item_position_z",
    FpMeleeSpearUseItemRotationY => "variable.fp_melee_spear_use_item_rotation_y",
    FpMeleeSpearUseItemRotationZ => "variable.fp_melee_spear_use_item_rotation_z",
    GlidingSpeedValue => "variable.gliding_speed_value",
    HasTarget => "variable.has_target",
    IsBlinking => "variable.is_blinking",
    IsBlocking => "variable.is_blocking",
    IsEnchanted => "variable.is_enchanted",
    IsFirstPerson => "variable.is_first_person",
    IsHoldingLeft => "variable.is_holding_left",
    IsHoldingRight => "variable.is_holding_right",
    IsHorizontalSplitscreen => "variable.is_horizontal_splitscreen",
    IsPaperdoll => "variable.is_paperdoll",
    IsSneaking => "variable.is_sneaking",
    IsUsingVr => "variable.is_using_vr",
    IsVerticalSplitscreen => "variable.is_vertical_splitscreen",
    LeftArmSwimAmount => "variable.left_arm_swim_amount",
    MapFaceIcon => "variable.map_face_icon",
    MeleeSpearEquipped => "variable.melee_spear_equipped",
    OpenMouth => "variable.open_mouth",
    PlayerArmHeight => "variable.player_arm_height",
    PlayerXRotation => "variable.player_x_rotation",
    RightArmSwimAmount => "variable.right_arm_swim_amount",
    ShakeTail => "variable.shake_tail",
    ShortArmOffsetLeft => "variable.short_arm_offset_left",
    ShortArmOffsetRight => "variable.short_arm_offset_right",
    ShulkerFacingDirection => "variable.shulker.facingdirection",
    ShulkerPeekAmount => "variable.shulker.peekamount",
    StandAnim => "variable.stand_anim",
    SwimAmount => "variable.swim_amount",
    TpMeleeSpearAttackArmRotationX => "variable.tp_melee_spear_attack_arm_rotation_x",
    TpMeleeSpearAttackAttachablePositionZ => "variable.tp_melee_spear_attack_attachable_position_z",
    TpMeleeSpearAttackItemRotationX => "variable.tp_melee_spear_attack_item_rotation_x",
    TpMeleeSpearBaseArmRotationX => "variable.tp_melee_spear_base_arm_rotation_x",
    TpMeleeSpearUseArmRotationX => "variable.tp_melee_spear_use_arm_rotation_x",
    TpMeleeSpearUseArmRotationY => "variable.tp_melee_spear_use_arm_rotation_y",
    TpMeleeSpearUseArmRotationZ => "variable.tp_melee_spear_use_arm_rotation_z",
    TpMeleeSpearUseAttachableRotationZ => "variable.tp_melee_spear_use_attachable_rotation_z",
    TpMeleeSpearUseItemRotationX => "variable.tp_melee_spear_use_item_rotation_x",
    TropicalfishBase => "variable.tropicalfish.base",
    TropicalfishPattern => "variable.tropicalfish.pattern",
    UseBlinkingAnimation => "variable.use_blinking_animation",
    UseItemIntervalProgress => "variable.use_item_interval_progress",
    UseItemStartupProgress => "variable.use_item_startup_progress",
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn engine_symbols_are_unique_variable_names() {
        let names = EngineVariable::ALL
            .iter()
            .map(|v| v.name())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), EngineVariable::ALL.len());
        assert!(
            names
                .iter()
                .all(|name| name.starts_with("variable.") && name.len() > "variable.".len())
        );
    }
}
