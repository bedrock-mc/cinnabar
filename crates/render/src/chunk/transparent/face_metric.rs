//! Perspective face ordering shared by every ordinary terrain-blend stream.
use crate::chunk::*;

// Current vanilla RenderChunkSorter perspective sort uses
// squared distance in chunks intersecting camera-block +/- four, otherwise
// projection onto the normalized chunk-grid direction. It does not use yaw.
const NEAR_CAMERA_BLOCK_RADIUS: i32 = 4;
const CHUNK_SIDE: i32 = chunk_origin(SubChunkKey::new(0, 1, 0, 0))[0];
const CENTROID_PACK_BIAS: f32 = 8.0;
const CENTROID_PACK_SCALE: f32 = 32.0;
const CENTROID_PACK_MAX: f32 = ((1_u32 << 10) - 1) as f32;

#[derive(Clone, Copy)]
pub(in crate::chunk) struct TransparentFaceMetric {
    camera: Vec3,
    camera_chunk: [i32; 3],
    near_min: [i32; 3],
    near_max: [i32; 3],
}

#[derive(Clone, Copy)]
pub(in crate::chunk) struct TransparentChunkFaceMetric {
    camera: Vec3,
    origin: Vec3,
    direction: Option<Vec3>,
}

impl TransparentChunkFaceMetric {
    pub(in crate::chunk) fn distance(self, centroid: Vec3) -> f32 {
        // Current CentroidPlusReverseBit packs the emitted-vertex
        // mean in chunk-local space; vanilla decodes it before face sorting.
        // Quantize only this ordering anchor, never the rendered geometry.
        let local = (centroid - self.origin).to_array().map(|value| {
            ((value + CENTROID_PACK_BIAS) * CENTROID_PACK_SCALE)
                .trunc()
                .clamp(0.0, CENTROID_PACK_MAX)
                / CENTROID_PACK_SCALE
                - CENTROID_PACK_BIAS
        });
        let delta = Vec3::from_array(local) + self.origin - self.camera;
        self.direction
            .map_or_else(|| delta.length_squared(), |direction| delta.dot(direction))
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
        }
    }

    pub(in crate::chunk) fn distance(self, key: SubChunkKey, centroid: Vec3) -> f32 {
        self.for_chunk(key).distance(centroid)
    }

    pub(in crate::chunk) fn for_chunk(self, key: SubChunkKey) -> TransparentChunkFaceMetric {
        let chunk = [key.x, key.y, key.z];
        let origin = Vec3::from_array(chunk_origin(key).map(|value| value as f32));
        if (0..3).all(|axis| (self.near_min[axis]..=self.near_max[axis]).contains(&chunk[axis])) {
            return TransparentChunkFaceMetric {
                camera: self.camera,
                origin,
                direction: None,
            };
        }
        let direction = Vec3::from_array(std::array::from_fn(|axis| {
            match chunk[axis].cmp(&self.camera_chunk[axis]) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            }
        }));
        TransparentChunkFaceMetric {
            camera: self.camera,
            origin,
            direction: Some(direction.normalize_or_zero()),
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

    #[test]
    fn distant_faces_project_on_chunk_grid_direction() {
        let metric = TransparentFaceMetric::new(Vec3::ZERO);
        let key = SubChunkKey::new(0, -2, 0, 3);
        let centroid = Vec3::new(-20.0, 4.0, 50.0);
        let expected = centroid.dot(Vec3::new(-1.0, 0.0, 1.0).normalize());
        assert!((metric.distance(key, centroid) - expected).abs() < 0.00001);
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
        let expected = packed.dot(Vec3::new(-1.0, 0.0, 1.0).normalize());
        assert!((metric.distance(key, centroid) - expected).abs() < 0.00001);
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
