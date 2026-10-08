//! Shared native GUI item coordinates for offline fallback baking and runtime model drawing.
//! The matching 1.26.50.26 witnesses are recorded in docs/reference/inventory-gui-geometry.md.

use crate::{BlockFace, EntityGeometryCube, EntityGeometryUv};

/// Native item-frame design pixels, not source-texture or framebuffer pixels.
pub const GUI_ITEM_SIDE: f32 = 16.0;
pub const CUBE_SCALE: f32 = 10.0;
pub const CUBE_OFFSET: [f32; 2] = [1.0, f32::from_bits(0x4147_ae14)];
/// Canonical `[sin(X), cos(X), sin(Y), cos(Y)]` for native Rx(210°) Ry(45°).
pub const CUBE_ROTATION_SIN_COS: [f32; 4] = [
    f32::from_bits(0xbeff_ffff),
    f32::from_bits(0xbf5d_b3d7),
    f32::from_bits(0x3f35_04f3),
    f32::from_bits(0x3f35_04f3),
];
pub const SHIELD_MODEL_UNIT: f32 = 1.0 / 16.0;
pub const SHIELD_GUI_TRANSLATION: [f32; 3] = [8.0, 10.0, -10.0];
pub const SHIELD_GUI_MODEL_SCALE: f32 = 11.0;
pub const SHIELD_MODEL_PART_HEIGHT: f32 = 24.0;
pub const SHIELD_GUI_ROTATION_RADIANS: f32 = f32::from_bits(0x3f06_0a92);
pub const SHIELD_ALPHA_CUTOFF: f32 = 0.5;
pub const SHIELD_ROOT_BONE: &str = "shield";
pub const SHIELD_IDENTIFIER: &str = "minecraft:shield";
pub const SHULKER_GUI_LIGHT: f32 = f32::from_bits(0x3f3a_e148);
pub const SHIELD_FACE_CORNERS: [[usize; 4]; 6] = [
    [3, 2, 1, 0],
    [6, 7, 4, 5],
    [7, 3, 0, 4],
    [2, 6, 5, 1],
    [7, 6, 2, 3],
    [0, 1, 5, 4],
];
pub type GuiCubeFace = (BlockFace, [[f32; 3]; 4], [[f32; 2]; 4], f32);
/// Native non-fullbright ordinary cube GUI order, geometry, UVs and brightness.
pub const CUBE_FACES: [GuiCubeFace; 3] = [
    (
        BlockFace::Up,
        [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]],
        [[0., 0.], [0., 1.], [1., 1.], [1., 0.]],
        1.0,
    ),
    (
        BlockFace::South,
        [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
        [[0., 1.], [1., 1.], [1., 0.], [0., 0.]],
        0.5,
    ),
    (
        BlockFace::West,
        [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
        [[0., 1.], [1., 1.], [1., 0.], [0., 0.]],
        f32::from_bits(0x3f3a_e148),
    ),
];

/// Design-pixel projection and the unscaled GUI view depth for an ordinary full cube.
#[must_use]
pub fn project_cube([x, y, z]: [f32; 3]) -> [f32; 3] {
    // Canonical f32 sin/cos results of the matched rotation angles. Shared fixed values keep
    // legacy offline bytes independent of the host library while retaining native matrix order.
    let [sx, cx, sy, cy] = CUBE_ROTATION_SIN_COS;
    let rotated_x = cy * x + sy * z;
    let rotated_z = -sy * x + cy * z;
    [
        CUBE_OFFSET[0] + CUBE_SCALE * rotated_x,
        CUBE_OFFSET[1] + CUBE_SCALE * (cx * y - sx * rotated_z),
        sx * y + cx * rotated_z,
    ]
}

/// Closed conduit model-part coordinates through its inventory matrix, in design pixels.
#[must_use]
pub fn project_conduit(point: [f32; 3]) -> [f32; 3] {
    project_entity(
        point,
        [8.0, 8.5, -10.0],
        18.5,
        f32::from_bits(0x3f06_0a92),
        f32::from_bits(0xbf5f_66f3),
    )
}

/// Plain decorated-pot model-part coordinates through its inventory matrix, in design pixels.
#[must_use]
pub fn project_decorated_pot([x, y, z]: [f32; 3]) -> [f32; 3] {
    project_entity(
        [x + 8.0, y, z + 8.0],
        [8.0, 9.0, -10.0],
        CUBE_SCALE,
        f32::from_bits(0xc027_8d36),
        f32::from_bits(0xbf49_0fdb),
    )
}

/// Applies a model-part inventory matrix while retaining its view depth.
fn project_entity(
    [x, y, z]: [f32; 3],
    offset: [f32; 3],
    scale: f32,
    pitch: f32,
    yaw: f32,
) -> [f32; 3] {
    let (sx, cx) = pitch.sin_cos();
    let (sy, cy) = yaw.sin_cos();
    let rotated_x = cy * x + sy * z;
    let rotated_z = -sy * x + cy * z;
    let rotated = [rotated_x, cx * y - sx * rotated_z, sx * y + cx * rotated_z];
    std::array::from_fn(|axis| offset[axis] + rotated[axis] * scale / 16.0)
}

/// Static standing-banner coordinates through its inventory matrix, in design pixels.
#[must_use]
pub fn project_banner(point: [f32; 3]) -> [f32; 3] {
    project_entity(
        point,
        [8.5, 11.0, -10.0],
        5.5,
        f32::from_bits(0x3eb2_b8c2),
        f32::from_bits(0xbf06_0a92),
    )
}

/// Static shield model-part geometry through its own vanilla GUI matrix, in design pixels.
#[must_use]
pub fn project_shield(authored: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = [
        authored[0],
        SHIELD_MODEL_PART_HEIGHT - authored[1],
        authored[2],
    ];
    let (sin, cos) = SHIELD_GUI_ROTATION_RADIANS.sin_cos();
    let rotated_x = cos * x + sin * z;
    let rotated_z = -sin * x + cos * z;
    let rotated_y = cos * y - sin * rotated_z;
    let rotated = [rotated_x, rotated_y, sin * y + cos * rotated_z];
    std::array::from_fn(|axis| {
        SHIELD_GUI_TRANSLATION[axis] + rotated[axis] * SHIELD_MODEL_UNIT * SHIELD_GUI_MODEL_SCALE
    })
}

/// Model-part box UV layout, or explicit authored face UV dimensions.
#[must_use]
pub fn shield_face_uvs(cube: &EntityGeometryCube) -> [Option<[[f32; 2]; 4]>; 6] {
    let quad =
        |[u, v]: [f32; 2], [w, h]: [f32; 2]| [[u, v], [u + w, v], [u + w, v + h], [u, v + h]];
    match &cube.uv {
        EntityGeometryUv::Box(uv) => {
            let [u, v] = uv.map(|value| value.get());
            let [x, y, z] = cube.size.map(|value| value.get().trunc());
            [
                [u + z, v + z, x, y],
                [u + z + x + z, v + z, x, y],
                [u, v + z, z, y],
                [u + z + x, v + z, z, y],
                [u + z, v, x, z],
                [u + z + x, v, x, z],
            ]
            .map(|[u, v, w, h]| Some(quad([u, v], [w, h])))
        }
        EntityGeometryUv::Faces(faces) => {
            let faces = [
                &faces.north,
                &faces.south,
                &faces.east,
                &faces.west,
                &faces.up,
                &faces.down,
            ];
            let dimensions = cube.face_uv_dimensions();
            std::array::from_fn(|index| {
                faces[index].as_ref().map(|face| {
                    quad(
                        face.uv.map(|value| value.get()),
                        face.uv_size
                            .map_or(dimensions[index], |size| size.map(|value| value.get())),
                    )
                })
            })
        }
    }
}
