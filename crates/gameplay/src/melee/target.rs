//! Actor picking and attack-target classification.
use client_world::ActorSnapshot;

/// Default pick-box inflation and actor-versus-block bias, evaluated as native floats.
const ACTOR_PICK_RADIUS: f32 = 0.1;
/// Axes with a smaller squared segment displacement do not supply a clipping plane.
const PICK_AXIS_EPSILON_SQUARED: f32 = 1e-7;
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

/// Returns the nearest actor's persistent pick ID when it wins the current actor/block ray.
pub fn actor_pick_unique_id(
    world: &client_world::WorldAuthority,
    origin: [f32; 3],
    direction: [f32; 3],
    reach: f64,
    block_distance: Option<f64>,
    excluded_unique_id: Option<i64>,
) -> Option<i64> {
    let actor = pick_actor_by(
        world.remote_actors(),
        |actor| world.pick_hit_boxes(actor),
        excluded_unique_id,
        origin,
        direction,
        reach,
    );
    let Crosshair::Actor(hit) = classify(actor, block_distance, reach) else {
        return None;
    };
    Some(world.actor(hit.runtime_id)?.unique_id)
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
    if !reach.is_finite() || reach < 0.0 {
        return None;
    }
    let direction = sim::normalize_direction(direction)?;
    let end = std::array::from_fn(|axis| origin[axis] + direction[axis] * reach as f32);
    actors
        .filter(|actor| Some(actor.unique_id) != excluded_unique_id && actor_is_pickable(actor))
        .filter_map(|actor| {
            let point = hit_boxes(actor).into_iter().find_map(|(min, max)| {
                let min = min.map(|axis| axis - ACTOR_PICK_RADIUS);
                let max = max.map(|axis| axis + ACTOR_PICK_RADIUS);
                pick_box_point(origin, end, min, max)
            })?;
            let distance_squared = pick_distance_squared(origin, point);
            Some((
                ActorHit {
                    runtime_id: actor.runtime_id,
                    distance: f64::from(distance_squared.sqrt()),
                    point,
                },
                distance_squared,
            ))
        })
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(hit, _)| hit)
}

/// Clips a float world-space segment to the inflated box, retaining the closest face.
/// An origin inside the box is itself the hit point; tied faces retain axis order.
fn pick_box_point(
    origin: [f32; 3],
    end: [f32; 3],
    min: [f32; 3],
    max: [f32; 3],
) -> Option<[f32; 3]> {
    if !origin
        .into_iter()
        .chain(end)
        .chain(min)
        .chain(max)
        .all(f32::is_finite)
        || (0..3).any(|axis| min[axis] >= max[axis])
    {
        return None;
    }
    if (0..3).all(|axis| (min[axis]..=max[axis]).contains(&origin[axis])) {
        return Some(origin);
    }
    let delta: [f32; 3] = std::array::from_fn(|axis| end[axis] - origin[axis]);
    let mut closest: Option<([f32; 3], f32)> = None;
    for axis in 0..3 {
        if delta[axis] * delta[axis] < PICK_AXIS_EPSILON_SQUARED {
            continue;
        }
        for plane in [min[axis], max[axis]] {
            let fraction = (plane - origin[axis]) / delta[axis];
            if !(0.0..=1.0).contains(&fraction) {
                continue;
            }
            let point = std::array::from_fn(|i| origin[i] + delta[i] * fraction);
            if (0..3).any(|i| i != axis && !(min[i]..=max[i]).contains(&point[i])) {
                continue;
            }
            let distance = pick_distance_squared(origin, point);
            if closest.is_none_or(|(_, previous)| distance < previous) {
                closest = Some((point, distance));
            }
        }
    }
    closest.map(|(point, _)| point)
}

/// Measures an unclamped world-space block intercept with actor occlusion rounding.
pub fn block_pick_distance(origin: [f32; 3], point: [f32; 3]) -> f64 {
    f64::from(pick_distance(origin, point))
}

/// Measures a pick intercept using single-precision offsets, products and square root.
pub fn pick_distance(origin: [f32; 3], point: [f32; 3]) -> f32 {
    pick_distance_squared(origin, point).sqrt()
}

/// Keeps nearest-hit ordering precise when distinct squared distances share a rounded root.
fn pick_distance_squared(origin: [f32; 3], point: [f32; 3]) -> f32 {
    let [x, y, z] = std::array::from_fn(|axis| point[axis] - origin[axis]);
    z * z + y * y + x * x
}

/// Reads the admitted obstruction disposition independently from picking.
pub fn obstructs_placement(actor: &ActorSnapshot) -> bool {
    matches!(actor.kind, protocol::ActorKind::Player { .. })
        || actor.interaction_traits.obstructs_placement() == Some(true)
}

/// Whether attack picking admits this actor; only a resolved non-pickable class is excluded,
/// so unresolved custom identities keep their collision box or server hitboxes.
pub fn actor_is_pickable(actor: &ActorSnapshot) -> bool {
    match &actor.kind {
        protocol::ActorKind::Player { .. } => true,
        protocol::ActorKind::Entity { .. } => actor.interaction_traits.pickable() != Some(false),
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

/// Resolves actor occlusion from the rounded world intercept, ignoring cached ray distance.
pub fn classify_block_hit(
    actor: Option<ActorHit>,
    origin: [f32; 3],
    block: Option<&sim::BlockHit>,
    reach: f64,
) -> Crosshair {
    let distance = block.map(|hit| {
        block_pick_distance(
            origin,
            [
                hit.hit_world.x as f32,
                hit.hit_world.y as f32,
                hit.hit_world.z as f32,
            ],
        )
    });
    classify(actor, distance, reach)
}

/// Resolves the press target; an actor wins only when clearly in front of the block.
pub fn classify(
    actor: Option<ActorHit>,
    block_distance: Option<f64>,
    attack_reach: f64,
) -> Crosshair {
    let limit = block_distance.map_or(f32::INFINITY, |distance| distance as f32);
    match actor {
        Some(hit) if hit.distance as f32 + ACTOR_PICK_RADIUS < limit.min(attack_reach as f32) => {
            Crosshair::Actor(hit)
        }
        Some(hit) if hit.distance as f32 + ACTOR_PICK_RADIUS < limit => Crosshair::Miss,
        _ if block_distance.is_some() => Crosshair::Block,
        _ => Crosshair::Miss,
    }
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;
