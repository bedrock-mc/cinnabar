//! Analytic rounded-rectangle Gaussian approximation shared with the UI shader.

/// UV is the centered local position; overlay is half-size XY, corner radius and blur sigma.
pub const UI_STYLE_SHADOW: u16 = 1 << 15;
pub const WGSL: &str = include_str!("ui_shadow.wgsl");

/// Evaluates the signed-distance Gaussian approximation, with one pixel of sharp-edge AA.
pub fn coverage(point: [f32; 2], [half_x, half_y, radius, sigma]: [f32; 4], pixel: f32) -> f32 {
    let q = [
        point[0].abs() - half_x + radius,
        point[1].abs() - half_y + radius,
    ];
    let distance = q[0].max(0.0).hypot(q[1].max(0.0)) + q[0].max(q[1]).min(0.0) - radius;
    if sigma <= 0.0 {
        return (0.5 - distance / pixel.max(f32::EPSILON)).clamp(0.0, 1.0);
    }
    let x = distance / sigma;
    let t = 1.0 / (1.0 + 0.2316419 * x.abs());
    let tail = 0.3989423
        * (-0.5 * x * x).exp()
        * t
        * (0.31938153 + t * (-0.35656378 + t * (1.7814779 + t * (-1.8212559 + t * 1.3302745))));
    if x >= 0.0 { tail } else { 1.0 - tail }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_shadows_gaussian_follows_curvature_and_fades_outside_the_edge() {
        let shape = [40.0, 20.0, 10.0, 4.0];
        assert!((coverage([0.0, 20.0], shape, 1.0) - 0.5).abs() < 0.00001);
        assert!(coverage([0.0, 24.0], shape, 1.0) < 0.16);
        assert!(coverage([0.0, 32.0], shape, 1.0) < 0.0014);
        assert!(coverage([40.0, 20.0], shape, 1.0) < coverage([30.0, 20.0], shape, 1.0));
        assert!(coverage([0.0, 0.0], shape, 1.0) > 0.99);
        assert_eq!(coverage([0.0, 20.5], [40.0, 20.0, 10.0, 0.0], 1.0), 0.0);
    }
}
