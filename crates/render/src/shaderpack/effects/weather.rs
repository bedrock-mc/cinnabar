//! Rain and snow for packs. Rain is Java's `WeatherEffectRenderer` columns: one 1-block quad
//! per column within [`RAIN_RADIUS`] of the camera, turned to face it, from the column's
//! surface (or `RAIN_RADIUS` below the camera) to `RAIN_RADIUS` above, scrolling down the
//! rain sheet ([`rain_sheet`]), which spans the quad's width and four blocks of its height as
//! `rain.png` does. Packs displace and slant these quads (Photon's `SLANTED_RAIN`), so their
//! shape matters. Snow keeps Cinnabar's precipitation layers (`PrecipitationScene`), placed as
//! `weather.wesl` places them: each layer wraps the shared particle mesh in a 30-block box
//! ahead of the camera and stretches every particle along its velocity. Vanilla's streak
//! width is a constant `dimensions.x / 1.778` blocks across the view; the vanilla fragment
//! shader's column occlusion is applied here per streak, cutting it at the column's surface.

use super::stars::JavaRandom;
use super::vertices::{Mesh, Vertex};
use crate::weather_render::PARTICLE_MESH_SEED;
use bevy::math::Vec3;
use render_model::{
    OCCLUSION_BLOCKED, OCCLUSION_SIDE, OcclusionGrid, PARTICLE_BOX, PARTICLE_POOL,
    PrecipitationLayerRecord, particle_mesh,
};
use std::sync::LazyLock;

/// Projection y-scale the vanilla widths are authored against (`weather.wesl`).
pub(super) const REFERENCE_PROJECTION_SCALE: f32 = 1.778_341;
/// Weather is lit as an open-sky column: sky 15, block 0.
const LIGHT: [f32; 2] = [0.0, 240.0];
/// `PrecipitationLayerRecord::kind` of rain.
pub(super) const RAIN_KIND: u32 = 0;
/// Java's rain radius with fancy graphics, in columns.
const RAIN_RADIUS: i32 = 10;
/// Blocks of column per repeat of the rain sheet (Java's `v = y / 4`).
const RAIN_SHEET_BLOCKS: f32 = 4.0;
/// Size of the rain sheet, Java's `rain.png`: a texel is 1/64 block across a column.
pub(super) const RAIN_SHEET_SIZE: [u32; 2] = [64, 256];
/// Streaks scattered over the rain sheet, about as many streak texels as `rain.png` holds.
const RAIN_SHEET_STREAKS: u32 = 40;
/// Seed of the streak placement on the rain sheet.
const RAIN_SHEET_SEED: i64 = 0x5241_494E;

static PARTICLES: LazyLock<Vec<[f32; 4]>> = LazyLock::new(|| particle_mesh(PARTICLE_MESH_SEED));

fn wrap_box(value: Vec3) -> Vec3 {
    value - PARTICLE_BOX * (value / PARTICLE_BOX).trunc()
}

/// The height below which `kind` does not fall at world column (`x`, `z`), or `None` when it
/// falls at any height; `Some(i32::MAX)` where it never shows.
fn surface(grid: &OcclusionGrid, kind: u32, position: Vec3) -> Option<i32> {
    let cell = [
        position.x.floor() as i32 - grid.origin[0],
        position.z.floor() as i32 - grid.origin[1],
    ];
    if cell.iter().any(|c| !(0..OCCLUSION_SIDE).contains(c)) {
        return None;
    }
    let plane = (OCCLUSION_SIDE * OCCLUSION_SIDE) as usize;
    let index = kind as usize * plane + (cell[1] * OCCLUSION_SIDE + cell[0]) as usize;
    grid.heights.get(index).copied()
}

