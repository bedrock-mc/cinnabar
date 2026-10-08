//! Reusable bounded CPU selection and conservative screen-tile admission.

use super::{
    LIGHT_RADIUS, LightSource, LocalLightSources, MAX_LOCAL_LIGHTS, MAX_SHADOWED_LIGHTS,
    PreparedInputs, TILE_LIGHTS, TILE_SIDE,
};
use bevy::math::{Vec2, Vec3};
use std::cmp::Ordering;

const LIGHT_REPLACEMENT_RATIO: f32 = 1.10;
const SHADOW_REPLACEMENT_RATIO: f32 = 1.25;

fn same_source(left: &LightSource, right: &LightSource) -> bool {
    left.dimension == right.dimension && left.position == right.position
}

fn source_order(left: &LightSource, right: &LightSource) -> Ordering {
    left.position
        .x
        .total_cmp(&right.position.x)
        .then_with(|| left.position.y.total_cmp(&right.position.y))
        .then_with(|| left.position.z.total_cmp(&right.position.z))
        .then_with(|| left.level.cmp(&right.level))
}

fn influence(source: &LightSource, input: &PreparedInputs) -> f32 {
    f32::from(source.level).powi(2) / source.position.distance_squared(input.camera).max(0.25)
}

pub(super) fn select(
    candidates: &mut Vec<LightSource>,
    sources: &LocalLightSources,
    input: &PreparedInputs,
) {
    select_biased(candidates, sources, input, &[], true);
}

pub(super) fn select_retaining_shadows(
    candidates: &mut Vec<LightSource>,
    sources: &LocalLightSources,
    input: &PreparedInputs,
    owners: &[Option<LightSource>],
) {
    select_biased(candidates, sources, input, owners, true);
}

pub(super) fn select_shadow_pool(
    candidates: &mut Vec<LightSource>,
    sources: &LocalLightSources,
    input: &PreparedInputs,
    owners: &[Option<LightSource>],
) {
    select_biased(candidates, sources, input, owners, false);
}

fn select_biased(
    candidates: &mut Vec<LightSource>,
    sources: &LocalLightSources,
    input: &PreparedInputs,
    owners: &[Option<LightSource>],
    visible_only: bool,
) {
    let previous: [Option<LightSource>; MAX_LOCAL_LIGHTS] =
        std::array::from_fn(|index| candidates.get(index).copied());
    let mut scores = [0.0; MAX_LOCAL_LIGHTS];
    candidates.clear();
    let Some(dimension) = input.dimension else {
        return;
    };
    for &source in sources.chunks.values().flatten() {
        if source.dimension != dimension || source.level == 0 || !source.position.is_finite() {
            continue;
        }
        let distance = source.position.distance_squared(input.camera);
        if distance > (LIGHT_RADIUS + 32.0).powi(2)
            || (visible_only && tile_bounds(source.position, input).is_none())
        {
            continue;
        }
        if let Some(index) = candidates.iter().position(|old| same_source(old, &source)) {
            if candidates[index].level >= source.level {
                continue;
            }
            scores.copy_within(index + 1..candidates.len(), index);
            candidates.remove(index);
        }
        let retention = if owners.iter().flatten().any(|old| same_source(old, &source)) {
            SHADOW_REPLACEMENT_RATIO
        } else if previous
            .iter()
            .flatten()
            .any(|old| same_source(old, &source))
        {
            LIGHT_REPLACEMENT_RATIO
        } else {
            1.0
        };
        let score = influence(&source, input) * retention;
        let mut rank = 0;
        let mut end = candidates.len();
        while rank < end {
            let middle = (rank + end) / 2;
            let compare = score
                .total_cmp(&scores[middle])
                .then_with(|| source_order(&candidates[middle], &source));
            if compare.is_lt() {
                rank = middle + 1;
            } else {
                end = middle;
            }
        }
        if rank < MAX_LOCAL_LIGHTS {
            if candidates.len() == MAX_LOCAL_LIGHTS {
                candidates.pop();
            }
            scores.copy_within(rank..candidates.len(), rank + 1);
            scores[rank] = score;
            candidates.insert(rank, source);
        }
    }
}

