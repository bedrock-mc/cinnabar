//! Per-view Enhanced uniforms: stable cascaded sun-shadow fitting, light
//! colours, and grading inputs derived from the atmosphere frame.

use bevy::math::{Mat4, UVec4, Vec3, Vec4};

use super::{EnhancedRendering, MAX_SHADOW_CASCADES};
use crate::AtmosphereFrame;

/// Blocks kept toward the light beyond a cascade so off-screen terrain above
/// the view (cave ceilings, mountains) still casts.
pub(crate) const SHADOW_CASTER_REACH: f32 = 384.0;
/// Log/linear blend of the practical split scheme.
const SPLIT_LAMBDA: f32 = 0.75;
const FIRST_SPLIT_NEAR: f32 = 0.1;

pub(crate) const FEATURE_SHADOWS: u32 = 1 << 0;
pub(crate) const FEATURE_BLOOM: u32 = 1 << 1;
pub(crate) const FEATURE_SHAFTS: u32 = 1 << 2;
pub(crate) const FEATURE_WAVING: u32 = 1 << 3;
pub(crate) const FEATURE_WATER: u32 = 1 << 4;

/// Mirrors `EnhancedFrame` in `enhanced/common.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct EnhancedFrameGpu {
    pub(crate) clip_from_world: Mat4,
    pub(crate) world_from_clip: Mat4,
    pub(crate) cascade_clip_from_world: [Mat4; MAX_SHADOW_CASCADES as usize],
    /// World units per shadow texel for each cascade; w = shadow fade distance.
    pub(crate) cascade_texel: Vec4,
    /// Shadow-map depth units per world block for each cascade.
    pub(crate) cascade_depth_scale: Vec4,
    /// xyz camera world position, w wrapped seconds.
    pub(crate) camera_time: Vec4,
    /// xyz unit vector toward the shadowing light, w direct strength.
    pub(crate) light_direction: Vec4,
    /// rgb direct light colour, w sky-ambient strength.
    pub(crate) light_colour: Vec4,
    /// rgb sky-ambient tint, w rain level.
    pub(crate) ambient_colour: Vec4,
    /// Width, height, and their reciprocals in physical pixels.
    pub(crate) viewport: Vec4,
    /// x warm(+)/cool(-) grade, y exposure, z bloom intensity, w shaft intensity.
    pub(crate) grade: Vec4,
    /// x feature bits, y cascade count, z shadow resolution.
    pub(crate) flags: UVec4,
    /// x reverse-Z near plane (view distance = near / depth).
    pub(crate) projection: Vec4,
}

/// Light-space box used to cull shadow casters for one cascade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CascadeBounds {
    pub(crate) light_from_world: Mat4,
    pub(crate) min: Vec3,
    pub(crate) max: Vec3,
}

