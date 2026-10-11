//! Versioned contracts for the queries admitted by the entity compiler.

use super::{MAX_MOLANG_QUERY_ARGUMENTS, MolangSymbol, MolangSymbolKind};

/// Version of the query contract inventory and runtime bindings.
pub const MOLANG_QUERY_MANIFEST_VERSION: u32 = 1;

/// Retained source a query reads; unimplemented entries deliberately use their idle value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MolangQueryHandler {
    Flag(u32),
    IntegerMetadata(u32),
    FloatMetadata { key: u32, idle: f32 },
    Evaluator,
    Unimplemented,
}

/// Value category the evaluator produces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MolangQueryOutput {
    Number,
    String,
    Property,
}

/// Implementation coverage, independent of whether a query name can be admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MolangQuerySupport {
    Implemented,
    Provisional,
    Unimplemented,
}

/// Argument counts consumed by handlers; evaluation keeps its existing permissive behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MolangQueryArguments {
    pub minimum: u8,
    pub maximum: u8,
}

/// One query's binding and explicit coverage disposition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MolangQueryDescriptor {
    pub name: &'static str,
    pub handler: MolangQueryHandler,
    pub arguments: MolangQueryArguments,
    pub output: MolangQueryOutput,
    pub support: MolangQuerySupport,
}

macro_rules! query_manifest {
    ($($variant:ident => ($name:literal, $handler:expr, $min:literal, $max:expr, $output:ident, $support:ident)),* $(,)?) => {
        /// Typed query identity, bound once when a program or carrier is admitted.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum MolangQuery { $($variant),* }
        /// Accepted query inventory, sorted for compiler admission.
        pub const MOLANG_QUERIES: &[&str] = &[$($name),*];
        /// Contracts in the same order as the accepted inventory.
        pub const MOLANG_QUERY_DESCRIPTORS: &[MolangQueryDescriptor] = &[$(MolangQueryDescriptor {
            name: $name, handler: $handler,
            arguments: MolangQueryArguments { minimum: $min, maximum: $max },
            output: MolangQueryOutput::$output, support: MolangQuerySupport::$support,
        }),*];
        impl MolangQuery {
            /// Resolves an admitted name during compilation or asset admission.
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$variant),)* _ => None }
            }
            /// Returns an integer metadata source when this contract declares one.
            pub const fn integer_metadata_key(self) -> Option<u32> {
                match self.descriptor().handler {
                    MolangQueryHandler::IntegerMetadata(key) => Some(key),
                    _ => None,
                }
            }
            /// Returns the manifest contract without a name lookup.
            pub const fn descriptor(self) -> MolangQueryDescriptor {
                match self { $(Self::$variant => MolangQueryDescriptor {
                    name: $name, handler: $handler,
                    arguments: MolangQueryArguments { minimum: $min, maximum: $max },
                    output: MolangQueryOutput::$output, support: MolangQuerySupport::$support,
                },)* }
            }
        }
    };
}

