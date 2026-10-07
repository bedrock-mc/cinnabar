//! Submitted-frame history and deterministic subpixel sampling.

use bevy::render::{camera::TemporalJitter, view::ExtractedView};
use bevy::{
    math::{Mat4, Vec2, Vec3},
    prelude::*,
};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{EnhancedRendering, frame::EnhancedFrameGpu};

const HISTORY_WEATHER_CUT: f32 = 0.15;
const HISTORY_CLOUD_COVERAGE_CUT: f32 = 0.1;
const HISTORY_CLOUD_LAYER_CUT: f32 = 0.1;
const HISTORY_LIGHT_RELATIVE_CUT: f32 = 0.2;

#[derive(Clone, Copy, Default)]
struct EnvironmentHistory {
    weather: f32,
    coverage: f32,
    base: f32,
    thickness: f32,
    direct: f32,
    sky: f32,
    underwater: f32,
}

impl EnvironmentHistory {
    fn from_frame(frame: &EnhancedFrameGpu) -> Self {
        Self {
            weather: frame.ambient_colour.w,
            coverage: frame.clouds.x,
            base: frame.clouds.y,
            thickness: frame.clouds.z,
            direct: frame.light_direction.w,
            sky: frame.atmosphere.x,
            underwater: frame.atmosphere.w,
        }
    }

    fn compatible(self, current: Self) -> bool {
        let thickness = self.thickness.abs().min(current.thickness.abs()).max(1.0);
        (self.weather - current.weather).abs() <= HISTORY_WEATHER_CUT
            && (self.coverage - current.coverage).abs() <= HISTORY_CLOUD_COVERAGE_CUT
            && (self.base - current.base).abs() <= (thickness * HISTORY_CLOUD_LAYER_CUT).max(2.0)
            && (self.thickness - current.thickness).abs() <= thickness * HISTORY_CLOUD_LAYER_CUT
            && (self.direct - current.direct).abs()
                <= 0.005 + self.direct.max(current.direct) * HISTORY_LIGHT_RELATIVE_CUT
            && self.sky == current.sky
            && self.underwater == current.underwater
    }
}

pub(crate) fn sample_offset(index: u32) -> Vec2 {
    fn radical_inverse(mut index: u32, base: u32) -> f32 {
        let mut value = 0.0;
        let mut scale = 1.0 / base as f32;
        while index > 0 {
            value += (index % base) as f32 * scale;
            index /= base;
            scale /= base as f32;
        }
        value
    }
    let sample = index % 16 + 1;
    Vec2::new(radical_inverse(sample, 2), radical_inverse(sample, 3)) - Vec2::splat(0.5)
}

pub(crate) fn prepare_jitter(
    mut views: Query<(&EnhancedRendering, &mut TemporalJitter)>,
    mut index: Local<u32>,
) {
    for (settings, mut jitter) in &mut views {
        jitter.offset = if settings.temporal_aa
            && !settings.reflection_capture
            && settings.shadow_debug == super::EnhancedShadowDebug::Off
        {
            sample_offset(*index)
        } else {
            Vec2::ZERO
        };
    }
    *index = index.wrapping_add(1);
}

pub(crate) fn temporal_history_allowed(
    submitted_valid: bool,
    settings_changed: bool,
    settings: &EnhancedRendering,
) -> bool {
    submitted_valid
        && !settings_changed
        && settings.temporal_aa
        && !settings.reflection_capture
        && settings.shadow_debug == super::EnhancedShadowDebug::Off
}

#[derive(Default)]
pub(crate) struct HistoryState {
    pub(crate) submitted: AtomicBool,
    pub(crate) previous: Mat4,
    pub(crate) camera: Vec3,
    pub(crate) forward: Vec3,
    pub(crate) size: [u32; 2],
    pub(crate) light: Vec3,
    pub(crate) seconds: f32,
    pub(crate) index: u32,
    environment: EnvironmentHistory,
}

impl HistoryState {
    pub(crate) fn advance(
        &mut self,
        view: &ExtractedView,
        clip: Mat4,
        frame: &EnhancedFrameGpu,
        seconds: f32,
    ) -> bool {
        let camera = view.world_from_view.translation();
        let forward = view.world_from_view.forward().as_vec3();
        let size = [view.viewport.z, view.viewport.w];
        let dt = seconds - self.seconds;
        let light = frame.light_direction.truncate();
        let environment = EnvironmentHistory::from_frame(frame);
        let valid = self.submitted.swap(false, Ordering::Relaxed)
            && self.environment.compatible(environment)
            && history_compatible(
                self.camera,
                camera,
                self.forward,
                forward,
                self.size,
                size,
                self.light,
                light,
                dt,
            );
        self.previous = clip;
        self.camera = camera;
        self.forward = forward;
        self.size = size;
        self.light = light;
        self.environment = environment;
        self.seconds = seconds;
        self.index = self.index.wrapping_add(1);
        valid
    }
}

