//! Per-view Enhanced uniforms: stable cascaded sun-shadow fitting, light
//! colours, and grading inputs derived from the atmosphere frame.

use bevy::math::{Mat4, UVec4, Vec3, Vec4};

use super::{EnhancedQualityBudget, EnhancedRendering, MAX_SHADOW_CASCADES};
use crate::AtmosphereFrame;

/// Calibrated irradiance units shared by surface, sky and cloud lighting.
pub(crate) const SOLAR_IRRADIANCE: f32 = 3.2;
pub(crate) const LUNAR_IRRADIANCE: f32 = 0.03;
const DAY_SKY_FILL: f32 = 0.14;
const NIGHT_SKY_FILL: f32 = 0.012;
const MOON_SKY_FILL: f32 = 0.014;
const WEATHER_LIGHT_LOSS: f32 = 0.72;
const DAY_INITIAL_EXPOSURE: f32 = 0.65;
const NIGHT_INITIAL_EXPOSURE: f32 = 1.4;

/// Blocks kept toward the light beyond a cascade so off-screen terrain above
/// the view (cave ceilings, mountains) still casts.
pub(crate) const SHADOW_CASTER_REACH: f32 = 384.0;
/// PCSS search/filter border shared with shaders through cascade_depth_scale.w.
pub(crate) const SHADOW_FILTER_MARGIN_TEXELS: f32 = 10.0;
const SHADOW_RECEIVER_GUARD_TEXELS: f32 = 1.5;
const SHADOW_DEPTH_GRID_BLOCKS: f32 = 1.0;
/// Log/linear blend of the practical split scheme.
const SPLIT_LAMBDA: f32 = 0.75;
const FIRST_SPLIT_NEAR: f32 = 0.1;

pub(crate) const FEATURE_SHADOWS: u32 = 1 << 0;
pub(crate) const FEATURE_BLOOM: u32 = 1 << 1;
pub(crate) const FEATURE_SHAFTS: u32 = 1 << 2;
pub(crate) const FEATURE_WAVING: u32 = 1 << 3;
pub(crate) const FEATURE_WATER: u32 = 1 << 4;
pub(crate) const FEATURE_PBR: u32 = 1 << 5;
pub(crate) const FEATURE_SSAO: u32 = 1 << 6;
pub(crate) const FEATURE_VOLUMETRIC_CLOUDS: u32 = 1 << 7;

/// Mirrors `EnhancedFrame` in `enhanced/common.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct EnhancedFrameGpu {
    pub(crate) clip_from_world: Mat4,
    pub(crate) world_from_clip: Mat4,
    pub(crate) cascade_clip_from_world: [Mat4; MAX_SHADOW_CASCADES as usize],
    /// World units per shadow texel for each cascade; w = shadow fade distance.
    pub(crate) cascade_texel: Vec4,
    /// xyz depth units per block; w shared filter border in shadow texels.
    pub(crate) cascade_depth_scale: Vec4,
    /// xyz receiver radii; w common physical PCSS border in blocks.
    pub(crate) cascade_receiver_radius: Vec4,
    /// xyz camera world position, w wrapped seconds.
    pub(crate) camera_time: Vec4,
    /// xyz unit vector toward the shadowing light, w direct strength.
    pub(crate) light_direction: Vec4,
    /// rgb direct light colour, w sky-ambient strength.
    pub(crate) light_colour: Vec4,
    /// rgb sky-ambient tint, w rain level.
    pub(crate) ambient_colour: Vec4,
    /// Linear sky colours used for the low-cost environment specular fallback.
    pub(crate) sky_zenith: Vec4,
    pub(crate) sky_horizon: Vec4,
    /// Width, height, and their reciprocals in physical pixels.
    pub(crate) viewport: Vec4,
    /// x warm(+)/cool(-) grade, y exposure, z bloom intensity, w shaft intensity.
    pub(crate) grade: Vec4,
    /// x feature bits, y cascade count, z shadow resolution.
    pub(crate) flags: UVec4,
    /// x reverse-Z near; yz solar/lunar irradiance; w visibility textures ready.
    pub(crate) projection: Vec4,
    /// x cloud coverage, y base height, z layer thickness, w wind offset in blocks.
    pub(crate) clouds: Vec4,
    /// Previous submitted jittered projection for depth reprojection.
    pub(crate) previous_clip_from_world: Mat4,
    /// xyz sun direction, w vanilla moon phase.
    pub(crate) celestial: Vec4,
    /// x Overworld sky, y delta seconds, z fog end, w underwater.
    pub(crate) atmosphere: Vec4,
    /// x sample index, y usable history, z delta seconds, w diagnostic mode.
    pub(crate) temporal: Vec4,
    /// xyz reflection probe origin, w capture radius (negative during capture).
    pub(crate) probe: Vec4,
    /// xy cloud-shadow centre, z world span, w usable map.
    pub(crate) cloud_shadow: Vec4,
    /// SSR, directional PCF, blocker and point-shadow filter sample budgets.
    pub(crate) quality: Vec4,
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

