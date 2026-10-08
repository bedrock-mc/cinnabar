use super::WHITE_LAYER;
use super::mesh::ItemMeshVertex;
use render_model::OPAQUE_WHITE;

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalized(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (length.is_finite() && length > 1e-6).then(|| [v[0] / length, v[1] / length, v[2] / length])
}

/// Point on the drooping rope at `t` in `0..=1`: the chord minus a parabolic sag of `sag` blocks
/// at the midpoint.
#[must_use]
pub fn rope_point(from: [f32; 3], to: [f32; 3], sag: f32, t: f32) -> [f32; 3] {
    std::array::from_fn(|axis| {
        let point = from[axis] + (to[axis] - from[axis]) * t;
        if axis == 1 {
            point - sag * 4.0 * t * (1.0 - t)
        } else {
            point
        }
    })
}

#[allow(clippy::too_many_arguments)]
/// Appends a camera-facing ribbon of `segments` quads (two triangles each) following the sagging
/// rope from `from` to `to`; degenerate or non-finite input appends nothing.
pub fn rope_ribbon(
    from: [f32; 3],
    to: [f32; 3],
    camera: [f32; 3],
    segments: usize,
    sag: f32,
    half_width: f32,
    color: u32,
    out: &mut Vec<ItemMeshVertex>,
) {
    let finite = from
        .iter()
        .chain(&to)
        .chain(&camera)
        .chain(&[sag, half_width])
        .all(|value| value.is_finite());
    if !finite || segments == 0 {
        return;
    }
    // Texel inside layer 0's opaque white tile.
    let uv = [0.01, 0.01];
    let mut push = |position: [f32; 3]| {
        out.push(ItemMeshVertex {
            position,
            uv,
            normal: [0.0, 1.0, 0.0],
            layer: WHITE_LAYER,
            color,
        });
    };
    for segment in 0..segments {
        let start = rope_point(from, to, sag, segment as f32 / segments as f32);
        let end = rope_point(from, to, sag, (segment + 1) as f32 / segments as f32);
        let middle = [
            (start[0] + end[0]) * 0.5,
            (start[1] + end[1]) * 0.5,
            (start[2] + end[2]) * 0.5,
        ];
        let Some(side) = normalized(cross(sub(camera, middle), sub(end, start))) else {
            continue;
        };
        let offset = side.map(|component| component * half_width);
        let corner = |base: [f32; 3], sign: f32| {
            [
                base[0] + offset[0] * sign,
                base[1] + offset[1] * sign,
                base[2] + offset[2] * sign,
            ]
        };
        let (a, b, c, d) = (
            corner(start, 1.0),
            corner(start, -1.0),
            corner(end, -1.0),
            corner(end, 1.0),
        );
        for point in [a, b, c, a, c, d] {
            push(point);
        }
    }
}

/// Default multiplier for an opaque rope colour packed as little-endian RGBA8.
#[must_use]
pub fn rope_color(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16) | (0xff << 24)
}

const _: () = assert!(OPAQUE_WHITE == 0xffff_ffff);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sag_is_zero_at_the_ends_and_maximal_at_the_middle() {
        let (from, to) = ([0.0, 5.0, 0.0], [4.0, 5.0, 0.0]);
        assert_eq!(rope_point(from, to, 1.0, 0.0), from);
        assert_eq!(rope_point(from, to, 1.0, 1.0), to);
        assert!((rope_point(from, to, 1.0, 0.5)[1] - 4.0).abs() < 1e-6);
    }

    #[test]
    fn ribbon_emits_six_vertices_per_segment_and_faces_the_camera() {
        let mut vertices = Vec::new();
        rope_ribbon(
            [0.0; 3],
            [4.0, 0.0, 0.0],
            [2.0, 0.0, 5.0],
            4,
            0.0,
            0.05,
            OPAQUE_WHITE,
            &mut vertices,
        );
        assert_eq!(vertices.len(), 24);
        // The rope runs along X and the camera sits on +Z, so the ribbon widens along Y.
        assert!(
            vertices
                .iter()
                .all(|vertex| vertex.position[2].abs() < 1e-6)
        );
        assert!(vertices.iter().any(|vertex| vertex.position[1] > 0.04));
    }

    #[test]
    fn degenerate_input_appends_nothing() {
        let mut vertices = Vec::new();
        rope_ribbon(
            [0.0; 3],
            [1.0, f32::NAN, 0.0],
            [0.0; 3],
            4,
            0.0,
            0.05,
            0,
            &mut vertices,
        );
        rope_ribbon(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0; 3],
            0,
            0.0,
            0.05,
            0,
            &mut vertices,
        );
        assert!(vertices.is_empty());
    }
    #[test]
    fn review_render_rope_triangles_face_the_camera() {
        let camera = [0.0, 0.0, 2.0];
        let mut vertices = Vec::new();
        rope_ribbon(
            [-1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            camera,
            1,
            0.0,
            0.1,
            u32::MAX,
            &mut vertices,
        );
        for triangle in vertices.chunks_exact(3) {
            let a = triangle[0].position;
            let normal = cross(sub(triangle[1].position, a), sub(triangle[2].position, a));
            let towards = sub(camera, a);
            assert!(normal.iter().zip(towards).map(|(n, v)| n * v).sum::<f32>() > 0.0);
        }
    }
}
