//! Which terrain sections the shadow pass draws, as Iris culls shadow casters.
//!
//! Iris picks a culler from the pack's `shadow.culling` directive (and whether the shadow
//! program voxelizes through a geometry stage):
//! - distance only (`false`, or the default for voxelizing packs): a box of half-size
//!   `shadowDistance × shadowDistanceRenderMul` around the camera, none when that is not
//!   positive or reaches past the render distance;
//! - light-extruded camera frustum (`true`, the default otherwise): sections that cannot cast
//!   a shadow onto anything the camera sees are dropped, within the same distance box;
//! - safe zone (`reversed` / `safe_zone`): the frustum test, inside a box of
//!   `shadowDistance × shadowDistanceRenderMul`, but every section within
//!   `voxelDistance × shadowDistanceRenderMul` is kept.
//!
//! The frustum test keeps the camera frustum's planes that face the light (its back planes as
//! seen from the light) and adds, along each silhouette edge between a back and a front
//! plane, the plane through that edge parallel to the light. A section outside any of them
//! casts no shadow into the view.

use bevy::math::{DVec3, Mat4, Vec3, Vec4, Vec4Swizzles};

/// The pack's `shadow.culling` directive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShadowCulling {
    /// No directive: the frustum test, or distance only when the shadow program voxelizes.
    #[default]
    Default,
    /// `false`: distance only.
    Distance,
    /// `true`: the frustum test.
    Advanced,
    /// `reversed` or `safe_zone`: the frustum test with a kept zone around the camera.
    SafeZone,
}

impl ShadowCulling {
    /// Parses a `shadow.culling` value; `None` for one Iris does not recognise.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "false" => Some(Self::Distance),
            "true" => Some(Self::Advanced),
            "reversed" | "safe_zone" => Some(Self::SafeZone),
            _ => None,
        }
    }
}

/// A pack's shadow-culling directives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowCullSettings {
    pub culling: ShadowCulling,
    /// The shadow program has a geometry stage, which Iris takes for voxelization.
    pub voxelizes: bool,
    /// `const float shadowDistance`, blocks.
    pub shadow_distance: f32,
    /// `const float shadowDistanceRenderMul`; negative (the default) leaves the distance to
    /// the player, which here is the render distance.
    pub render_mul: f32,
    /// `const float voxelDistance`, blocks.
    pub voxel_distance: f32,
}

impl Default for ShadowCullSettings {
    fn default() -> Self {
        Self {
            culling: ShadowCulling::Default,
            voxelizes: false,
            shadow_distance: 160.0,
            render_mul: -1.0,
            voxel_distance: 0.0,
        }
    }
}

/// Planes that bound the light-extruded camera frustum: at most the six frustum planes and
/// one edge plane per back/front pair.
const MAX_PLANES: usize = 6 + 12;

/// One frame's shadow culler for a camera.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShadowFrustum {
    eye: DVec3,
    /// Half-size of the box around the camera outside which nothing is drawn.
    distance: Option<f64>,
    /// Half-size of the box around the camera inside which everything is drawn.
    keep: Option<f64>,
    /// Camera-relative planes `(n, w)`, a point `p` inside when `n·p + w >= 0`; `None` skips
    /// the frustum test.
    planes: Option<Vec<Vec4>>,
}

