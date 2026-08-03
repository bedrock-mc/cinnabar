//! Small software-rendered player preview used by the gameplay HUD.
//!
//! The preview is deliberately generated from the same standard-biped vertex
//! and UV contract used by the 3-D actor renderer. It is cached by the caller
//! and uploaded as one UI texture layer only when the authoritative skin
//! changes, so it does not add a per-frame GPU upload or a second world camera.

use render::{ActorVertex, standard_biped_vertices};

pub(crate) const PREVIEW_WIDTH: u32 = 96;
pub(crate) const PREVIEW_HEIGHT: u32 = 112;

const MODEL_SCALE: f32 = 48.0;
const MODEL_YAW_RADIANS: f32 = -0.38;
const MODEL_BOTTOM: f32 = 106.0;

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
pub(crate) fn render(skin: &[u8]) -> Vec<u8> {
    let width = PREVIEW_WIDTH as usize;
    let height = PREVIEW_HEIGHT as usize;
    let mut pixels = vec![0u8; width * height * 4];
    let mut depth = vec![f32::NEG_INFINITY; width * height];
    let vertices = standard_biped_vertices();
    let (sin_yaw, cos_yaw) = MODEL_YAW_RADIANS.sin_cos();

    for triangle in vertices.chunks_exact(3) {
        let projected = [
            project(triangle[0], sin_yaw, cos_yaw),
            project(triangle[1], sin_yaw, cos_yaw),
            project(triangle[2], sin_yaw, cos_yaw),
        ];
        let area = edge(
            projected[0].screen,
            projected[1].screen,
            projected[2].screen,
        );
        if !area.is_finite() || area.abs() < f32::EPSILON {
            continue;
        }
        let min_x = projected
            .iter()
            .map(|vertex| vertex.screen[0].floor() as i32)
            .min()
            .unwrap_or_default()
            .clamp(0, PREVIEW_WIDTH as i32 - 1);
        let max_x = projected
            .iter()
            .map(|vertex| vertex.screen[0].ceil() as i32)
            .max()
            .unwrap_or_default()
            .clamp(0, PREVIEW_WIDTH as i32 - 1);
        let min_y = projected
            .iter()
            .map(|vertex| vertex.screen[1].floor() as i32)
            .min()
            .unwrap_or_default()
            .clamp(0, PREVIEW_HEIGHT as i32 - 1);
        let max_y = projected
            .iter()
            .map(|vertex| vertex.screen[1].ceil() as i32)
            .max()
            .unwrap_or_default()
            .clamp(0, PREVIEW_HEIGHT as i32 - 1);
        if min_x > max_x || min_y > max_y {
            continue;
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
    pixels
}

fn project(vertex: ActorVertex, sin_yaw: f32, cos_yaw: f32) -> ProjectedVertex {
    let [x, y, z] = vertex.position;
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
