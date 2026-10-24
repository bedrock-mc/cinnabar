//! Versioned contracts for the queries admitted by the entity compiler.

use protocol_ids::{ActorDataKey, ActorFlag};

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

/// Actor inputs that can change while a completed tick is being drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MolangQuerySampling {
    Tick,
    Frame,
    Camera,
    Rotation,
    Walk,
    Swell,
}

/// One query's binding and explicit coverage disposition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MolangQueryDescriptor {
    pub name: &'static str,
    pub handler: MolangQueryHandler,
    pub arguments: MolangQueryArguments,
    pub output: MolangQueryOutput,
    pub support: MolangQuerySupport,
    pub sampling: MolangQuerySampling,
}

macro_rules! query_manifest {
    ($($variant:ident => ($name:literal, $handler:expr, $min:literal, $max:expr, $output:ident, $support:ident, $sampling:ident)),* $(,)?) => {
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
            sampling: MolangQuerySampling::$sampling,
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
                    sampling: MolangQuerySampling::$sampling,
                },)* }
            }
        }
    };
}

query_manifest! {
    AllAnimationsFinished => ("query.all_animations_finished", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    AnimTime => ("query.anim_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    AnyAnimationFinished => ("query.any_animation_finished", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    ApproxEq => ("query.approx_eq", MolangQueryHandler::Evaluator, 2, MAX_MOLANG_QUERY_ARGUMENTS, Number, Implemented, Tick),
    ArmorColorSlot => ("query.armor_color_slot", MolangQueryHandler::Evaluator, 0, 2, Number, Implemented, Tick),
    ArmorTextureSlot => ("query.armor_texture_slot", MolangQueryHandler::Evaluator, 0, 1, Number, Provisional, Tick),
    BaseSwingDuration => ("query.base_swing_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    Blocking => ("query.blocking", MolangQueryHandler::Flag(ActorFlag::Blocking.wire_value()), 0, 0, Number, Implemented, Tick),
    BodyXRotation => ("query.body_x_rotation", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Rotation),
    BodyYRotation => ("query.body_y_rotation", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Rotation),
    BoneAabb => ("query.bone_aabb", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    BoneOrigin => ("query.bone_origin", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    BoneRotation => ("query.bone_rotation", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    CameraDistanceRangeLerp => ("query.camera_distance_range_lerp", MolangQueryHandler::Evaluator, 2, 2, Number, Implemented, Camera),
    CameraRotation => ("query.camera_rotation", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented, Camera),
    CanDamageNearbyMobs => ("query.can_damage_nearby_mobs", MolangQueryHandler::Flag(ActorFlag::DamageNearbyMobs.wire_value()), 0, 0, Number, Implemented, Tick),
    CapeFlapAmount => ("query.cape_flap_amount", MolangQueryHandler::Evaluator, 0, 0, Number, Provisional, Tick),
    CurrentSquishValue => ("query.current_squish_value", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    DeathTicks => ("query.death_ticks", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    DeltaTime => ("query.delta_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    DistanceFromCamera => ("query.distance_from_camera", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Camera),
    EquipmentCount => ("query.equipment_count", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    EquippedItemAnyTag => ("query.equipped_item_any_tag", MolangQueryHandler::Evaluator, 2, MAX_MOLANG_QUERY_ARGUMENTS, Number, Provisional, Tick),
    EyeTargetXRotation => ("query.eye_target_x_rotation", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    EyeTargetYRotation => ("query.eye_target_y_rotation", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    FacingTargetToRangeAttack => ("query.facing_target_to_range_attack", MolangQueryHandler::Flag(ActorFlag::FacingTargetToRangeAttack.wire_value()), 0, 0, Number, Implemented, Tick),
    FrameAlpha => ("query.frame_alpha", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Frame),
    FuseTime => ("query.fuse_time", MolangQueryHandler::IntegerMetadata(ActorDataKey::FuseTime.wire_value()), 0, 0, Number, Implemented, Tick),
    GetAnimationFrame => ("query.get_animation_frame", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    GetDefaultBonePivot => ("query.get_default_bone_pivot", MolangQueryHandler::Evaluator, 2, 2, Number, Implemented, Tick),
    GetEquippedItemName => ("query.get_equipped_item_name", MolangQueryHandler::Evaluator, 0, 1, String, Implemented, Tick),
    GetName => ("query.get_name", MolangQueryHandler::Evaluator, 0, 0, String, Implemented, Tick),
    GetRootLocatorOffset => ("query.get_root_locator_offset", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    GroundSpeed => ("query.ground_speed", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    HasAnyLeashedEntityOfType => ("query.has_any_leashed_entity_of_type", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    HasArmorSlot => ("query.has_armor_slot", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented, Tick),
    HasCape => ("query.has_cape", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    HasDashCooldown => ("query.has_dash_cooldown", MolangQueryHandler::Flag(ActorFlag::HasDashTimeout.wire_value()), 0, 0, Number, Implemented, Tick),
    HasHeadGear => ("query.has_head_gear", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    HasPlayerRider => ("query.has_player_rider", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    HasProperty => ("query.has_property", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented, Tick),
    HasRider => ("query.has_rider", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    HasTarget => ("query.has_target", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    HeadRollAngle => ("query.head_roll_angle", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    HeadXRotation => ("query.head_x_rotation", MolangQueryHandler::Evaluator, 0, 1, Number, Implemented, Rotation),
    HeadYRotation => ("query.head_y_rotation", MolangQueryHandler::Evaluator, 0, 1, Number, Implemented, Rotation),
    Health => ("query.health", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    HeartbeatPhase => ("query.heartbeat_phase", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    HurtDirection => ("query.hurt_direction", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    HurtTime => ("query.hurt_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    InvulnerableTicks => ("query.invulnerable_ticks", MolangQueryHandler::IntegerMetadata(ActorDataKey::InvulnerableTicks.wire_value()), 0, 0, Number, Implemented, Tick),
    IsAdmiring => ("query.is_admiring", MolangQueryHandler::Flag(ActorFlag::Admiring.wire_value()), 0, 0, Number, Implemented, Tick),
    IsAlive => ("query.is_alive", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    IsAngry => ("query.is_angry", MolangQueryHandler::Flag(ActorFlag::Angry.wire_value()), 0, 0, Number, Implemented, Tick),
    IsAttachedToEntity => ("query.is_attached_to_entity", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    IsBaby => ("query.is_baby", MolangQueryHandler::Flag(ActorFlag::Baby.wire_value()), 0, 0, Number, Implemented, Tick),
    IsCarryingBlock => ("query.is_carrying_block", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    IsCasting => ("query.is_casting", MolangQueryHandler::Flag(ActorFlag::Casting.wire_value()), 0, 0, Number, Implemented, Tick),
    IsCelebrating => ("query.is_celebrating", MolangQueryHandler::Flag(ActorFlag::Celebrating.wire_value()), 0, 0, Number, Implemented, Tick),
    IsCelebratingSpecial => ("query.is_celebrating_special", MolangQueryHandler::Flag(ActorFlag::CelebratingSpecial.wire_value()), 0, 0, Number, Implemented, Tick),
    IsCharged => ("query.is_charged", MolangQueryHandler::Flag(ActorFlag::Charged.wire_value()), 0, 0, Number, Implemented, Tick),
    IsCharging => ("query.is_charging", MolangQueryHandler::Flag(ActorFlag::Charging.wire_value()), 0, 0, Number, Implemented, Tick),
    IsChested => ("query.is_chested", MolangQueryHandler::Flag(ActorFlag::Chested.wire_value()), 0, 0, Number, Implemented, Tick),
    IsCrawling => ("query.is_crawling", MolangQueryHandler::Flag(ActorFlag::Crawling.wire_value()), 0, 0, Number, Implemented, Tick),
    IsCroaking => ("query.is_croaking", MolangQueryHandler::Flag(ActorFlag::Croaking.wire_value()), 0, 0, Number, Implemented, Tick),
    IsDancing => ("query.is_dancing", MolangQueryHandler::Flag(ActorFlag::Dancing.wire_value()), 0, 0, Number, Implemented, Tick),
    IsDelayedAttacking => ("query.is_delayed_attacking", MolangQueryHandler::Flag(ActorFlag::DelayedAttack.wire_value()), 0, 0, Number, Implemented, Tick),
    IsDigging => ("query.is_digging", MolangQueryHandler::Flag(ActorFlag::Digging.wire_value()), 0, 0, Number, Implemented, Tick),
    IsEating => ("query.is_eating", MolangQueryHandler::Flag(ActorFlag::Eating.wire_value()), 0, 0, Number, Implemented, Tick),
    IsEatingMob => ("query.is_eating_mob", MolangQueryHandler::Flag(ActorFlag::DigestMob.wire_value()), 0, 0, Number, Implemented, Tick),
    IsElder => ("query.is_elder", MolangQueryHandler::Flag(ActorFlag::Elder.wire_value()), 0, 0, Number, Implemented, Tick),
    IsEmerging => ("query.is_emerging", MolangQueryHandler::Flag(ActorFlag::Emerging.wire_value()), 0, 0, Number, Implemented, Tick),
    IsEmoting => ("query.is_emoting", MolangQueryHandler::Flag(ActorFlag::Emoting.wire_value()), 0, 0, Number, Implemented, Tick),
    IsGhost => ("query.is_ghost", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    IsGliding => ("query.is_gliding", MolangQueryHandler::Flag(ActorFlag::Gliding.wire_value()), 0, 0, Number, Implemented, Tick),
    IsGrazing => ("query.is_grazing", MolangQueryHandler::Flag(ActorFlag::Eating.wire_value()), 0, 0, Number, Provisional, Tick),
    IsInLava => ("query.is_in_lava", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    IsInUi => ("query.is_in_ui", MolangQueryHandler::Flag(ActorFlag::InUI.wire_value()), 0, 0, Number, Implemented, Tick),
    IsInWater => ("query.is_in_water", MolangQueryHandler::Evaluator, 0, 0, Number, Provisional, Tick),
    IsInterested => ("query.is_interested", MolangQueryHandler::Flag(ActorFlag::Interested.wire_value()), 0, 0, Number, Implemented, Tick),
    IsInvisible => ("query.is_invisible", MolangQueryHandler::Flag(ActorFlag::Invisible.wire_value()), 0, 0, Number, Implemented, Tick),
    IsItemEquipped => ("query.is_item_equipped", MolangQueryHandler::Evaluator, 0, 1, Number, Implemented, Tick),
    IsItemNameAny => ("query.is_item_name_any", MolangQueryHandler::Evaluator, 2, MAX_MOLANG_QUERY_ARGUMENTS, Number, Implemented, Tick),
    IsJumpGoalJumping => ("query.is_jump_goal_jumping", MolangQueryHandler::Flag(ActorFlag::JumpGoal.wire_value()), 0, 0, Number, Implemented, Tick),
    IsJumping => ("query.is_jumping", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    IsLayingEgg => ("query.is_laying_egg", MolangQueryHandler::Flag(ActorFlag::LayingEgg.wire_value()), 0, 0, Number, Implemented, Tick),
    IsLeashed => ("query.is_leashed", MolangQueryHandler::Flag(ActorFlag::Leashed.wire_value()), 0, 0, Number, Implemented, Tick),
    IsLevitating => ("query.is_levitating", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    IsLocalPlayer => ("query.is_local_player", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    IsMoving => ("query.is_moving", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    IsOnFire => ("query.is_on_fire", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    IsOnGround => ("query.is_on_ground", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    IsPersonaOrPremiumSkin => ("query.is_persona_or_premium_skin", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    IsPlayingDead => ("query.is_playing_dead", MolangQueryHandler::Flag(ActorFlag::PlayingDead.wire_value()), 0, 0, Number, Implemented, Tick),
    IsPowered => ("query.is_powered", MolangQueryHandler::Flag(ActorFlag::Powered.wire_value()), 0, 0, Number, Implemented, Tick),
    IsPregnant => ("query.is_pregnant", MolangQueryHandler::Flag(ActorFlag::Pregnant.wire_value()), 0, 0, Number, Implemented, Tick),
    IsResting => ("query.is_resting", MolangQueryHandler::Flag(ActorFlag::Resting.wire_value()), 0, 0, Number, Implemented, Tick),
    IsRiding => ("query.is_riding", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    IsRidingAnyEntityOfType => ("query.is_riding_any_entity_of_type", MolangQueryHandler::Evaluator, 1, MAX_MOLANG_QUERY_ARGUMENTS, Number, Implemented, Tick),
    IsRoaring => ("query.is_roaring", MolangQueryHandler::Flag(ActorFlag::Roaring.wire_value()), 0, 0, Number, Implemented, Tick),
    IsSaddled => ("query.is_saddled", MolangQueryHandler::Flag(ActorFlag::Saddled.wire_value()), 0, 0, Number, Implemented, Tick),
    IsScared => ("query.is_scared", MolangQueryHandler::Flag(ActorFlag::Scared.wire_value()), 0, 0, Number, Implemented, Tick),
    IsSearching => ("query.is_searching", MolangQueryHandler::Flag(ActorFlag::Searching.wire_value()), 0, 0, Number, Implemented, Tick),
    IsShaking => ("query.is_shaking", MolangQueryHandler::Flag(ActorFlag::Shaking.wire_value()), 0, 0, Number, Implemented, Tick),
    IsShakingWetness => ("query.is_shaking_wetness", MolangQueryHandler::Flag(ActorFlag::Shaking.wire_value()), 0, 0, Number, Implemented, Tick),
    IsSheared => ("query.is_sheared", MolangQueryHandler::Flag(ActorFlag::Sheared.wire_value()), 0, 0, Number, Implemented, Tick),
    IsShieldPowered => ("query.is_shield_powered", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    IsSitting => ("query.is_sitting", MolangQueryHandler::Flag(ActorFlag::Sitting.wire_value()), 0, 0, Number, Implemented, Tick),
    IsSleeping => ("query.is_sleeping", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    IsSneaking => ("query.is_sneaking", MolangQueryHandler::Flag(ActorFlag::Sneaking.wire_value()), 0, 0, Number, Implemented, Tick),
    IsSniffing => ("query.is_sniffing", MolangQueryHandler::Flag(ActorFlag::Sniffing.wire_value()), 0, 0, Number, Implemented, Tick),
    IsSonicBoom => ("query.is_sonic_boom", MolangQueryHandler::Flag(ActorFlag::SonicBoom.wire_value()), 0, 0, Number, Implemented, Tick),
    IsSpectator => ("query.is_spectator", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    IsSprinting => ("query.is_sprinting", MolangQueryHandler::Flag(ActorFlag::Sprinting.wire_value()), 0, 0, Number, Implemented, Tick),
    IsStalking => ("query.is_stalking", MolangQueryHandler::Flag(ActorFlag::Stalking.wire_value()), 0, 0, Number, Implemented, Tick),
    IsStanding => ("query.is_standing", MolangQueryHandler::Flag(ActorFlag::Standing.wire_value()), 0, 0, Number, Implemented, Tick),
    IsStunned => ("query.is_stunned", MolangQueryHandler::Flag(ActorFlag::Stunned.wire_value()), 0, 0, Number, Implemented, Tick),
    IsSwimming => ("query.is_swimming", MolangQueryHandler::Flag(ActorFlag::Swimming.wire_value()), 0, 0, Number, Implemented, Tick),
    IsTamed => ("query.is_tamed", MolangQueryHandler::Flag(ActorFlag::Tamed.wire_value()), 0, 0, Number, Implemented, Tick),
    IsUsingItem => ("query.is_using_item", MolangQueryHandler::Flag(ActorFlag::UsingItem.wire_value()), 0, 0, Number, Implemented, Tick),
    ItemIsCharged => ("query.item_is_charged", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    ItemRemainingUseDuration => ("query.item_remaining_use_duration", MolangQueryHandler::Evaluator, 0, 2, Number, Implemented, Tick),
    ItemSlotToBoneName => ("query.item_slot_to_bone_name", MolangQueryHandler::Evaluator, 1, 1, String, Implemented, Tick),
    KeyFrameLerpTime => ("query.key_frame_lerp_time", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    KineticWeaponDamageDuration => ("query.kinetic_weapon_damage_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    KineticWeaponDelay => ("query.kinetic_weapon_delay", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    KineticWeaponDismountDuration => ("query.kinetic_weapon_dismount_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    KineticWeaponKnockbackDuration => ("query.kinetic_weapon_knockback_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    LieAmount => ("query.lie_amount", MolangQueryHandler::FloatMetadata { key: ActorDataKey::LayingAmount.wire_value(), idle: 0.0 }, 0, 0, Number, Implemented, Tick),
    LifeSpan => ("query.life_span", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    LifeTime => ("query.life_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Frame),
    MainHandItemMaxDuration => ("query.main_hand_item_max_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    MainHandItemUseDuration => ("query.main_hand_item_use_duration", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    MarkVariant => ("query.mark_variant", MolangQueryHandler::IntegerMetadata(ActorDataKey::MarkVariant.wire_value()), 0, 0, Number, Implemented, Tick),
    MaxTradeTier => ("query.max_trade_tier", MolangQueryHandler::IntegerMetadata(ActorDataKey::MaxTradeTier.wire_value()), 0, 0, Number, Implemented, Tick),
    ModelScale => ("query.model_scale", MolangQueryHandler::FloatMetadata { key: ActorDataKey::Scale.wire_value(), idle: 1.0 }, 0, 0, Number, Implemented, Tick),
    ModifiedDistanceMoved => ("query.modified_distance_moved", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Walk),
    ModifiedMoveSpeed => ("query.modified_move_speed", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Walk),
    MovementDirection => ("query.movement_direction", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented, Tick),
    OverlayAlpha => ("query.overlay_alpha", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    OwnerIdentifier => ("query.owner_identifier", MolangQueryHandler::Evaluator, 0, 0, String, Implemented, Tick),
    Position => ("query.position", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented, Tick),
    PositionDelta => ("query.position_delta", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented, Tick),
    PreviousSquishValue => ("query.previous_squish_value", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    Property => ("query.property", MolangQueryHandler::Evaluator, 1, 1, Property, Implemented, Tick),
    RollCounter => ("query.roll_counter", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    RotationToCamera => ("query.rotation_to_camera", MolangQueryHandler::Evaluator, 1, 1, Number, Implemented, Camera),
    ShakeAngle => ("query.shake_angle", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    ShakeTime => ("query.shake_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    ShowBottom => ("query.show_bottom", MolangQueryHandler::Flag(ActorFlag::ShowBottom.wire_value()), 0, 0, Number, Implemented, Tick),
    SitAmount => ("query.sit_amount", MolangQueryHandler::FloatMetadata { key: ActorDataKey::SittingAmount.wire_value(), idle: 0.0 }, 0, 0, Number, Implemented, Tick),
    SkinId => ("query.skin_id", MolangQueryHandler::IntegerMetadata(ActorDataKey::SkinID.wire_value()), 0, 0, Number, Implemented, Tick),
    SleepRotation => ("query.sleep_rotation", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    SneezeCounter => ("query.sneeze_counter", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    StandingScale => ("query.standing_scale", MolangQueryHandler::Evaluator, 0, 0, Number, Provisional, Tick),
    StateTime => ("query.state_time", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    StructuralIntegrity => ("query.structural_integrity", MolangQueryHandler::IntegerMetadata(ActorDataKey::StructuralIntegrity.wire_value()), 0, 0, Number, Implemented, Tick),
    SurfaceParticleColor => ("query.surface_particle_color", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    SurfaceParticleTextureCoordinate => ("query.surface_particle_texture_coordinate", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    SurfaceParticleTextureSize => ("query.surface_particle_texture_size", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    SwellAmount => ("query.swell_amount", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Swell),
    SwellingDir => ("query.swelling_dir", MolangQueryHandler::IntegerMetadata(ActorDataKey::SwellDirection.wire_value()), 0, 0, Number, Implemented, Tick),
    SwimAmount => ("query.swim_amount", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    TailAngle => ("query.tail_angle", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    TargetXRotation => ("query.target_x_rotation", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Rotation),
    TargetYRotation => ("query.target_y_rotation", MolangQueryHandler::Evaluator, 0, 0, Number, Provisional, Rotation),
    TextureFrameIndex => ("query.texture_frame_index", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    TicksSinceLastKineticWeaponHit => ("query.ticks_since_last_kinetic_weapon_hit", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    TimeSinceLastVibrationDetection => ("query.time_since_last_vibration_detection", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    TimeStamp => ("query.time_stamp", MolangQueryHandler::Evaluator, 0, 0, Number, Provisional, Tick),
    TimerFlag1 => ("query.timer_flag_1", MolangQueryHandler::Flag(ActorFlag::TimerFlag1.wire_value()), 0, 0, Number, Implemented, Tick),
    TimerFlag2 => ("query.timer_flag_2", MolangQueryHandler::Flag(ActorFlag::TimerFlag2.wire_value()), 0, 0, Number, Implemented, Tick),
    TimerFlag3 => ("query.timer_flag_3", MolangQueryHandler::Flag(ActorFlag::TimerFlag3.wire_value()), 0, 0, Number, Implemented, Tick),
    TradeTier => ("query.trade_tier", MolangQueryHandler::IntegerMetadata(ActorDataKey::TradeTier.wire_value()), 0, 0, Number, Implemented, Tick),
    UnhappyCounter => ("query.unhappy_counter", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
    Variant => ("query.variant", MolangQueryHandler::IntegerMetadata(ActorDataKey::Variant.wire_value()), 0, 0, Number, Implemented, Tick),
    VerticalSpeed => ("query.vertical_speed", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    WalkDistance => ("query.walk_distance", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    WingFlapPosition => ("query.wing_flap_position", MolangQueryHandler::Evaluator, 0, 0, Number, Implemented, Tick),
    WingFlapSpeed => ("query.wing_flap_speed", MolangQueryHandler::Unimplemented, 0, 0, Number, Unimplemented, Tick),
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
    fn retained_query_sources_are_named_protocol_ids() {
        for descriptor in MOLANG_QUERY_DESCRIPTORS {
            match descriptor.handler {
                MolangQueryHandler::Flag(bit) => {
                    assert!(ActorFlag::from_wire(bit).is_some(), "{}", descriptor.name)
                }
                MolangQueryHandler::IntegerMetadata(key)
                | MolangQueryHandler::FloatMetadata { key, .. } => assert!(
                    ActorDataKey::from_wire(key).is_some(),
                    "{}",
                    descriptor.name
                ),
                _ => {}
            }
        }
        assert_eq!(
            MolangQuery::IsEating.descriptor().handler,
            MolangQuery::IsGrazing.descriptor().handler
        );
    }

    #[test]
    fn item_remaining_use_duration_contract_accepts_optional_slot_and_range() {
        let descriptor = MolangQuery::ItemRemainingUseDuration.descriptor();
        assert_eq!(
            descriptor.arguments,
            MolangQueryArguments {
                minimum: 0,
                maximum: 2,
            }
        );
        assert_eq!(descriptor.handler, MolangQueryHandler::Evaluator);
        assert_eq!(descriptor.output, MolangQueryOutput::Number);
        assert_eq!(descriptor.support, MolangQuerySupport::Implemented);
    }

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
