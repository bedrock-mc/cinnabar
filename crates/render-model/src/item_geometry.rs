//! Item-space geometry shared by every item renderer: sprites extruded one texel deep and unit
//! block cubes, with UVs mapped into a caller-chosen region of its texture layer.
//!
//! Each slab face shows the sprite unmirrored to a viewer on that face's side, so an item reads
//! correctly from either side. Vertices sit on rig bone 0; `back_uv` is what a viewer behind the
//! triangle sees.

use crate::ActorRigVertex;

/// Extrudes an RGBA8 sprite into a slab centred on the origin: the longer side spans one unit,
/// the slab is one texel thick, and each edge quad samples its own texel. `uv_rect` is the
/// sprite's `[u0, v0, u1, v1]` region of its texture layer. `None` for a malformed pixel buffer;
/// a fully transparent sprite still gets its (discarded) faces.
#[must_use]
pub fn extruded_sprite_vertices(
    width: usize,
    height: usize,
    rgba8: &[u8],
    uv_rect: [f32; 4],
) -> Option<Vec<ActorRigVertex>> {
    if width == 0 || height == 0 || rgba8.len() != width.checked_mul(height)?.checked_mul(4)? {
        return None;
    }
    let opaque = |column: isize, row: isize| {
        column >= 0
            && row >= 0
            && (column as usize) < width
            && (row as usize) < height
            && rgba8[(row as usize * width + column as usize) * 4 + 3] != 0
    };
    let texel = 1.0 / width.max(height) as f32;
    let half_depth = texel * 0.5;
    let x_at = |column: usize| (column as f32 - width as f32 * 0.5) * texel;
    let y_at = |row: usize| (height as f32 * 0.5 - row as f32) * texel;
    let to_region = |[u, v]: [f32; 2]| {
        [
            uv_rect[0] + (uv_rect[2] - uv_rect[0]) * u,
            uv_rect[1] + (uv_rect[3] - uv_rect[1]) * v,
        ]
    };
    let mut vertices = Vec::new();
    let (x0, x1) = (x_at(0), x_at(width));
    let (y0, y1) = (y_at(height), y_at(0));
    let upright = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]].map(to_region);
    let mirrored = [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]].map(to_region);
    for (z, normal, uvs, back_uvs) in [
        (half_depth, [0.0, 0.0, 1.0], upright, mirrored),
        (-half_depth, [0.0, 0.0, -1.0], mirrored, upright),
    ] {
        let corners = [[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]];
        push_quad(&mut vertices, corners, uvs, back_uvs, normal);
    }
    for row in 0..height {
        for column in 0..width {
            if !opaque(column as isize, row as isize) {
                continue;
            }
            let (column_i, row_i) = (column as isize, row as isize);
            let uv = to_region([
                (column as f32 + 0.5) / width as f32,
                (row as f32 + 0.5) / height as f32,
            ]);
            let (left, right) = (x_at(column), x_at(column + 1));
            let (top, bottom) = (y_at(row), y_at(row + 1));
            let edges = [
                (
                    opaque(column_i - 1, row_i),
                    [-1.0, 0.0, 0.0],
                    [left, left],
                    [bottom, top],
                ),
                (
                    opaque(column_i + 1, row_i),
                    [1.0, 0.0, 0.0],
                    [right, right],
                    [bottom, top],
                ),
                (
                    opaque(column_i, row_i - 1),
                    [0.0, 1.0, 0.0],
                    [left, right],
                    [top, top],
                ),
                (
                    opaque(column_i, row_i + 1),
                    [0.0, -1.0, 0.0],
                    [left, right],
                    [bottom, bottom],
                ),
            ];
            for (neighbour_opaque, normal, xs, ys) in edges {
                if neighbour_opaque {
                    continue;
                }
                let corners = [
                    [xs[0], ys[0], half_depth],
                    [xs[0], ys[0], -half_depth],
                    [xs[1], ys[1], -half_depth],
                    [xs[1], ys[1], half_depth],
                ];
                push_quad(&mut vertices, corners, [uv; 4], [uv; 4], normal);
            }
        }
    }
    Some(vertices)
}

/// The slab in vanilla's held-item tessellation frame: column `c` at `X = -c`, row `r` at
/// `Y = height - r` (texels of the longer side spanning one unit) and one texel deep towards
/// `-Z`. Both faces sample the same texel at a point, so the back reads mirrored, as vanilla's.
#[must_use]
pub fn held_sprite_vertices(
    width: usize,
    height: usize,
    rgba8: &[u8],
    uv_rect: [f32; 4],
) -> Option<Vec<ActorRigVertex>> {
    let texel = 1.0 / width.max(height) as f32;
    let offset = [
        width as f32 * texel * 0.5,
        height as f32 * texel * 0.5,
        -texel * 0.5,
    ];
    let mut vertices = extruded_sprite_vertices(width, height, rgba8, uv_rect)?;
    for vertex in &mut vertices {
        let [x, y, z] = vertex.position;
        vertex.position = [-(x + offset[0]), y + offset[1], z + offset[2]];
        vertex.normal[0] = -vertex.normal[0];
        // The +Z face's front UVs and the -Z face's back UVs carry the unmirrored layout.
        if vertex.normal[2] < 0.0 {
            vertex.uv = vertex.back_uv;
        } else {
            vertex.back_uv = vertex.uv;
        }
    }
    // Mirroring X reverses every triangle; restore counter-clockwise winding about the normal.
    for triangle in vertices.as_chunks_mut::<3>().0 {
        triangle.swap(1, 2);
    }
    Some(vertices)
}

