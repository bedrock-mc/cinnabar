//! The standard player biped: six skin-mapped cuboids and their second layer.
use bytemuck::{Pod, Zeroable};

pub const STANDARD_BIPED_VERTEX_COUNT: usize = 6 * 6 * 6;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ActorVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub part: u32,
}

#[derive(Clone, Copy)]
struct Cuboid {
    min: [f32; 3],
    max: [f32; 3],
    uv_origin: [f32; 2],
    dimensions: [f32; 3],
}

#[must_use]
pub fn standard_biped_vertices() -> Vec<ActorVertex> {
    const P: f32 = 1.0 / 16.0;
    let cuboids = [
        Cuboid {
            min: [-4.0 * P, 24.0 * P, -4.0 * P],
            max: [4.0 * P, 32.0 * P, 4.0 * P],
            uv_origin: [0.0, 0.0],
            dimensions: [8.0, 8.0, 8.0],
        },
        Cuboid {
            min: [-4.0 * P, 12.0 * P, -2.0 * P],
            max: [4.0 * P, 24.0 * P, 2.0 * P],
            uv_origin: [16.0, 16.0],
            dimensions: [8.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-8.0 * P, 12.0 * P, -2.0 * P],
            max: [-4.0 * P, 24.0 * P, 2.0 * P],
            uv_origin: [40.0, 16.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [4.0 * P, 12.0 * P, -2.0 * P],
            max: [8.0 * P, 24.0 * P, 2.0 * P],
            uv_origin: [32.0, 48.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-4.0 * P, 0.0, -2.0 * P],
            max: [0.0, 12.0 * P, 2.0 * P],
            uv_origin: [0.0, 16.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [0.0, 0.0, -2.0 * P],
            max: [4.0 * P, 12.0 * P, 2.0 * P],
            uv_origin: [16.0, 48.0],
            dimensions: [4.0, 12.0, 4.0],
        },
    ];
    let mut vertices = Vec::with_capacity(STANDARD_BIPED_VERTEX_COUNT);
    for (part, cuboid) in cuboids.into_iter().enumerate() {
        append_cuboid(&mut vertices, cuboid, part as u32);
    }
    vertices
}

/// Returns the optional second skin layer (hat, jacket, sleeves, and pants)
/// around the shared base biped. Bedrock and LCE both keep this layer in the
/// same player-model path as the base cuboids; exposing it here lets the HUD
/// preview and first-person carrier use the authoritative skin appearance
/// without duplicating the UV contract in the UI crate.
#[must_use]
pub fn standard_biped_overlay_vertices() -> Vec<ActorVertex> {
    const P: f32 = 1.0 / 16.0;
    const OUTER: f32 = 0.5 * P;
    let cuboids = [
        Cuboid {
            min: [-4.0 * P - OUTER, 24.0 * P - OUTER, -4.0 * P - OUTER],
            max: [4.0 * P + OUTER, 32.0 * P + OUTER, 4.0 * P + OUTER],
            uv_origin: [32.0, 0.0],
            dimensions: [8.0, 8.0, 8.0],
        },
        Cuboid {
            min: [-4.0 * P - OUTER, 12.0 * P - OUTER, -2.0 * P - OUTER],
            max: [4.0 * P + OUTER, 24.0 * P + OUTER, 2.0 * P + OUTER],
            uv_origin: [16.0, 32.0],
            dimensions: [8.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-8.0 * P - OUTER, 12.0 * P - OUTER, -2.0 * P - OUTER],
            max: [-4.0 * P + OUTER, 24.0 * P + OUTER, 2.0 * P + OUTER],
            uv_origin: [40.0, 32.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [4.0 * P - OUTER, 12.0 * P - OUTER, -2.0 * P - OUTER],
            max: [8.0 * P + OUTER, 24.0 * P + OUTER, 2.0 * P + OUTER],
            uv_origin: [48.0, 48.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-4.0 * P - OUTER, -OUTER, -2.0 * P - OUTER],
            max: [OUTER, 12.0 * P + OUTER, 2.0 * P + OUTER],
            uv_origin: [0.0, 32.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-OUTER, -OUTER, -2.0 * P - OUTER],
            max: [4.0 * P + OUTER, 12.0 * P + OUTER, 2.0 * P + OUTER],
            uv_origin: [0.0, 48.0],
            dimensions: [4.0, 12.0, 4.0],
        },
    ];
    let mut vertices = Vec::with_capacity(STANDARD_BIPED_VERTEX_COUNT);
    for (part, cuboid) in cuboids.into_iter().enumerate() {
        append_cuboid(&mut vertices, cuboid, part as u32);
    }
    vertices
}

fn append_cuboid(vertices: &mut Vec<ActorVertex>, cuboid: Cuboid, part: u32) {
    let [x0, y0, z0] = cuboid.min;
    let [x1, y1, z1] = cuboid.max;
    let [u, v] = cuboid.uv_origin;
    let [dx, dy, dz] = cuboid.dimensions;
    let faces = [
        (
            [[x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]],
            [u, v + dz, dz, dy],
        ),
        (
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            [u + dz, v + dz, dx, dy],
        ),
        (
            [[x0, y0, z1], [x0, y0, z0], [x0, y1, z0], [x0, y1, z1]],
            [u + dz + dx, v + dz, dz, dy],
        ),
        (
            [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
            [u + dz + dx + dz, v + dz, dx, dy],
        ),
        (
            [[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]],
            [u + dz, v, dx, dz],
        ),
        (
            [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
            [u + dz + dx, v, dx, dz],
        ),
    ];
    for (positions, [face_u, face_v, face_width, face_height]) in faces {
        let u0 = face_u / 64.0;
        let v0 = face_v / 64.0;
        let u1 = (face_u + face_width) / 64.0;
        let v1 = (face_v + face_height) / 64.0;
        let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        for index in [0, 1, 2, 0, 2, 3] {
            vertices.push(ActorVertex {
                position: positions[index],
                uv: uvs[index],
                part,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_biped_is_six_cuboids_with_a_complete_base_layer_uv_mesh() {
        let vertices = standard_biped_vertices();
        assert_eq!(vertices.len(), STANDARD_BIPED_VERTEX_COUNT);
        assert!(vertices.iter().all(|vertex| {
            vertex.position.iter().all(|value| value.is_finite())
                && vertex.uv.iter().all(|value| (0.0..=1.0).contains(value))
        }));
        let min_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::INFINITY, f32::min);
        let max_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!([min_y, max_y], [0.0, 2.0]);
    }
}