impl ShadowFrustum {
    /// The culler for a camera at `eye` whose camera-relative view-projection (Iris's
    /// `gbufferProjection × gbufferModelView`) is `view_projection`, with the shadow light in
    /// world direction `light`, `render_distance` blocks of terrain and the pack's `settings`.
    pub(crate) fn new(
        settings: &ShadowCullSettings,
        render_distance: f32,
        eye: DVec3,
        view_projection: Mat4,
        light: Vec3,
    ) -> Self {
        let render_distance = f64::from(render_distance);
        let half_plane = f64::from(settings.shadow_distance);
        let mut mul = f64::from(settings.render_mul);
        let distance_only = settings.culling == ShadowCulling::Distance
            || (settings.culling == ShadowCulling::Default && settings.voxelizes);
        if distance_only {
            let distance = half_plane * mul;
            return Self {
                eye,
                distance: (distance > 0.0 && distance <= render_distance).then_some(distance),
                keep: None,
                planes: None,
            };
        }
        let safe_zone = settings.culling == ShadowCulling::SafeZone;
        if safe_zone && mul < 0.0 {
            mul = 1.0;
        }
        let inner = if mul < 0.0 {
            render_distance
        } else if safe_zone {
            f64::from(settings.voxel_distance) * mul
        } else {
            half_plane * mul
        };
        let inner = (safe_zone || inner < render_distance).then_some(inner);
        let planes = Some(extruded_planes(view_projection, light.normalize_or_zero()));
        if safe_zone {
            Self {
                eye,
                distance: Some(half_plane * mul),
                keep: inner,
                planes,
            }
        } else {
            Self {
                eye,
                distance: inner,
                keep: None,
                planes,
            }
        }
    }

    /// Whether the 16-block section at block `origin` may cast a shadow the pack draws.
    pub(crate) fn visible(&self, origin: [i32; 3]) -> bool {
        let min = DVec3::new(
            f64::from(origin[0]),
            f64::from(origin[1]),
            f64::from(origin[2]),
        ) - self.eye;
        let max = min + DVec3::splat(16.0);
        let within =
            |half: f64| min.cmple(DVec3::splat(half)).all() && max.cmpge(DVec3::splat(-half)).all();
        if self.distance.is_some_and(|half| !within(half)) {
            return false;
        }
        if self.keep.is_some_and(within) {
            return true;
        }
        let Some(planes) = &self.planes else {
            return true;
        };
        let (min, max) = (min.as_vec3(), max.as_vec3());
        planes.iter().all(|plane| {
            let farthest = Vec3::select(plane.xyz().cmplt(Vec3::ZERO), min, max);
            plane.xyz().dot(farthest) + plane.w >= 0.0
        })
    }

    /// Feeds every value [`Self::visible`] reads into `hasher`: equal fingerprints keep the
    /// same sections.
    pub(crate) fn hash_into(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.eye.to_array().map(f64::to_bits).hash(hasher);
        self.distance.map(f64::to_bits).hash(hasher);
        self.keep.map(f64::to_bits).hash(hasher);
        self.planes.as_ref().map(Vec::len).hash(hasher);
        for plane in self.planes.iter().flatten() {
            plane.to_array().map(f32::to_bits).hash(hasher);
        }
    }
}

/// The camera frustum's six planes from `view_projection` (inside-positive), in the order
/// -X, +X, -Y, +Y, far, near clip bounds, each normalised as a 4-vector.
fn frustum_planes(view_projection: Mat4) -> [Vec4; 6] {
    let rows = view_projection.transpose();
    [
        Vec4::new(-1.0, 0.0, 0.0, 1.0),
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(0.0, -1.0, 0.0, 1.0),
        Vec4::new(0.0, 1.0, 0.0, 1.0),
        Vec4::new(0.0, 0.0, -1.0, 1.0),
        Vec4::new(0.0, 0.0, 1.0, 1.0),
    ]
    .map(|bound| (rows * bound).normalize_or_zero())
}

/// The planes bounding everything that can shadow the camera frustum from direction `light`:
/// the frustum planes facing the light, and the edge planes along its silhouette.
fn extruded_planes(view_projection: Mat4, light: Vec3) -> Vec<Vec4> {
    let base = frustum_planes(view_projection);
    let back = base.map(|plane| plane.xyz().dot(light) > 0.0);
    let mut planes = Vec::with_capacity(MAX_PLANES);
    for (plane, &is_back) in base.iter().zip(&back) {
        if is_back || plane.xyz().dot(light) == 0.0 {
            planes.push(*plane);
        }
    }
    for (index, plane) in base.iter().enumerate() {
        if !back[index] {
            continue;
        }
        // The planes sharing an edge with this one: those of the other two axes.
        let axis = index / 2;
        for neighbour in (0..6).filter(|n| n / 2 != axis) {
            if !back[neighbour] {
                planes.push(edge_plane(*plane, base[neighbour], light));
            }
        }
    }
    planes
}

