use assets::{MolangQuery as Q, MolangQueryHandler};
use {
    super::{evaluation::MolangValue, *},
    world::TICK_DURATION as ACTOR_TICK_DURATION,
};

mod potion;
mod wolf;

#[cfg(test)]
mod player_tests;

#[cfg(test)]
mod fish_tests;
#[cfg(test)]
mod manifest_tests;
#[cfg(test)]
mod pack_query_tests;
#[cfg(test)]
mod potion_tests;
#[cfg(test)]
mod tropical_fish_tests;

// Actor flag bits and metadata keys follow gophertunnel v1.61.0
// `minecraft/protocol/entity_metadata.go` (`EntityDataFlag*` and `EntityDataKey*`, iota from
// zero); flag bits from 64 live in the overflow flag word.
pub(super) const FLAG_SNEAKING: u32 = 1;
pub(super) const FLAG_USING_ITEM: u32 = 4;
pub(super) use crate::actor_store::FLAG_BABY;
pub(super) const FLAG_BLOCKING: u32 = 72;
pub(super) const FLAG_DAMAGE_NEARBY_MOBS: u32 = 56;
pub(super) const FLAG_GLIDING: u32 = 32;
pub(super) const FLAG_EMOTING: u32 = 92;
const FLAG_ANGRY: u32 = 25;
use crate::actor_store::ACTOR_FLAG_TAMED as FLAG_TAMED;

const KEY_CARRY_BLOCK: u32 = 23;
const KEY_NAME: u32 = 4;
const KEY_TARGET: u32 = 6;
const KEY_SWELL: u32 = 19;
pub(super) const FLAG_STANDING: u32 = 39;
pub(super) const FLAG_SWIMMING: u32 = 57;

use crate::actor_store::creeper::SWELL_FULL_TICKS;

// Actors that swim in place, so airborne means in water; without a fluid sample this stands in
// for the fish-on-land flop.
const AQUATIC: [&str; 10] = [
    "cod",
    "salmon",
    "pufferfish",
    "tropicalfish",
    "squid",
    "glow_squid",
    "guardian",
    "elder_guardian",
    "dolphin",
    "axolotl",
];

// Head-over-body yaw bound for look-at queries; needs independent measurement.
const TARGET_YAW_LIMIT: f32 = 85.0;

/// Actor state one query reads.
#[derive(Clone, Copy)]
pub(super) struct QueryInputs<'a> {
    pub(super) actor: &'a ActorSnapshot,
    pub(super) input: &'a ActorTickInput,
    pub(super) context: &'a ActorTickContext,
    pub(super) anim_tick: u64,
    pub(super) anim_time: Option<f32>,
    pub(super) swell_amount: Option<f32>,
    pub(super) life_tick: u64,
    /// Whether all and any animations of the controller state being left have finished.
    pub(super) finished: (bool, bool),
    /// Seconds since the controller state being evaluated was entered.
    pub(super) state_time: f32,
    pub(super) bones: &'a [RuntimeBone],
    pub(super) bone_names: &'a [Box<str>],
}

/// Reads one query from retained actor state; a listed query the client has no data for
/// reads its idle value (0.0, or `''` for names).
pub(super) fn query(
    evaluator: &QueryInputs<'_>,
    name: Q,
    arguments: &[MolangValue],
) -> MolangValue {
    let text = |value: Option<&str>| MolangValue::String(Arc::from(value.unwrap_or("")));
    match name {
        Q::IsSpectator => MolangValue::Number(truth(match evaluator.actor.player_game_mode {
            Some(protocol::GameModeUpdate::Explicit(mode)) => {
                mode == protocol::PlayerGameMode::Spectator
            }
            Some(protocol::GameModeUpdate::WorldDefault) => {
                evaluator.context.world_game_mode == Some(protocol::PlayerGameMode::Spectator)
            }
            _ => false,
        })),
        Q::GetEquippedItemName => {
            text(hand_item(evaluator.context, arguments.first()).map(item_name))
        }
        Q::GetName => text(match evaluator.actor.metadata.get(&KEY_NAME) {
            Some(ActorMetadataValue::String(name)) => Some(name.as_ref()),
            _ => None,
        }),
        Q::OwnerIdentifier => text(evaluator.context.attachable.map(
            |_| match &evaluator.actor.kind {
                ActorKind::Player { .. } => "minecraft:player",
                ActorKind::Entity { identifier } => identifier.as_ref(),
            },
        )),
        // Native query requires a string argument; unknown names pass through.
        Q::ItemSlotToBoneName => text(evaluator.context.attachable.and_then(|_| {
            let Some(MolangValue::String(slot)) = arguments.first() else {
                return None;
            };
            Some(match slot.as_ref() {
                "main_hand" => "rightitem",
                "off_hand" => "leftitem",
                name => name,
            })
        })),
        Q::Property => property(evaluator, arguments.first()),
        Q::HasProperty => MolangValue::Number(truth(match arguments {
            [MolangValue::String(name)] => {
                evaluator
                    .context
                    .properties
                    .as_deref()
                    .is_some_and(|definitions| {
                        definitions
                            .iter()
                            .any(|definition| definition.name == *name)
                    })
            }
            _ => false,
        })),
        Q::GetDefaultBonePivot => MolangValue::Number(default_bone_pivot(evaluator, arguments)),
        _ => MolangValue::Number(number(evaluator, name, arguments)),
    }
}

