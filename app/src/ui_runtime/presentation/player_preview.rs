//! Small software-rendered player preview used by the gameplay HUD.
//!
//! The preview is deliberately generated from the same standard-biped vertex
//! and UV contract used by the 3-D actor renderer. It is cached by the caller
//! and uploaded as one UI texture layer only when the authoritative skin or
//! pose changes, so it does not add a per-frame GPU upload or a second camera.

use render::{ActorVertex, standard_biped_overlay_vertices, standard_biped_vertices};

pub(crate) const PREVIEW_WIDTH: u32 = 96;
pub(crate) const PREVIEW_HEIGHT: u32 = 112;
pub(crate) const HAND_WIDTH: u32 = 64;
pub(crate) const HAND_HEIGHT: u32 = 64;

const MODEL_SCALE: f32 = 48.0;
const CAMERA_YAW_RADIANS: f32 = -0.38;
const MODEL_BOTTOM: f32 = 106.0;

/// Quantized authoritative pose used by the small HUD avatar. Keeping the
/// angles to quarter-degree steps avoids rebuilding the UI texture array for
/// insignificant network noise while still tracking normal mouse movement.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PlayerPreviewPose {
    pub(crate) body_yaw_degrees: f32,
    pub(crate) head_yaw_degrees: f32,
    pub(crate) pitch_degrees: f32,
    pub(crate) sneaking: bool,
}

/// Raster bundle retained so the presentation can rebuild its bounded dynamic
/// texture pages when menu artwork or held-item carriers change.
#[derive(Clone, Debug)]
pub(crate) struct PlayerPreviewRasters {
    pub(crate) preview: Vec<u8>,
    pub(crate) left_hand: Vec<u8>,
    pub(crate) right_hand: Vec<u8>,
}

impl PlayerPreviewPose {
    pub(crate) fn new(
        body_yaw_degrees: f32,
        head_yaw_degrees: f32,
        pitch_degrees: f32,
        sneaking: bool,
    ) -> Self {
        Self {
            body_yaw_degrees: quantize_angle(body_yaw_degrees),
            head_yaw_degrees: quantize_angle(head_yaw_degrees),
            pitch_degrees: quantize_angle(pitch_degrees),
            sneaking,
        }
    }
}

#[derive(Clone, Copy)]
struct ProjectedVertex {
    screen: [f32; 2],
    depth: f32,
    uv: [f32; 2],
    world: [f32; 3],
}

/// Renders a nearest-neighbour, orthographic 3-D biped preview from a
/// validated 64x64 player skin. Transparent pixels remain transparent so the
/// HUD has no artificial square around the avatar.
pub(crate) fn render(skin: &[u8], pose: PlayerPreviewPose) -> Vec<u8> {
    let width = PREVIEW_WIDTH as usize;
    let height = PREVIEW_HEIGHT as usize;
    let mut pixels = vec![0u8; width * height * 4];
    let mut depth = vec![f32::NEG_INFINITY; width * height];
    let mut vertices = standard_biped_vertices();
    vertices.extend(standard_biped_overlay_vertices());
    let body_yaw = pose.body_yaw_degrees.to_radians() + CAMERA_YAW_RADIANS;
    let (sin_yaw, cos_yaw) = body_yaw.sin_cos();
    let head_yaw = (pose.head_yaw_degrees - pose.body_yaw_degrees)
        .to_radians()
        .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
    let head_pitch = (-pose.pitch_degrees.to_radians() * 0.5)
        .clamp(-std::f32::consts::FRAC_PI_4, std::f32::consts::FRAC_PI_4);

    for triangle in vertices.chunks_exact(3) {
        let projected = [
            project(
                triangle[0],
                sin_yaw,
                cos_yaw,
                head_yaw,
                head_pitch,
                pose.sneaking,
            ),
            project(
                triangle[1],
                sin_yaw,
                cos_yaw,
                head_yaw,
                head_pitch,
                pose.sneaking,
            ),
            project(
                triangle[2],
                sin_yaw,
                cos_yaw,
                head_yaw,
                head_pitch,
                pose.sneaking,
            ),
        ];
        rasterize_triangle(&mut pixels, &mut depth, width, height, skin, projected);
    }
    pixels
}