/// The plane through the intersection line of `back` and `front` that contains `light`.
fn edge_plane(back: Vec4, front: Vec4, light: Vec3) -> Vec4 {
    let line = back.xyz().cross(front.xyz());
    let normal = line.cross(light);
    // A point on the line: where it meets the plane through the origin perpendicular to it.
    let point = (line.cross(back.xyz()) * -front.w + front.xyz().cross(line) * -back.w)
        / line.length_squared();
    normal.extend(-normal.dot(point))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Mat4 {
        // Looking along -Z (no rotation), 90° vertical FOV.
        crate::shaderpack::uniforms::gl_perspective([1.0, 1.0], 0.05, 512.0)
    }

    fn settings(culling: ShadowCulling) -> ShadowCullSettings {
        ShadowCullSettings {
            culling,
            shadow_distance: 128.0,
            render_mul: 1.0,
            voxel_distance: 32.0,
            ..ShadowCullSettings::default()
        }
    }

    #[test]
    fn overhead_light_keeps_casters_above_the_view_and_drops_those_behind() {
        let frustum = ShadowFrustum::new(
            &settings(ShadowCulling::Advanced),
            256.0,
            DVec3::ZERO,
            camera(),
            Vec3::Y,
        );
        // In front of the camera, and high above the part of the world it sees.
        assert!(frustum.visible([-8, -8, -64]));
        assert!(frustum.visible([-8, 96, -64]));
        // Behind the camera: nothing there shadows what it sees from straight above.
        assert!(!frustum.visible([-8, -8, 64]));
        assert!(!frustum.visible([-8, 96, 64]));
    }

    #[test]
    fn the_distance_box_drops_far_sections_and_the_safe_zone_keeps_near_ones() {
        let safe = ShadowFrustum::new(
            &settings(ShadowCulling::SafeZone),
            256.0,
            DVec3::ZERO,
            camera(),
            Vec3::Y,
        );
        assert!(!safe.visible([-8, -8, -200]), "beyond shadowDistance");
        assert!(
            safe.visible([-8, -8, 16]),
            "behind, but inside voxelDistance"
        );
        assert!(!safe.visible([-8, -8, 64]), "behind, outside voxelDistance");
        let distance = ShadowFrustum::new(
            &settings(ShadowCulling::Distance),
            256.0,
            DVec3::ZERO,
            camera(),
            Vec3::Y,
        );
        assert!(
            distance.visible([-8, -8, 64]),
            "distance only keeps everything nearby"
        );
        assert!(!distance.visible([-8, -8, -200]));
    }

    #[test]
    fn a_distance_past_the_render_distance_culls_nothing_by_distance() {
        let mut far = settings(ShadowCulling::Distance);
        far.shadow_distance = 1024.0;
        let frustum = ShadowFrustum::new(&far, 256.0, DVec3::ZERO, camera(), Vec3::Y);
        assert!(frustum.visible([2000, 0, 0]));
    }

    #[test]
    fn directives_parse_as_iris_reads_them() {
        assert_eq!(
            ShadowCulling::parse("reversed"),
            Some(ShadowCulling::SafeZone)
        );
        assert_eq!(
            ShadowCulling::parse("safe_zone"),
            Some(ShadowCulling::SafeZone)
        );
        assert_eq!(ShadowCulling::parse(" true"), Some(ShadowCulling::Advanced));
        assert_eq!(ShadowCulling::parse("false"), Some(ShadowCulling::Distance));
        assert_eq!(ShadowCulling::parse("maybe"), None);
    }
}