/// The rain sheet packs sample as `gtexture`, [`RAIN_SHEET_SIZE`] RGBA texels. Java's
/// `rain.png` is a sparse field of thin streaks that each column samples across its whole
/// width, while the Bedrock weather sheet (`size`, `rgba`) holds one streak per sprite cell:
/// sampled across a column, one cell draws a band a quarter of a block wide. This scatters
/// the cells of `rect` (sprites side by side along `u`, as in
/// [`render_model::PrecipitationParams::uv_rect`]) over the sheet at fixed places, wrapping
/// at its edges so it tiles, and keeps the more opaque texel where streaks overlap.
pub(super) fn rain_sheet(size: [u32; 2], rgba: &[u8], rect: [f32; 4]) -> Vec<u8> {
    let [width, height] = RAIN_SHEET_SIZE;
    let mut sheet = vec![0; (width * height * 4) as usize];
    let texels = |fraction: f32, side: u32| (fraction * side as f32).round().max(0.0) as u32;
    let (x0, y0) = (texels(rect[0], size[0]), texels(rect[1], size[1]));
    let (cell_width, cell_height) = (texels(rect[2], size[0]), texels(rect[3], size[1]));
    let cells = size[0]
        .saturating_sub(x0)
        .checked_div(cell_width)
        .unwrap_or(0);
    if cells == 0 {
        return sheet;
    }
    let mut random = JavaRandom::new(RAIN_SHEET_SEED);
    for streak in 0..RAIN_SHEET_STREAKS {
        let left = x0 + streak % cells * cell_width;
        let at = [width, height].map(|side| (random.next_float() * side as f32) as u32);
        for y in 0..cell_height {
            for x in 0..cell_width {
                let source = (((y0 + y) * size[0] + left + x) * 4) as usize;
                let Some(texel) = rgba.get(source..source + 4) else {
                    continue;
                };
                let target = ((((at[1] + y) % height) * width + (at[0] + x) % width) * 4) as usize;
                if texel[3] > sheet[target + 3] {
                    sheet[target..target + 4].copy_from_slice(texel);
                }
            }
        }
    }
    sheet
}

/// Appends Java's rain columns around `camera` over the rain sheet ([`rain_sheet`]): `level`
/// is the rain level and `ticks` the game time. Each column's `v` runs over whole repeats of
/// the sheet, so a quad splits where the sheet wraps and every coordinate stays in `0..=1`.
pub(super) fn rain_columns(
    mesh: &mut Mesh,
    grid: &OcclusionGrid,
    camera: Vec3,
    level: f32,
    ticks: f32,
) {
    if level <= 0.0 || !level.is_finite() || !ticks.is_finite() {
        return;
    }
    let column = |axis: f32| axis.floor() as i32;
    let (cam_x, cam_y, cam_z) = (column(camera.x), column(camera.y), column(camera.z));
    let whole_ticks = ticks.floor();
    let (wrapped, partial) = ((whole_ticks as i64 & 131_071) as f32, ticks - whole_ticks);
    let radius_squared = (RAIN_RADIUS * RAIN_RADIUS) as f32;
    for z in cam_z - RAIN_RADIUS..=cam_z + RAIN_RADIUS {
        for x in cam_x - RAIN_RADIUS..=cam_x + RAIN_RADIUS {
            let (dx, dz) = (x - cam_x, z - cam_z);
            // Java's width table divides by zero in the camera's own column, which draws
            // nothing.
            if dx == 0 && dz == 0 {
                continue;
            }
            let terrain = match surface(grid, RAIN_KIND, Vec3::new(x as f32, 0.0, z as f32)) {
                Some(OCCLUSION_BLOCKED) => continue,
                Some(height) => height,
                None => i32::MIN,
            };
            let bottom = (cam_y - RAIN_RADIUS).max(terrain);
            let top = (cam_y + RAIN_RADIUS).max(terrain);
            if top == bottom {
                continue;
            }
            let x_term = x
                .wrapping_mul(x)
                .wrapping_mul(3121)
                .wrapping_add(x.wrapping_mul(45_238_971));
            let z_term = z
                .wrapping_mul(z)
                .wrapping_mul(418_711)
                .wrapping_add(z.wrapping_mul(13_761));
            let speed = 3.0 + JavaRandom::new(i64::from(x_term ^ z_term)).next_float();
            let offset = (x_term.wrapping_add(z_term) & 0xFF) as f32;
            let v_offset = (-(wrapped + offset + partial) / 32.0 * speed) % 32.0;
            let relative = Vec3::new(x as f32 + 0.5, 0.0, z as f32 + 0.5) - camera;
            let distance_squared = relative.x * relative.x + relative.z * relative.z;
            let alpha = (1.0 - 0.5 * (distance_squared / radius_squared).min(1.0)) * level;
            let length = ((dx * dx + dz * dz) as f32).sqrt();
            let half = Vec3::new(-dz as f32 / length, 0.0, dx as f32 / length) * 0.5;
            let normal = -Vec3::new(dx as f32, 0.0, dz as f32) / length;
            // Java maps the top of the column to `v = bottom / 4` and the bottom to `top / 4`.
            // f64 keeps whole sheet repeats distinct at any finite height; only the small
            // camera-relative results narrow to f32.
            let blocks = f64::from(RAIN_SHEET_BLOCKS);
            let (top_y, bottom_y) = (f64::from(top), f64::from(bottom));
            let v_top = bottom_y / blocks + f64::from(v_offset);
            let v_bottom = top_y / blocks + f64::from(v_offset);
            let y_at = |v: f64| (top_y - (v - v_top) * blocks - f64::from(camera.y)) as f32;
            let mut start = v_top;
            while start < v_bottom {
                let repeat = start.floor();
                let end = (repeat + 1.0).min(v_bottom);
                let corner = |side: f32, v: f64| Vertex {
                    position: Vec3::new(relative.x, y_at(v), relative.z) + half * side,
                    colour: [1.0, 1.0, 1.0, alpha],
                    normal,
                    uv: [(side + 1.0) * 0.5, (v - repeat) as f32],
                    mid_uv: [0.5; 2],
                    light: LIGHT,
                };
                mesh.quad([
                    corner(-1.0, start),
                    corner(1.0, start),
                    corner(1.0, end),
                    corner(-1.0, end),
                ]);
                start = end;
            }
        }
    }
}