/// Renders one first-person arm from the authoritative player skin. Bedrock's
/// HUD has a distinct viewmodel path (`short_arm_offset_*` and
/// `player_arm_height` in the client), so this is deliberately separate from
/// the corner paper-doll. It gives empty hands and item carriers a real skin
/// silhouette while the full 3-D item viewmodel is still on the render-path
/// roadmap.
pub(crate) fn render_hand(skin: &[u8], pose: PlayerPreviewPose, left: bool) -> Vec<u8> {
    let width = HAND_WIDTH as usize;
    let height = HAND_HEIGHT as usize;
    let mut pixels = vec![0u8; width * height * 4];
    let mut depth = vec![f32::NEG_INFINITY; width * height];
    let part = if left { 2 } else { 3 };
    let mut vertices = standard_biped_vertices();
    vertices.extend(standard_biped_overlay_vertices());
    for triangle in vertices.chunks_exact(3) {
        if triangle.iter().any(|vertex| vertex.part != part) {
            continue;
        }
        let projected = [
            project_hand(triangle[0], pose, left),
            project_hand(triangle[1], pose, left),
            project_hand(triangle[2], pose, left),
        ];
        rasterize_triangle(&mut pixels, &mut depth, width, height, skin, projected);
    }
    pixels
}

fn rasterize_triangle(
    pixels: &mut [u8],
    depth: &mut [f32],
    width: usize,
    height: usize,
    skin: &[u8],
    projected: [ProjectedVertex; 3],
) {
    let area = edge(
        projected[0].screen,
        projected[1].screen,
        projected[2].screen,
    );
    if !area.is_finite() || area.abs() < f32::EPSILON {
        return;
    }
    let min_x = projected
        .iter()
        .map(|vertex| vertex.screen[0].floor() as i32)
        .min()
        .unwrap_or_default()
        .clamp(0, width as i32 - 1);
    let max_x = projected
        .iter()
        .map(|vertex| vertex.screen[0].ceil() as i32)
        .max()
        .unwrap_or_default()
        .clamp(0, width as i32 - 1);
    let min_y = projected
        .iter()
        .map(|vertex| vertex.screen[1].floor() as i32)
        .min()
        .unwrap_or_default()
        .clamp(0, height as i32 - 1);
    let max_y = projected
        .iter()
        .map(|vertex| vertex.screen[1].ceil() as i32)
        .max()
        .unwrap_or_default()
        .clamp(0, height as i32 - 1);
    if min_x > max_x || min_y > max_y {
        return;
    }

    let shade = face_shade(projected[0].world, projected[1].world, projected[2].world);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let point = [x as f32 + 0.5, y as f32 + 0.5];
            let weights = [
                edge(projected[1].screen, projected[2].screen, point) / area,
                edge(projected[2].screen, projected[0].screen, point) / area,
                edge(projected[0].screen, projected[1].screen, point) / area,
            ];
            if weights.iter().any(|weight| *weight < 0.0) {
                continue;
            }
            let pixel_index = y as usize * width + x as usize;
            let candidate_depth = weights
                .iter()
                .zip(projected.iter())
                .map(|(weight, vertex)| *weight * vertex.depth)
                .sum::<f32>();
            if candidate_depth <= depth[pixel_index] {
                continue;
            }
            let uv = std::array::from_fn(|axis| {
                weights[0] * projected[0].uv[axis]
                    + weights[1] * projected[1].uv[axis]
                    + weights[2] * projected[2].uv[axis]
            });
            let Some(mut color) = sample_skin(skin, uv) else {
                continue;
            };
            if color[3] < 10 {
                continue;
            }
            color[0] = (f32::from(color[0]) * shade).round() as u8;
            color[1] = (f32::from(color[1]) * shade).round() as u8;
            color[2] = (f32::from(color[2]) * shade).round() as u8;
            depth[pixel_index] = candidate_depth;
            let target = pixel_index * 4;
            pixels[target..target + 4].copy_from_slice(&color);
        }
    }
}

fn project(
    vertex: ActorVertex,
    sin_yaw: f32,
    cos_yaw: f32,
    head_yaw: f32,
    head_pitch: f32,
    sneaking: bool,
) -> ProjectedVertex {
    let mut local = vertex.position;
    if sneaking {
        local = sneak_pose(local, vertex.part);
    }
    if vertex.part == 0 {
        local = rotate_y(local, head_yaw, [0.0, 1.75, 0.0]);
        local = rotate_x(local, head_pitch, [0.0, 1.75, 0.0]);
    }
    let [x, y, z] = local;
    let world = [x * cos_yaw - z * sin_yaw, y, x * sin_yaw + z * cos_yaw];
    ProjectedVertex {
        screen: [
            PREVIEW_WIDTH as f32 * 0.5 + world[0] * MODEL_SCALE,
            MODEL_BOTTOM - world[1] * MODEL_SCALE,
        ],
        depth: world[2],
        uv: vertex.uv,
        world,
    }
}

