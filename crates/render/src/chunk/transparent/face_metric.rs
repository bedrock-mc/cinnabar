//! Perspective face ordering shared by every ordinary terrain-blend stream.
use crate::chunk::*;

// Vanilla's chunk perspective sort uses
// squared distance in chunks intersecting camera-block +/- four, otherwise
// projection onto the normalized chunk-grid direction. It does not use yaw.
const NEAR_CAMERA_BLOCK_RADIUS: i32 = 4;
const CHUNK_SIDE: i32 = chunk_origin(SubChunkKey::new(0, 1, 0, 0))[0];
const CENTROID_PACK_BIAS: f32 = 8.0;
const CENTROID_PACK_SCALE: f32 = 32.0;
const CENTROID_PACK_MAX: f32 = ((1_u32 << 10) - 1) as f32;

// Ordering may reuse a near sub-chunk's radial sort through sub-pixel camera motion.
const CAMERA_POSITION_SORT_QUANTUM: f32 = 1.0 / 64.0;

/// Canonical bits of the camera position at sort-cache precision, or `None` when non-finite.
pub(in crate::chunk) fn quantized_position_bits(camera: Vec3) -> Option<[u32; 3]> {
    let values = camera
        .to_array()
        .map(|value| (value / CAMERA_POSITION_SORT_QUANTUM).round() * CAMERA_POSITION_SORT_QUANTUM);
    values
        .iter()
        .all(|value| value.is_finite())
        .then(|| values.map(|value| if value == 0.0 { 0 } else { value.to_bits() }))
}

/// What one sub-chunk's face order depends on besides its own mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::chunk) enum FaceOrderClass {
    /// Radial order around the exact camera; reused only at the same quantized position.
    Near([u32; 3]),
    /// Projection on one of 26 chunk-grid directions; independent of the camera position.
    Far([i8; 3]),
}

/// Camera state from which every sub-chunk's [`FaceOrderClass`] follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::chunk) struct FaceOrderCamera {
    camera_chunk: [i32; 3],
    near_min: [i32; 3],
    near_max: [i32; 3],
    /// Present only while some keyed sub-chunk is near, so far-only views ignore small moves.
    near_position_bits: Option<[u32; 3]>,
}