impl CascadeBounds {
    /// Conservative overlap test for a world-space AABB.
    #[must_use]
    pub(crate) fn intersects_aabb(&self, center: Vec3, half_extent: Vec3) -> bool {
        let center = self.light_from_world.transform_point3(center);
        let axes = [
            self.light_from_world.x_axis.truncate(),
            self.light_from_world.y_axis.truncate(),
            self.light_from_world.z_axis.truncate(),
        ];
        let radius = Vec3::new(
            axes[0].x.abs() * half_extent.x
                + axes[1].x.abs() * half_extent.y
                + axes[2].x.abs() * half_extent.z,
            axes[0].y.abs() * half_extent.x
                + axes[1].y.abs() * half_extent.y
                + axes[2].y.abs() * half_extent.z,
            axes[0].z.abs() * half_extent.x
                + axes[1].z.abs() * half_extent.y
                + axes[2].z.abs() * half_extent.z,
        );
        (center + radius).cmpge(self.min).all() && (center - radius).cmple(self.max).all()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CascadeFit {
    pub(crate) clip_from_world: Mat4,
    pub(crate) texel_world: f32,
    pub(crate) bounds: CascadeBounds,
}

/// Far distance of each cascade (practical split scheme).
#[must_use]
pub(crate) fn cascade_splits(distance: f32, count: u32) -> [f32; MAX_SHADOW_CASCADES as usize] {
    let count = count.clamp(1, MAX_SHADOW_CASCADES);
    let distance = distance.max(FIRST_SPLIT_NEAR * 2.0);
    let mut splits = [distance; MAX_SHADOW_CASCADES as usize];
    for (index, split) in splits.iter_mut().enumerate().take(count as usize) {
        let fraction = (index + 1) as f32 / count as f32;
        let logarithmic = FIRST_SPLIT_NEAR * (distance / FIRST_SPLIT_NEAR).powf(fraction);
        let linear = FIRST_SPLIT_NEAR + (distance - FIRST_SPLIT_NEAR) * fraction;
        *split = SPLIT_LAMBDA * logarithmic + (1.0 - SPLIT_LAMBDA) * linear;
    }
    splits
}

/// Distance along the view axis and radius of the smallest axis-centred
/// sphere holding the frustum slice `near..far`, where `slope` is the
/// half-diagonal tangent of the view frustum.
#[must_use]
pub(crate) fn slice_bounding_sphere(near: f32, far: f32, slope: f32) -> (f32, f32) {
    let slope_sq = slope * slope;
    // Equidistant from the near and far corner rings, clamped to the far plane.
    let center = ((far + near) * 0.5 * (1.0 + slope_sq)).min(far);
    let far_ring = (far - center).powi(2) + far * far * slope_sq;
    let near_ring = (center - near).powi(2) + near * near * slope_sq;
    (center, far_ring.max(near_ring).sqrt())
}

/// Rotates world coordinates into the light basis.
fn light_view(light_direction: Vec3) -> Mat4 {
    let up = if light_direction.z.abs() < 0.99 {
        Vec3::Z
    } else {
        Vec3::X
    };
    Mat4::look_to_rh(Vec3::ZERO, -light_direction, up)
}

/// Rotation-invariant cascade whose light-space origin snaps to whole texels,
/// preserving the shadow grid while the camera moves.
#[must_use]
pub(crate) fn fit_cascade(
    camera: Vec3,
    forward: Vec3,
    near: f32,
    far: f32,
    slope: f32,
    light_direction: Vec3,
    resolution: u32,
) -> CascadeFit {
    let (center_distance, radius) = slice_bounding_sphere(near, far, slope);
    // Reserve half a texel for center snapping, retaining all frustum corners.
    let radius = (radius * 16.0).ceil() / 16.0;
    let radius = radius * resolution.max(2) as f32 / (resolution.max(2) - 1) as f32;
    let light_from_world = light_view(light_direction);
    let texel = 2.0 * radius / resolution.max(1) as f32;
    let mut center = light_from_world.transform_point3(camera + forward * center_distance);
    center.x = (center.x / texel).round() * texel;
    center.y = (center.y / texel).round() * texel;
    let min = Vec3::new(center.x - radius, center.y - radius, center.z - radius);
    let max = Vec3::new(
        center.x + radius,
        center.y + radius,
        center.z + radius + SHADOW_CASTER_REACH,
    );
    // View space looks down -Z, so depth distance is the negated z bound.
    let projection = Mat4::orthographic_rh(min.x, max.x, min.y, max.y, -max.z, -min.z);
    CascadeFit {
        clip_from_world: projection * light_from_world,
        texel_world: texel,
        bounds: CascadeBounds {
            light_from_world,
            min,
            max,
        },
    }
}

/// Direct and ambient light for the Enhanced lighting model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LightState {
    pub(crate) direction: Vec3,
    pub(crate) strength: f32,
    pub(crate) colour: Vec3,
    pub(crate) ambient: f32,
    pub(crate) ambient_colour: Vec3,
    pub(crate) warmth: f32,
    pub(crate) rain: f32,
}

/// Smoothly fades between two thresholds.
fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Derives the extension light palette from the current sky.
#[must_use]
pub(crate) fn light_state(atmosphere: &AtmosphereFrame) -> LightState {
    let sun = Vec3::from_array(atmosphere.sun_direction()).normalize_or(Vec3::Y);
    let moon = -sun;
    let zenith = Vec3::from_array(atmosphere.sky_zenith());
    let horizon = Vec3::from_array(atmosphere.sky_horizon());
    let rain = atmosphere.rain_level().clamp(0.0, 1.0);
    let storm = (rain * 0.75 + atmosphere.thunder_level().clamp(0.0, 1.0) * 0.25).clamp(0.0, 1.0);
    let day = smoothstep(-0.12, 0.3, sun.y);
    let (direction, mut strength, colour) = if sun.y > -0.02 {
        let colour = Vec3::new(1.0, 0.52, 0.3)
            .lerp(Vec3::new(1.0, 0.95, 0.88), smoothstep(0.0, 0.45, sun.y));
        (
            sun,
            smoothstep(-0.02, 0.12, sun.y) * 1.25 * (1.0 - 0.8 * storm),
            colour,
        )
    } else {
        let strength = smoothstep(-0.02, 0.12, moon.y) * 0.14 * (1.0 - 0.85 * storm);
        (moon, strength, Vec3::new(0.55, 0.66, 1.0))
    };
    if atmosphere.sky_kind() != crate::SkyKind::Overworld {
        strength = 0.0;
    }
    let sky = horizon.lerp(zenith, 0.6);
    let sky_tint = sky / sky.max_element().max(1.0e-3);
    let golden = 1.0 - smoothstep(0.0, 0.35, sun.y.abs());
    LightState {
        direction,
        strength,
        colour,
        ambient: 0.07 + 0.41 * day,
        ambient_colour: Vec3::ONE.lerp(sky_tint, 0.45),
        warmth: golden * day.max(0.35) - (1.0 - day) * 0.6,
        rain,
    }
}

/// Per-view inputs that do not come from the atmosphere frame.
pub(crate) struct ViewInputs {
    pub(crate) clip_from_world: Mat4,
    pub(crate) world_from_clip: Mat4,
    pub(crate) camera: Vec3,
    pub(crate) forward: Vec3,
    /// Half-diagonal tangent of the view frustum.
    pub(crate) slope: f32,
    pub(crate) near: f32,
    pub(crate) viewport: [u32; 2],
    pub(crate) seconds: f32,
}

/// Frame uniform plus per-cascade caster culling bounds.
#[must_use]
pub(crate) fn build_frame(
    view: &ViewInputs,
    settings: &EnhancedRendering,
    atmosphere: &AtmosphereFrame,
) -> (EnhancedFrameGpu, Vec<CascadeFit>) {
    let light = light_state(atmosphere);
    let cascades = settings.shadow_cascades.clamp(2, MAX_SHADOW_CASCADES);
    let resolution = settings
        .shadow_resolution
        .clamp(super::MIN_SHADOW_RESOLUTION, 4096);
    let shadows = light.strength > 0.0 && settings.shadows;
    let distance = if settings.shadow_distance.is_finite() {
        settings.shadow_distance.clamp(16.0, 256.0)
    } else {
        EnhancedRendering::default().shadow_distance
    };
    let splits = cascade_splits(distance, cascades);
    let mut fits = Vec::with_capacity(cascades as usize);
    let mut near = FIRST_SPLIT_NEAR;
    for &far in splits.iter().take(cascades as usize) {
        fits.push(fit_cascade(
            view.camera,
            view.forward,
            near,
            far,
            view.slope,
            light.direction,
            resolution,
        ));
        near = far;
    }
    let mut cascade_clip_from_world = [Mat4::IDENTITY; MAX_SHADOW_CASCADES as usize];
    let mut texel = [0.0; MAX_SHADOW_CASCADES as usize];
    let mut depth_scale = [0.0; MAX_SHADOW_CASCADES as usize];
    for (index, fit) in fits.iter().enumerate() {
        cascade_clip_from_world[index] = fit.clip_from_world;
        texel[index] = fit.texel_world;
        depth_scale[index] = 1.0 / (fit.bounds.max.z - fit.bounds.min.z).max(1.0e-3);
    }
    let mut features = 0;
    for (enabled, bit) in [
        (shadows, FEATURE_SHADOWS),
        (settings.bloom, FEATURE_BLOOM),
        (settings.light_shafts && shadows, FEATURE_SHAFTS),
        (settings.waving, FEATURE_WAVING),
        (settings.water_reflections, FEATURE_WATER),
    ] {
        if enabled {
            features |= bit;
        }
    }
    let [width, height] = view.viewport.map(|value| value.max(1) as f32);
    let frame = EnhancedFrameGpu {
        clip_from_world: view.clip_from_world,
        world_from_clip: view.world_from_clip,
        cascade_clip_from_world,
        cascade_texel: Vec4::new(texel[0], texel[1], texel[2], splits[cascades as usize - 1]),
        cascade_depth_scale: Vec4::new(depth_scale[0], depth_scale[1], depth_scale[2], 0.0),
        camera_time: view.camera.extend(view.seconds),
        light_direction: light.direction.extend(light.strength),
        light_colour: light.colour.extend(light.ambient),
        ambient_colour: light.ambient_colour.extend(light.rain),
        viewport: Vec4::new(width, height, 1.0 / width, 1.0 / height),
        grade: Vec4::new(light.warmth, 1.0, 0.08, 0.35),
        flags: UVec4::new(features, cascades, resolution, 0),
        projection: Vec4::new(view.near, 0.0, 0.0, 0.0),
    };
    (frame, fits)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLOPE: f32 = 1.147;

    /// Measures the grid phase of a fixed world point.
    fn texel_phase(fit: &CascadeFit, point: Vec3, resolution: u32) -> Vec3 {
        let ndc = fit.clip_from_world.project_point3(point);
        let texels = (ndc.truncate() * 0.5 + 0.5) * resolution as f32;
        Vec3::new(texels.x.rem_euclid(1.0), texels.y.rem_euclid(1.0), 0.0)
    }

    // A fixed world point keeps its sub-texel phase however the camera moves.
    #[test]
    fn cascades_snap_to_whole_texels_under_camera_translation_and_rotation() {
        let light = Vec3::new(0.6, 0.8, 0.0).normalize();
        let point = Vec3::new(3.3, 64.7, -9.1);
        let base = fit_cascade(
            Vec3::new(0.0, 64.0, 0.0),
            Vec3::X,
            0.1,
            24.0,
            SLOPE,
            light,
            2048,
        );
        let expected = texel_phase(&base, point, 2048);
        for step in 1..40 {
            let offset = step as f32 * 0.137;
            let yaw = step as f32 * 0.31;
            let forward = Vec3::new(yaw.cos(), -0.2, yaw.sin()).normalize();
            let fit = fit_cascade(
                Vec3::new(offset, 64.0 + offset * 0.25, -offset),
                forward,
                0.1,
                24.0,
                SLOPE,
                light,
                2048,
            );
            assert_eq!(fit.texel_world, base.texel_world);
            let phase = texel_phase(&fit, point, 2048);
            assert!(
                (phase - expected).abs().max_element() < 2.0e-2,
                "{phase} vs {expected}"
            );
        }
    }

    #[test]
    fn slice_sphere_contains_every_corner_of_its_frustum_slice() {
        for (near, far) in [(0.1, 12.0), (12.0, 32.0), (32.0, 96.0)] {
            let (center, radius) = slice_bounding_sphere(near, far, SLOPE);
            for distance in [near, far] {
                let corner = (center - distance).hypot(distance * SLOPE);
                assert!(corner <= radius + 1.0e-3, "{near}..{far}");
            }
        }
    }

    #[test]
    fn splits_increase_and_end_at_the_shadow_distance() {
        let splits = cascade_splits(96.0, 3);
        assert!(splits[0] < splits[1] && splits[1] < splits[2]);
        assert!((splits[2] - 96.0).abs() < 1.0e-3);
        assert!((cascade_splits(64.0, 1)[0] - 64.0).abs() < 1.0e-3);
    }

    #[test]
    fn cascade_bounds_keep_casters_toward_the_light_and_cull_behind() {
        let light = Vec3::Y;
        let fit = fit_cascade(Vec3::ZERO, Vec3::X, 0.1, 16.0, SLOPE, light, 1024);
        let half = Vec3::splat(8.0);
        assert!(fit.bounds.intersects_aabb(Vec3::new(8.0, 200.0, 0.0), half));
        assert!(
            !fit.bounds
                .intersects_aabb(Vec3::new(8.0, -200.0, 0.0), half)
        );
        assert!(!fit.bounds.intersects_aabb(Vec3::new(500.0, 0.0, 0.0), half));
    }

    #[cfg(feature = "enhanced-diagnostics")]
    #[test]
    fn diagnostic_limits_preserve_the_requested_shadow_target_size() {
        let settings = EnhancedRendering::bounded_diagnostic();
        let inputs = ViewInputs {
            clip_from_world: Mat4::IDENTITY,
            world_from_clip: Mat4::IDENTITY,
            camera: Vec3::ZERO,
            forward: Vec3::NEG_Z,
            slope: SLOPE,
            near: 0.1,
            viewport: render_model::ENHANCED_DIAGNOSTIC_MAX_VIEWPORT,
            seconds: 0.0,
        };
        let (frame, _) = build_frame(&inputs, &settings, &AtmosphereFrame::default());
        assert_eq!(frame.flags.z, settings.shadow_resolution);
    }

    #[test]
    fn frame_disables_shadow_sampling_when_shadows_are_off() {
        let inputs = ViewInputs {
            clip_from_world: Mat4::IDENTITY,
            world_from_clip: Mat4::IDENTITY,
            camera: Vec3::ZERO,
            forward: Vec3::NEG_Z,
            slope: SLOPE,
            near: 0.1,
            viewport: [1280, 720],
            seconds: 0.0,
        };
        let settings = EnhancedRendering {
            shadows: false,
            ..Default::default()
        };
        let (frame, _) = build_frame(&inputs, &settings, &AtmosphereFrame::default());
        assert_eq!(frame.flags.x & (FEATURE_SHADOWS | FEATURE_SHAFTS), 0);
    }

    #[test]
    fn noon_sun_is_the_bright_warm_neutral_light_and_midnight_uses_the_dim_moon() {
        let noon = light_state(&AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0));
        assert!(noon.direction.y > 0.99 && noon.strength > 1.0);
        let midnight = light_state(&AtmosphereFrame::from_bedrock_time(18_000.0, 0.0, 0.0));
        assert!(midnight.direction.y > 0.99);
        assert!(midnight.strength < 0.2 && midnight.ambient < noon.ambient);
        assert!(midnight.warmth < 0.0);
    }
}