fn history_compatible(
    old: Vec3,
    new: Vec3,
    old_forward: Vec3,
    forward: Vec3,
    old_size: [u32; 2],
    size: [u32; 2],
    old_light: Vec3,
    light: Vec3,
    dt: f32,
) -> bool {
    old_size == size
        && dt > 0.0
        && dt < 0.25
        && old.distance_squared(new) < 16.0
        && old_forward.dot(forward) > 0.75
        && old_light.dot(light) > 0.995
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submitted_environment_rejects_weather_layer_and_light_cuts_but_retains_wind() {
        let mut frame: EnhancedFrameGpu = bytemuck::Zeroable::zeroed();
        frame.clouds = Vec4::new(0.18, 192.0, 96.0, 0.0);
        frame.light_direction = Vec3::Y.extend(super::super::frame::SOLAR_IRRADIANCE);
        frame.atmosphere.x = 1.0;
        let baseline = EnvironmentHistory::from_frame(&frame);
        let accepts = |changed: &EnhancedFrameGpu| {
            baseline.compatible(EnvironmentHistory::from_frame(changed))
        };
        let mut wind = frame;
        wind.clouds.w = 4096.0;
        wind.camera_time.w = 100.0;
        wind.temporal.x = 71.0;
        assert!(accepts(&wind), "cloud advection retains submitted history");
        let mut gradual = wind;
        gradual.ambient_colour.w = 0.05;
        gradual.clouds.x += 0.02;
        gradual.clouds.y += 1.0;
        gradual.clouds.z += 2.0;
        gradual.light_direction.w *= 0.98;
        assert!(accepts(&gradual));
        for changed in [
            {
                let mut value = frame;
                value.ambient_colour.w = 0.6;
                value
            },
            {
                let mut value = frame;
                value.clouds.x = 0.7;
                value
            },
            {
                let mut value = frame;
                value.clouds.y += 48.0;
                value
            },
            {
                let mut value = frame;
                value.clouds.z *= 1.5;
                value
            },
            {
                let mut value = frame;
                value.light_direction.w *= 0.1;
                value
            },
            {
                let mut value = frame;
                value.atmosphere.w = 1.0;
                value
            },
        ] {
            assert!(
                !accepts(&changed),
                "stale cloud/lighting silhouettes must reset"
            );
        }
    }

    #[test]
    fn jitter_stops_for_debug_and_capture_then_resumes_after_debug() {
        let mut app = App::new();
        app.add_systems(Update, prepare_jitter);
        let camera = app.world_mut().spawn(EnhancedRendering::default()).id();
        app.update();
        assert_ne!(
            app.world().get::<TemporalJitter>(camera).unwrap().offset,
            Vec2::ZERO,
        );
        app.world_mut()
            .get_mut::<EnhancedRendering>(camera)
            .unwrap()
            .shadow_debug = super::super::EnhancedShadowDebug::Cascades;
        app.update();
        assert_eq!(
            app.world().get::<TemporalJitter>(camera).unwrap().offset,
            Vec2::ZERO,
        );
        app.world_mut()
            .get_mut::<EnhancedRendering>(camera)
            .unwrap()
            .shadow_debug = super::super::EnhancedShadowDebug::Off;
        app.update();
        assert_ne!(
            app.world().get::<TemporalJitter>(camera).unwrap().offset,
            Vec2::ZERO,
        );
        app.world_mut()
            .get_mut::<EnhancedRendering>(camera)
            .unwrap()
            .reflection_capture = true;
        app.update();
        assert_eq!(
            app.world().get::<TemporalJitter>(camera).unwrap().offset,
            Vec2::ZERO,
        );
    }

    #[test]
    fn history_rejects_debug_frames_and_the_first_frame_after_a_mode_change() {
        let mut settings = EnhancedRendering::default();
        assert!(temporal_history_allowed(true, false, &settings));
        settings.shadow_debug = super::super::EnhancedShadowDebug::Visibility;
        assert!(!temporal_history_allowed(true, false, &settings));
        settings.shadow_debug = super::super::EnhancedShadowDebug::Off;
        assert!(!temporal_history_allowed(true, true, &settings));
        assert!(temporal_history_allowed(true, false, &settings));
        assert!(!temporal_history_allowed(false, false, &settings));
        settings.reflection_capture = true;
        assert!(!temporal_history_allowed(true, false, &settings));
        settings.reflection_capture = false;
        settings.temporal_aa = false;
        assert!(!temporal_history_allowed(true, false, &settings));
    }

    #[test]
    fn temporal_samples_cover_both_sides_without_leaving_a_pixel() {
        let offsets: Vec<_> = (0..16).map(sample_offset).collect();
        assert!(offsets.iter().all(|v| v.abs().max_element() <= 0.5));
        assert!(offsets.iter().any(|v| v.x > 0.0 && v.y > 0.0));
        assert!(offsets.iter().any(|v| v.x < 0.0 && v.y < 0.0));
    }

    #[test]
    fn history_rejects_resize_teleport_and_lighting_cuts() {
        let accepted = |new, size, light| {
            history_compatible(
                Vec3::ZERO,
                new,
                Vec3::NEG_Z,
                Vec3::NEG_Z,
                [1920, 1080],
                size,
                Vec3::Y,
                light,
                1.0 / 60.0,
            )
        };
        assert!(accepted(Vec3::X * 0.1, [1920, 1080], Vec3::Y));
        assert!(!accepted(Vec3::X * 5.0, [1920, 1080], Vec3::Y));
        assert!(!accepted(Vec3::ZERO, [1280, 720], Vec3::Y));
        assert!(!accepted(Vec3::ZERO, [1920, 1080], Vec3::X));
    }
}
