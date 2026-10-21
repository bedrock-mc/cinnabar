//! The block selection outline, block cracks and lightning bolts.
//!
//! Iris widens Java's line geometry in the vertex stage it injects into `gbuffers_line`
//! (`iris_widen_lines`): each segment end moves `LineWidth / 2` pixels to either side of
//! the segment's screen direction. A translated program has no such stage, so the host
//! widens on the CPU to the same screen-space quads, with Java's block-outline width.

use super::frame::EffectCamera;
use super::vertices::{FULL_BRIGHT, Mesh, Vertex};
use crate::shaderpack::uniforms::GL_NEAR;
use bevy::math::{Vec2, Vec3, Vec3Swizzles};
use render_model::BlockEntityVertex;
use render_scene::BoltRecord;
use std::collections::HashSet;

/// Java's block outline colour (`0x66000000`).
const OUTLINE_COLOUR: [f32; 4] = [0.0, 0.0, 0.0, 0.4];
/// Cinnabar's bolt colour (`lightning.wesl`).
pub(super) const BOLT_COLOUR: [f32; 3] = [0.85, 0.9, 1.0];

/// Java's line width of the block outline: 2.5 pixels, scaled with windows wider than 1920.
pub(super) fn outline_width(screen_width: u32) -> f32 {
    (screen_width as f32 / 1920.0 * 2.5).max(2.5)
}

/// The segment ends in view space, clipped to the near plane; `None` when it is behind.
fn clip_to_view(camera: &EffectCamera, from: Vec3, to: Vec3) -> Option<(Vec3, Vec3)> {
    let view = |p: Vec3| camera.model_view.transform_point3(p);
    let (mut a, mut b) = (view(from), view(to));
    if a.z > -GL_NEAR && b.z > -GL_NEAR {
        return None;
    }
    let clip = |inside: Vec3, outside: Vec3| {
        let t = (-GL_NEAR - inside.z) / (outside.z - inside.z);
        inside.lerp(outside, t)
    };
    if a.z > -GL_NEAR {
        a = clip(b, a);
    } else if b.z > -GL_NEAR {
        b = clip(a, b);
    }
    Some((a, b))
}

/// A screen-space line `width` pixels wide from `from` to `to` (camera-relative world), as a
/// quad; `None` when the segment is behind the camera or degenerate on screen.
pub(super) fn widen(camera: &EffectCamera, from: Vec3, to: Vec3, width: f32) -> Option<[Vec3; 4]> {
    let (a, b) = clip_to_view(camera, from, to)?;
    let [fx, fy] = camera.focal;
    let [sw, sh] = camera.screen.map(|s| s.max(1) as f32);
    // Pixels per view unit at depth 1 along each axis.
    let scale = Vec2::new(fx * sw * 0.5, fy * sh * 0.5);
    let screen = |p: Vec3| p.xy() / -p.z * scale;
    let direction = (screen(b) - screen(a)).try_normalize()?;
    let mut offset = Vec2::new(-direction.y, direction.x) * (width * 0.5);
    if offset.x < 0.0 {
        offset = -offset;
    }
    let world = camera.model_view.inverse();
    let shift = |p: Vec3, sign: f32| {
        let view = p + (offset * sign * -p.z / scale).extend(0.0);
        world.transform_point3(view)
    };
    Some([shift(a, 1.0), shift(a, -1.0), shift(b, -1.0), shift(b, 1.0)])
}

/// Appends a widened selection segment when its visible projection is nondegenerate.
fn line(mesh: &mut Mesh, camera: &EffectCamera, from: Vec3, to: Vec3, width: f32) {
    let normal = (to - from).normalize_or_zero();
    if let Some(corners) = widen(camera, from, to, width) {
        mesh.quad(corners.map(|p| Vertex::plain(p, OUTLINE_COLOUR, normal)));
    }
}

