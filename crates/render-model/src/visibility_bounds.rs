//! Conservative clip-space tests shared by entity preparation and draw submission.

use glam::{Mat4, Vec3, Vec4};

const CLIP_ROUNDING_MARGIN: f32 = 0.0001;

/// The eight corners of finite, ordered world bounds under a finite clip transform.
fn corners(matrix: Mat4, low: [f32; 3], high: [f32; 3]) -> Option<[Vec4; 8]> {
    let determinant = matrix.determinant();
    if !determinant.is_finite()
        || determinant == 0.0
        || !matrix.is_finite()
        || low.iter().chain(&high).any(|x| !x.is_finite())
        || (0..3).any(|a| low[a] > high[a])
    {
        return None;
    }
    let corners: [Vec4; 8] = std::array::from_fn(|i| {
        matrix
            * Vec3::from_array(std::array::from_fn(|a| {
                if i & (1 << a) == 0 { low[a] } else { high[a] }
            }))
            .extend(1.0)
    });
    corners.iter().all(|p| p.is_finite()).then_some(corners)
}

/// Covers rounding differences between CPU clip tests and GPU matrix multiplication.
fn clip_error(matrix: Mat4, low: [f32; 3], high: [f32; 3]) -> Vec4 {
    let magnitude = Vec4::from_array([
        low[0].abs().max(high[0].abs()),
        low[1].abs().max(high[1].abs()),
        low[2].abs().max(high[2].abs()),
        1.0,
    ]);
    let absolute = Mat4::from_cols_array(&matrix.to_cols_array().map(f32::abs));
    absolute * magnitude * (16.0 * f32::EPSILON) + Vec4::splat(CLIP_ROUNDING_MARGIN)
}

/// Whether any part of a box can intersect the view. Uncertain bounds always draw.
pub fn box_intersects_view(matrix: Mat4, low: [f32; 3], high: [f32; 3]) -> bool {
    let Some(p) = corners(matrix, low, high) else {
        return true;
    };
    let error = clip_error(matrix, low, high);
    !(p.iter().all(|p| p.x < -p.w - error.x - error.w)
        || p.iter().all(|p| p.x > p.w + error.x + error.w)
        || p.iter().all(|p| p.y < -p.w - error.y - error.w)
        || p.iter().all(|p| p.y > p.w + error.y + error.w)
        || p.iter().all(|p| p.z < -error.z)
        || p.iter().all(|p| p.z > p.w + error.z + error.w)
        || p.iter().all(|p| p.w < -error.w))
}

/// Supports point-origin occlusion proofs only when the perspective eye matches and
/// every near-plane point lies closer than `near_radius_limit`. This test does not depend
/// on an occluder's view direction, so ordinary camera turns keep preparation stable.
pub fn perspective_supports_occlusion(matrix: Mat4, eye: [f32; 3], near_radius_limit: f64) -> bool {
    let eye = Vec3::from_array(eye).as_dvec3();
    if !matrix.is_finite()
        || !eye.is_finite()
        || !near_radius_limit.is_finite()
        || near_radius_limit <= 0.0
        || matrix.determinant() == 0.0
    {
        return false;
    }
    let inverse = matrix.as_dmat4().inverse();
    let origin = inverse * glam::DVec4::Z;
    if !origin.is_finite()
        || origin.w <= 0.0
        || (origin.truncate() / origin.w).distance(eye) > f64::from(CLIP_ROUNDING_MARGIN)
    {
        return false;
    }
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            let corner = inverse * glam::DVec4::new(x, y, 1.0, 1.0);
            if !corner.is_finite()
                || corner.w <= 0.0
                || (corner.truncate() / corner.w).distance_squared(eye)
                    >= near_radius_limit * near_radius_limit
            {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncertain_clip_boundaries_draw() {
        let edge = [1.0 + f32::EPSILON, 0.0, 0.5];
        assert!(box_intersects_view(Mat4::IDENTITY, edge, edge));
    }

    #[test]
    fn single_eye_proofs_require_a_matching_perspective_origin() {
        assert!(!perspective_supports_occlusion(
            Mat4::IDENTITY,
            [0.0; 3],
            1.0
        ));
        let projection =
            glam::camera::rh::proj::directx::perspective_infinite_reverse(1.0, 1.0, 0.1);
        assert!(perspective_supports_occlusion(projection, [0.0; 3], 1.0));
        assert!(!perspective_supports_occlusion(
            projection,
            [0.0, 0.0, 2.0],
            1.0
        ));
    }

    #[test]
    fn near_clipping_safety_is_stable_under_rotation() {
        let projection =
            glam::camera::rh::proj::directx::perspective_infinite_reverse(1.0, 1.0, 0.1);
        for yaw in [0.0, 0.5, 1.5, 3.0] {
            assert!(perspective_supports_occlusion(
                projection * Mat4::from_rotation_y(yaw),
                [0.0; 3],
                1.0
            ));
        }
        let large_near =
            glam::camera::rh::proj::directx::perspective_infinite_reverse(1.0, 1.0, 2.0);
        assert!(!perspective_supports_occlusion(large_near, [0.0; 3], 1.0));
        assert!(!perspective_supports_occlusion(Mat4::ZERO, [0.0; 3], 1.0));
    }
}