/// Corners (top-left, top-right, bottom-right, bottom-left as seen from outside) and outward
/// normal per cube face, in `West, East, Down, Up, North, South` order.
const CUBE_FACES: [([[f32; 3]; 4], [f32; 3]); 6] = [
    (
        [
            [-0.5, 0.5, -0.5],
            [-0.5, 0.5, 0.5],
            [-0.5, -0.5, 0.5],
            [-0.5, -0.5, -0.5],
        ],
        [-1.0, 0.0, 0.0],
    ),
    (
        [
            [0.5, 0.5, 0.5],
            [0.5, 0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
        ],
        [1.0, 0.0, 0.0],
    ),
    (
        [
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, -0.5, -0.5],
            [-0.5, -0.5, -0.5],
        ],
        [0.0, -1.0, 0.0],
    ),
    (
        [
            [-0.5, 0.5, -0.5],
            [0.5, 0.5, -0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ],
        [0.0, 1.0, 0.0],
    ),
    (
        [
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
        ],
        [0.0, 0.0, -1.0],
    ),
    (
        [
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, -0.5, 0.5],
            [-0.5, -0.5, 0.5],
        ],
        [0.0, 0.0, 1.0],
    ),
];

/// A unit cube centred on the origin, six vertices per face in `CUBE_FACES` order. Face `f`
/// shows its upright tile from `face_rects[f]` (`[u0, v0, u1, v1]`) to a viewer outside it.
#[must_use]
pub fn textured_cube_vertices(face_rects: [[f32; 4]; 6]) -> Vec<ActorRigVertex> {
    let mut vertices = Vec::with_capacity(36);
    for ((corners, normal), rect) in CUBE_FACES.into_iter().zip(face_rects) {
        let uvs = [
            [rect[0], rect[1]],
            [rect[2], rect[1]],
            [rect[2], rect[3]],
            [rect[0], rect[3]],
        ];
        push_quad(&mut vertices, corners, uvs, uvs, normal);
    }
    vertices
}

