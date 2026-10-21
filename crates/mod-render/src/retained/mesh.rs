//! Camera-independent line expansion; the host shader resolves width and near-plane clipping.
use super::*;
use bytemuck::{Pod, Zeroable};
use mod_api::drawing::ROUND_SEGMENTS;

/// Every field is a complete WGSL vec4; layout is checked against the host shader in render.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct GpuVertex {
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub c: [f32; 4],
    pub color_a: [f32; 4],
    pub color_b: [f32; 4],
    pub uv: [f32; 4],
    pub data: [f32; 4],
    pub shape: [f32; 4],
}
/// Builds a single immutable GPU mesh shared by all placements and line styles.
pub fn build(geometry: &Geometry) -> Vec<GpuVertex> {
    let mut output = Vec::with_capacity(geometry.expanded_vertices);
    if !geometry.polyline {
        for &index in geometry.indices.iter() {
            let vertex = geometry.vertices[index as usize];
            output.push(GpuVertex {
                a: point(vertex.position, 0.),
                color_a: vertex.color,
                uv: [vertex.uv[0], vertex.uv[1], 0., 0.],
                data: vertex.data,
                ..Default::default()
            });
        }
        return output;
    }
    let vertices = &geometry.vertices;
    let segments: Vec<_> = (0..vertices.len() - usize::from(!geometry.closed))
        .filter_map(|index| {
            let a = vertices[index];
            let b = vertices[(index + 1) % vertices.len()];
            if a.position == b.position {
                return None;
            }
            let length = a
                .position
                .iter()
                .zip(b.position)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                .sqrt();
            Some((a, b, length))
        })
        .collect();
    let mut distance = 0.;
    for (index, &(a, b, length)) in segments.iter().enumerate() {
        let previous = index
            .checked_sub(1)
            .and_then(|i| segments.get(i))
            .or_else(|| segments.last().filter(|_| geometry.closed));
        let following = segments
            .get(index + 1)
            .or_else(|| segments.first().filter(|_| geometry.closed));
        let base = GpuVertex {
            a: point(a.position, distance),
            b: point(b.position, distance + length),
            color_a: a.color,
            color_b: b.color,
            uv: [a.uv[0], a.uv[1], b.uv[0], b.uv[1]],
            data: a.data,
            ..Default::default()
        };
        for (t, side) in [
            (0., -1.),
            (1., -1.),
            (1., 1.),
            (0., -1.),
            (1., 1.),
            (0., 1.),
        ] {
            let neighbor = if t == 0. {
                previous.map(|s| s.0)
            } else {
                following.map(|s| s.1)
            };
            output.push(GpuVertex {
                c: neighbor.map_or([0.; 4], |v| point(v.position, 0.)),
                shape: [1., t, side, f32::from(u8::from(neighbor.is_some()))],
                ..base
            });
        }
        // Full fans are immutable. The instance selects bevel, bounded miter or round joins.
        if let Some((_, c, _)) = following {
            let joint = GpuVertex {
                c: point(c.position, 0.),
                ..base
            };
            fan(&mut output, joint, 3., 1.);
        }
        if previous.is_none() {
            fan(&mut output, base, 2., 0.);
        }
        if following.is_none() {
            fan(&mut output, base, 2., 1.);
        }
        distance += length;
    }
    output
}

/// Emits a bounded triangle fan; negative radial phase names the fan's center.
fn fan(output: &mut Vec<GpuVertex>, base: GpuVertex, kind: f32, endpoint: f32) {
    for index in 0..ROUND_SEGMENTS {
        for phase in [
            -1.,
            index as f32 / ROUND_SEGMENTS as f32,
            (index + 1) as f32 / ROUND_SEGMENTS as f32,
        ] {
            output.push(GpuVertex {
                shape: [kind, endpoint, phase, 0.],
                ..base
            });
        }
    }
}
/// Packs position and cumulative line distance into the shared shader layout.
fn point(position: [f32; 3], distance: f32) -> [f32; 4] {
    [position[0], position[1], position[2], distance]
}
