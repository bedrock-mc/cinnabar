//! Vanilla star candidate generation.

/// Builds tangent square triangles with the client's candidate and random-draw ordering.
pub(crate) fn vertices() -> Vec<[f32; 4]> {
    let mut random = assets::ClientRandom::new(10842);
    let mut vertices = Vec::new();
    for _ in 0..1500 {
        let position: [f32; 3] = std::array::from_fn(|_| {
            let value = random.next_float();
            value + value - 1.0
        });
        let size = random.next_float() * 0.1 + 0.15;
        let [x, y, z] = position;
        let length_squared = z * z + y * y + x * x;
        if !(0.01..1.0).contains(&length_squared) || length_squared == 0.01 {
            continue;
        }
        let alpha = ((random.next_float() * 0.3 + 0.7) * 255.0) as u8;
        let inverse_length = 1.0 / length_squared.sqrt();
        let [x, y, z] = position.map(|value| value * inverse_length);
        let yaw = x.atan2(z);
        let pitch = (z * z + x * x).sqrt().atan2(y);
        let angle = random.next_float() * std::f32::consts::PI * 2.0;
        let (sy, cy) = yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let (sa, ca) = angle.sin_cos();
        let corners = [[-size, -size], [size, -size], [size, size], [-size, size]].map(|[u, v]| {
            let right = u * ca + v * sa;
            let up = v * ca - u * sa;
            [
                x * 100.0 - cy * right - sy * cp * up,
                y * 100.0 + sp * up,
                z * 100.0 + sy * right - cy * cp * up,
                f32::from(alpha) / 255.0,
            ]
        });
        vertices.extend([0, 1, 2, 0, 2, 3].map(|index| corners[index]));
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_candidates_are_repeatable_square_quads_on_the_reference_sphere() {
        let vertices = vertices();
        assert_eq!(vertices, super::vertices());
        assert_eq!(vertices.len(), 4488);
        let fixtures = [
            ([-0.505_164_6, 68.919_01, -72.456_31], 0.245_766_55, 219),
            ([-89.447_38, 42.337_2, 14.378_074], 0.197_087_02, 191),
            ([96.904_35, 23.478_968, 7.634_454_7], 0.185_844_88, 240),
        ];
        for (quad, (expected, size, alpha)) in vertices.as_chunks::<6>().0.iter().zip(fixtures) {
            for (axis, coordinate) in expected.into_iter().enumerate() {
                assert!(((quad[0][axis] + quad[2][axis]) * 0.5 - coordinate).abs() < 0.0001);
            }
            let edge_squared = (0..3)
                .map(|axis| (quad[0][axis] - quad[1][axis]).powi(2))
                .sum::<f32>();
            assert!((edge_squared.sqrt() - 2.0 * size).abs() < 0.0001);
            assert_eq!(quad[0][3], alpha as f32 / 255.0);
        }
        for quad in vertices.as_chunks::<6>().0 {
            let center: [f32; 3] = std::array::from_fn(|i| (quad[0][i] + quad[2][i]) * 0.5);
            assert!((center.iter().map(|x| x * x).sum::<f32>().sqrt() - 100.0).abs() < 0.0001);
            assert!((178.0 / 255.0..=1.0).contains(&quad[0][3]));
        }
    }
}
