//! Item-space geometry: centered one-texel sprite slabs, fixed-depth held sprites and unit
//! block cubes, with UVs mapped into a caller-chosen region of the texture layer.
//!
//! Both slab faces sample the same texel at each point, as vanilla's per-texel item
//! tessellation does: the front reads unmirrored, the back mirrored, and transparent texels
//! never reveal different rear art. Vertices sit on rig bone 0; `back_uv` is what a viewer
//! behind the triangle sees.

use crate::ActorRigVertex;

/// Ordinary held-sprite depth after normalizing the longer side to one model unit.
const HELD_SPRITE_DEPTH: f32 = 1.0 / 16.0;

/// Extrudes an RGBA8 sprite into a slab centred on the origin: the longer side spans one unit,
/// the slab is one texel thick, and each edge quad samples its own texel. `uv_rect` is the
/// sprite's `[u0, v0, u1, v1]` region of its texture layer. `None` for malformed pixels or
/// geometry exceeding the shared retained-storage budget;
/// a fully transparent sprite still gets its (discarded) faces.
#[must_use]
pub fn extruded_sprite_vertices(
    width: usize,
    height: usize,
    rgba8: &[u8],
    uv_rect: [f32; 4],
) -> Option<Vec<ActorRigVertex>> {
    sprite_vertices(width, height, rgba8, uv_rect, 1)
}

/// Builds a centered slab whose side faces follow texels at or above `minimum_alpha`.
fn sprite_vertices(
    width: usize,
    height: usize,
    rgba8: &[u8],
    uv_rect: [f32; 4],
    minimum_alpha: u8,
) -> Option<Vec<ActorRigVertex>> {
    if width == 0 || height == 0 || rgba8.len() != width.checked_mul(height)?.checked_mul(4)? {
        return None;
    }
    let max_vertices = crate::MAX_ACTOR_RIG_VERTICES
        .min(assets::texture::policy::MAX_SUBSCRIBER_BYTES / std::mem::size_of::<ActorRigVertex>());
    let opaque = |column: isize, row: isize| {
        column >= 0
            && row >= 0
            && (column as usize) < width
            && (row as usize) < height
            && rgba8[(row as usize * width + column as usize) * 4 + 3] >= minimum_alpha
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
    for (z, normal) in [
        (half_depth, [0.0, 0.0, 1.0]),
        (-half_depth, [0.0, 0.0, -1.0]),
    ] {
        let corners = [[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]];
        push_quad(
            &mut vertices,
            corners,
            upright,
            upright,
            normal,
            [uv_rect[0], uv_rect[1]],
        );
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
                if vertices.len() + 6 > max_vertices {
                    return None;
                }
                let corners = [
                    [xs[0], ys[0], half_depth],
                    [xs[0], ys[0], -half_depth],
                    [xs[1], ys[1], -half_depth],
                    [xs[1], ys[1], half_depth],
                ];
                push_quad(
                    &mut vertices,
                    corners,
                    [uv; 4],
                    [uv; 4],
                    normal,
                    [uv_rect[0], uv_rect[1]],
                );
            }
        }
    }
    Some(vertices)
}

/// The slab in vanilla's held-item tessellation frame: column `c` at `X = -c`, row `r` at
/// `Y = height - r`, divided by the longer side. Depth spans 1/16 model unit towards `-Z`,
/// independent of sprite resolution. Both faces sample the same texel at a point, so the
/// back reads mirrored, as vanilla's. Alpha 0 and 1 do not contribute side faces.
#[must_use]
pub fn held_sprite_vertices(
    width: usize,
    height: usize,
    rgba8: &[u8],
    uv_rect: [f32; 4],
) -> Option<Vec<ActorRigVertex>> {
    let texel = 1.0 / width.max(height) as f32;
    let offset = [width as f32 * texel * 0.5, height as f32 * texel * 0.5];
    let mut vertices = sprite_vertices(width, height, rgba8, uv_rect, 2)?;
    for vertex in &mut vertices {
        let [x, y, z] = vertex.position;
        vertex.position = [
            -(x + offset[0]),
            y + offset[1],
            if z > 0.0 { 0.0 } else { -HELD_SPRITE_DEPTH },
        ];
        vertex.normal[0] = -vertex.normal[0];
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
        push_quad(&mut vertices, corners, uvs, uvs, normal, [rect[0], rect[1]]);
    }
    vertices
}