/// Rotates world coordinates into the light basis.
fn light_view(light_direction: Vec3) -> Mat4 {
    let up = if light_direction.z.abs() < 0.99 {
        Vec3::Z
    } else {
        Vec3::X
    };
    Mat4::look_to_rh(Vec3::ZERO, -light_direction, up)
}

fn receiver_radius(radius: f32) -> f32 {
    (radius.max(FIRST_SPLIT_NEAR) * 16.0).ceil() / 16.0
}

fn shadow_filter_border(receiver_radius: f32, resolution: u32) -> f32 {
    let denominator =
        resolution as f32 - 2.0 * (SHADOW_FILTER_MARGIN_TEXELS + SHADOW_RECEIVER_GUARD_TEXELS);
    2.0 * receiver_radius * SHADOW_FILTER_MARGIN_TEXELS / denominator.max(1.0)
}

/// Nested receiver spheres share a camera-centred, texel-snapped light basis.
/// Radial coverage matches the receiver fade and cannot move when the view turns.
#[must_use]
pub(crate) fn fit_cascade(
    camera: Vec3,
    receiver_radius: f32,
    light_direction: Vec3,
    resolution: u32,
) -> CascadeFit {
    let resolution = resolution
        .max((2.0 * (SHADOW_FILTER_MARGIN_TEXELS + SHADOW_RECEIVER_GUARD_TEXELS) + 2.0) as u32);
    let radius = self::receiver_radius(receiver_radius);
    fit_cascade_with_border(
        camera,
        radius,
        light_direction,
        resolution,
        shadow_filter_border(radius, resolution),
    )
}