/// The actor's synced property by name: enums read as their value name, everything else as its
/// stored number; an unsynced or unknown property reads 0.
fn property(evaluator: &QueryInputs<'_>, name: Option<&MolangValue>) -> MolangValue {
    use crate::actor_store::properties::PropertyKind;
    let (actor, context) = (evaluator.actor, evaluator.context);
    let Some(MolangValue::String(name)) = name else {
        return MolangValue::Number(0.0);
    };
    let found = context.properties.as_deref().and_then(|definitions| {
        definitions
            .iter()
            .position(|definition| definition.name == *name)
            .map(|index| (index as u32, &definitions[index].kind))
    });
    let Some((index, kind)) = found else {
        return MolangValue::Number(0.0);
    };
    if let Some(value) = actor.float_properties.get(&index) {
        return MolangValue::Number(*value);
    }
    // Before the server sets it, an actor reads the pack-declared default (0 without one).
    let default = context
        .properties
        .as_deref()
        .map_or(0.0, |definitions| definitions[index as usize].default);
    let value = actor
        .int_properties
        .get(&index)
        .copied()
        .unwrap_or(default as i32);
    match kind {
        PropertyKind::Enum(values) => usize::try_from(value)
            .ok()
            .and_then(|index| values.get(index))
            .map_or(MolangValue::Number(0.0), |name| {
                MolangValue::String(Arc::clone(name))
            }),
        PropertyKind::Number if !actor.int_properties.contains_key(&index) => {
            MolangValue::Number(default)
        }
        PropertyKind::Number => MolangValue::Number(value as f32),
    }
}

/// A bone's rest pivot on one axis in authored pixels; 0 for an unknown bone or axis.
fn default_bone_pivot(evaluator: &QueryInputs<'_>, arguments: &[MolangValue]) -> f32 {
    let (Some(MolangValue::String(name)), Some(axis)) = (arguments.first(), arguments.get(1))
    else {
        return 0.0;
    };
    let Some(bone) = evaluator
        .bone_names
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(name))
        .and_then(|index| evaluator.bones.get(index))
    else {
        return 0.0;
    };
    // Rig pivots mirror authored X.
    match axis.number() as i32 {
        0 => -bone.pivot[0],
        1 => bone.pivot[1],
        2 => bone.pivot[2],
        _ => 0.0,
    }
}