query_manifest! {
    AllAnimationsFinished => ("query.all_animations_finished", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    AnimTime => ("query.anim_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    AnyAnimationFinished => ("query.any_animation_finished", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    ApproxEq => ("query.approx_eq", MolangQueryHandler::Evaluator, 2, MAX_MOLANG_QUERY_ARGUMENTS, Number, Implemented),
    ArmorColorSlot => ("query.armor_color_slot", MolangQueryHandler::Evaluator, 0, 2, Number, Implemented),
    ArmorTextureSlot => ("query.armor_texture_slot", MolangQueryHandler::Evaluator, 0, 1, Number, Provisional),
    BaseSwingDuration => ("query.base_swing_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    Blocking => ("query.blocking", MolangQueryHandler::Flag(72), 0, 0, Number, Implemented),
    BodyXRotation => ("query.body_x_rotation", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    BodyYRotation => ("query.body_y_rotation", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    BoneAabb => ("query.bone_aabb", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    BoneOrigin => ("query.bone_origin", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    BoneRotation => ("query.bone_rotation", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    CameraDistanceRangeLerp => ("query.camera_distance_range_lerp", MolangQueryHandler::Evaluator, 2, 2, Number, Implemented),
    CameraRotation => ("query.camera_rotation", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented),
    CanDamageNearbyMobs => ("query.can_damage_nearby_mobs", MolangQueryHandler::Flag(56), 0, 0, Number, Implemented),
    CapeFlapAmount => ("query.cape_flap_amount", MolangQueryHandler::Evaluator, 0, 0, Number, Provisional),
    CurrentSquishValue => ("query.current_squish_value", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    DeathTicks => ("query.death_ticks", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    DeltaTime => ("query.delta_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    DistanceFromCamera => ("query.distance_from_camera", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    EquipmentCount => ("query.equipment_count", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    EquippedItemAnyTag => ("query.equipped_item_any_tag", MolangQueryHandler::Evaluator, 2, MAX_MOLANG_QUERY_ARGUMENTS, Number, Provisional),
    EyeTargetXRotation => ("query.eye_target_x_rotation", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    EyeTargetYRotation => ("query.eye_target_y_rotation", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    FacingTargetToRangeAttack => ("query.facing_target_to_range_attack", MolangQueryHandler::Flag(88), 0, 0, Number, Implemented),
    FrameAlpha => ("query.frame_alpha", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    FuseTime => ("query.fuse_time", MolangQueryHandler::IntegerMetadata(55), 0, 0, Number, Implemented),
    GetAnimationFrame => ("query.get_animation_frame", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    GetDefaultBonePivot => ("query.get_default_bone_pivot", MolangQueryHandler::Evaluator, 2, 2, Number, Implemented),
    GetEquippedItemName => ("query.get_equipped_item_name", MolangQueryHandler::Evaluator, 0, 1, String, Implemented),
    GetName => ("query.get_name", MolangQueryHandler::Evaluator, 0, 0, String, Implemented),
    GetRootLocatorOffset => ("query.get_root_locator_offset", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    GroundSpeed => ("query.ground_speed", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    HasAnyLeashedEntityOfType => ("query.has_any_leashed_entity_of_type", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    HasArmorSlot => ("query.has_armor_slot", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented),
    HasCape => ("query.has_cape", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    HasDashCooldown => ("query.has_dash_cooldown", MolangQueryHandler::Flag(108), 0, 0, Number, Implemented),
    HasHeadGear => ("query.has_head_gear", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    HasPlayerRider => ("query.has_player_rider", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    HasProperty => ("query.has_property", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented),
    HasRider => ("query.has_rider", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    HasTarget => ("query.has_target", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    HeadRollAngle => ("query.head_roll_angle", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    HeadXRotation => ("query.head_x_rotation", MolangQueryHandler::Evaluator, 0, 1, Number, Implemented),
    HeadYRotation => ("query.head_y_rotation", MolangQueryHandler::Evaluator, 0, 1, Number, Implemented),
    Health => ("query.health", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    HeartbeatPhase => ("query.heartbeat_phase", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    HurtDirection => ("query.hurt_direction", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    HurtTime => ("query.hurt_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    InvulnerableTicks => ("query.invulnerable_ticks", MolangQueryHandler::IntegerMetadata(48), 0, 0, Number, Implemented),
    IsAdmiring => ("query.is_admiring", MolangQueryHandler::Flag(94), 0, 0, Number, Implemented),
    IsAlive => ("query.is_alive", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    IsAngry => ("query.is_angry", MolangQueryHandler::Flag(25), 0, 0, Number, Implemented),
    IsAttachedToEntity => ("query.is_attached_to_entity", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    IsBaby => ("query.is_baby", MolangQueryHandler::Flag(11), 0, 0, Number, Implemented),
    IsCarryingBlock => ("query.is_carrying_block", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    IsCasting => ("query.is_casting", MolangQueryHandler::Flag(42), 0, 0, Number, Implemented),
    IsCelebrating => ("query.is_celebrating", MolangQueryHandler::Flag(93), 0, 0, Number, Implemented),
    IsCelebratingSpecial => ("query.is_celebrating_special", MolangQueryHandler::Flag(95), 0, 0, Number, Implemented),
    IsCharged => ("query.is_charged", MolangQueryHandler::Flag(27), 0, 0, Number, Implemented),
    IsCharging => ("query.is_charging", MolangQueryHandler::Flag(43), 0, 0, Number, Implemented),
    IsChested => ("query.is_chested", MolangQueryHandler::Flag(36), 0, 0, Number, Implemented),
    IsCrawling => ("query.is_crawling", MolangQueryHandler::Flag(114), 0, 0, Number, Implemented),
    IsCroaking => ("query.is_croaking", MolangQueryHandler::Flag(101), 0, 0, Number, Implemented),
    IsDancing => ("query.is_dancing", MolangQueryHandler::Flag(51), 0, 0, Number, Implemented),
    IsDelayedAttacking => ("query.is_delayed_attacking", MolangQueryHandler::Flag(85), 0, 0, Number, Implemented),
    IsDigging => ("query.is_digging", MolangQueryHandler::Flag(106), 0, 0, Number, Implemented),
    IsEating => ("query.is_eating", MolangQueryHandler::Flag(63), 0, 0, Number, Implemented),
    IsEatingMob => ("query.is_eating_mob", MolangQueryHandler::Flag(102), 0, 0, Number, Implemented),
    IsElder => ("query.is_elder", MolangQueryHandler::Flag(33), 0, 0, Number, Implemented),
    IsEmerging => ("query.is_emerging", MolangQueryHandler::Flag(104), 0, 0, Number, Implemented),
    IsEmoting => ("query.is_emoting", MolangQueryHandler::Flag(92), 0, 0, Number, Implemented),
    IsGhost => ("query.is_ghost", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    IsGliding => ("query.is_gliding", MolangQueryHandler::Flag(32), 0, 0, Number, Implemented),
    IsGrazing => ("query.is_grazing", MolangQueryHandler::Flag(63), 0, 0, Number, Provisional),
    IsInLava => ("query.is_in_lava", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    IsInUi => ("query.is_in_ui", MolangQueryHandler::Flag(90), 0, 0, Number, Implemented),
    IsInWater => ("query.is_in_water", MolangQueryHandler::Evaluator, 0, 0, Number, Provisional),
    IsInterested => ("query.is_interested", MolangQueryHandler::Flag(26), 0, 0, Number, Implemented),
    IsInvisible => ("query.is_invisible", MolangQueryHandler::Flag(5), 0, 0, Number, Implemented),
    IsItemEquipped => ("query.is_item_equipped", MolangQueryHandler::Evaluator, 0, 1, Number, Implemented),
    IsItemNameAny => ("query.is_item_name_any", MolangQueryHandler::Evaluator, 2, MAX_MOLANG_QUERY_ARGUMENTS, Number, Implemented),
    IsJumpGoalJumping => ("query.is_jump_goal_jumping", MolangQueryHandler::Flag(103), 0, 0, Number, Implemented),
    IsJumping => ("query.is_jumping", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    IsLayingEgg => ("query.is_laying_egg", MolangQueryHandler::Flag(60), 0, 0, Number, Implemented),
    IsLeashed => ("query.is_leashed", MolangQueryHandler::Flag(30), 0, 0, Number, Implemented),
    IsLevitating => ("query.is_levitating", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    IsLocalPlayer => ("query.is_local_player", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    IsMoving => ("query.is_moving", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    IsOnFire => ("query.is_on_fire", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    IsOnGround => ("query.is_on_ground", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    IsPersonaOrPremiumSkin => ("query.is_persona_or_premium_skin", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    IsPlayingDead => ("query.is_playing_dead", MolangQueryHandler::Flag(98), 0, 0, Number, Implemented),
    IsPowered => ("query.is_powered", MolangQueryHandler::Flag(9), 0, 0, Number, Implemented),
    IsPregnant => ("query.is_pregnant", MolangQueryHandler::Flag(59), 0, 0, Number, Implemented),
    IsResting => ("query.is_resting", MolangQueryHandler::Flag(23), 0, 0, Number, Implemented),
    IsRiding => ("query.is_riding", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    IsRidingAnyEntityOfType => ("query.is_riding_any_entity_of_type", MolangQueryHandler::Evaluator, 1, MAX_MOLANG_QUERY_ARGUMENTS, Number, Implemented),
    IsRoaring => ("query.is_roaring", MolangQueryHandler::Flag(84), 0, 0, Number, Implemented),
    IsSaddled => ("query.is_saddled", MolangQueryHandler::Flag(8), 0, 0, Number, Implemented),
    IsScared => ("query.is_scared", MolangQueryHandler::Flag(68), 0, 0, Number, Implemented),
    IsSearching => ("query.is_searching", MolangQueryHandler::Flag(113), 0, 0, Number, Implemented),
    IsShaking => ("query.is_shaking", MolangQueryHandler::Flag(40), 0, 0, Number, Implemented),
    IsShakingWetness => ("query.is_shaking_wetness", MolangQueryHandler::Flag(40), 0, 0, Number, Implemented),
    IsSheared => ("query.is_sheared", MolangQueryHandler::Flag(31), 0, 0, Number, Implemented),
    IsShieldPowered => ("query.is_shield_powered", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    IsSitting => ("query.is_sitting", MolangQueryHandler::Flag(24), 0, 0, Number, Implemented),
    IsSleeping => ("query.is_sleeping", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    IsSneaking => ("query.is_sneaking", MolangQueryHandler::Flag(1), 0, 0, Number, Implemented),
    IsSniffing => ("query.is_sniffing", MolangQueryHandler::Flag(105), 0, 0, Number, Implemented),
    IsSonicBoom => ("query.is_sonic_boom", MolangQueryHandler::Flag(107), 0, 0, Number, Implemented),
    IsSpectator => ("query.is_spectator", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    IsSprinting => ("query.is_sprinting", MolangQueryHandler::Flag(3), 0, 0, Number, Implemented),
    IsStalking => ("query.is_stalking", MolangQueryHandler::Flag(91), 0, 0, Number, Implemented),
    IsStanding => ("query.is_standing", MolangQueryHandler::Flag(39), 0, 0, Number, Implemented),
    IsStunned => ("query.is_stunned", MolangQueryHandler::Flag(83), 0, 0, Number, Implemented),
    IsSwimming => ("query.is_swimming", MolangQueryHandler::Flag(57), 0, 0, Number, Implemented),
    IsTamed => ("query.is_tamed", MolangQueryHandler::Flag(28), 0, 0, Number, Implemented),
    IsUsingItem => ("query.is_using_item", MolangQueryHandler::Flag(4), 0, 0, Number, Implemented),
    ItemIsCharged => ("query.item_is_charged", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    ItemRemainingUseDuration => ("query.item_remaining_use_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    ItemSlotToBoneName => ("query.item_slot_to_bone_name", MolangQueryHandler::Evaluator, 1, 1, String, Implemented),
    KeyFrameLerpTime => ("query.key_frame_lerp_time", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    KineticWeaponDamageDuration => ("query.kinetic_weapon_damage_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    KineticWeaponDelay => ("query.kinetic_weapon_delay", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    KineticWeaponDismountDuration => ("query.kinetic_weapon_dismount_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    KineticWeaponKnockbackDuration => ("query.kinetic_weapon_knockback_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    LieAmount => ("query.lie_amount", MolangQueryHandler::FloatMetadata { key: 93, idle: 0.0 }, 0, 0, Number, Implemented),
    LifeSpan => ("query.life_span", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    LifeTime => ("query.life_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    MainHandItemMaxDuration => ("query.main_hand_item_max_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    MainHandItemUseDuration => ("query.main_hand_item_use_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    MarkVariant => ("query.mark_variant", MolangQueryHandler::IntegerMetadata(43), 0, 0, Number, Implemented),
    MaxTradeTier => ("query.max_trade_tier", MolangQueryHandler::IntegerMetadata(102), 0, 0, Number, Implemented),
    ModelScale => ("query.model_scale", MolangQueryHandler::FloatMetadata { key: 38, idle: 1.0 }, 0, 0, Number, Implemented),
    ModifiedDistanceMoved => ("query.modified_distance_moved", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    ModifiedMoveSpeed => ("query.modified_move_speed", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    MovementDirection => ("query.movement_direction", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented),
    OverlayAlpha => ("query.overlay_alpha", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    OwnerIdentifier => ("query.owner_identifier", MolangQueryHandler::Evaluator, 0, 0, String, Implemented),
    Position => ("query.position", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented),
    PositionDelta => ("query.position_delta", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented),
    PreviousSquishValue => ("query.previous_squish_value", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    Property => ("query.property", MolangQueryHandler::Evaluator, 1, 1, Property, Implemented),
    RollCounter => ("query.roll_counter", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    RotationToCamera => ("query.rotation_to_camera", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented),
    ShakeAngle => ("query.shake_angle", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    ShakeTime => ("query.shake_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    ShowBottom => ("query.show_bottom", MolangQueryHandler::Flag(38), 0, 0, Number, Implemented),
    SitAmount => ("query.sit_amount", MolangQueryHandler::FloatMetadata { key: 89, idle: 0.0 }, 0, 0, Number, Implemented),
    SkinId => ("query.skin_id", MolangQueryHandler::IntegerMetadata(104), 0, 0, Number, Implemented),
    SleepRotation => ("query.sleep_rotation", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    SneezeCounter => ("query.sneeze_counter", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    StandingScale => ("query.standing_scale", MolangQueryHandler::Evaluator, 0, 0, Number, Provisional),
    StateTime => ("query.state_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    StructuralIntegrity => ("query.structural_integrity", MolangQueryHandler::IntegerMetadata(1), 0, 0, Number, Implemented),
    SurfaceParticleColor => ("query.surface_particle_color", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    SurfaceParticleTextureCoordinate => ("query.surface_particle_texture_coordinate", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    SurfaceParticleTextureSize => ("query.surface_particle_texture_size", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    SwellAmount => ("query.swell_amount", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    SwellingDir => ("query.swelling_dir", MolangQueryHandler::IntegerMetadata(21), 0, 0, Number, Implemented),
    SwimAmount => ("query.swim_amount", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    TailAngle => ("query.tail_angle", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    TargetXRotation => ("query.target_x_rotation", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    TargetYRotation => ("query.target_y_rotation", MolangQueryHandler::Evaluator, 0, 0, Number, Provisional),
    TextureFrameIndex => ("query.texture_frame_index", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    TicksSinceLastKineticWeaponHit => ("query.ticks_since_last_kinetic_weapon_hit", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    TimeSinceLastVibrationDetection => ("query.time_since_last_vibration_detection", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    TimeStamp => ("query.time_stamp", MolangQueryHandler::Evaluator, 0, 0, Number, Provisional),
    TimerFlag1 => ("query.timer_flag_1", MolangQueryHandler::Flag(115), 0, 0, Number, Implemented),
    TimerFlag2 => ("query.timer_flag_2", MolangQueryHandler::Flag(116), 0, 0, Number, Implemented),
    TimerFlag3 => ("query.timer_flag_3", MolangQueryHandler::Flag(117), 0, 0, Number, Implemented),
    TradeTier => ("query.trade_tier", MolangQueryHandler::IntegerMetadata(101), 0, 0, Number, Implemented),
    UnhappyCounter => ("query.unhappy_counter", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
    Variant => ("query.variant", MolangQueryHandler::IntegerMetadata(2), 0, 0, Number, Implemented),
    VerticalSpeed => ("query.vertical_speed", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    WalkDistance => ("query.walk_distance", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    WingFlapPosition => ("query.wing_flap_position", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented),
    WingFlapSpeed => ("query.wing_flap_speed", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented),
}

/// Resolves each query symbol once; non-query slots retain no handler.
pub fn bind_molang_queries(symbols: &[MolangSymbol]) -> Box<[Option<MolangQuery>]> {
    symbols
        .iter()
        .map(|symbol| {
            (symbol.kind == MolangSymbolKind::Query)
                .then(|| MolangQuery::from_name(&symbol.identifier))
                .flatten()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_bindings_keep_nonquery_slots_and_unimplemented_queries_distinct() {
        let symbols = [
            MolangSymbol {
                kind: MolangSymbolKind::Variable,
                identifier: "variable.phase".into(),
            },
            MolangSymbol {
                kind: MolangSymbolKind::Query,
                identifier: "query.is_levitating".into(),
            },
            MolangSymbol {
                kind: MolangSymbolKind::Query,
                identifier: "query.life_time".into(),
            },
        ];
        let bindings = bind_molang_queries(&symbols);
        assert_eq!(bindings[0], None);
        assert_eq!(bindings[1], Some(MolangQuery::IsLevitating));
        assert_eq!(
            bindings[1].unwrap().descriptor().support,
            MolangQuerySupport::Unimplemented
        );
        assert_eq!(bindings[2], Some(MolangQuery::LifeTime));
        assert_eq!(MolangQuery::from_name("query.not_admitted"), None);
    }

    #[test]
    fn query_contracts_have_ordered_unique_names_and_valid_argument_ranges() {
        assert!(MOLANG_QUERIES.windows(2).all(|pair| pair[0] < pair[1]));
        for descriptor in MOLANG_QUERY_DESCRIPTORS {
            assert!(descriptor.arguments.minimum <= descriptor.arguments.maximum);
            assert!(descriptor.arguments.maximum <= super::super::MAX_MOLANG_QUERY_ARGUMENTS);
            assert_eq!(
                MolangQuery::from_name(descriptor.name)
                    .unwrap()
                    .descriptor(),
                *descriptor
            );
            assert_eq!(
                descriptor.handler == MolangQueryHandler::Unimplemented,
                descriptor.support == MolangQuerySupport::Unimplemented
            );
        }
    }
}