fn push_quad(
    vertices: &mut Vec<ActorRigVertex>,
    corners: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    back_uvs: [[f32; 2]; 4],
    normal: [f32; 3],
) {
    let edge_a = std::array::from_fn::<f32, 3, _>(|axis| corners[1][axis] - corners[0][axis]);
    let edge_b = std::array::from_fn::<f32, 3, _>(|axis| corners[2][axis] - corners[0][axis]);
    let cross = [
        edge_a[1] * edge_b[2] - edge_a[2] * edge_b[1],
        edge_a[2] * edge_b[0] - edge_a[0] * edge_b[2],
        edge_a[0] * edge_b[1] - edge_a[1] * edge_b[0],
    ];
    let facing = cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2];
    let order: [usize; 6] = if facing >= 0.0 {
        [0, 1, 2, 0, 2, 3]
    } else {
        [0, 2, 1, 0, 3, 2]
    };
    for index in order {
        vertices.push(ActorRigVertex {
            position: corners[index],
            normal,
            uv: uvs[index],
            back_uv: back_uvs[index],
            bone_index: 0,
            surface: crate::ActorRigSurface::SINGLE_FACE,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{extruded_sprite_vertices, held_sprite_vertices};

    const FULL: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    fn opaque(width: usize, height: usize) -> Vec<u8> {
        vec![255; width * height * 4]
    }

    #[test]
    fn opaque_slab_has_two_faces_and_only_outer_edges() {
        let single = extruded_sprite_vertices(1, 1, &opaque(1, 1), FULL).unwrap();
        assert_eq!(single.len(), 12 + 4 * 6);
        let pair = extruded_sprite_vertices(2, 1, &opaque(2, 1), FULL).unwrap();
        assert_eq!(pair.len(), 12 + 6 * 6);
    }

    #[test]
    fn transparent_sprites_keep_only_their_faces_and_malformed_ones_have_no_mesh() {
        assert_eq!(
            extruded_sprite_vertices(2, 2, &[0; 16], FULL)
                .unwrap()
                .len(),
            12
        );
        assert!(extruded_sprite_vertices(2, 2, &[255; 15], FULL).is_none());
        assert!(extruded_sprite_vertices(0, 2, &[], FULL).is_none());
    }

    #[test]
    fn slab_is_one_texel_thick_and_unit_wide() {
        let vertices = extruded_sprite_vertices(16, 16, &opaque(16, 16), FULL).unwrap();
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for vertex in &vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex.position[axis]);
                max[axis] = max[axis].max(vertex.position[axis]);
            }
            assert!(vertex.uv.iter().all(|value| (0.0..=1.0).contains(value)));
        }
        assert!((max[0] - min[0] - 1.0).abs() < 1e-6);
        assert!((max[2] - min[2] - 1.0 / 16.0).abs() < 1e-6);
    }

    #[test]
    fn uvs_map_into_the_requested_atlas_region() {
        let region = [0.25, 0.5, 0.5, 0.75];
        for vertex in extruded_sprite_vertices(4, 4, &opaque(4, 4), region).unwrap() {
            assert!((0.25..=0.5).contains(&vertex.uv[0]));
            assert!((0.5..=0.75).contains(&vertex.uv[1]));
        }
    }

    #[test]
    fn faces_show_the_sprite_unmirrored_from_their_own_side() {
        let vertices = extruded_sprite_vertices(4, 4, &opaque(4, 4), FULL).unwrap();
        for triangle in vertices[..12].as_chunks::<3>().0 {
            for vertex in triangle {
                // The front-facing UV of one slab is the other slab's back-facing UV, mirrored in u.
                let other = vertices[..12]
                    .iter()
                    .find(|other| {
                        other.position[..2] == vertex.position[..2]
                            && other.position[2] != vertex.position[2]
                    })
                    .unwrap();
                assert!((vertex.uv[0] - (1.0 - other.uv[0])).abs() < 1e-6);
                assert_eq!(vertex.uv[1], other.uv[1]);
                assert_eq!(vertex.back_uv, other.uv);
            }
        }
    }

    #[test]
    fn cube_has_six_outward_upright_faces_inside_their_tiles() {
        let rects = std::array::from_fn(|face| {
            let x = face as f32 * 0.1;
            [x, 0.0, x + 0.1, 0.5]
        });
        let vertices = super::textured_cube_vertices(rects);
        assert_eq!(vertices.len(), 36);
        for (index, triangle) in vertices.as_chunks::<3>().0.iter().enumerate() {
            let rect = rects[index / 2];
            let normal = triangle[0].normal;
            assert_eq!(normal.iter().map(|value| value.abs()).sum::<f32>(), 1.0);
            let a = triangle[0].position;
            let (b, c) = (triangle[1].position, triangle[2].position);
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            assert!(cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2] > 0.0);
            let plane = a[0] * normal[0] + a[1] * normal[1] + a[2] * normal[2];
            assert!((plane - 0.5).abs() < 1e-6);
            for vertex in triangle {
                assert!((rect[0]..=rect[2]).contains(&vertex.uv[0]));
                assert!((rect[1]..=rect[3]).contains(&vertex.uv[1]));
            }
        }
        // The side faces put the tile's top edge on the higher vertices.
        let west = &vertices[..6];
        assert!(
            west.iter()
                .all(|vertex| (vertex.position[1] > 0.0) == (vertex.uv[1] == 0.0))
        );
    }

    #[test]
    fn every_triangle_winds_counter_clockwise_about_its_normal() {
        let mut sprite = opaque(3, 3);
        sprite[(4 * 4) + 3] = 0;
        for triangle in extruded_sprite_vertices(3, 3, &sprite, FULL)
            .unwrap()
            .as_chunks::<3>()
            .0
            .iter()
        {
            let a = triangle[0].position;
            let b = triangle[1].position;
            let c = triangle[2].position;
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let normal = triangle[0].normal;
            assert!(cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2] > 0.0);
        }
    }

    // Vanilla's held tessellation: column 0 at X = 0 running to -X, the top row at Y = 1, one
    // texel deep towards -Z, and the same texel at a point from either face.
    #[test]
    fn held_layout_matches_vanilla_and_shares_texels_across_faces() {
        let vertices = held_sprite_vertices(16, 16, &opaque(16, 16), FULL).unwrap();
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for vertex in &vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex.position[axis]);
                max[axis] = max[axis].max(vertex.position[axis]);
            }
            assert_eq!(vertex.uv, vertex.back_uv);
        }
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6);
        assert!(close(min, [-1.0, 0.0, -1.0 / 16.0]) && close(max, [0.0, 1.0, 0.0]));
        for face in vertices.iter().filter(|vertex| vertex.normal[2] != 0.0) {
            let [x, y, _] = face.position;
            assert!((face.uv[0] + x).abs() < 1e-6 && (face.uv[1] + y - 1.0).abs() < 1e-6);
        }
        for triangle in vertices.as_chunks::<3>().0 {
            let [a, b, c] = [0, 1, 2].map(|corner| triangle[corner].position);
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let normal = triangle[0].normal;
            assert!(cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2] > 0.0);
        }
    }
}
