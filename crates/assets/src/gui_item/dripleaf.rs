//! Leaf surfaces and crossed stems in the carried dripleaf poses.

use super::{GuiBlockQuad, cube_face};
use crate::BlockFace;

const STEM_TOP: f32 = 15.0 / 16.0;

/// Emits the leaf and stem materials with independent ordinary face shading.
#[must_use]
pub fn dripleaf_quads(small: bool) -> Vec<(GuiBlockQuad, bool)> {
    if small { small_quads() } else { big_quads() }
}

/// Creates one unshaded, source-textured surface.
fn surface(corners: [[f32; 3]; 4], uvs: [[f32; 2]; 4], face: BlockFace) -> (GuiBlockQuad, bool) {
    (
        GuiBlockQuad {
            corners,
            uvs,
            material: face as u32,
        },
        false,
    )
}

/// Crossed stem planes retain the narrow authored strip and the carried height.
fn stems([x, z]: [f32; 2]) -> Vec<(GuiBlockQuad, bool)> {
    let radius = 3.0 / 16.0;
    [
        [[x - radius, z + radius], [x + radius, z - radius]],
        [[x - radius, z - radius], [x + radius, z + radius]],
    ]
    .into_iter()
    .map(|[[ax, az], [bx, bz]]| {
        surface(
            [
                [ax, 0., az],
                [bx, 0., bz],
                [bx, STEM_TOP, bz],
                [ax, STEM_TOP, az],
            ],
            [[0.3125, 1.], [0.75, 1.], [0.75, 0.0625], [0.3125, 0.0625]],
            BlockFace::North,
        )
    })
    .collect()
}

/// Builds the level large leaf with its three hanging edges and short stem.
fn big_quads() -> Vec<(GuiBlockQuad, bool)> {
    let mut out = stems([0.5, 0.25]);
    out.push(surface(
        [
            [0., STEM_TOP, 1.],
            [1., STEM_TOP, 1.],
            [1., STEM_TOP, 0.],
            [0., STEM_TOP, 0.],
        ],
        [[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
        BlockFace::South,
    ));
    let edge_uv = [[1., 0.25], [0., 0.25], [0., 0.], [1., 0.]];
    out.push(surface(
        [
            [0., STEM_TOP - 0.25, 1.],
            [1., STEM_TOP - 0.25, 1.],
            [1., STEM_TOP, 1.],
            [0., STEM_TOP, 1.],
        ],
        edge_uv,
        BlockFace::Up,
    ));
    for x in [1.0 / 64.0, 1.0 - 1.0 / 64.0] {
        out.push(surface(
            [
                [x, STEM_TOP - 0.25, 0.],
                [x, STEM_TOP - 0.25, 1.],
                [x, STEM_TOP, 1.],
                [x, STEM_TOP, 0.],
            ],
            edge_uv,
            BlockFace::Down,
        ));
    }
    out
}

/// Builds the upper small plant with three staggered leaves facing its carried axis.
fn small_quads() -> Vec<(GuiBlockQuad, bool)> {
    let mut out = stems([0.25, 0.5]);
    let leaves = [
        ([-0.25, -0.3125, 0.], [false, false]),
        ([0.25, 0., 0.5], [true, true]),
        ([0.25, 0.375, 0.], [true, false]),
    ];
    for (offset, _) in leaves {
        for face in BlockFace::ALL
            .into_iter()
            .filter(|face| face.is_horizontal())
        {
            out.push(small_surface(face, offset, [false; 2]));
        }
    }
    for (offset, flip) in leaves {
        out.push(small_surface(BlockFace::Up, offset, flip));
    }
    out
}

/// Keeps a small leaf's top and walls on the same half-block bounds and artwork quarter.
fn small_surface(face: BlockFace, offset: [f32; 3], flip: [bool; 2]) -> (GuiBlockQuad, bool) {
    let (corners, uvs) = cube_face(face);
    let horizontal = face.is_horizontal();
    (
        GuiBlockQuad {
            corners: corners.map(|point| std::array::from_fn(|i| offset[i] + point[i] * 0.5)),
            uvs: uvs.map(|uv| {
                std::array::from_fn(|i| {
                    if flip[i] {
                        (1.0 - uv[i]) * 0.5
                    } else {
                        uv[i] * 0.5
                    }
                })
            }),
            material: if horizontal {
                BlockFace::Down
            } else {
                BlockFace::Up
            } as u32,
        },
        horizontal,
    )
}