/// Appends every snow layer's streaks relative to `camera`; rain draws as [`rain_columns`].
pub(super) fn weather_quads(
    mesh: &mut Mesh,
    layers: &[PrecipitationLayerRecord],
    forward_offset: [f32; 3],
    grid: &OcclusionGrid,
    camera: Vec3,
) {
    let particles = PARTICLES.as_slice();
    if particles.is_empty() {
        return;
    }
    let forward = Vec3::from_array(forward_offset);
    for layer in layers.iter().filter(|layer| layer.kind != RAIN_KIND) {
        let base = Vec3::from_slice(&layer.base_offset[..3]);
        let velocity = Vec3::from_slice(&layer.velocity[..3]);
        let [u0, v0, du, dv] = layer.uv_rect;
        let [width, length] = layer.dimensions;
        let half = width / REFERENCE_PROJECTION_SCALE * 0.5;
        for quad in 0..layer.particle_count {
            let index = ((layer.first_particle + quad) % PARTICLE_POOL) as usize;
            let Some(particle) = particles.get(index) else {
                continue;
            };
            let relative = wrap_box(Vec3::new(particle[0], particle[1], particle[2]) + base)
                - Vec3::splat(PARTICLE_BOX * 0.5)
                + forward;
            // `head` is the particle (`v` 1), `tail` its trail along the velocity (`v` 0).
            let mut head = relative;
            let mut tail = relative + velocity * length;
            let (mut head_v, mut tail_v) = (1.0, 0.0);
            let lower = if head.y < tail.y { head } else { tail };
            match surface(grid, layer.kind, lower + camera) {
                Some(OCCLUSION_BLOCKED) => continue,
                Some(height) => {
                    let floor = height as f32 + 0.5 - camera.y;
                    if head.y < floor && tail.y < floor {
                        continue;
                    }
                    let t = |from: Vec3, to: Vec3| (floor - from.y) / (to.y - from.y);
                    if head.y < floor {
                        let s = t(head, tail);
                        head = head.lerp(tail, s);
                        head_v = 1.0 - s;
                    } else if tail.y < floor {
                        let s = t(tail, head);
                        tail = tail.lerp(head, s);
                        tail_v = s;
                    }
                }
                None => {}
            }
            let across = (tail - head).cross(head).normalize_or(Vec3::X) * half;
            let u = u0 + particle[3] * du;
            let normal = -head.normalize_or(Vec3::Z);
            let corner = |position: Vec3, x: f32, y: f32| Vertex {
                position,
                colour: [1.0; 4],
                normal,
                uv: [u + x * du, v0 + y * dv],
                mid_uv: [u + 0.5 * du, v0 + 0.5 * dv],
                light: LIGHT,
            };
            mesh.quad([
                corner(tail + across, 0.0, tail_v),
                corner(tail - across, 1.0, tail_v),
                corner(head - across, 1.0, head_v),
                corner(head + across, 0.0, head_v),
            ]);
        }
    }
}
