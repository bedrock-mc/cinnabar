//! Read-only entity inspection, including decorative actors that cannot be attacked.

use bevy::prelude::Vec3;
use chunk_pipeline::WorldStream;

use super::{Lines, spatial::TARGET_RANGE_BLOCKS};

type Bounds = ([f32; 3], [f32; 3]);
const MAX_LAYER_GEOMETRIES: usize = 3;

pub(super) fn append_target_entity(
    lines: &mut Lines<'_>,
    stream: &WorldStream,
    eye: Vec3,
    direction: Vec3,
    block_distance: Option<f64>,
) {
    #[cfg(feature = "tracy")]
    let _zone = bevy::log::info_span!("ui.f3.entities").entered();
    let authority = stream.authority();
    let candidates = authority.remote_actors().filter_map(|actor| {
        let bounds = actor.bounding_box().or_else(|| {
            let rig = authority.actor_rig(actor.runtime_id)?;
            Some(rig.culling_bounds().at(actor.position, rig.scale))
        })?;
        Some((actor.runtime_id, bounds))
    });
    let target = nearest_entity(candidates, eye, direction, block_distance);
    lines.right.push("");
    let Some((runtime_id, distance)) = target else {
        lines.right.push(format_args!(
            "Targeted Entity: none within {TARGET_RANGE_BLOCKS:.0} blocks"
        ));
        return;
    };
    let Some(actor) = authority.actor(runtime_id) else {
        return;
    };
    lines.right.push("Targeted Entity:");
    let identifier = match &actor.kind {
        protocol::ActorKind::Player { .. } => "minecraft:player",
        protocol::ActorKind::Entity { identifier } => identifier.as_ref(),
    };
    lines.right.push(identifier);
    lines.right.push(format_args!(
        "Runtime ID: {runtime_id} | unique ID: {}",
        actor.unique_id
    ));
    lines
        .right
        .push(format_args!("Distance: {distance:.2} blocks"));
    lines.right.push(format_args!(
        "Invisible: {} | render scale: {}",
        actor.is_invisible(),
        actor.render_scale()
    ));
    if let Some((min, max)) = actor.bounding_box() {
        lines.right.push(format_args!(
            "Hitbox: {:.2} wide | {:.2} high",
            max[0] - min[0],
            max[1] - min[1]
        ));
    } else {
        lines.right.push("Hitbox: unavailable for interaction");
    }
    let mut previous = None;
    let mut wrote_flags = false;
    loop {
        let next = actor
            .metadata
            .iter()
            .filter_map(|(key, value)| {
                if previous.is_some_and(|previous| *key <= previous) {
                    return None;
                }
                match value {
                    protocol::ActorMetadataValue::Flags(word)
                    | protocol::ActorMetadataValue::FlagsExtended(word) => Some((*key, *word)),
                    _ => None,
                }
            })
            .min_by_key(|(key, _)| *key);
        let Some((key, word)) = next else {
            break;
        };
        lines.right.push(format_args!("Flags[{key}]: {word:#018x}"));
        previous = Some(key);
        wrote_flags = true;
    }
    if !wrote_flags {
        lines.right.push("Flags: absent");
    }
    if let Some(name) = authority.actor_display_name(actor.unique_id) {
        lines.right.push_with(|line| {
            line.push_str("Name: ");
            for character in name.chars() {
                line.push(if matches!(character, '\n' | '\r') {
                    ' '
                } else {
                    character
                });
            }
            Ok(())
        });
    }
    if let Some(rig) = authority.actor_rig(runtime_id) {
        lines.right.push(format_args!(
            "Rig: {} | fallback: {:?} | layers: {}",
            rig.rig.0,
            rig.fallback,
            rig.render.len()
        ));
        if let Some((catalog, geometry)) = rig.geometry_source()
            && let Some(geometry) = catalog.geometries().get(geometry)
        {
            lines
                .right
                .push(format_args!("Base geometry: {}", geometry.identifier));
            for layer in rig.render.iter().take(MAX_LAYER_GEOMETRIES) {
                if let Some(selected) = layer
                    .geometry
                    .and_then(|index| catalog.geometries().get(index as usize))
                {
                    lines
                        .right
                        .push(format_args!("Draw geometry: {}", selected.identifier));
                }
            }
        }
    } else {
        lines.right.push("Rig: unavailable");
    }
}

pub(super) fn nearest_entity(
    candidates: impl Iterator<Item = (u64, Bounds)>,
    eye: Vec3,
    direction: Vec3,
    block_distance: Option<f64>,
) -> Option<(u64, f64)> {
    let direction = direction.try_normalize()?;
    let origin = eye.to_array().map(f64::from);
    if !eye.is_finite() {
        return None;
    }
    let direction = direction.to_array().map(f64::from);
    let limit = block_distance
        .filter(|distance| distance.is_finite() && *distance >= 0.0)
        .unwrap_or(TARGET_RANGE_BLOCKS)
        .min(TARGET_RANGE_BLOCKS);
    candidates
        .filter_map(|(id, (min, max))| {
            if (0..3).any(|axis| {
                !min[axis].is_finite() || !max[axis].is_finite() || min[axis] > max[axis]
            }) {
                return None;
            }
            let distance = gameplay::melee::ray_box_entry(
                origin,
                direction,
                min.map(f64::from),
                max.map(f64::from),
            )?;
            (distance <= limit).then_some((id, distance))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1).then(left.0.cmp(&right.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes() -> impl Iterator<Item = (u64, Bounds)> {
        [
            (8, ([-0.5, 0.0, -6.0], [0.5, 2.0, -5.0])),
            (3, ([-0.5, 0.0, -3.0], [0.5, 2.0, -2.0])),
        ]
        .into_iter()
    }

    #[test]
    fn entity_inspection_chooses_nearest_and_respects_terrain() {
        let eye = Vec3::Y;
        assert_eq!(
            nearest_entity(boxes(), eye, Vec3::NEG_Z, None),
            Some((3, 2.0))
        );
        assert_eq!(nearest_entity(boxes(), eye, Vec3::NEG_Z, Some(1.5)), None);
        assert_eq!(
            nearest_entity(boxes(), eye, Vec3::NEG_Z * 7.0, Some(4.0)),
            Some((3, 2.0))
        );
        assert_eq!(nearest_entity(boxes(), eye, Vec3::Z, None), None);
    }

    #[test]
    fn entity_inspection_handles_invalid_boxes_and_eye_inside() {
        assert_eq!(nearest_entity(boxes(), Vec3::NAN, Vec3::NEG_Z, None), None);
        assert_eq!(nearest_entity(boxes(), Vec3::Y, Vec3::ZERO, None), None);
        let candidates = [
            (1, ([f32::NAN; 3], [2.0; 3])),
            (2, ([2.0; 3], [-2.0; 3])),
            (4, ([-1.0; 3], [1.0; 3])),
        ];
        assert_eq!(
            nearest_entity(candidates.into_iter(), Vec3::ZERO, Vec3::NEG_Z, None),
            Some((4, 0.0))
        );
    }
}
