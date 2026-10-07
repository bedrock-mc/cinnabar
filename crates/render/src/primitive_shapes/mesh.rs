//! Shared unit line meshes; only segment-count changes select another mesh.
use bevy::math::Vec3;
use render_api::primitive_shapes::PrimitiveShapeKind;
use render_model::primitive_shapes::PrimitiveMeshKey;

/// `w` marks arrow head vertices or the negative zero-segment sentinel.
pub(super) type Vertex = [f32; 4];

/// Builds each immutable unit mesh once when its first instance arrives.
pub(super) fn build(key: PrimitiveMeshKey) -> Vec<Vertex> {
    match key.kind {
        PrimitiveShapeKind::Line => vec![[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
        PrimitiveShapeKind::Box => box_mesh(),
        PrimitiveShapeKind::Circle => ring(key.segments, 1),
        PrimitiveShapeKind::Sphere => [1, 2, 0]
            .into_iter()
            .flat_map(|axis| ring(key.segments, axis))
            .collect(),
        PrimitiveShapeKind::Arrow => arrow(key.segments),
        PrimitiveShapeKind::Text => Vec::new(),
    }
}

/// Emits the twelve axis-aligned edges around the box center.
fn box_mesh() -> Vec<Vertex> {
    let mut vertices = Vec::with_capacity(24);
    for axis in 0..3 {
        for first in [-0.5, 0.5] {
            for second in [-0.5, 0.5] {
                for end in [-0.5, 0.5] {
                    let mut point = [0.0; 4];
                    point[axis] = end;
                    point[(axis + 1) % 3] = first;
                    point[(axis + 2) % 3] = second;
                    vertices.push(point);
                }
            }
        }
    }
    vertices
}

/// Uses the native quantized sine table and closes each ring explicitly.
fn ring(segments: u8, axis: usize) -> Vec<Vertex> {
    let count = usize::from(segments);
    let mut vertices = Vec::with_capacity(count * 2);
    if count == 0 {
        return vec![[0.0, 0.0, 0.0, -1.0]; 2];
    }
    for segment in 0..count {
        for point in [segment, (segment + 1) % count] {
            let angle = point as f32 * (360.0 / count as f32) * (std::f32::consts::PI / 180.0);
            let normal = [Vec3::X, Vec3::Y, Vec3::Z][axis];
            let absolute = normal.abs();
            let least = if absolute.x < absolute.y {
                if absolute.x < absolute.z {
                    Vec3::X
                } else {
                    Vec3::Z
                }
            } else if absolute.y < absolute.z {
                Vec3::Y
            } else {
                Vec3::Z
            };
            let u = normal.cross(least);
            let v = normal.cross(u);
            let point = u * crate::native_trig::cosine(angle) + v * crate::native_trig::sine(angle);
            vertices.push(point.extend(0.0).to_array());
        }
    }
    vertices
}

/// The head repeats both endpoints of each ring edge as spokes, then adds its shaft.
fn arrow(segments: u8) -> Vec<Vertex> {
    let mut vertices = ring(segments, 2);
    for vertex in &mut vertices {
        vertex[0] = -vertex[0];
        vertex[1] = -vertex[1];
    }
    for vertex in &mut vertices {
        vertex[3] = 1.0;
    }
    let ring_vertices = vertices.len();
    vertices.reserve(ring_vertices * 2 + 2);
    for index in 0..ring_vertices {
        vertices.push(vertices[index]);
        vertices.push([0.0, 0.0, 1.0, 0.0]);
    }
    vertices.extend([[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]);
    vertices
}
