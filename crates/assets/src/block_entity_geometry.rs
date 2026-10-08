//! Static authoring geometry shared by placed block entities and their inventory models.

pub type ModelBox = ([f32; 3], [f32; 3], [f32; 2]);
pub type ModelFace = ([[f32; 3]; 4], [f32; 4]);

pub const BANNER_TEXTURE: (&str, [f32; 2]) = ("textures/entity/banner/banner_base", [64.0; 2]);
pub const BANNER_POLE: ModelBox = ([-1.0, -30.0, -1.0], [2.0, 42.0, 2.0], [44.0, 0.0]);
pub const BANNER_BAR: ModelBox = ([-10.0, -32.0, -1.0], [20.0, 2.0, 2.0], [0.0, 42.0]);
pub const BANNER_CLOTH: ModelBox = ([-10.0, 0.0, -2.0], [20.0, 40.0, 1.0], [0.0, 0.0]);
pub const BANNER_CLOTH_PIVOT: [f32; 3] = [0.0, -32.0, 0.0];

pub const SHULKER_TEXTURE_SIZE: [f32; 2] = [64.0; 2];
pub const CONDUIT_TEXTURE: (&str, [f32; 2]) = ("textures/blocks/conduit_base", [24.0, 12.0]);
pub const POT_BASE_TEXTURE: (&str, [f32; 2]) = ("textures/blocks/decorated_pot_base", [32.0; 2]);
pub const POT_SIDE_TEXTURE: (&str, [f32; 2]) = ("textures/blocks/decorated_pot_side", [16.0; 2]);
pub const LECTERN_TEXTURES: [&str; 4] = [
    "textures/blocks/lectern_base",
    "textures/blocks/lectern_sides",
    "textures/blocks/lectern_top",
    "textures/blocks/lectern_front",
];

pub const SHULKER_BASE: ModelBox = ([-8.0, 0.0, -8.0], [16.0, 8.0, 16.0], [0.0, 28.0]);
pub const SHULKER_LID: ModelBox = ([-8.0, 4.0, -8.0], [16.0, 12.0, 16.0], [0.0, 0.0]);
pub const CONDUIT_SHELL: ModelBox = ([-3.0; 3], [6.0; 3], [0.0; 2]);
pub const POT_BODY_HALF: f32 = 7.0;
pub const POT_BODY_HEIGHT: f32 = 16.0;
pub const POT_NECK: ModelBox = ([-4.0, 14.0, -4.0], [8.0, 3.0, 8.0], [0.0, 0.0]);
pub const POT_LIP: ModelBox = ([-3.0, POT_BODY_HEIGHT, -3.0], [6.0, 1.0, 6.0], [0.0, 5.0]);
pub const POT_NECK_INFLATE: f32 = -0.1;
pub const POT_LIP_INFLATE: f32 = 0.2;
pub const POT_PLANES: [([f32; 4], f32); 2] = [
    ([0.0, 13.0, 14.0, 14.0], POT_BODY_HEIGHT),
    ([14.0, 13.0, 14.0, 14.0], 0.0),
];

/// The pot body side faces in front, back, left and right order.
#[must_use]
pub fn pot_sides() -> [[[f32; 3]; 4]; 4] {
    let faces = tile_box_faces([
        [-POT_BODY_HALF, 0.0, -POT_BODY_HALF],
        [POT_BODY_HALF, POT_BODY_HEIGHT, POT_BODY_HALF],
    ]);
    [faces[4], faces[5], faces[1], faces[0]]
}
pub const LECTERN_BASE: [[f32; 3]; 2] = [[-8.0, 0.0, -8.0], [8.0, 2.0, 8.0]];
pub const LECTERN_POST: [[f32; 3]; 2] = [[-4.0, 2.0, -4.0], [4.0, 14.0, 4.0]];
pub const LECTERN_BOARD: [[f32; 3]; 2] = [[-7.9, 11.0, -6.0], [7.9, 15.0, 7.0]];
pub const LECTERN_BOARD_PIVOT: [f32; 3] = [0.0, 7.0, -1.0];
pub const LECTERN_BOARD_OFFSET: [f32; 3] = [0.0, 1.05, 1.0];
pub const LECTERN_SLOPE_DEGREES: f32 = 22.5;

