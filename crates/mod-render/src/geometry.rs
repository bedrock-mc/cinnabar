//! Camera-independent triangle lists; the vertex shader expands strips and billboards to face
//! the viewer, so geometry is rebuilt only when a mod commits new primitives.

use crate::{BillboardPattern, DecalStyle, Primitives, Vec3};
use mod_api::{
    MAX_RENDER_BEAMS, MAX_RENDER_BILLBOARDS, MAX_RENDER_DECALS, MAX_RENDER_RIBBONS,
    MAX_RIBBON_POINTS,
};

pub const KIND_GROUND: f32 = 0.0;
pub const KIND_STRIP: f32 = 1.0;
pub const KIND_BILLBOARD: f32 = 2.0;
pub const KIND_UPRIGHT: f32 = 3.0;
pub const STYLE_RIBBON: f32 = 10.0;
pub const STYLE_BEAM: f32 = 11.0;
/// Billboard style ids start here, offset by pattern.
pub const STYLE_BILLBOARD: f32 = 20.0;
/// Lifts decals off the ground they are placed on, avoiding depth fighting.
pub const DECAL_LIFT_BLOCKS: f32 = 0.03;
/// Ribbon tails narrow to this fraction of the head width.
pub const RIBBON_TAIL_WIDTH: f32 = 0.2;
pub const MAX_VERTICES: usize = (MAX_RENDER_DECALS
    + MAX_RENDER_RIBBONS * (MAX_RIBBON_POINTS - 1)
    + MAX_RENDER_BEAMS
    + MAX_RENDER_BILLBOARDS)
    * 6;

/// Mirrors `ModVertex` in the renderer's world-primitive shader.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModVertex {
    /// World anchor and kind.
    pub anchor: [f32; 4],
    /// Strip axis and half width, or billboard half extents.
    pub shape: [f32; 4],
    pub color: [f32; 4],
    /// Local corner in -1..1 (strips: side and head-to-tail parameter).
    pub uv: [f32; 4],
    /// Style id, progress, length in blocks, intensity.
    pub style: [f32; 4],
}

const QUAD: [[f32; 2]; 6] = [
    [-1.0, -1.0],
    [1.0, -1.0],
    [1.0, 1.0],
    [-1.0, -1.0],
    [1.0, 1.0],
    [-1.0, 1.0],
];

pub fn decal_style_id(style: DecalStyle) -> f32 {
    style as u8 as f32
}

pub fn billboard_style_id(pattern: BillboardPattern) -> f32 {
    STYLE_BILLBOARD + pattern as u8 as f32
}

/// Ground decals, ribbons, beams, then billboards, as a triangle list.
pub fn build(primitives: &Primitives) -> Vec<ModVertex> {
    let mut out = Vec::with_capacity(vertex_count(primitives));
    for decal in &primitives.decals {
        let [x, y, z] = decal.center;
        for [u, v] in QUAD {
            out.push(ModVertex {
                anchor: [
                    x + u * decal.radius,
                    y + DECAL_LIFT_BLOCKS,
                    z + v * decal.radius,
                    KIND_GROUND,
                ],
                color: decal.color,
                uv: [u, v, 0.0, 0.0],
                style: [
                    decal_style_id(decal.style),
                    decal.progress.clamp(0.0, 1.0),
                    decal.radius,
                    1.0,
                ],
                ..Default::default()
            });
        }
    }
    for ribbon in &primitives.ribbons {
        let last = ribbon.points.len() - 1;
        let tangent = |i: usize| {
            let a = ribbon.points[i.saturating_sub(1)];
            let b = ribbon.points[(i + 1).min(last)];
            normalize(sub(b, a))
        };
        for i in 0..last {
            let ends = [i, i + 1].map(|j| {
                let t = j as f32 / last as f32;
                let half = ribbon.width * 0.5 * (1.0 - (1.0 - RIBBON_TAIL_WIDTH) * t);
                (ribbon.points[j], tangent(j), half, t)
            });
            for [side, end] in QUAD {
                let (point, axis, half, t) = ends[usize::from(end > 0.0)];
                out.push(strip_vertex(
                    point,
                    axis,
                    half,
                    ribbon.color,
                    side,
                    t,
                    [STYLE_RIBBON, 0.0, 0.0, 1.0],
                ));
            }
        }
    }
    for beam in &primitives.beams {
        let axis = normalize(sub(beam.end, beam.start));
        let length = length(sub(beam.end, beam.start));
        for [side, end] in QUAD {
            let (point, t) = if end > 0.0 {
                (beam.end, 1.0)
            } else {
                (beam.start, 0.0)
            };
            out.push(strip_vertex(
                point,
                axis,
                beam.width * 0.5,
                beam.color,
                side,
                t,
                [STYLE_BEAM, 0.0, length, beam.intensity],
            ));
        }
    }
    for billboard in &primitives.billboards {
        let [x, y, z] = billboard.position;
        let kind = if billboard.upright {
            KIND_UPRIGHT
        } else {
            KIND_BILLBOARD
        };
        for [u, v] in QUAD {
            out.push(ModVertex {
                anchor: [x, y, z, kind],
                shape: [billboard.width * 0.5, billboard.height * 0.5, 0.0, 0.0],
                color: billboard.color,
                uv: [u, v, 0.0, 0.0],
                style: [billboard_style_id(billboard.pattern), 0.0, 0.0, 1.0],
            });
        }
    }
    out
}

fn vertex_count(primitives: &Primitives) -> usize {
    (primitives.decals.len()
        + primitives
            .ribbons
            .iter()
            .map(|ribbon| ribbon.points.len().saturating_sub(1))
            .sum::<usize>()
        + primitives.beams.len()
        + primitives.billboards.len())
        * 6
}

fn strip_vertex(
    point: Vec3,
    axis: Vec3,
    half_width: f32,
    color: [f32; 4],
    side: f32,
    t: f32,
    style: [f32; 4],
) -> ModVertex {
    ModVertex {
        anchor: [point[0], point[1], point[2], KIND_STRIP],
        shape: [axis[0], axis[1], axis[2], half_width],
        color,
        uv: [side, t, 0.0, 0.0],
        style,
    }
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn length(v: Vec3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Degenerate directions fall back to +X so strips never collapse to NaN.
fn normalize(v: Vec3) -> Vec3 {
    let len = length(v);
    if len > 1e-6 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        [1.0, 0.0, 0.0]
    }
}

#[cfg(test)]
mod tests;