/// Emits a wound quad with the sprite or face origin shared by all its vertices.
fn push_quad(
    vertices: &mut Vec<ActorRigVertex>,
    corners: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    back_uvs: [[f32; 2]; 4],
    normal: [f32; 3],
    glint_origin: [f32; 2],
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
            glint_origin,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{extruded_sprite_vertices, held_sprite_vertices};

    const FULL: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    #[test]
    fn dense_hd_transparency_cannot_expand_into_unbounded_geometry() {
        let side = 512;
        let pixels = (0..side * side)
            .flat_map(|index| {
                [
                    255,
                    73,
                    91,
                    if (index % side + index / side) % 2 == 0 {
                        255
                    } else {
                        0
                    },
                ]
            })
            .collect::<Vec<_>>();
        assert!(extruded_sprite_vertices(side, side, &pixels, FULL).is_none());
        assert!(held_sprite_vertices(side, side, &pixels, FULL).is_none());
        assert!(extruded_sprite_vertices(side, side, &opaque(side, side), FULL).is_some());
    }

    fn opaque(width: usize, height: usize) -> Vec<u8> {
        vec![255; width * height * 4]
    }

    /// Scales an original eight-cell silhouette with a hole and one-cell-wide protrusions.
    fn resolution_fixture(width: usize, height: usize) -> Vec<u8> {
        (0..height)
            .flat_map(|row| {
                (0..width).flat_map(move |column| {
                    let longer = width.max(height);
                    let (x, y) = (column * 8 / longer, row * 8 / longer);
                    let solid = ((1..7).contains(&x)
                        && (1..7).contains(&y)
                        && !((3..5).contains(&x) && (3..5).contains(&y)))
                        || (x == y);
                    [29, 173, 211, if solid { 255 } else { 0 }]
                })
            })
            .collect()
    }

    /// Measures total outward side area by axis, including walls inside transparent holes.
    fn side_areas(vertices: &[crate::ActorRigVertex]) -> [f32; 2] {
        let mut areas = [0.0; 2];
        for triangle in vertices.as_chunks::<3>().0 {
            let normal = triangle[0].normal;
            if normal[2] != 0.0 {
                continue;
            }
            let [a, b, c] = [0, 1, 2].map(|index| triangle[index].position);
            let axis = usize::from(normal[0] == 0.0);
            let along = 1 - axis;
            areas[axis] += ((b[along] - a[along]) * (c[2] - a[2])
                - (c[along] - a[along]) * (b[2] - a[2]))
                .abs()
                * 0.5;
        }
        areas
    }

    #[test]
    fn held_sprite_depth_and_contour_do_not_change_with_pack_resolution() {
        let region = [0.25, 0.5, 0.5, 0.75];
        for aspect in [0, 1, 2] {
            let mut reference_areas = None;
            for side in [8, 16, 32, 64, 128] {
                let (width, height) = match aspect {
                    1 => (side, side / 2),
                    2 => (side / 2, side),
                    _ => (side, side),
                };
                let pixels = resolution_fixture(width, height);
                let vertices = held_sprite_vertices(width, height, &pixels, region).unwrap();
                let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
                for vertex in &vertices {
                    for axis in 0..3 {
                        min[axis] = min[axis].min(vertex.position[axis]);
                        max[axis] = max[axis].max(vertex.position[axis]);
                    }
                    if vertex.normal[2] == 0.0 {
                        let [u, v] = std::array::from_fn(|axis| {
                            (vertex.uv[axis] - region[axis]) / (region[axis + 2] - region[axis])
                        });
                        let column = (u * width as f32).floor() as usize;
                        let row = (v * height as f32).floor() as usize;
                        assert_eq!(pixels[(row * width + column) * 4 + 3], 255);
                        assert_eq!(vertex.uv, vertex.back_uv);
                    }
                    assert_eq!(vertex.glint_origin, [region[0], region[1]]);
                }
                assert_eq!(
                    min,
                    [-((width as f32) / side as f32), 0.0, -1.0 / 16.0],
                    "resolution {width}x{height}"
                );
                assert_eq!(max, [0.0, height as f32 / side as f32, 0.0]);
                let areas = side_areas(&vertices);
                let reference = *reference_areas.get_or_insert(areas);
                for (actual, expected) in areas.into_iter().zip(reference) {
                    assert!(
                        (actual - expected).abs() < 1e-6,
                        "resolution {width}x{height}"
                    );
                }
            }
        }
    }

    #[test]
    fn centered_sprite_keeps_source_texel_depth_at_every_resolution() {
        for side in [8, 16, 32, 64, 128] {
            let vertices = extruded_sprite_vertices(side, side, &opaque(side, side), FULL).unwrap();
            let half_depth = 0.5 / side as f32;
            assert!(
                vertices
                    .iter()
                    .all(|vertex| vertex.position[2].abs() == half_depth)
            );
            assert!(
                vertices
                    .iter()
                    .any(|vertex| vertex.position[2] == half_depth)
            );
            assert!(
                vertices
                    .iter()
                    .any(|vertex| vertex.position[2] == -half_depth)
            );
        }
    }

    #[test]
    fn held_sprite_edges_ignore_alpha_one_without_hiding_their_neighbours() {
        let region = [0.25, 0.5, 0.5, 0.75];
        for side in [16, 32, 64, 128] {
            let mut pixels = resolution_fixture(side, side);
            let expected = held_sprite_vertices(side, side, &pixels, region).unwrap();
            for pixel in pixels.as_chunks_mut::<4>().0 {
                if pixel[3] == 0 {
                    pixel[3] = 1;
                }
            }
            let actual = held_sprite_vertices(side, side, &pixels, region).unwrap();
            assert_eq!(actual, expected, "resolution {side}");
            assert!(actual.iter().any(|vertex| vertex.normal[0] != 0.0));
            assert!(actual.iter().any(|vertex| vertex.normal[1] != 0.0));
        }
    }

    #[test]
    fn held_sprite_edge_mask_keeps_alpha_two_and_partial_coverage() {
        for side in [16, 32, 64, 128] {
            for alpha in [0, 1, 2, 127, 128, 255] {
                let mut pixels = vec![0; side * side * 4];
                let (column, row) = (side / 2, side / 4);
                pixels[(row * side + column) * 4..][..4].copy_from_slice(&[23, 149, 211, alpha]);
                let vertices = held_sprite_vertices(side, side, &pixels, FULL).unwrap();
                let sides = &vertices[12..];
                assert_eq!(sides.len(), if alpha < 2 { 0 } else { 24 });
                let uv = [
                    (column as f32 + 0.5) / side as f32,
                    (row as f32 + 0.5) / side as f32,
                ];
                for vertex in sides {
                    assert_eq!(vertex.uv, uv);
                    assert_eq!(vertex.back_uv, uv);
                    assert!(
                        vertex.position[2] == 0.0
                            || vertex.position[2] == -super::HELD_SPRITE_DEPTH
                    );
                }
            }
        }
        // Centered sprites retain their separate nonzero-alpha edge contract.
        assert_eq!(
            extruded_sprite_vertices(1, 1, &[255, 255, 255, 1], FULL)
                .unwrap()
                .len(),
            36
        );
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
            assert_eq!(vertex.glint_origin, [region[0], region[1]]);
        }
    }

    #[test]
    fn held_sprite_glint_coordinates_are_independent_of_atlas_placement() {
        let plain = held_sprite_vertices(4, 4, &opaque(4, 4), FULL).unwrap();
        let placed = held_sprite_vertices(4, 4, &opaque(4, 4), [0.25, 0.5, 0.5, 0.625]).unwrap();
        for (plain, placed) in plain.iter().zip(&placed) {
            for (axis, span) in [0.25, 0.125].into_iter().enumerate() {
                assert_eq!(
                    (placed.uv[axis] - placed.glint_origin[axis]) / span,
                    plain.uv[axis]
                );
                assert_eq!(
                    (placed.back_uv[axis] - placed.glint_origin[axis]) / span,
                    plain.back_uv[axis]
                );
            }
        }
    }

    // A framed or campfire item is drawn two-sided; rear art that differs from the front at a
    // point shows through transparent texels as a mirrored second copy.
    #[test]
    fn both_faces_sample_the_same_texel_at_each_point() {
        let vertices = extruded_sprite_vertices(4, 4, &opaque(4, 4), FULL).unwrap();
        let faces = &vertices[..12];
        for vertex in faces {
            let other = faces
                .iter()
                .find(|other| {
                    other.position[..2] == vertex.position[..2]
                        && other.position[2] != vertex.position[2]
                })
                .unwrap();
            assert_eq!(vertex.uv, other.uv);
            assert_eq!(vertex.back_uv, vertex.uv);
        }
        // The +Z face reads unmirrored: column 0 on the left, row 0 at the top.
        for vertex in faces.iter().filter(|vertex| vertex.normal[2] > 0.0) {
            assert_eq!(
                vertex.uv[0],
                if vertex.position[0] < 0.0 { 0.0 } else { 1.0 }
            );
            assert_eq!(
                vertex.uv[1],
                if vertex.position[1] > 0.0 { 0.0 } else { 1.0 }
            );
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
