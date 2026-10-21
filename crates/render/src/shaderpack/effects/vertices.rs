//! The CPU mesh every effect is written into: Iris vertices and `u32` indices.

use super::super::geometry::textured_host_vertex;
use bevy::math::Vec3;

/// Java's raw full-bright lightmap coordinates (`LightTexture.FULL_BRIGHT`), which Iris also
/// gives vertex formats without a lightmap.
pub(super) const FULL_BRIGHT: [f32; 2] = [240.0, 240.0];

/// One effect vertex, relative to the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Vertex {
    pub position: Vec3,
    /// Gamma RGBA (`gl_Color`).
    pub colour: [f32; 4],
    pub normal: Vec3,
    pub uv: [f32; 2],
    /// `mc_midTexCoord`.
    pub mid_uv: [f32; 2],
    /// Raw lightmap coordinates, block then sky.
    pub light: [f32; 2],
}

impl Vertex {
    /// An untextured full-bright vertex.
    pub(super) fn plain(position: Vec3, colour: [f32; 4], normal: Vec3) -> Self {
        Self {
            position,
            colour,
            normal,
            uv: [0.0; 2],
            mid_uv: [0.0; 2],
            light: FULL_BRIGHT,
        }
    }
}

/// Vertices and indices of one frame's effects.
#[derive(Default)]
pub(super) struct Mesh {
    pub words: Vec<u32>,
    pub indices: Vec<u32>,
    vertices: u32,
}

impl Mesh {
    pub(super) fn clear(&mut self) {
        self.words.clear();
        self.indices.clear();
        self.vertices = 0;
    }

    pub(super) fn index_count(&self) -> u32 {
        self.indices.len() as u32
    }

    fn push(&mut self, vertex: &Vertex) -> u32 {
        self.words.extend(textured_host_vertex(
            vertex.position.to_array(),
            vertex.colour,
            vertex.normal.normalize_or_zero().to_array(),
            vertex.uv,
            vertex.mid_uv,
            vertex.light,
        ));
        self.vertices += 1;
        self.vertices - 1
    }

    /// A quad of corners in winding order, as the triangles (0, 1, 2) and (0, 2, 3).
    pub(super) fn quad(&mut self, corners: [Vertex; 4]) {
        let base = self.vertices;
        for corner in &corners {
            self.push(corner);
        }
        self.indices
            .extend([0, 1, 2, 0, 2, 3].map(|offset| base + offset));
    }

    pub(super) fn triangle(&mut self, corners: [Vertex; 3]) {
        for corner in &corners {
            let index = self.push(corner);
            self.indices.push(index);
        }
    }
}
