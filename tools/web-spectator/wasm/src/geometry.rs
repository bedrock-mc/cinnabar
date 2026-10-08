use meshing::{Face, PackedQuad};

pub(super) fn append_quad(
    vertices: &mut Vec<f32>,
    quad: &PackedQuad,
    chunk_origin: [f32; 3],
    color: [f32; 3],
) {
    let local = quad.origin().map(f32::from);
    let origin = std::array::from_fn(|axis| chunk_origin[axis] + local[axis]);
    let [width, height] = [quad.width(), quad.height()].map(f32::from);
    // Identical face axes and outward winding to Cinnabar's chunk.wgsl.
    let (normal, base, u, v) = match quad.face() {
        Face::NegativeX => ([-1., 0., 0.], origin, [0., 0., width], [0., height, 0.]),
        Face::PositiveX => (
            [1., 0., 0.],
            add(origin, [1., 0., 0.]),
            [0., height, 0.],
            [0., 0., width],
        ),
        Face::NegativeY => ([0., -1., 0.], origin, [width, 0., 0.], [0., 0., height]),
        Face::PositiveY => (
            [0., 1., 0.],
            add(origin, [0., 1., 0.]),
            [0., 0., height],
            [width, 0., 0.],
        ),
        Face::NegativeZ => ([0., 0., -1.], origin, [0., height, 0.], [width, 0., 0.]),
        Face::PositiveZ => (
            [0., 0., 1.],
            add(origin, [0., 0., 1.]),
            [width, 0., 0.],
            [0., height, 0.],
        ),
    };
    let corners = [base, add(base, u), add(add(base, u), v), add(base, v)];
    for corner in [0, 1, 2, 0, 2, 3] {
        vertices.extend_from_slice(&corners[corner]);
        vertices.extend_from_slice(&normal);
        vertices.extend_from_slice(&color);
    }
}

fn add(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|axis| left[axis] + right[axis])
}