fn number(evaluator: &QueryInputs<'_>, name: Q, arguments: &[MolangValue]) -> f32 {
    let (actor, input, context) = (evaluator.actor, evaluator.input, evaluator.context);
    if name == Q::AnimTime
        && let Some(time) = evaluator.anim_time
    {
        return time;
    }
    if let Some(attachable) = context.attachable {
        let remaining = attachable.use_elapsed_ticks.map_or(0, |elapsed| {
            attachable.max_use_ticks.saturating_sub(elapsed)
        }) as f32;
        match name {
            Q::FrameAlpha => return attachable.frame_alpha,
            Q::GetAnimationFrame => return attachable.animation_frame as f32,
            Q::MainHandItemUseDuration | Q::ItemRemainingUseDuration => return remaining,
            Q::MainHandItemMaxDuration => return attachable.max_use_ticks as f32,
            Q::IsUsingItem => return truth(attachable.use_elapsed_ticks.is_some()),
            Q::AnimTime => return (evaluator.anim_tick as f32 + attachable.frame_alpha) * 0.05,
            Q::LifeTime => {
                return (attachable.owner_life_tick as f32 + attachable.frame_alpha) * 0.05;
            }
            _ => {}
        }
    }
    let argument = |index: usize| arguments.get(index).map(MolangValue::number);
    if name == Q::IsInUi && evaluator.context.is_in_ui {
        return 1.0;
    }
    if name == Q::IsGrazing && actor.is_horse() {
        return truth(super::horse::is_grazing(actor));
    }
    if name == Q::SwellingDir && actor.is_creeper() {
        return actor.creeper_swelling_direction();
    }
    if let MolangQueryHandler::Flag(bit) = name.descriptor().handler {
        return truth(actor_flag(actor, bit));
    }
    if name == Q::Variant
        && let Some(variant) = potion::variant(actor)
    {
        return variant;
    }
    match name.descriptor().handler {
        MolangQueryHandler::IntegerMetadata(key) => {
            return metadata_number(actor, key).unwrap_or(0.0);
        }
        MolangQueryHandler::FloatMetadata { key, idle } => {
            return metadata_number(actor, key).unwrap_or(idle);
        }
        MolangQueryHandler::Unimplemented => return 0.0,
        _ => {}
    }
    match name {
        Q::ApproxEq => {
            let Some((first, rest)) = arguments.split_first().filter(|(_, rest)| !rest.is_empty())
            else {
                return 0.0;
            };
            truth(
                rest.iter()
                    .all(|argument| first.number() == argument.number()),
            )
        }
        Q::IsLocalPlayer => truth(context.is_local_player),
        Q::IsOnFire => truth(actor.is_on_fire()),
        Q::FrameAlpha => context.frame_alpha,
        Q::AnimTime => evaluator.anim_tick as f32 * ACTOR_TICK_DURATION.as_secs_f32(),
        Q::LifeTime => {
            (evaluator.life_tick as f32 + context.frame_alpha) * ACTOR_TICK_DURATION.as_secs_f32()
        }
        Q::DeltaTime => delta_time(context),
        Q::ModifiedDistanceMoved => input.distance_moved,
        Q::ModifiedMoveSpeed => input.move_speed,
        Q::WalkDistance => input.walk_distance,
        Q::GroundSpeed => input.velocity[0].hypot(input.velocity[2]),
        Q::VerticalSpeed => input.velocity[1],
        Q::WingFlapPosition => actor
            .dragon_animation
            .as_ref()
            .map_or(0.0, |state| state.flap_phase),
        Q::PositionDelta => argument(0)
            .filter(|axis| (0.0..3.0).contains(axis))
            .map_or(0.0, |axis| input.position_delta[axis as usize]),
        Q::MovementDirection => {
            let length = input
                .position_delta
                .iter()
                .map(|axis| axis * axis)
                .sum::<f32>();
            argument(0)
                .filter(|axis| (0.0..3.0).contains(axis) && length > 0.0)
                .map_or(0.0, |axis| {
                    input.position_delta[axis as usize] / length.sqrt()
                })
        }
        // Degrees; billboards turn to face the view, sampled at the last camera feed.
        Q::CameraRotation => argument(0).map_or(0.0, |axis| match axis as i32 {
            0 => context.camera_rotation[0],
            1 => context.camera_rotation[1],
            _ => 0.0,
        }),
        // Native actor-state position, before the actor store removes collision offsets.
        Q::Position if arguments.len() == 1 => argument(0).filter(|axis| axis.is_finite()).map_or(0.0, |axis| match axis as i32 {
            0 => input.position[0],
            1 => input.position[1] + actor.network_position_offset(),
            2 => input.position[2],
            _ => 0.0,
        }),
        Q::Position => 0.0,
        Q::RotationToCamera => argument(0).map_or(0.0, |axis| {
            rotation_to_camera(input.position, context.camera_position, axis)
        }),
        Q::DistanceFromCamera => distance_from_camera(input, context),
        Q::CameraDistanceRangeLerp => match arguments {
            [start, end] => camera_distance_range_lerp(
                distance_from_camera(input, context),
                start.number(),
                end.number(),
            ),
            _ => 0.0,
        },
        Q::TextureFrameIndex => texture_frame_index(actor),
        // Client-derived from the Hurt event; streamed metadata is not authoritative.
        Q::OverlayAlpha => {
            if actor.hurt_overlay_active() {
                crate::actor_store::HURT_OVERLAY_ALPHA
            } else {
                0.0
            }
        }
        Q::HurtTime => f32::from(actor.status.hurt_time),
        // 26.50 returns the signed actor shake counter as float.
        Q::ShakeTime => actor.status.shake_time as f32,
        Q::HurtDirection => actor.status.hurt_direction.unwrap_or(0.0),
        Q::IsCarryingBlock => truth(metadata_number(actor, KEY_CARRY_BLOCK).unwrap_or(0.0) != 0.0),
        Q::MainHandItemUseDuration => {
            input.item_use_ticks as f32 * ACTOR_TICK_DURATION.as_secs_f32()
        }
        Q::MainHandItemMaxDuration => {
            context.main_hand_max_use_ticks as f32 * ACTOR_TICK_DURATION.as_secs_f32()
        }
        Q::ItemRemainingUseDuration => {
            context
                .main_hand_max_use_ticks
                .saturating_sub(input.item_use_ticks) as f32
                * ACTOR_TICK_DURATION.as_secs_f32()
        }
        Q::BaseSwingDuration if arguments.is_empty() => context
            .main_hand_swing_seconds
            .unwrap_or(super::motion::ACTOR_SWING_TICKS as f32 * ACTOR_TICK_DURATION.as_secs_f32()),
        Q::EquippedItemAnyTag => truth(context.main_hand_is_spear
            && matches!(arguments.first(), Some(MolangValue::String(slot)) if slot.as_ref() == "slot.weapon.mainhand")
            && arguments.iter().skip(1).any(|tag| matches!(tag, MolangValue::String(tag) if tag.as_ref() == "minecraft:is_spear"))),
        Q::KineticWeaponDelay => context
            .main_hand_kinetic
            .map_or(0.0, |timing| timing.delay_ticks as f32),
        Q::KineticWeaponDismountDuration => context
            .main_hand_kinetic
            .map_or(0.0, |timing| timing.dismount_ticks as f32),
        Q::KineticWeaponKnockbackDuration => context
            .main_hand_kinetic
            .map_or(0.0, |timing| timing.knockback_ticks as f32),
        Q::KineticWeaponDamageDuration => context
            .main_hand_kinetic
            .map_or(0.0, |timing| timing.damage_ticks as f32),
        Q::TicksSinceLastKineticWeaponHit => {
            if input.item_use_ticks > 0 {
                actor
                    .status
                    .kinetic_hit_ticks
                    .map_or(-1.0, |ticks| ticks as f32)
            } else {
                -1.0
            }
        }
        Q::DeathTicks => f32::from(actor.status.death_ticks()),
        // Ticks stand in for the world clock; only the phase between actors differs.
        Q::TimeStamp => evaluator.life_tick as f32,
        Q::HasTarget => truth(has_target(actor)),
        Q::SwellAmount if actor.is_creeper() => evaluator
            .swell_amount
            .unwrap_or_else(|| actor.creeper_swell_amount(context.frame_alpha)),
        Q::SwellAmount => metadata_number(actor, KEY_SWELL)
            .map_or(0.0, |swell| (swell / SWELL_FULL_TICKS).max(0.0)),
        // Wither armor shows below half health.
        Q::IsShieldPowered => truth(
            actor
                .attributes
                .get("minecraft:health")
                .is_some_and(|health| health.max > 0.0 && health.current <= health.max * 0.5),
        ),
        Q::SwimAmount => input.swim_amount,
        // Unsmoothed 0/1 stand-in for the pose blend.
        Q::StandingScale => truth(actor_flag(actor, FLAG_STANDING)),
        Q::IsInWater => truth(in_water(actor, input)),
        Q::SleepRotation => actor.status.sleep_rotation.unwrap_or(0.0),
        // Grows with ground speed; the scale needs independent measurement.
        Q::CapeFlapAmount => (input.velocity[0].hypot(input.velocity[2]) * 4.0).clamp(0.0, 1.0),
        Q::HasCape => truth(context.has_cape),
        Q::ItemIsCharged => truth(context.hand_charged),
        Q::IsInLava => truth(actor.status.fluid.is_some_and(|(_, lava)| lava)),
        Q::ArmorTextureSlot => argument(0).map_or(0.0, |slot| armor_texture_slot(context, slot)),
        Q::ArmorColorSlot => armor_color_slot(context, argument(0), argument(1)),
        Q::HasArmorSlot => truth(match arguments {
            [slot] => {
                let slot = slot.number().floor();
                (0.0..4.0).contains(&slot) && worn_armor(context, slot).is_some()
            }
            _ => false,
        }),
        Q::IsOnGround => truth(input.on_ground),
        Q::IsRiding => truth(input.is_riding),
        Q::IsMoving => truth(input.position_delta.iter().any(|axis| *axis != 0.0)),
        Q::IsAlive => truth(health(actor).is_none_or(|health| health > 0.0)),
        Q::Health => health(actor).unwrap_or(0.0),
        Q::TailAngle => wolf::tail_angle(actor),
        Q::IsSleeping => truth(actor.player_is_sleeping()),
        Q::BodyYRotation => input.body_yaw,
        Q::BodyXRotation | Q::TargetXRotation => input.pitch,
        Q::TargetYRotation => {
            if actor.target_rotation_is_absolute() {
                input.yaw
            } else {
                head_relative_yaw(input, TARGET_YAW_LIMIT)
            }
        }
        // Only the one-argument forms carry a value; the bare forms read as zero.
        Q::HeadYRotation => argument(0).map_or(0.0, |limit| head_relative_yaw(input, limit.abs())),
        Q::HeadXRotation => argument(0).map_or(0.0, |_| input.pitch),
        Q::StateTime => evaluator.state_time,
        Q::AllAnimationsFinished => truth(evaluator.finished.0),
        Q::AnyAnimationFinished => truth(evaluator.finished.1),
        Q::IsItemEquipped => truth(hand_item(context, arguments.first()).is_some()),
        Q::IsItemNameAny => truth(item_name_matches(context, arguments)),
        Q::IsRidingAnyEntityOfType => truth(context.ridden.as_deref().is_some_and(|ridden| {
            arguments
                .iter()
                .any(|name| matches!(name, MolangValue::String(name) if name.as_ref() == ridden))
        })),
        Q::HasRider => truth(context.has_rider),
        Q::HasPlayerRider => truth(context.has_player_rider),
        Q::GetAnimationFrame | Q::BaseSwingDuration => 0.0,
        // Metadata and unresolved handlers have already returned above; string and property
        // handlers are dispatched by `query`. A new evaluator must supply its own arm.
        _ => unreachable!("query manifest evaluator has no dispatch arm"),
    }
}