// Keep owner slots until a challenger provides a meaningful influence gain.
pub(super) fn shadow_owners(
    candidates: &mut [LightSource],
    input: &PreparedInputs,
    owners: &mut [Option<LightSource>; MAX_SHADOWED_LIGHTS],
    limit: usize,
) {
    let limit = limit.min(MAX_SHADOWED_LIGHTS);
    owners[limit..].fill(None);
    for owner in owners.iter_mut().take(limit) {
        *owner = (*owner).and_then(|old| {
            candidates
                .iter()
                .find(|source| same_source(source, &old))
                .copied()
        });
    }
    for slot in 0..limit {
        if owners[slot].is_none() {
            owners[slot] = best_unowned(candidates, input, owners);
        }
    }
    for _ in 0..limit {
        let Some(challenger) = best_unowned(candidates, input, owners) else {
            break;
        };
        let weakest = (0..limit)
            .filter(|&slot| owners[slot].is_some())
            .min_by(|&left, &right| {
                influence(owners[left].as_ref().unwrap(), input)
                    .total_cmp(&influence(owners[right].as_ref().unwrap(), input))
                    .then_with(|| right.cmp(&left))
            });
        let Some(slot) = weakest else { break };
        let incumbent = owners[slot].as_ref().unwrap();
        if influence(&challenger, input) <= influence(incumbent, input) * SHADOW_REPLACEMENT_RATIO {
            break;
        }
        owners[slot] = Some(challenger);
    }
    // The dense shadow prefix matches the receiver-history lanes and atlas slot order.
    let mut count = 0;
    for slot in 0..limit {
        if owners[slot].is_some() {
            owners.swap(count, slot);
            count += 1;
        }
    }
    for (slot, owner) in owners.iter().flatten().enumerate() {
        let index = candidates
            .iter()
            .position(|source| same_source(source, owner))
            .unwrap();
        candidates.swap(slot, index);
    }
    candidates[count..].sort_unstable_by(source_order);
}

// Offscreen owners keep their atlas lane and enter tiles only where their light sphere projects.
pub(super) fn admit_shadow_owners(
    candidates: &mut Vec<LightSource>,
    owners: &[Option<LightSource>; MAX_SHADOWED_LIGHTS],
) {
    for (slot, &owner) in owners.iter().flatten().enumerate() {
        if let Some(index) = candidates
            .iter()
            .position(|source| same_source(source, &owner))
        {
            candidates.remove(index);
        } else if candidates.len() == MAX_LOCAL_LIGHTS {
            candidates.pop();
        }
        candidates.insert(slot, owner);
    }
    let count = owners.iter().flatten().count();
    candidates[count..].sort_unstable_by(source_order);
}

fn best_unowned(
    candidates: &[LightSource],
    input: &PreparedInputs,
    owners: &[Option<LightSource>],
) -> Option<LightSource> {
    candidates
        .iter()
        .filter(|source| !owners.iter().flatten().any(|old| same_source(old, source)))
        .min_by(|left, right| {
            influence(right, input)
                .total_cmp(&influence(left, input))
                .then_with(|| source_order(left, right))
        })
        .copied()
}

fn dimensions(input: &PreparedInputs) -> [u32; 2] {
    [
        input.size[0].div_ceil(TILE_SIDE).max(1),
        input.size[1].div_ceil(TILE_SIDE).max(1),
    ]
}

