//! One-time perspective projections of the menu cube for static UI artwork.

use super::{PanoramaFaces, PanoramaView};
use glam::Vec3;

impl PanoramaFaces {
    /// Projects a perspective view into sRGB RGBA8 pixels, using the menu shader's axes.
    /// Invalid views or sizes return no image. Call during preparation, never while painting.
    pub fn project(&self, view: PanoramaView, size: [u32; 2]) -> Option<Vec<u8>> {
        let [width, height] = size;
        if width == 0
            || height == 0
            || width > crate::MAX_UI_TEXTURE_SIDE
            || height > crate::MAX_UI_TEXTURE_SIDE
            || ![
                view.yaw_radians,
                view.pitch_radians,
                view.vertical_fov_radians,
                view.aspect,
            ]
            .iter()
            .chain(&view.tint)
            .all(|value| value.is_finite())
            || view.aspect <= 0.0
            || !(0.0..std::f32::consts::PI).contains(&view.vertical_fov_radians)
            || view.vertical_fov_radians == 0.0
        {
            return None;
        }
        let tan_half = (view.vertical_fov_radians * 0.5).tan();
        let (sy, cy) = view.yaw_radians.sin_cos();
        let (sp, cp) = view.pitch_radians.sin_cos();
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                let dx = (2.0 * (x as f32 + 0.5) / width as f32 - 1.0) * tan_half * view.aspect;
                let dy = (1.0 - 2.0 * (y as f32 + 0.5) / height as f32) * tan_half;
                let dir = Vec3::new(dx, dy * cp + sp, dy * sp - cp);
                let color = self.sample(Vec3::new(
                    dir.x * cy - dir.z * sy,
                    dir.y,
                    dir.x * sy + dir.z * cy,
                ));
                for (channel, tint) in color.into_iter().zip(view.tint) {
                    pixels.push(linear_to_srgb(
                        channel * (1.0 - view.tint[3]) + tint * view.tint[3],
                    ));
                }
                pixels.push(255);
            }
        }
        Some(pixels)
    }

    /// Bilinearly filters across face edges rather than clamping each face separately.
    fn sample(&self, dir: Vec3) -> [f32; 3] {
        static LINEAR: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
        let linear = LINEAR.get_or_init(|| {
            std::array::from_fn(|value| crate::celestial::srgb_to_linear(value as f32 / 255.0))
        });
        let (face, uv) = face_uv(dir);
        let [x, y] = uv.map(|v| (v * 0.5 + 0.5) * self.side() as f32 - 0.5);
        let (ix, iy) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - x.floor(), y - y.floor());
        let mut color = [0.0; 3];
        for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
            for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
                let texel = self.texel(face, ix + dx, iy + dy);
                for (channel, texel) in color.iter_mut().zip(texel) {
                    *channel += linear[usize::from(*texel)] * wx * wy;
                }
            }
        }
        color
    }

    /// Resolves an out-of-face filter tap on its adjacent face using the same cube axes.
    fn texel(&self, mut face: usize, x: i32, y: i32) -> &[u8] {
        let side = self.side() as i32;
        let (x, y) = if (0..side).contains(&x) && (0..side).contains(&y) {
            (x, y)
        } else {
            let uv = [x, y].map(|v| 2.0 * (v as f32 + 0.5) / side as f32 - 1.0);
            let (neighbor, uv) = face_uv(face_direction(face, uv));
            face = neighbor;
            let [x, y] = uv.map(|v| ((v * 0.5 + 0.5) * side as f32).floor() as i32);
            (x.clamp(0, side - 1), y.clamp(0, side - 1))
        };
        let offset = ((face * side as usize + y as usize) * side as usize + x as usize) * 4;
        &self.layer_major()[offset..offset + 4]
    }
}

/// Encodes filtered linear light back into the artwork's sRGB texture format.
fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

/// Selects pack-order faces and inside-facing coordinates, as in `panorama.wgsl`.
fn face_uv(dir: Vec3) -> (usize, [f32; 2]) {
    let a = dir.abs();
    if a.y >= a.x && a.y >= a.z {
        if dir.y > 0.0 {
            (4, [dir.x / a.y, -dir.z / a.y])
        } else {
            (5, [dir.x / a.y, dir.z / a.y])
        }
    } else if a.x >= a.z {
        if dir.x > 0.0 {
            (1, [dir.z / a.x, -dir.y / a.x])
        } else {
            (3, [-dir.z / a.x, -dir.y / a.x])
        }
    } else if dir.z < 0.0 {
        (0, [dir.x / a.z, -dir.y / a.z])
    } else {
        (2, [-dir.x / a.z, -dir.y / a.z])
    }
}

/// Converts a face coordinate back to a ray, including taps just outside its edges.
fn face_direction(face: usize, [u, v]: [f32; 2]) -> Vec3 {
    match face {
        0 => Vec3::new(u, -v, -1.0),
        1 => Vec3::new(1.0, -v, u),
        2 => Vec3::new(-u, -v, 1.0),
        3 => Vec3::new(-1.0, -v, -u),
        4 => Vec3::new(u, 1.0, -v),
        5 => Vec3::new(u, -1.0, v),
        _ => unreachable!("cube face"),
    }
}

#[cfg(test)]
mod tests;