fn project_hand(vertex: ActorVertex, pose: PlayerPreviewPose, left: bool) -> ProjectedVertex {
    let side = if left { -1.0 } else { 1.0 };
    let mut local = vertex.position;
    // Re-center the standard biped arm into a compact first-person carrier.
    // The two arms are uploaded into separate sprites, so each remains at the
    // edge of the view instead of inheriting the paper-doll's shoulder span.
    local[0] = (local[0] - side * 0.375) * 0.82 + side * 0.05;
    local[1] = (local[1] - 0.75) * 0.86 + 0.08;
    if pose.sneaking {
        local[1] -= 0.03;
    }
    let pitch = (-pose.pitch_degrees.to_radians() * 0.18)
        .clamp(-std::f32::consts::FRAC_PI_6, std::f32::consts::FRAC_PI_6);
    local = rotate_x(local, pitch - 0.34, [0.0, 0.18, 0.0]);
    // The first-person carrier exposes the top and inner faces of the arm.
    // Applying yaw before the inward roll preserves the cuboid silhouette;
    // the previous roll-only projection collapsed the depth axis and read as
    // a large flat rectangle even though the source geometry was 3-D.
    local = rotate_y(local, -side * 0.72, [0.0, 0.08, 0.0]);
    local = rotate_z(local, side * 0.24, [0.0, 0.08, 0.0]);
    ProjectedVertex {
        screen: [
            HAND_WIDTH as f32 * 0.5 + local[0] * 104.0,
            HAND_HEIGHT as f32 - 3.0 - local[1] * 55.0,
        ],
        depth: local[2],
        uv: vertex.uv,
        world: local,
    }
}

fn sneak_pose(mut point: [f32; 3], part: u32) -> [f32; 3] {
    match part {
        0 => {
            point[1] -= 0.22;
            point[2] += 0.16;
        }
        1..=3 => {
            point = rotate_x(point, -0.38, [0.0, 1.5, 0.0]);
            point[1] -= 0.04;
            point[2] += 0.08;
        }
        4 | 5 => {
            point[1] *= 0.92;
        }
        _ => {}
    }
    point
}

fn rotate_x(mut point: [f32; 3], angle: f32, pivot: [f32; 3]) -> [f32; 3] {
    let y = point[1] - pivot[1];
    let z = point[2] - pivot[2];
    let (sin, cos) = angle.sin_cos();
    point[1] = pivot[1] + y * cos - z * sin;
    point[2] = pivot[2] + y * sin + z * cos;
    point
}

fn rotate_y(mut point: [f32; 3], angle: f32, pivot: [f32; 3]) -> [f32; 3] {
    let x = point[0] - pivot[0];
    let z = point[2] - pivot[2];
    let (sin, cos) = angle.sin_cos();
    point[0] = pivot[0] + x * cos - z * sin;
    point[2] = pivot[2] + x * sin + z * cos;
    point
}

fn rotate_z(mut point: [f32; 3], angle: f32, pivot: [f32; 3]) -> [f32; 3] {
    let x = point[0] - pivot[0];
    let y = point[1] - pivot[1];
    let (sin, cos) = angle.sin_cos();
    point[0] = pivot[0] + x * cos - y * sin;
    point[1] = pivot[1] + x * sin + y * cos;
    point
}

fn quantize_angle(value: f32) -> f32 {
    if value.is_finite() {
        (value * 4.0).round() / 4.0
    } else {
        0.0
    }
}

fn edge(a: [f32; 2], b: [f32; 2], point: [f32; 2]) -> f32 {
    (point[0] - a[0]) * (b[1] - a[1]) - (point[1] - a[1]) * (b[0] - a[0])
}

fn face_shade(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let normal = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2])
        .sqrt()
        .max(f32::EPSILON);
    let light = [0.35, 0.8, 0.5];
    let dot = (normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]) / length;
    (0.62 + dot.max(0.0) * 0.38).clamp(0.45, 1.0)
}

fn sample_skin(skin: &[u8], uv: [f32; 2]) -> Option<[u8; 4]> {
    if skin.len() != 64 * 64 * 4 || !uv.iter().all(|value| value.is_finite()) {
        return None;
    }
    let x = ((uv[0] * 64.0).floor() as i32).clamp(0, 63) as usize;
    let y = ((uv[1] * 64.0).floor() as i32).clamp(0, 63) as usize;
    let offset = (y * 64 + x) * 4;
    skin[offset..offset + 4].try_into().ok()
}
