//! Shared offline rasterization for first-person pose and session-item witnesses.

use bevy::math::{Mat4, Vec3, Vec4};
use render_model::ActorRigVertex;
#[cfg(test)]
use render_model::RenderBoneTransform;

pub(crate) const WIDTH: usize = 854;
pub(crate) const HEIGHT: usize = 480;

pub(crate) struct Image {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) rgba8: Vec<u8>,
}

impl Image {
    /// Loads a local texture, skipping an absent fixture while rejecting malformed images.
    #[cfg(test)]
    pub(crate) fn load(path: &std::path::Path) -> Option<Self> {
        let image = match image::open(path) {
            Ok(image) => image.to_rgba8(),
            Err(image::ImageError::IoError(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                eprintln!("skipping frame fixture: missing {}", path.display());
                return None;
            }
            Err(error) => panic!("{}: {error}", path.display()),
        };
        Some(Self {
            width: image.width() as usize,
            height: image.height() as usize,
            rgba8: image.into_raw(),
        })
    }

    /// Samples normalized atlas coordinates with nearest-neighbor filtering.
    fn sample(&self, [u, v]: [f32; 2]) -> [u8; 4] {
        let x = ((u * self.width as f32) as usize).min(self.width - 1);
        let y = ((v * self.height as f32) as usize).min(self.height - 1);
        let offset = (y * self.width + x) * 4;
        std::array::from_fn(|channel| self.rgba8[offset + channel])
    }
}

pub(crate) struct Target {
    pub(crate) color: Vec<u8>,
    pub(crate) depth: Vec<f32>,
}

/// One camera-space vertex with both face UVs.
#[derive(Clone, Copy)]
struct Placed {
    view: Vec3,
    uv: [f32; 2],
    back_uv: [f32; 2],
}

/// Projects camera-space coordinates using the production hand field of view.
fn project(view: Vec3) -> Option<Vec3> {
    if view.z > -0.025 {
        return None;
    }
    let focal = 1.0 / (crate::actor_publication::HAND_FOV_DEGREES.to_radians() * 0.5).tan();
    let aspect = WIDTH as f32 / HEIGHT as f32;
    let (x, y) = (focal / aspect * view.x / -view.z, focal * view.y / -view.z);
    Some(Vec3::new(
        (x + 1.0) * 0.5 * WIDTH as f32,
        (1.0 - y) * 0.5 * HEIGHT as f32,
        -view.z,
    ))
}

/// Rasterizes one posed triangle with perspective-correct UVs, depth, and alpha testing.
fn draw(target: &mut Target, triangle: [Placed; 3], texture: &Image) {
    let (Some(a), Some(b), Some(c)) = (
        project(triangle[0].view),
        project(triangle[1].view),
        project(triangle[2].view),
    ) else {
        return;
    };
    let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if area.abs() < 1.0e-6 {
        return;
    }
    // Screen Y points down, so a counter-clockwise (front) face has negative area here.
    let front = area < 0.0;
    let normal = (triangle[1].view - triangle[0].view)
        .cross(triangle[2].view - triangle[0].view)
        .normalize_or_zero();
    let shade = 0.55 + 0.45 * normal.dot(Vec3::new(0.3, 0.8, 0.5).normalize()).abs();
    let (min_x, max_x) = (a.x.min(b.x).min(c.x).max(0.0), a.x.max(b.x).max(c.x));
    let (min_y, max_y) = (a.y.min(b.y).min(c.y).max(0.0), a.y.max(b.y).max(c.y));
    for y in min_y as usize..(max_y.ceil() as usize).min(HEIGHT) {
        for x in min_x as usize..(max_x.ceil() as usize).min(WIDTH) {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((b.x - px) * (c.y - py) - (b.y - py) * (c.x - px)) / area;
            let w1 = ((c.x - px) * (a.y - py) - (c.y - py) * (a.x - px)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let inverse = w0 / a.z + w1 / b.z + w2 / c.z;
            let depth = 1.0 / inverse;
            let index = y * WIDTH + x;
            if depth >= target.depth[index] {
                continue;
            }
            // A one-sided face's back is discarded, as both actor shaders do.
            if !front && triangle[0].back_uv[0] < -1.0e8 {
                continue;
            }
            let uv_of = |vertex: &Placed| if front { vertex.uv } else { vertex.back_uv };
            let uv: [f32; 2] = std::array::from_fn(|axis| {
                (uv_of(&triangle[0])[axis] * w0 / a.z
                    + uv_of(&triangle[1])[axis] * w1 / b.z
                    + uv_of(&triangle[2])[axis] * w2 / c.z)
                    * depth
            });
            let texel = texture.sample(uv);
            if texel[3] < 26 {
                continue;
            }
            target.depth[index] = depth;
            for (pixel, value) in target.color[index * 4..index * 4 + 3].iter_mut().zip(texel) {
                *pixel = (f32::from(value) * shade) as u8;
            }
        }
    }
}

/// Transforms the supplied rig vertices and draws their atlas-backed triangles.
pub(crate) fn draw_mesh(
    target: &mut Target,
    vertices: &[ActorRigVertex],
    to_view: impl Fn(&ActorRigVertex) -> Vec3,
    texture: &Image,
) {
    for triangle in vertices.as_chunks::<3>().0 {
        let placed = std::array::from_fn(|corner| Placed {
            view: to_view(&triangle[corner]),
            uv: triangle[corner].uv,
            back_uv: triangle[corner].back_uv,
        });
        draw(target, placed, texture);
    }
}

/// Restores the homogeneous transform represented by the rig frame's three rows.
pub(crate) fn rows_to_mat(rows: [[f32; 4]; 3]) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(rows[0][0], rows[1][0], rows[2][0], 0.0),
        Vec4::new(rows[0][1], rows[1][1], rows[2][1], 0.0),
        Vec4::new(rows[0][2], rows[1][2], rows[2][2], 0.0),
        Vec4::new(rows[0][3], rows[1][3], rows[2][3], 1.0),
    )
}

#[cfg(test)]
/// Converts a sampled bone into the matrix used by the offline pose witness.
pub(crate) fn bone_mat(bone: RenderBoneTransform) -> Mat4 {
    let [x, y, z, w] = bone.rotation;
    let scale = Vec3::from_slice(&bone.axis_scale[..3]) * bone.translation_scale[3];
    Mat4::from_scale_rotation_translation(
        scale,
        bevy::math::Quat::from_xyzw(x, y, z, w).normalize(),
        Vec3::from_slice(&bone.translation_scale[..3]),
    )
}