impl FaceOrderCamera {
    /// The class of a sub-chunk among the keys this camera was built from.
    pub(in crate::chunk) fn class(&self, key: SubChunkKey) -> FaceOrderClass {
        let chunk = [key.x, key.y, key.z];
        if (0..3).all(|axis| (self.near_min[axis]..=self.near_max[axis]).contains(&chunk[axis])) {
            FaceOrderClass::Near(self.near_position_bits.unwrap_or_default())
        } else {
            FaceOrderClass::Far(std::array::from_fn(|axis| {
                chunk[axis].cmp(&self.camera_chunk[axis]) as i8
            }))
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::chunk) struct TransparentFaceMetric {
    camera: Vec3,
    camera_chunk: [i32; 3],
    near_min: [i32; 3],
    near_max: [i32; 3],
    position_bits: [u32; 3],
}

#[derive(Clone, Copy)]
pub(in crate::chunk) struct TransparentChunkFaceMetric {
    camera: Vec3,
    origin: Vec3,
    direction: Option<Vec3>,
}

impl TransparentChunkFaceMetric {
    /// Larger is drawn first; comparable only between faces of the same sub-chunk.
    pub(in crate::chunk) fn distance(self, centroid: Vec3) -> f32 {
        // Vanilla packs the emitted-vertex
        // mean in chunk-local space; vanilla decodes it before face sorting.
        // Quantize only this ordering anchor, never the rendered geometry.
        let local = Vec3::from_array((centroid - self.origin).to_array().map(|value| {
            ((value + CENTROID_PACK_BIAS) * CENTROID_PACK_SCALE)
                .trunc()
                .clamp(0.0, CENTROID_PACK_MAX)
                / CENTROID_PACK_SCALE
                - CENTROID_PACK_BIAS
        }));
        match self.direction {
            None => (local + self.origin - self.camera).length_squared(),
            // Within one sub-chunk the camera and origin terms of the grid projection are
            // constant, and the unnormalized sign vector keeps this sum of 1/32 steps exact.
            Some(direction) => local.dot(direction),
        }
    }
}

impl TransparentFaceMetric {
    pub(in crate::chunk) fn new(camera: Vec3) -> Self {
        let block = camera.to_array().map(|value| value.floor() as i32);
        Self {
            camera,
            camera_chunk: block.map(|value| value.div_euclid(CHUNK_SIDE)),
            near_min: block.map(|value| {
                value
                    .saturating_sub(NEAR_CAMERA_BLOCK_RADIUS)
                    .div_euclid(CHUNK_SIDE)
            }),
            near_max: block.map(|value| {
                value
                    .saturating_add(NEAR_CAMERA_BLOCK_RADIUS)
                    .div_euclid(CHUNK_SIDE)
            }),
            position_bits: quantized_position_bits(camera).unwrap_or_default(),
        }
    }

    #[cfg(test)]
    pub(in crate::chunk) fn distance(self, key: SubChunkKey, centroid: Vec3) -> f32 {
        self.for_chunk(key).distance(centroid)
    }

    fn is_near(self, key: SubChunkKey) -> bool {
        let chunk = [key.x, key.y, key.z];
        (0..3).all(|axis| (self.near_min[axis]..=self.near_max[axis]).contains(&chunk[axis]))
    }

    fn direction_signs(self, key: SubChunkKey) -> [i8; 3] {
        let chunk = [key.x, key.y, key.z];
        std::array::from_fn(|axis| chunk[axis].cmp(&self.camera_chunk[axis]) as i8)
    }

    pub(in crate::chunk) fn class(self, key: SubChunkKey) -> FaceOrderClass {
        if self.is_near(key) {
            FaceOrderClass::Near(self.position_bits)
        } else {
            FaceOrderClass::Far(self.direction_signs(key))
        }
    }

    pub(in crate::chunk) fn order_camera(
        self,
        keys: impl IntoIterator<Item = SubChunkKey>,
    ) -> FaceOrderCamera {
        let near = keys.into_iter().any(|key| self.is_near(key));
        FaceOrderCamera {
            camera_chunk: self.camera_chunk,
            near_min: self.near_min,
            near_max: self.near_max,
            near_position_bits: near.then_some(self.position_bits),
        }
    }

    pub(in crate::chunk) fn for_chunk(self, key: SubChunkKey) -> TransparentChunkFaceMetric {
        let origin = Vec3::from_array(chunk_origin(key).map(|value| value as f32));
        TransparentChunkFaceMetric {
            camera: self.camera,
            origin,
            direction: (!self.is_near(key))
                .then(|| Vec3::from_array(self.direction_signs(key).map(f32::from))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearby_faces_use_radial_distance_not_view_depth() {
        let metric = TransparentFaceMetric::new(Vec3::new(1.0, 1.0, 1.0));
        let key = SubChunkKey::new(0, 0, 0, 0);
        assert_eq!(metric.distance(key, Vec3::new(2.0, 3.0, 4.0)), 14.0);
        assert!(metric.distance(key, Vec3::new(5.0, 1.0, 2.0)) > 14.0);
    }

    #[test]
    fn near_interval_crosses_chunk_boundaries_including_negative_coordinates() {
        let metric = TransparentFaceMetric::new(Vec3::new(-0.1, 15.9, 15.9));
        let key = SubChunkKey::new(0, -1, 1, 1);
        let centroid = Vec3::new(-1.0, 17.0, 17.0);
        assert_eq!(
            metric.distance(key, centroid),
            (centroid - metric.camera).length_squared()
        );
    }

    /// Far order is the exact grid projection, so it cannot change while the class holds.
    #[test]
    fn distant_faces_order_by_exact_grid_projection_for_any_camera_in_class() {
        let key = SubChunkKey::new(0, -2, 0, 3);
        let origin = Vec3::from_array(chunk_origin(key).map(|value| value as f32));
        let direction = Vec3::new(-1.0, 0.0, 1.0).normalize();
        let centroids = [
            Vec3::new(3.5, 4.0, 2.5),
            Vec3::new(2.5, 4.0, 3.5),
            Vec3::new(7.5, 1.0, 9.5),
            Vec3::new(0.5, 9.0, 0.5),
        ]
        .map(|local| origin + local);
        for camera in [
            Vec3::ZERO,
            Vec3::new(13.37, 10.1, 5.9),
            Vec3::new(0.01, 15.9, 0.01),
        ] {
            let metric = TransparentFaceMetric::new(camera);
            assert_eq!(metric.class(key), FaceOrderClass::Far([-1, 0, 1]));
            for left in centroids {
                for right in centroids {
                    let exact = f64::from((left - right).dot(direction));
                    let ordered = metric
                        .distance(key, left)
                        .total_cmp(&metric.distance(key, right));
                    assert_eq!(Some(ordered), exact.partial_cmp(&0.0));
                }
            }
        }
    }

    /// Exact far keys reorder only faces whose former float projections tied within noise.
    #[test]
    fn exact_far_key_agrees_with_float_projection_beyond_rounding_noise() {
        let pack = |value: f32| ((value + 8.0) * 32.0).trunc().clamp(0.0, 1023.0) / 32.0 - 8.0;
        let key = SubChunkKey::new(0, 3, 1, -2);
        let origin = Vec3::from_array(chunk_origin(key).map(|value| value as f32));
        let locals = (0..16 * 16)
            .map(|index| {
                let (a, b) = ((index % 16) as f32, (index / 16) as f32);
                match index % 3 {
                    0 => Vec3::new(a + 0.5, 14.875, b + 0.5),
                    1 => Vec3::new(a, b + 0.5, 15.5 - a),
                    _ => Vec3::new(b + 0.5, a + 0.5, b),
                }
            })
            .collect::<Vec<_>>();
        for camera in [Vec3::new(3.7, 1.62, 9.1), Vec3::new(0.01, 15.99, 15.99)] {
            let metric = TransparentFaceMetric::new(camera);
            let direction = match metric.class(key) {
                FaceOrderClass::Far(signs) => Vec3::from_array(signs.map(f32::from)),
                FaceOrderClass::Near(_) => unreachable!(),
            };
            let former = |local: Vec3| {
                (local.map(pack) + origin - camera).dot(direction.normalize_or_zero())
            };
            for &left in &locals {
                for &right in &locals {
                    let (old_left, old_right) = (former(left), former(right));
                    if (old_left - old_right).abs() > 1.0e-3 {
                        assert_eq!(
                            metric
                                .distance(key, origin + left)
                                .total_cmp(&metric.distance(key, origin + right)),
                            old_left.total_cmp(&old_right)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn quantized_camera_canonicalizes_zero_and_refuses_nonfinite() {
        assert_eq!(
            quantized_position_bits(Vec3::ZERO),
            quantized_position_bits(Vec3::splat(-0.0))
        );
        assert_ne!(
            quantized_position_bits(Vec3::ZERO),
            quantized_position_bits(Vec3::Z)
        );
        assert!(quantized_position_bits(Vec3::splat(f32::INFINITY)).is_none());
        assert!(quantized_position_bits(Vec3::splat(f32::NAN)).is_none());
    }

    #[test]
    fn near_class_follows_quantized_camera_and_far_class_ignores_it() {
        let near = SubChunkKey::new(0, 0, 0, 0);
        let far = SubChunkKey::new(0, 3, 0, -2);
        let base = TransparentFaceMetric::new(Vec3::new(8.0, 8.0, 8.0));
        let jitter = TransparentFaceMetric::new(Vec3::new(8.001, 8.0, 8.0));
        let moved = TransparentFaceMetric::new(Vec3::new(8.25, 8.0, 8.0));
        assert_eq!(base.class(near), jitter.class(near));
        assert_ne!(base.class(near), moved.class(near));
        assert_eq!(base.class(far), moved.class(far));
        assert_eq!(base.order_camera([far]), moved.order_camera([far]));
        let keyed = moved.order_camera([near, far]);
        assert_eq!(keyed.class(near), moved.class(near));
        assert_eq!(keyed.class(far), moved.class(far));
        assert_ne!(
            base.order_camera([near, far]),
            moved.order_camera([near, far])
        );
    }

    #[test]
    fn native_packed_centroids_reverse_close_ice_water_order() {
        let camera = Vec3::new(0.5, 2.0, 0.04);
        let metric = TransparentFaceMetric::new(camera);
        let key = SubChunkKey::new(0, 0, 0, 0);
        let ice_side = Vec3::new(0.5, 0.5, 1.0);
        let water_top = Vec3::new(0.5, 1.0 - meshing::liquid::LIQUID_FACE_INSET, 1.5);
        assert!((ice_side - camera).length_squared() > (water_top - camera).length_squared());
        assert!(metric.distance(key, water_top) > metric.distance(key, ice_side));
    }

    #[test]
    fn native_packing_uses_chunk_local_coordinates_at_negative_origins() {
        let key = SubChunkKey::new(0, -2, -3, -7);
        let origin = Vec3::from_array(chunk_origin(key).map(|value| value as f32));
        let camera = origin + Vec3::splat(4.0);
        let metric = TransparentFaceMetric::new(camera);
        let centroid = origin + Vec3::new(0.01, 0.999, 15.999);
        let packed = origin + Vec3::new(0.0, 0.96875, 15.96875);
        assert_eq!(
            metric.distance(key, centroid),
            (packed - camera).length_squared()
        );
    }

    #[test]
    fn distant_faces_project_the_native_packed_anchor() {
        let key = SubChunkKey::new(0, -2, 0, 3);
        let origin = Vec3::from_array(chunk_origin(key).map(|value| value as f32));
        let metric = TransparentFaceMetric::new(Vec3::ZERO);
        let centroid = origin + Vec3::new(12.01, 4.999, 2.501);
        let packed = origin + Vec3::new(12.0, 4.96875, 2.5);
        assert_eq!(metric.distance(key, centroid), metric.distance(key, packed));
        assert_eq!(metric.distance(key, packed), 2.5 - 12.0);
    }

    #[test]
    fn native_packing_preserves_half_integer_cube_face_centers() {
        let key = SubChunkKey::new(0, 2, 3, -7);
        let origin = Vec3::from_array(chunk_origin(key).map(|value| value as f32));
        let camera = origin + Vec3::splat(4.0);
        let metric = TransparentFaceMetric::new(camera);
        for local in [
            Vec3::new(0.0, 0.5, 0.5),
            Vec3::new(16.0, 15.5, 15.5),
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(15.5, 16.0, 15.5),
            Vec3::new(0.5, 0.5, 0.0),
            Vec3::new(15.5, 15.5, 16.0),
        ] {
            let centroid = origin + local;
            assert_eq!(
                metric.distance(key, centroid),
                (centroid - camera).length_squared()
            );
        }
    }

    #[test]
    fn native_centroid_packing_clamps_the_ten_bit_range() {
        let key = SubChunkKey::new(0, 0, 0, 0);
        let metric = TransparentFaceMetric::new(Vec3::ZERO);
        for (local, packed) in [
            (-100.0, -8.0),
            (-8.001, -8.0),
            (-8.0, -8.0),
            (-7.999, -8.0),
            (23.96875, 23.96875),
            (24.0, 23.96875),
            (100.0, 23.96875),
        ] {
            assert_eq!(
                metric.distance(key, Vec3::splat(local)),
                Vec3::splat(packed).length_squared()
            );
        }
    }
}
