//! Native entity.vertex FANCY lighting over the posed UI bone/model normals.

use std::collections::BTreeMap;

use render_model::ActorVertex;

/// Base and outer-layer cuboids of one biped part share their local center.
pub(super) fn part_centers(source: &[ActorVertex]) -> BTreeMap<u32, [f32; 3]> {
    let mut bounds = BTreeMap::<u32, ([f32; 3], [f32; 3])>::new();
    for vertex in source {
        let (min, max) = bounds
            .entry(vertex.part)
            .or_insert((vertex.position, vertex.position));
        for axis in 0..3 {
            min[axis] = min[axis].min(vertex.position[axis]);
            max[axis] = max[axis].max(vertex.position[axis]);
        }
    }
    bounds
        .into_iter()
        .map(|(part, (min, max))| (part, [0, 1, 2].map(|axis| (min[axis] + max[axis]) * 0.5)))
        .collect()
}

pub(super) fn triangle_light(
    source: &[ActorVertex],
    world: [[f32; 3]; 3],
    center: [f32; 3],
    fancy: bool,
) -> Option<f32> {
    if !fancy {
        return Some(1.0);
    }
    let local = [0, 1, 2].map(|corner| source[corner].position);
    let local_normal = cross(sub(local[1], local[0]), sub(local[2], local[0]));
    let face_center =
        [0, 1, 2].map(|axis| (local[0][axis] + local[1][axis] + local[2][axis]) / 3.0);
    // The standard-biped carrier has reversed winding on the two side faces.
    // Recover the outward cuboid normal before applying the posed transforms.
    let direction = if dot(local_normal, sub(face_center, center)) < 0.0 {
        -1.0
    } else {
        1.0
    };
    let normal = cross(sub(world[1], world[0]), sub(world[2], world[0]));
    let length = dot(normal, normal).sqrt();
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    let normal = normal.map(|axis| axis / length * direction);
    Some(fancy_intensity(normal))
}

/// UI shading supplies TILE_LIGHT_COLOR=(1,1,1,1), so its W direction is +1.
/// This is the entity shader's formula, not world ambient or a byte tint.
pub(super) fn fancy_intensity(normal: [f32; 3]) -> f32 {
    render_api::fancy_actor_shade(normal, 0.0)
}

fn sub(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|axis| left[axis] - right[axis])
}

fn cross([x, y, z]: [f32; 3], [a, b, c]: [f32; 3]) -> [f32; 3] {
    [y * c - z * b, z * a - x * c, x * b - y * a]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left.into_iter().zip(right).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_fancy_cardinal_normals_and_oblique_normal_are_not_quantized() {
        for (normal, expected) in [
            ([0.0, 1.0, 0.0], 1.0),
            ([0.0, -1.0, 0.0], 0.45),
            ([1.0, 0.0, 0.0], 0.625),
            ([0.0, 0.0, 1.0], 0.825),
        ] {
            assert!((fancy_intensity(normal) - expected).abs() < 1e-6);
        }
        let normal = [0.3, 0.4, 0.75f32.sqrt()];
        let expected = 0.7 * 0.55 - 0.009 + 0.075 + 0.45;
        assert!((fancy_intensity(normal) - expected).abs() < 1e-6);
        assert!((fancy_intensity(normal) * 255.0).fract().abs() > 0.01);
    }
}
