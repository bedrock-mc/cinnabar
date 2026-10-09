//! Actor picking and attack-target classification.
use client_world::ActorSnapshot;

/// Pick-box inflation and actor-versus-block bias. Needs independent measurement.
const ACTOR_PICK_RADIUS: f64 = 0.1;
/// Actors vanilla cannot pick: drops, orbs, projectiles and effect carriers.
const UNPICKABLE_ACTORS: &[&str] = &[
    "minecraft:item",
    "minecraft:xp_orb",
    "minecraft:arrow",
    "minecraft:thrown_trident",
    "minecraft:snowball",
    "minecraft:egg",
    "minecraft:ender_pearl",
    "minecraft:splash_potion",
    "minecraft:lingering_potion",
    "minecraft:xp_bottle",
    "minecraft:fireball",
    "minecraft:small_fireball",
    "minecraft:wither_skull",
    "minecraft:wither_skull_dangerous",
    "minecraft:dragon_fireball",
    "minecraft:wind_charge_projectile",
    "minecraft:breeze_wind_charge_projectile",
    "minecraft:fishing_hook",
    "minecraft:falling_block",
    "minecraft:lightning_bolt",
    "minecraft:area_effect_cloud",
    "minecraft:evocation_fang",
    "minecraft:eye_of_ender_signal",
    "minecraft:fireworks_rocket",
    "minecraft:llama_spit",
    "minecraft:shulker_bullet",
];

/// The nearest pickable actor along the crosshair ray.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorHit {
    pub runtime_id: u64,
    pub distance: f64,
    pub point: [f32; 3],
}

/// What an attack press resolves to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Crosshair {
    Actor(ActorHit),
    Block,
    /// Nothing in reach, including an actor in front but beyond melee reach.
    Miss,
}

/// Nearest actor with an inflated hitbox the ray enters within `reach`.
pub fn pick_actor<'a>(
    actors: impl Iterator<Item = &'a ActorSnapshot>,
    excluded_unique_id: Option<i64>,
    origin: [f32; 3],
    direction: [f32; 3],
    reach: f64,
) -> Option<ActorHit> {
    pick_actor_by(
        actors,
        ActorSnapshot::hit_boxes,
        excluded_unique_id,
        origin,
        direction,
        reach,
    )
}

/// [`pick_actor`] against the `(min, max)` boxes `hit_boxes` places each actor at; the
/// first intersecting box in server order within reach supplies each actor's hit.
pub fn pick_actor_by<'a, Boxes>(
    actors: impl Iterator<Item = &'a ActorSnapshot>,
    hit_boxes: impl Fn(&'a ActorSnapshot) -> Boxes,
    excluded_unique_id: Option<i64>,
    origin: [f32; 3],
    direction: [f32; 3],
    reach: f64,
) -> Option<ActorHit>
where
    Boxes: IntoIterator<Item = ([f32; 3], [f32; 3])>,
{
    let origin = origin.map(f64::from);
    let length = direction
        .into_iter()
        .map(|axis| f64::from(axis).powi(2))
        .sum::<f64>()
        .sqrt();
    if !length.is_finite() || length == 0.0 {
        return None;
    }
    let direction = direction.map(|axis| f64::from(axis) / length);
    actors
        .filter(|actor| Some(actor.unique_id) != excluded_unique_id && pickable(actor))
        .filter_map(|actor| {
            let distance = hit_boxes(actor).into_iter().find_map(|(min, max)| {
                let min = min.map(|axis| f64::from(axis) - ACTOR_PICK_RADIUS);
                let max = max.map(|axis| f64::from(axis) + ACTOR_PICK_RADIUS);
                ray_box_entry(origin, direction, min, max).filter(|distance| *distance <= reach)
            })?;
            Some(ActorHit {
                runtime_id: actor.runtime_id,
                distance,
                point: [0, 1, 2].map(|axis| (origin[axis] + direction[axis] * distance) as f32),
            })
        })
        .min_by(|left, right| left.distance.total_cmp(&right.distance))
}

/// Drops, orbs, projectiles and effect carriers never block a placement either.
pub fn obstructs_placement(actor: &ActorSnapshot) -> bool {
    pickable(actor)
}

/// Only actors with selectable collision boxes can consume the crosshair.
fn pickable(actor: &ActorSnapshot) -> bool {
    match &actor.kind {
        protocol::ActorKind::Player { .. } => true,
        protocol::ActorKind::Entity { identifier } => {
            !UNPICKABLE_ACTORS.contains(&identifier.as_ref())
        }
    }
}

/// Distance along a unit `direction` at which the ray enters the box; zero from inside.
pub fn ray_box_entry(
    origin: [f64; 3],
    direction: [f64; 3],
    min: [f64; 3],
    max: [f64; 3],
) -> Option<f64> {
    let mut near = 0.0_f64;
    let mut far = f64::INFINITY;
    for axis in 0..3 {
        if direction[axis].abs() < f64::EPSILON {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let first = (min[axis] - origin[axis]) / direction[axis];
        let second = (max[axis] - origin[axis]) / direction[axis];
        near = near.max(first.min(second));
        far = far.min(first.max(second));
        if near > far {
            return None;
        }
    }
    Some(near)
}

/// Resolves the press target; an actor wins only when clearly in front of the block.
pub fn classify(
    actor: Option<ActorHit>,
    block_distance: Option<f64>,
    attack_reach: f64,
) -> Crosshair {
    let limit = block_distance.unwrap_or(f64::INFINITY);
    match actor {
        Some(hit) if hit.distance + ACTOR_PICK_RADIUS < limit.min(attack_reach) => {
            Crosshair::Actor(hit)
        }
        Some(hit) if hit.distance + ACTOR_PICK_RADIUS < limit => Crosshair::Miss,
        _ if block_distance.is_some() => Crosshair::Block,
        _ => Crosshair::Miss,
    }
}