fn distance_from_camera(input: &ActorTickInput, context: &ActorTickContext) -> f32 {
    (0..3)
        .map(|axis| (context.camera_position[axis] - input.position[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
}

fn camera_distance_range_lerp(distance: f32, start: f32, end: f32) -> f32 {
    let (near, far) = (start.min(end), start.max(end));
    let amount = if distance <= near {
        0.0
    } else if distance >= far {
        1.0
    } else {
        (distance - near) / (far - near)
    };
    if end < start { 1.0 - amount } else { amount }
}

pub(super) fn delta_time(context: &ActorTickContext) -> f32 {
    context
        .attachable
        .and_then(|input| input.delta_seconds)
        .unwrap_or_else(|| {
            context.animation_elapsed_ticks.unwrap_or(1) as f32 * ACTOR_TICK_DURATION.as_secs_f32()
        })
}

pub(super) fn is_arrow(actor: &ActorSnapshot) -> bool {
    matches!(&actor.kind, ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:arrow")
}

fn worn_armor(context: &ActorTickContext, slot: f32) -> Option<&super::tick::WornArmor> {
    if !(0.0..5.0).contains(&slot) {
        return None;
    }
    context.armor[slot as usize].as_ref()
}

/// Elytra suppresses the player's cape and outer chest skin layer while equipped.
pub(super) fn wearing_elytra(context: &ActorTickContext) -> bool {
    worn_armor(context, 1.0).is_some_and(|armor| item_name(&armor.item) == "elytra")
}

// Material indices follow the order of the pack's armor texture arrays (none, leather, iron, gold,
// diamond, copper, netherite); chainmail, turtle and elytra need independent measurement.
fn armor_texture_slot(context: &ActorTickContext, slot: f32) -> f32 {
    let Some(armor) = worn_armor(context, slot) else {
        return 0.0;
    };
    let name = item_name(&armor.item);
    // The chest slot reads 5 for an elytra, which hides the cape.
    if slot == 1.0 && wearing_elytra(context) {
        return 5.0;
    }
    let material = name.split('_').next().unwrap_or("");
    match material {
        "leather" => 1.0,
        "iron" => 2.0,
        "golden" | "gold" => 3.0,
        "diamond" => 4.0,
        "copper" => 5.0,
        "netherite" => 6.0,
        _ => 0.0,
    }
}

// Undyed leather tints with the public default colour; every other stack tints white.
use assets::DEFAULT_LEATHER_RGB;

/// One 0..1 channel (0 red, 1 green, 2 blue, 3 alpha) of the worn stack's tint.
fn armor_color_slot(context: &ActorTickContext, slot: Option<f32>, channel: Option<f32>) -> f32 {
    let (Some(slot), Some(channel)) = (slot, channel) else {
        return 0.0;
    };
    let Some(armor) = worn_armor(context, slot) else {
        return 1.0;
    };
    let rgb = armor.dye_rgb.or_else(|| {
        item_name(&armor.item)
            .starts_with("leather_")
            .then_some(DEFAULT_LEATHER_RGB)
    });
    match (channel as i32, rgb) {
        (0..=2, Some(rgb)) => ((rgb >> (16 - 8 * channel as u32)) & 0xff) as f32 / 255.0,
        _ => 1.0,
    }
}

/// The equipped item a hand argument names: 0 or `'main_hand'` (the default), 1 or
/// `'off_hand'`.
fn hand_item<'a>(context: &'a ActorTickContext, hand: Option<&MolangValue>) -> Option<&'a str> {
    let off_hand = match hand {
        None => false,
        Some(MolangValue::Number(value)) => value.trunc() == 1.0,
        Some(MolangValue::String(name)) => name.as_ref() == "off_hand",
        Some(MolangValue::ActorReference(_)) => false,
    };
    if off_hand {
        context.off_hand.as_deref()
    } else {
        context.main_hand.as_deref()
    }
}

/// Legacy item name without its namespace.
fn item_name(identifier: &str) -> &str {
    identifier
        .split_once(':')
        .map_or(identifier, |(_, name)| name)
}

fn item_name_matches(context: &ActorTickContext, arguments: &[MolangValue]) -> bool {
    let Some(MolangValue::String(slot)) = arguments.first() else {
        return false;
    };
    let item = match slot.as_ref() {
        "slot.weapon.mainhand" => context.main_hand.as_deref(),
        "slot.weapon.offhand" => context.off_hand.as_deref(),
        _ => None,
    };
    let names = match arguments.get(1) {
        Some(MolangValue::Number(_)) => &arguments[2..],
        _ => &arguments[1..],
    };
    item.is_some_and(|item| {
        names
            .iter()
            .any(|name| matches!(name, MolangValue::String(name) if name.as_ref() == item))
    })
}

const KEY_ACTOR_VALUE: u32 = 15;

/// Sprite frame of an experience orb, chosen by its XP value; other actors use frame 0.
fn texture_frame_index(actor: &ActorSnapshot) -> f32 {
    let is_orb = matches!(&actor.kind, ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:xp_orb");
    if !is_orb {
        return 0.0;
    }
    let Some(ActorMetadataValue::Int(value)) = actor.metadata.get(&KEY_ACTOR_VALUE) else {
        return 0.0;
    };
    const UPPER_BOUNDS: [i32; 10] = [2, 6, 16, 36, 72, 148, 306, 616, 1236, 2476];
    UPPER_BOUNDS
        .iter()
        .position(|bound| value <= bound)
        .unwrap_or(UPPER_BOUNDS.len()) as f32
}

pub(super) fn has_target(actor: &ActorSnapshot) -> bool {
    matches!(actor.metadata.get(&KEY_TARGET), Some(ActorMetadataValue::Long(id)) if *id != 0 && *id != -1)
}

/// The native updater tests the low byte of the Int variant for the base,
/// and the Int mark variant selects one of six patterns within that base family.
pub(super) fn tropical_fish_variables(actor: &ActorSnapshot) -> Option<[f32; 2]> {
    const PATTERNS_PER_FAMILY: i32 = 6;
    if !matches!(&actor.kind, ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:tropicalfish")
    {
        return None;
    }
    // Unlike the numeric Molang queries, this native updater requires Int metadata.
    let integer = |query: Q| {
        query
            .integer_metadata_key()
            .and_then(|key| match actor.metadata.get(&key) {
                Some(ActorMetadataValue::Int(value)) => Some(*value),
                _ => None,
            })
            .unwrap_or(0)
    };
    let base = integer(Q::Variant) as u8 != 0;
    let mark = integer(Q::MarkVariant);
    let pattern = if (0..PATTERNS_PER_FAMILY).contains(&mark) {
        mark
    } else {
        0
    };
    Some([
        truth(base),
        (pattern + if base { PATTERNS_PER_FAMILY } else { 0 }) as f32,
    ])
}

/// Sampled fluid at the actor when available; otherwise the swimming flag or airborne fish.
fn in_water(actor: &ActorSnapshot, input: &ActorTickInput) -> bool {
    if let Some((water, _)) = actor.status.fluid {
        return water;
    }
    let aquatic = match &actor.kind {
        ActorKind::Entity { identifier } => {
            let name = identifier.as_ref();
            AQUATIC.contains(&name.strip_prefix("minecraft:").unwrap_or(name))
        }
        ActorKind::Player { .. } => false,
    };
    actor_flag(actor, FLAG_SWIMMING) || (aquatic && !input.on_ground)
}

fn health(actor: &ActorSnapshot) -> Option<f32> {
    actor
        .attributes
        .get("minecraft:health")
        .map(|health| health.current)
}

fn metadata_number(actor: &ActorSnapshot, key: u32) -> Option<f32> {
    match actor.metadata.get(&key)? {
        ActorMetadataValue::Byte(value) => Some(f32::from(*value)),
        ActorMetadataValue::Short(value) => Some(f32::from(*value)),
        ActorMetadataValue::Int(value) => Some(*value as f32),
        ActorMetadataValue::Long(value) => Some(*value as f32),
        ActorMetadataValue::Float(value) => Some(*value),
        _ => None,
    }
}

pub(super) fn actor_flag(actor: &ActorSnapshot, bit: u32) -> bool {
    actor.flag(bit)
}

fn head_relative_yaw(input: &ActorTickInput, limit: f32) -> f32 {
    wrap_degrees(input.head_yaw - input.body_yaw).clamp(-limit, limit)
}

pub(super) fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

fn truth(value: bool) -> f32 {
    if value { 1.0 } else { 0.0 }
}

/// Degrees that aim from `actor` at `camera`: axis 0 the pitch, axis 1 the yaw, as the vanilla
/// client computes them from the normalised offset (a zero offset reads 0 and -90).
fn rotation_to_camera(actor: [f32; 3], camera: [f32; 3], axis: f32) -> f32 {
    let offset: [f32; 3] = std::array::from_fn(|index| camera[index] - actor[index]);
    let length = offset.iter().map(|value| value * value).sum::<f32>().sqrt();
    let [x, y, z] = if length < 1.0e-4 {
        [0.0; 3]
    } else {
        offset.map(|value| value / length)
    };
    match axis as i32 {
        0 => -y.atan2(z.hypot(x)).to_degrees(),
        1 => z.atan2(x).to_degrees() - 90.0,
        _ => 0.0,
    }
}

#[cfg(test)]
/// Binds a test's named query through the same admission contract.
fn named_query(inputs: &QueryInputs<'_>, name: &str, arguments: &[MolangValue]) -> MolangValue {
    query(
        inputs,
        Q::from_name(name).expect("test query is admitted"),
        arguments,
    )
}