pub(super) fn tile_bounds(position: Vec3, input: &PreparedInputs) -> Option<([u32; 2], [u32; 2])> {
    let dims = dimensions(input);
    let projected = input.clip * position.extend(1.0);
    if !projected.is_finite() {
        return None;
    }
    let row_x = Vec3::new(
        input.clip.x_axis.x,
        input.clip.y_axis.x,
        input.clip.z_axis.x,
    )
    .length();
    let row_y = Vec3::new(
        input.clip.x_axis.y,
        input.clip.y_axis.y,
        input.clip.z_axis.y,
    )
    .length();
    let row_w = Vec3::new(
        input.clip.x_axis.w,
        input.clip.y_axis.w,
        input.clip.z_axis.w,
    )
    .length();
    let depth_radius = LIGHT_RADIUS * row_w;
    if projected.w <= -depth_radius {
        return None;
    }
    if position.distance_squared(input.camera) < LIGHT_RADIUS * LIGHT_RADIUS
        || projected.w <= depth_radius
    {
        return Some(([0, 0], [dims[0] - 1, dims[1] - 1]));
    }
    let ndc = projected.truncate().truncate() / projected.w;
    let uv = ndc * Vec2::new(0.5, -0.5) + Vec2::splat(0.5);
    // Include the perspective denominator's variation for off-axis spheres.
    let spread = (Vec2::new(row_x, row_y) + ndc.abs() * row_w)
        * (LIGHT_RADIUS * 0.5 / (projected.w - depth_radius).max(0.0001));
    let tile_scale = Vec2::new(input.size[0] as f32, input.size[1] as f32) / TILE_SIDE as f32;
    let low = ((uv - spread) * tile_scale).floor().max(Vec2::ZERO);
    let high = ((uv + spread) * tile_scale)
        .floor()
        .min(Vec2::new((dims[0] - 1) as f32, (dims[1] - 1) as f32));
    if low.x > high.x || low.y > high.y {
        return None;
    }
    Some(([low.x as u32, low.y as u32], [high.x as u32, high.y as u32]))
}

pub(super) fn tiles(
    candidates: &[LightSource],
    shadowed_count: usize,
    input: &PreparedInputs,
    words: &mut Vec<u32>,
    scores: &mut Vec<f32>,
    rays: &mut Vec<Vec3>,
) {
    let dims = dimensions(input);
    let tile_count = (dims[0] * dims[1]) as usize;
    words.clear();
    words.resize(4 + tile_count * (TILE_LIGHTS + 1), 0);
    words[..4].copy_from_slice(&[dims[0], dims[1], TILE_SIDE, TILE_LIGHTS as u32]);
    scores.clear();
    scores.resize(tile_count * TILE_LIGHTS, -1.0);
    rays.clear();
    let inverse = input.clip.inverse();
    for y in 0..dims[1] {
        for x in 0..dims[0] {
            let pixel = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * TILE_SIDE as f32;
            let uv = pixel / Vec2::new(input.size[0].max(1) as f32, input.size[1].max(1) as f32);
            let projected = inverse
                * (uv * Vec2::new(2.0, -2.0) + Vec2::new(-1.0, 1.0))
                    .extend(0.5)
                    .extend(1.0);
            let point = projected.truncate() / projected.w;
            rays.push((point - input.camera).normalize_or_zero());
        }
    }
    for (index, source) in candidates.iter().enumerate() {
        let Some((low, high)) = tile_bounds(source.position, input) else {
            continue;
        };
        for y in low[1]..=high[1] {
            for x in low[0]..=high[0] {
                let tile = (y * dims[0] + x) as usize;
                let relative = source.position - input.camera;
                let closest = rays[tile] * relative.dot(rays[tile]).max(0.0);
                let distance_squared = relative.distance_squared(closest);
                let window =
                    (1.0 - (distance_squared / (LIGHT_RADIUS * LIGHT_RADIUS)).powi(2)).max(0.0);
                let importance =
                    f32::from(source.level).powi(2) * window * window / distance_squared.max(0.25);
                // Camera tile ranking cannot evict a retained receiver-history owner.
                let importance = if index < shadowed_count {
                    f32::INFINITY
                } else {
                    importance
                };
                let offset = 4 + tile * (TILE_LIGHTS + 1);
                let count = words[offset] as usize;
                let rank = (0..count)
                    .find(|&slot| importance > scores[tile * TILE_LIGHTS + slot])
                    .unwrap_or(count);
                if rank >= TILE_LIGHTS {
                    continue;
                }
                let next_count = (count + 1).min(TILE_LIGHTS);
                for slot in (rank + 1..next_count).rev() {
                    words[offset + 1 + slot] = words[offset + slot];
                    scores[tile * TILE_LIGHTS + slot] = scores[tile * TILE_LIGHTS + slot - 1];
                }
                words[offset + 1 + rank] = index as u32;
                scores[tile * TILE_LIGHTS + rank] = importance;
                words[offset] = next_count as u32;
            }
        }
    }
}