/// Closed north-facing stand in authoring pixels; each face indexes [`LECTERN_TEXTURES`].
#[must_use]
pub fn lectern_faces() -> [(usize, ModelFace); 18] {
    let full = [0.0, 0.0, 16.0, 16.0];
    let (bottom, sides, top, front) = (0, 1, 2, 3);
    let surfaces = [
        [
            (sides, [0.0, 6.0, 16.0, 2.0], 0),
            (sides, [0.0, 6.0, 16.0, 2.0], 0),
            (bottom, [0.0, 16.0, 16.0, -16.0], 0),
            (sides, full, 0),
            (sides, [0.0, 14.0, 16.0, 2.0], 0),
            (sides, [0.0, 6.0, 16.0, 2.0], 0),
        ],
        [
            (sides, [2.0, 8.0, 13.0, 8.0], 0),
            (sides, [2.0, 16.0, 13.0, -8.0], 0),
            (sides, [4.0, 12.0, 8.0, -8.0], 0),
            (sides, [4.0, 4.0, 8.0, 8.0], 2),
            (front, [0.0, 0.0, 8.0, 13.0], 1),
            (front, [8.0, 3.0, 8.0, 13.0], 1),
        ],
        [
            (sides, [0.0, 4.0, 13.0, 4.0], 0),
            (sides, [0.0, 4.0, 13.0, 4.0], 0),
            (bottom, [0.0, 13.0, 16.0, -13.0], 0),
            (top, [0.0, 1.0, 16.0, 13.0], 0),
            (sides, [0.0, 0.0, 16.0, 4.0], 0),
            (sides, [0.0, 4.0, 16.0, 4.0], 0),
        ],
    ];
    let bounds = [LECTERN_BASE, LECTERN_POST, LECTERN_BOARD];
    let faces = bounds.map(tile_box_faces);
    std::array::from_fn(|index| {
        let (part, face) = (index / 6, index % 6);
        let (texture, texels, turns) = surfaces[part][face];
        let mut corners = faces[part][face];
        if part == 2 {
            corners = corners.map(lectern_board_point);
        }
        corners.rotate_right(turns);
        (texture, (corners, texels))
    })
}

/// The shared entity texture stem for one shulker color.
#[must_use]
pub fn shulker_texture(color: &str) -> String {
    format!("textures/entity/shulker/shulker_{color}")
}

/// Tilts a lectern board point around its authored pivot and then offsets it.
#[must_use]
pub fn lectern_board_point([x, y, z]: [f32; 3]) -> [f32; 3] {
    let [_, py, pz] = LECTERN_BOARD_PIVOT;
    let [tx, ty, tz] = LECTERN_BOARD_OFFSET;
    let (sin, cos) = (-LECTERN_SLOPE_DEGREES.to_radians()).sin_cos();
    [
        x + tx,
        cos * (y - py) - sin * (z - pz) + py + ty,
        sin * (y - py) + cos * (z - pz) + pz + tz,
    ]
}

/// Model-part box UV layout; texel rectangles preserve the bottom face's reversed V.
#[must_use]
pub fn box_faces(origin: [f32; 3], size: [f32; 3], uv: [f32; 2], inflate: f32) -> [ModelFace; 6] {
    let [ox, oy, oz] = origin;
    let [sx, sy, sz] = size;
    let (x0, x1) = (ox - inflate, ox + sx + inflate);
    let (y0, y1) = (oy - inflate, oy + sy + inflate);
    let (z0, z1) = (oz - inflate, oz + sz + inflate);
    let [u, v] = uv;
    [
        (
            [[x1, y1, z0], [x0, y1, z0], [x0, y0, z0], [x1, y0, z0]],
            [u + sz, v + sz, sx, sy],
        ),
        (
            [[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]],
            [u + sz + sx + sz, v + sz, sx, sy],
        ),
        (
            [[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]],
            [u, v + sz, sz, sy],
        ),
        (
            [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]],
            [u + sz + sx, v + sz, sz, sy],
        ),
        (
            [[x1, y1, z1], [x0, y1, z1], [x0, y1, z0], [x1, y1, z0]],
            [u + sz, v, sx, sz],
        ),
        (
            [[x1, y0, z0], [x0, y0, z0], [x0, y0, z1], [x1, y0, z1]],
            [u + sz + sx, v + sz, sx, -sz],
        ),
    ]
}

/// Bounds faces in `[west, east, down, up, north, south]` order, top-left first.
#[must_use]
pub fn tile_box_faces([min, max]: [[f32; 3]; 2]) -> [[[f32; 3]; 4]; 6] {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    [
        [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]],
        [[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]],
        [[x1, y0, z0], [x0, y0, z0], [x0, y0, z1], [x1, y0, z1]],
        [[x1, y1, z1], [x0, y1, z1], [x0, y1, z0], [x1, y1, z0]],
        [[x1, y1, z0], [x0, y1, z0], [x0, y0, z0], [x1, y0, z0]],
        [[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]],
    ]
}

/// The texture suffix for a shulker-box block name such as `minecraft:silver_shulker_box`.
#[must_use]
pub fn shulker_color_from_block_name(name: &str) -> Option<&'static str> {
    let stem = name
        .strip_prefix("minecraft:")?
        .strip_suffix("_shulker_box")?;
    Some(match stem {
        "undyed" => "undyed",
        "white" => "white",
        "orange" => "orange",
        "magenta" => "magenta",
        "light_blue" => "light_blue",
        "yellow" => "yellow",
        "lime" => "lime",
        "pink" => "pink",
        "gray" => "gray",
        "silver" | "light_gray" => "silver",
        "cyan" => "cyan",
        "purple" => "purple",
        "blue" => "blue",
        "brown" => "brown",
        "green" => "green",
        "red" => "red",
        "black" => "black",
        _ => return None,
    })
}