/// Fits a receiver sphere with the complete cascade family's sampling border.
pub(crate) fn fit_cascade_with_border(
    camera: Vec3,
    receiver_radius: f32,
    light_direction: Vec3,
    resolution: u32,
    filter_border: f32,
) -> CascadeFit {
    // All cascades cover the same physical filter footprint through handovers.
    let radius = (receiver_radius + filter_border) * resolution as f32
        / (resolution as f32 - 2.0 * SHADOW_RECEIVER_GUARD_TEXELS);
    let light_from_world = light_view(light_direction);
    let texel = 2.0 * radius / resolution.max(1) as f32;
    let mut center = light_from_world.transform_point3(camera);
    center.x = (center.x / texel).round() * texel;
    center.y = (center.y / texel).round() * texel;
    center.z = (center.z / SHADOW_DEPTH_GRID_BLOCKS).round() * SHADOW_DEPTH_GRID_BLOCKS;
    let depth_radius = radius + 0.5 * SHADOW_DEPTH_GRID_BLOCKS;
    let min = Vec3::new(
        center.x - radius,
        center.y - radius,
        center.z - depth_radius,
    );
    let max = Vec3::new(
        center.x + radius,
        center.y + radius,
        center.z + depth_radius + SHADOW_CASTER_REACH,
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
    pub(crate) exposure: f32,
    pub(crate) solar_source: f32,
    pub(crate) lunar_source: f32,
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
    let phase = (0.5
        + 0.5 * (f32::from(atmosphere.moon_phase()) * std::f32::consts::FRAC_PI_4).cos())
    .clamp(0.0, 1.0);
    let weather = 1.0 - WEATHER_LIGHT_LOSS * storm;
    let moon_access = smoothstep(-0.02, 0.25, moon.y) * phase;
    let (direction, mut strength, colour) = if sun.y > -0.02 {
        (
            sun,
            smoothstep(-0.02, 0.12, sun.y) * SOLAR_IRRADIANCE * weather,
            Vec3::ONE,
        )
    } else {
        let strength = moon_access * LUNAR_IRRADIANCE * weather;
        (moon, strength, Vec3::ONE)
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
        ambient: ((NIGHT_SKY_FILL + MOON_SKY_FILL * moon_access) * (1.0 - day)
            + DAY_SKY_FILL * day)
            * weather,
        ambient_colour: Vec3::ONE.lerp(sky_tint, 0.45),
        warmth: golden * day.max(0.35) - (1.0 - day) * 0.6,
        rain: storm,
        exposure: NIGHT_INITIAL_EXPOSURE + (DAY_INITIAL_EXPOSURE - NIGHT_INITIAL_EXPOSURE) * day,
        solar_source: SOLAR_IRRADIANCE * weather,
        lunar_source: LUNAR_IRRADIANCE * weather,
    }
}

/// Applies the user's cloud preference without changing the camera's configured options.
pub(crate) fn applied_cloud_settings(
    mut settings: EnhancedRendering,
    visibility: Option<&crate::CloudVisibility>,
) -> EnhancedRendering {
    settings.volumetric_clouds &= visibility.is_none_or(|visibility| visibility.0);
    settings
}

/// Per-view inputs that do not come from the atmosphere frame.
pub(crate) struct ViewInputs {
    pub(crate) clip_from_world: Mat4,
    pub(crate) world_from_clip: Mat4,
    pub(crate) camera: Vec3,
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
    let receiver_radii = splits.map(receiver_radius);
    let filter_border = shadow_filter_border(receiver_radii[cascades as usize - 1], resolution);
    let mut fits = Vec::new();
    if !settings.reflection_capture {
        fits.reserve(cascades as usize);
        for (index, &radius) in receiver_radii.iter().take(cascades as usize).enumerate() {
            fits.push(if index + 1 == cascades as usize {
                fit_cascade(view.camera, radius, light.direction, resolution)
            } else {
                fit_cascade_with_border(
                    view.camera,
                    radius,
                    light.direction,
                    resolution,
                    filter_border,
                )
            });
        }
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
        (settings.physically_based, FEATURE_PBR),
        (settings.ssao, FEATURE_SSAO),
        (settings.volumetric_clouds, FEATURE_VOLUMETRIC_CLOUDS),
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
        cascade_depth_scale: Vec4::new(
            depth_scale[0],
            depth_scale[1],
            depth_scale[2],
            SHADOW_FILTER_MARGIN_TEXELS,
        ),
        cascade_receiver_radius: Vec4::new(
            receiver_radii[0],
            receiver_radii[1],
            receiver_radii[2],
            filter_border,
        ),
        camera_time: view.camera.extend(view.seconds),
        light_direction: light.direction.extend(light.strength),
        light_colour: light.colour.extend(light.ambient),
        ambient_colour: light.ambient_colour.extend(light.rain),
        sky_zenith: Vec3::from_array(atmosphere.sky_zenith()).extend(1.0),
        sky_horizon: Vec3::from_array(atmosphere.sky_horizon()).extend(1.0),
        viewport: Vec4::new(width, height, 1.0 / width, 1.0 / height),
        grade: Vec4::new(light.warmth, light.exposure, 0.08, 0.35),
        flags: UVec4::new(
            features,
            cascades,
            resolution,
            settings.quality.cloud_steps(),
        ),
        projection: Vec4::new(view.near, light.solar_source, light.lunar_source, 0.0),
        clouds: Vec4::new(
            (0.18 + 0.52 * light.rain).clamp(0.12, 0.85),
            meshing::CLOUD_UNDERSIDE_Y,
            // The native mesh is four blocks thick; the Enhanced layer uses a
            // deeper participating medium so the post pass has real volume.
            meshing::CLOUD_THICKNESS_BLOCKS.max(96.0),
            view.seconds * 0.72,
        ),
        previous_clip_from_world: view.clip_from_world,
        celestial: Vec3::from_array(atmosphere.sun_direction())
            .extend(f32::from(atmosphere.moon_phase())),
        atmosphere: Vec4::new(
            f32::from(u8::from(atmosphere.sky_kind() == crate::SkyKind::Overworld)),
            0.0,
            atmosphere.fog_end(),
            f32::from(u8::from(
                atmosphere.camera_medium() == meshing::CameraMedium::Water,
            )),
        ),
        temporal: Vec4::ZERO,
        probe: view.camera.extend(32.0),
        cloud_shadow: Vec4::ZERO,
        quality: Vec4::from_array(settings.quality.surface_samples()),
    };
    (frame, fits)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Measures the grid phase of a fixed world point.
    fn texel_phase(fit: &CascadeFit, point: Vec3, resolution: u32) -> Vec3 {
        let ndc = fit.clip_from_world.project_point3(point);
        let texels = (ndc.truncate() * 0.5 + 0.5) * resolution as f32;
        Vec3::new(texels.x.rem_euclid(1.0), texels.y.rem_euclid(1.0), 0.0)
    }

    #[test]
    fn cascades_snap_to_whole_texels_under_camera_translation() {
        let light = Vec3::new(0.6, 0.8, 0.0).normalize();
        let point = Vec3::new(3.3, 64.7, -9.1);
        let base = fit_cascade(Vec3::new(0.0, 64.0, 0.0), 24.0, light, 2048);
        let expected = texel_phase(&base, point, 2048);
        for step in 1..40 {
            let offset = step as f32 * 0.137;
            let fit = fit_cascade(
                Vec3::new(offset, 64.0 + offset * 0.25, -offset),
                24.0,
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
    fn receiver_coverage_and_caster_bounds_stay_fixed_through_yaw_pitch_and_fov() {
        let camera = Vec3::new(4.23, 64.91, -7.13);
        let settings = EnhancedRendering::default();
        let atmosphere = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
        let make_inputs = |yaw: f32, pitch: f32, fov: f32| {
            let forward = Vec3::new(
                yaw.sin() * pitch.cos(),
                pitch.sin(),
                -yaw.cos() * pitch.cos(),
            );
            let clip = Mat4::perspective_infinite_reverse_rh(fov.to_radians(), 16.0 / 9.0, 0.1)
                * Mat4::look_to_rh(camera, forward, Vec3::Y);
            ViewInputs {
                clip_from_world: clip,
                world_from_clip: clip.inverse(),
                camera,
                near: 0.1,
                viewport: [1280, 720],
                seconds: 0.0,
            }
        };
        let (_, expected) = build_frame(&make_inputs(0.0, 0.0, 70.0), &settings, &atmosphere);
        for yaw in [0.0, 0.7, 1.5, 3.1, 5.7] {
            for pitch in [-1.2, 0.0, 1.2] {
                for fov in [45.0, 70.0, 110.0] {
                    let (_, fits) =
                        build_frame(&make_inputs(yaw, pitch, fov), &settings, &atmosphere);
                    assert_eq!(fits, expected, "rotation cannot replace caster coverage");
                }
            }
        }
    }

    #[test]
    fn receiver_spheres_stay_inside_the_shadow_filter_border_in_every_direction() {
        let camera = Vec3::new(4.23, 64.91, -7.13);
        for resolution in [256, 1024, 4096] {
            let margin = SHADOW_FILTER_MARGIN_TEXELS / resolution as f32;
            for radius in [8.0, 32.0, 96.0] {
                for light in [
                    Vec3::X,
                    Vec3::Y,
                    Vec3::Z,
                    Vec3::new(0.7, 0.6, 0.2).normalize(),
                ] {
                    let fit = fit_cascade(camera, radius, light, resolution);
                    for x in -3..=3 {
                        for y in -3..=3 {
                            for z in -3..=3 {
                                let direction =
                                    Vec3::new(x as f32, y as f32, z as f32).normalize_or(Vec3::X);
                                let point = camera + direction * radius;
                                let clip = fit.clip_from_world.project_point3(point);
                                let uv = clip.truncate() * 0.5 + bevy::math::Vec2::splat(0.5);
                                assert!(uv.cmpgt(bevy::math::Vec2::splat(margin)).all());
                                assert!(uv.cmplt(bevy::math::Vec2::splat(1.0 - margin)).all());
                                assert!((0.0..=1.0).contains(&clip.z));
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn subtexel_translation_keeps_the_shadow_projection_and_caster_grid() {
        let light = Vec3::new(-0.45, 0.7, 0.55).normalize();
        let fit = fit_cascade(Vec3::ZERO, 32.0, light, 1024);
        let translation = fit
            .bounds
            .light_from_world
            .inverse()
            .transform_vector3(Vec3::new(
                0.24 * fit.texel_world,
                0.24 * fit.texel_world,
                0.0,
            ));
        let moved = fit_cascade(translation, 32.0, light, 1024);
        for (a, b) in fit
            .clip_from_world
            .to_cols_array()
            .iter()
            .zip(moved.clip_from_world.to_cols_array())
        {
            assert!((a - b).abs() < 1.0e-6);
        }
        let receiver = Vec3::new(8.0, 0.0, -4.0);
        for caster in [receiver + light * 300.0, receiver + light * 350.0] {
            assert!(fit.bounds.intersects_aabb(caster, Vec3::splat(0.5)));
            assert!(moved.bounds.intersects_aabb(caster, Vec3::splat(0.5)));
        }
    }

    #[test]
    fn shadow_projection_orders_toward_light_casters_before_receivers() {
        for light in [Vec3::Y, Vec3::new(-0.45, 0.7, 0.55).normalize()] {
            let fit = fit_cascade(Vec3::new(0.0, 70.0, 0.0), 32.0, light, 1024);
            let receiver = Vec3::new(0.0, 70.0, -16.0);
            let caster = receiver + light * 100.0;
            let front = fit.clip_from_world.project_point3(caster);
            let back = fit.clip_from_world.project_point3(receiver);
            assert!((0.0..=1.0).contains(&front.z));
            assert!((0.0..=1.0).contains(&back.z));
            assert!(front.z < back.z, "LessEqual must accept the nearer caster");
            assert!((front.truncate() - back.truncate()).length() < 1.0e-4);
            assert!(fit.bounds.intersects_aabb(caster, Vec3::splat(0.5)));
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
        let fit = fit_cascade(Vec3::ZERO, 16.0, light, 1024);
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
        let settings = EnhancedRendering::default().bounded_diagnostic();
        let inputs = ViewInputs {
            clip_from_world: Mat4::IDENTITY,
            world_from_clip: Mat4::IDENTITY,
            camera: Vec3::ZERO,
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
        assert_eq!(frame.flags.w, settings.quality.cloud_steps());
        assert_eq!(frame.projection.y, SOLAR_IRRADIANCE);
        assert_eq!(frame.projection.z, LUNAR_IRRADIANCE);
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

    #[test]
    fn celestial_sources_share_weather_units_and_moon_phase_preserves_a_night_floor() {
        let day = crate::atmosphere::BEDROCK_DAY_TICKS;
        let noon = light_state(&AtmosphereFrame::from_bedrock_time(day * 0.25, 0.0, 0.0));
        let midnight = light_state(&AtmosphereFrame::from_bedrock_time(day * 0.75, 0.0, 0.0));
        let new_moon = light_state(&AtmosphereFrame::from_bedrock_time(day * 4.75, 0.0, 0.0));
        let storm = light_state(&AtmosphereFrame::from_bedrock_time(day * 0.25, 1.0, 1.0));
        assert_eq!(noon.solar_source, SOLAR_IRRADIANCE);
        assert_eq!(midnight.lunar_source, LUNAR_IRRADIANCE);
        assert!(noon.strength <= noon.solar_source);
        assert!(midnight.strength <= midnight.lunar_source);
        assert!(new_moon.strength < midnight.strength * 0.01);
        assert!(new_moon.ambient > 0.0 && new_moon.ambient < midnight.ambient);
        assert!(midnight.ambient < noon.ambient * 0.25);
        assert!(storm.solar_source < noon.solar_source * 0.5);
        assert!(
            (storm.solar_source / storm.lunar_source - noon.solar_source / noon.lunar_source).abs()
                < 0.001
        );
        assert_eq!(noon.colour, Vec3::ONE);
        assert_eq!(midnight.colour, Vec3::ONE);
    }
}
