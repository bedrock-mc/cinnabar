//! Ray intersections with the same neighbour-selected model templates terrain draws.

use std::cell::OnceCell;

use assets::{NetworkIdMode, RuntimeAssets, VisualKind, model_template_parts};
use world::MeshNeighbourhood;

use crate::{
    BlockClassifier, Face,
    chunk::models::{PaletteResolutionContext, select_model_templates},
    contributors::{PaletteFacts, is_model_entry},
};

/// Finds the nearest model surface along a unit ray, with the ray origin relative to the
/// queried block. Uses terrain's layered contributors, connected templates and rotations.
/// Cube and liquid cells are handled by the caller's cell traversal.
#[must_use]
pub fn model_ray_distance(
    classifier: BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    local: [usize; 3],
    origin: [f32; 3],
    direction: [f32; 3],
) -> Option<f32> {
    let center = neighbourhood.sub_chunk([0; 3])?;
    let facts = PaletteFacts::new(classifier, visuals, network_id_mode, center);
    let neighbours: [OnceCell<PaletteFacts<'_>>; Face::ALL.len()] =
        std::array::from_fn(|_| OnceCell::new());
    let context = PaletteResolutionContext {
        classifier,
        visuals,
        network_id_mode,
        neighbourhood,
    };
    let world = std::array::from_fn(|axis| {
        neighbourhood.block_origin()[axis].wrapping_add(local[axis] as i32)
    });
    let mut nearest: Option<f32> = None;
    for entry in facts
        .model_entries_at(local[0], local[1], local[2])
        .into_iter()
        .flatten()
        .filter(|entry| is_model_entry(*entry))
    {
        let (templates, count) = select_model_templates(context, &facts, &neighbours, local, entry);
        for template in templates.into_iter().take(count as usize) {
            for (part_index, part) in model_template_parts(visuals.model_templates(), template)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let start = part.quad_start as usize;
                let quads = visuals
                    .model_quads()
                    .get(start..start + part.quad_count as usize)?;
                let offset = visuals
                    .model_random_offset(template + part_index as u32)
                    .map(|offset| offset.offset(world));
                let transform = if entry.kind == VisualKind::Cross {
                    0
                } else {
                    assets::bamboo::transform_for_template(part.flags, entry.variant, world)
                };
                for (index, quad) in quads.iter().enumerate() {
                    let bamboo = part.flags & assets::MODEL_TEMPLATE_FLAG_BAMBOO != 0;
                    let bamboo_offset = if bamboo {
                        let mut value = assets::bamboo::quad_offset(transform, index as u32);
                        if offset.is_some() {
                            let base = block_transform::bamboo::offset_from_transform(transform);
                            for axis in 0..3 {
                                value[axis] -= base[axis];
                            }
                        }
                        value
                    } else {
                        [0.0; 3]
                    };
                    let points = quad.positions.map(|point| {
                        let [x, y, z] = std::array::from_fn(|axis| {
                            f32::from(point[axis]) / f32::from(assets::MODEL_POSITION_SCALE)
                                + bamboo_offset[axis]
                        });
                        let rotated = match if bamboo { 0 } else { transform & 3 } {
                            1 => [1.0 - z, y, x],
                            2 => [1.0 - x, y, 1.0 - z],
                            3 => [z, y, 1.0 - x],
                            _ => [x, y, z],
                        };
                        std::array::from_fn(|axis| rotated[axis] + offset.unwrap_or([0.0; 3])[axis])
                    });
                    for [a, b, c] in [[0, 1, 2], [0, 2, 3]] {
                        if let Some(distance) =
                            triangle_distance(origin, direction, [points[a], points[b], points[c]])
                            && nearest.is_none_or(|old| distance < old)
                        {
                            nearest = Some(distance);
                        }
                    }
                }
            }
        }
    }
    nearest
}

/// Intersects either side of a triangle; degenerate and parallel surfaces produce no hit.
fn triangle_distance(
    origin: [f32; 3],
    direction: [f32; 3],
    [a, b, c]: [[f32; 3]; 3],
) -> Option<f32> {
    let subtract = |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|axis| a[axis] - b[axis]);
    let dot = |a: [f32; 3], b: [f32; 3]| a.into_iter().zip(b).map(|(a, b)| a * b).sum::<f32>();
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let edge = subtract(b, a);
    let other = subtract(c, a);
    let h = cross(direction, other);
    let determinant = dot(edge, h);
    if determinant == 0.0 {
        return None;
    }
    let s = subtract(origin, a);
    let u = dot(s, h) / determinant;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, edge);
    let v = dot(direction, q) / determinant;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = dot(other, q) / determinant;
    (distance >= 0.0 && distance.is_finite()).then_some(distance)
}
