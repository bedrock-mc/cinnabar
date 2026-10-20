//! Clouds from Cinnabar's cloud mesher (`mesh_cloud_viewport`, the window the vanilla cloud
//! renderer draws): one 16-block cell per occupied `clouds.png` texel, scrolled with the
//! renderer's cloud offset and coloured by the baked face shade times the frame's cloud
//! colour. Fancy draws the cuboids (the cap facing the camera and open sides); fast draws a
//! flat layer at the cloud base, visible from both sides. Source RGBA is baked into vertex
//! colours; the draw binds white so textured fallback programs apply it only once.

use super::vertices::{FULL_BRIGHT, Mesh, Vertex};
use assets::AtmosphereTexture;
use bevy::math::{DVec3, Vec3};
use meshing::cloud::{
    CLOUD_CELL_BLOCKS, CLOUD_MASK_SIZE, CLOUD_TOP_Y, CLOUD_UNDERSIDE_Y, CloudFace,
};
use meshing::cloud_viewport::{CloudViewport, ViewportCloudQuad, mesh_cloud_viewport};

/// Cells per side of the meshed window, as the vanilla cloud renderer's fancy config.
const MESH_SIZE: u16 = 64;

/// The meshed window a view draws, kept until the camera moves far enough.
#[derive(Default)]
pub(super) struct CloudCache {
    key: Option<([u8; 32], bool)>,
    viewport: Option<CloudViewport>,
    quads: Vec<ViewportCloudQuad>,
}

/// What the clouds depend on this frame.
pub(super) struct CloudFrame<'a> {
    pub texture: &'a AtmosphereTexture,
    /// Identity of the atmosphere textures, which re-meshes on resource-pack changes.
    pub identity: [u8; 32],
    /// Blocks the clouds have scrolled along +X.
    pub scroll: f32,
    /// Gamma RGBA the baked colours are multiplied by.
    pub colour: [f32; 4],
    pub fast: bool,
}

impl CloudCache {
    /// Reuses the meshed viewport until its texture, mode or camera window changes.
    fn quads(&mut self, frame: &CloudFrame<'_>, camera: DVec3) -> &[ViewportCloudQuad] {
        let above = !frame.fast && camera.y >= f64::from(CLOUD_UNDERSIDE_Y.ceil());
        let Some(viewport) = CloudViewport::try_new(
            [camera.x + f64::from(frame.scroll), camera.z],
            MESH_SIZE,
            1,
            above,
            frame.fast,
        ) else {
            self.quads.clear();
            self.viewport = None;
            return &self.quads;
        };
        let key = Some((frame.identity, frame.fast));
        let stale = self.key != key
            || self
                .viewport
                .is_none_or(|current| current.needs_rebuild(viewport));
        if stale {
            self.quads = mesh_cloud_viewport(frame.texture, viewport).unwrap_or_default();
            if frame.fast {
                self.quads
                    .retain(|q| q.face == CloudFace::Down as u32 || q.face == CloudFace::Up as u32);
            }
            self.key = key;
            self.viewport = Some(viewport);
        }
        &self.quads
    }
}

/// The corner `(u, v)` a face's quad vertex sits at, in `cloud.wesl`'s winding.
fn face_corner(face: CloudFace, corner: [f32; 2]) -> [f32; 2] {
    match face {
        CloudFace::Down => [1.0 - corner[1], corner[0]],
        CloudFace::Up => [corner[1], corner[0]],
        CloudFace::North | CloudFace::East => [corner[0], 1.0 - corner[1]],
        CloudFace::South | CloudFace::West => corner,
    }
}

/// Returns the outward unit normal of a cloud face.
fn normal(face: CloudFace) -> Vec3 {
    match face {
        CloudFace::Down => Vec3::NEG_Y,
        CloudFace::Up => Vec3::Y,
        CloudFace::North => Vec3::NEG_Z,
        CloudFace::South => Vec3::Z,
        CloudFace::West => Vec3::NEG_X,
        CloudFace::East => Vec3::X,
    }
}

/// Appends this frame's clouds relative to `camera`.
pub(super) fn cloud_quads(
    mesh: &mut Mesh,
    cache: &mut CloudCache,
    frame: &CloudFrame<'_>,
    camera: DVec3,
) {
    let (lower, upper) = (f64::from(CLOUD_UNDERSIDE_Y), f64::from(CLOUD_TOP_Y));
    let cell_blocks = f64::from(CLOUD_CELL_BLOCKS);
    let scroll = f64::from(frame.scroll);
    for quad in cache.quads(frame, camera) {
        let Some(face) = CloudFace::from_u32(quad.face) else {
            continue;
        };
        let [r, g, b, a] = quad.colour.to_le_bytes().map(|c| f32::from(c) / 255.0);
        let colour = [
            r * frame.colour[0],
            g * frame.colour[1],
            b * frame.colour[2],
            a * frame.colour[3],
        ];
        let side = CLOUD_MASK_SIZE as i32;
        let texel = [
            (quad.cell[0].rem_euclid(side) as f32 + 0.5) / side as f32,
            (quad.cell[1].rem_euclid(side) as f32 + 0.5) / side as f32,
        ];
        let cell = [f64::from(quad.cell[0]), f64::from(quad.cell[1])];
        let corners = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]].map(|corner| {
            let [u, v] = face_corner(face, corner).map(f64::from);
            let y = lower + (upper - lower) * v;
            let local = match face {
                CloudFace::Down => [cell[0] + u, lower, cell[1] + v],
                CloudFace::Up if frame.fast => [cell[0] + u, lower, cell[1] + v],
                CloudFace::Up => [cell[0] + u, upper, cell[1] + v],
                CloudFace::North => [cell[0] + u, y, cell[1]],
                CloudFace::South => [cell[0] + u, y, cell[1] + 1.0],
                CloudFace::West => [cell[0], y, cell[1] + u],
                CloudFace::East => [cell[0] + 1.0, y, cell[1] + u],
            };
            let world = DVec3::new(
                local[0] * cell_blocks - scroll,
                local[1],
                local[2] * cell_blocks,
            );
            Vertex {
                position: (world - camera).as_vec3(),
                colour,
                normal: normal(face),
                uv: texel,
                mid_uv: texel,
                light: FULL_BRIGHT,
            }
        });
        mesh.quad(corners);
    }
}