/// The selection's outline: the wire mode's endpoint pairs, or in highlight mode the boundary
/// edges of the highlighted faces, each once (Java always outlines the selected shape).
pub(super) fn outline(
    mesh: &mut Mesh,
    camera: &EffectCamera,
    wire: &[BlockEntityVertex],
    highlight: &[BlockEntityVertex],
) {
    let width = outline_width(camera.screen[0]);
    let point = |v: &BlockEntityVertex| Vec3::from_array(v.position) - camera.position;
    if !wire.is_empty() {
        for pair in wire.as_chunks::<2>().0 {
            line(mesh, camera, point(&pair[0]), point(&pair[1]), width);
        }
        return;
    }
    // Highlight faces are lifted off the block, so neighbouring faces' shared edges differ
    // by that lift; a 1/64-block key merges them.
    let mut drawn = HashSet::new();
    for face in highlight.as_chunks::<6>().0 {
        for (from, to) in boundary_edges(face) {
            if drawn.insert(edge_key(&from, &to, 64.0)) {
                line(mesh, camera, point(&from), point(&to), width);
            }
        }
    }
}

type EdgeKey = ([i64; 3], [i64; 3]);

/// An undirected edge's key at `resolution` steps per block.
fn edge_key(a: &BlockEntityVertex, b: &BlockEntityVertex, resolution: f32) -> EdgeKey {
    let key = |v: &BlockEntityVertex| v.position.map(|c| (c * resolution).round() as i64);
    let (ka, kb) = (key(a), key(b));
    if ka <= kb { (ka, kb) } else { (kb, ka) }
}

/// The edges of a two-triangle face that are not its shared diagonal, in triangle order.
fn boundary_edges(face: &[BlockEntityVertex]) -> Vec<(BlockEntityVertex, BlockEntityVertex)> {
    let mut edges: Vec<(EdgeKey, BlockEntityVertex, BlockEntityVertex)> = vec![];
    for triangle in face.as_chunks::<3>().0 {
        for i in 0..3 {
            let (a, b) = (triangle[i], triangle[(i + 1) % 3]);
            edges.push((edge_key(&a, &b, 4096.0), a, b));
        }
    }
    let count = |key: &EdgeKey| edges.iter().filter(|(k, ..)| k == key).count();
    edges
        .iter()
        .filter(|(key, ..)| count(key) == 1)
        .map(|(_, a, b)| (*a, *b))
        .collect()
}

/// Crack faces: Cinnabar's crack triangles with their destroy-stage atlas coordinates,
/// full-bright as Iris's `CRUMBLING` key.
pub(super) fn cracks(mesh: &mut Mesh, vertices: &[BlockEntityVertex], camera: Vec3) {
    for face in vertices.chunks(6) {
        let mut low = [f32::INFINITY; 2];
        let mut high = [f32::NEG_INFINITY; 2];
        for vertex in face {
            for axis in 0..2 {
                low[axis] = low[axis].min(vertex.uv[axis]);
                high[axis] = high[axis].max(vertex.uv[axis]);
            }
        }
        let mid = std::array::from_fn(|axis| (low[axis] + high[axis]) * 0.5);
        for triangle in face.as_chunks::<3>().0 {
            mesh.triangle([0, 1, 2].map(|i| {
                let v = &triangle[i];
                Vertex {
                    position: Vec3::from_array(v.position) - camera,
                    colour: v.color,
                    normal: Vec3::from_array(v.normal),
                    uv: v.uv,
                    mid_uv: mid,
                    light: FULL_BRIGHT,
                }
            }));
        }
    }
}

/// Bolt ribbons facing the camera, bright along their middle and fading to the edges as
/// `lightning.wesl` shades them.
pub(super) fn lightning(mesh: &mut Mesh, records: &[BoltRecord], camera: Vec3) {
    for bolt in records {
        let start = Vec3::from_array(bolt.start) - camera;
        let end = Vec3::from_array(bolt.end) - camera;
        let middle = (start + end) * 0.5;
        let side = ((end - start).cross(-middle) + Vec3::new(1.0e-6, 0.0, 0.0)).normalize_or_zero()
            * bolt.half_width;
        let normal = -middle.normalize_or(Vec3::Z);
        let core = BOLT_COLOUR.map(|c| c * bolt.intensity);
        let bright = [core[0], core[1], core[2], 1.0];
        let dark = [0.0, 0.0, 0.0, 1.0];
        let vertex = |p: Vec3, colour| Vertex::plain(p, colour, normal);
        for sign in [-1.0, 1.0] {
            mesh.quad([
                vertex(start, bright),
                vertex(start + side * sign, dark),
                vertex(end + side * sign, dark),
                vertex(end, bright),
            ]);
        }
    }
}
