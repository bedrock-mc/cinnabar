//! Persistent atmospheric passes refresh only after a bounded input change.

use std::sync::Mutex;

use bevy::math::{Vec2, Vec3, Vec3Swizzles, Vec4};

use super::{
    EnhancedQuality,
    frame::{EnhancedFrameGpu, FEATURE_VOLUMETRIC_CLOUDS},
    quality::budget,
};

pub(crate) const SKY_LUT_SIZE: [u32; 2] = [192, 108];
#[cfg(test)]
pub(crate) const CLOUD_SHADOW_SIZE: u32 = 128;
const SKY_DIRECTION_TOLERANCE: f32 = 0.0003;
const SKY_ALTITUDE_TOLERANCE: f32 = 0.5;
const CLOUD_SHADOW_TEXEL_TOLERANCE: f32 = 0.25;
const CLOUD_SLOPE_TOLERANCE: f32 = 0.002;

#[derive(Clone, Copy, PartialEq)]
struct SkyKey {
    sun: Vec3,
    phase: f32,
    rain: f32,
    altitude: f32,
    overworld: bool,
    horizon: Vec4,
    zenith: Vec4,
    source_irradiance: Vec2,
    sky_fill: f32,
}

impl SkyKey {
    fn from_frame(frame: &EnhancedFrameGpu) -> Self {
        Self {
            sun: frame.celestial.truncate(),
            phase: frame.celestial.w,
            rain: frame.ambient_colour.w,
            altitude: frame.camera_time.y.clamp(2.0, 250_000.0),
            overworld: frame.atmosphere.x > 0.5,
            horizon: frame.sky_horizon,
            zenith: frame.sky_zenith,
            source_irradiance: Vec2::new(frame.projection.y, frame.projection.z),
            sky_fill: frame.light_colour.w,
        }
    }

    fn same_sources(self, next: Self) -> bool {
        if self.overworld != next.overworld {
            return false;
        }
        if !self.overworld {
            return self.horizon == next.horizon && self.zenith == next.zenith;
        }
        self.phase == next.phase
            && self.rain == next.rain
            && self.source_irradiance == next.source_irradiance
            && self.sky_fill == next.sky_fill
    }

    fn reusable(self, next: Self) -> bool {
        self.same_sources(next)
            && (!self.overworld
                || ((self.altitude - next.altitude).abs() <= SKY_ALTITUDE_TOLERANCE
                    && self.sun.distance(next.sun) <= SKY_DIRECTION_TOLERANCE))
    }
}

#[derive(Clone, Copy, PartialEq)]
struct CloudEnvironmentKey {
    layer: Vec3,
    position: Vec3,
    wind: f32,
    view_steps: u32,
}

#[derive(Clone, Copy)]
struct EnvironmentKey {
    sky: SkyKey,
    clouds: Option<CloudEnvironmentKey>,
    seconds: f32,
    refresh: f32,
}

impl EnvironmentKey {
    fn from_frame(frame: &EnhancedFrameGpu, quality: EnhancedQuality) -> Self {
        let enabled = frame.atmosphere.x > 0.5
            && frame.flags.x & FEATURE_VOLUMETRIC_CLOUDS != 0
            && (frame.clouds.x > 0.0 || frame.ambient_colour.w > 0.0);
        Self {
            sky: SkyKey::from_frame(frame),
            clouds: enabled.then_some(CloudEnvironmentKey {
                layer: frame.clouds.truncate(),
                position: frame.camera_time.truncate(),
                wind: frame.clouds.w,
                view_steps: frame.flags.w,
            }),
            seconds: frame.camera_time.w,
            refresh: budget(quality).cloud_environment_refresh,
        }
    }

    fn reusable(self, next: Self) -> bool {
        if !self.sky.same_sources(next.sky) {
            return false;
        }
        match (self.clouds, next.clouds) {
            (Some(previous), Some(requested)) => {
                if previous.layer != requested.layer
                    || previous.view_steps != requested.view_steps
                    || self.refresh != next.refresh
                {
                    return false;
                }
                if previous == requested && self.sky.reusable(next.sky) {
                    return true;
                }
                let elapsed = next.seconds - self.seconds;
                elapsed >= 0.0 && elapsed < next.refresh
            }
            (None, None) => self.sky.reusable(next.sky),
            _ => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct CloudShadowKey {
    enabled: bool,
    layer: Vec3,
    wind: f32,
    rain: f32,
    direction: Vec3,
    receiver_height: f32,
    center: Vec2,
    span: f32,
    resolution: u32,
}

impl CloudShadowKey {
    fn from_frame(frame: &EnhancedFrameGpu, resolution: u32) -> Self {
        Self {
            enabled: frame.atmosphere.x > 0.5
                && frame.flags.x & FEATURE_VOLUMETRIC_CLOUDS != 0
                && frame.light_direction.w > 0.0
                && (frame.clouds.x > 0.0 || frame.ambient_colour.w > 0.0),
            layer: frame.clouds.truncate(),
            wind: frame.clouds.w,
            rain: frame.ambient_colour.w,
            direction: frame.light_direction.truncate(),
            receiver_height: frame.camera_time.y.min(frame.clouds.y),
            center: frame.cloud_shadow.truncate().truncate(),
            span: frame.cloud_shadow.z,
            resolution,
        }
    }

    fn projected_layer(self) -> Vec2 {
        let midpoint = self.layer.y + self.layer.z * 0.5;
        self.direction.xz() / self.direction.y.max(0.01)
            * (midpoint - self.receiver_height).max(0.0)
    }

    fn reusable(self, next: Self) -> bool {
        if self.enabled != next.enabled {
            return false;
        }
        if !self.enabled {
            return true;
        }
        if self.layer != next.layer
            || self.rain != next.rain
            || self.center != next.center
            || self.span != next.span
            || self.resolution != next.resolution
            || (self.direction.y > 0.01) != (next.direction.y > 0.01)
        {
            return false;
        }
        if self.direction.y <= 0.01 {
            return true;
        }
        if (self.receiver_height >= self.layer.y || next.receiver_height >= next.layer.y)
            && self.receiver_height != next.receiver_height
        {
            return false;
        }
        let light_drift = self.projected_layer().distance(next.projected_layer());
        let wind_drift = (self.wind - next.wind).abs() * 2.0;
        let texel = self.span / self.resolution.max(1) as f32;
        let slope_change = (self.direction.y - next.direction.y).abs()
            / self.direction.y.max(next.direction.y).max(0.01);
        light_drift + wind_drift < texel * CLOUD_SHADOW_TEXEL_TOLERANCE
            && slope_change < CLOUD_SLOPE_TOLERANCE
    }
}

#[derive(Default)]
struct AtmosphericKeys {
    requested_sky: Option<SkyKey>,
    rendered_sky: Option<SkyKey>,
    requested_cloud: Option<CloudShadowKey>,
    rendered_cloud: Option<CloudShadowKey>,
    requested_environment: Option<EnvironmentKey>,
    rendered_environment: Option<EnvironmentKey>,
    environment_generation: u64,
}

#[derive(Default)]
pub(crate) struct AtmosphereCache(Mutex<AtmosphericKeys>);

impl AtmosphereCache {
    pub(crate) fn invalidate_radiance(&self) {
        let mut keys = self.0.lock().expect("atmospheric pass cache");
        keys.rendered_sky = None;
        keys.rendered_environment = None;
    }

    pub(crate) fn invalidate_environment(&self) {
        self.0
            .lock()
            .expect("atmospheric pass cache")
            .rendered_environment = None;
    }
    #[cfg(test)]
    pub(crate) fn prepare(&self, frame: &EnhancedFrameGpu) {
        self.prepare_with_quality(frame, CLOUD_SHADOW_SIZE, EnhancedQuality::default());
    }

    pub(crate) fn prepare_with_quality(
        &self,
        frame: &EnhancedFrameGpu,
        resolution: u32,
        quality: EnhancedQuality,
    ) {
        let mut keys = self.0.lock().expect("atmospheric pass cache");
        keys.requested_sky = Some(SkyKey::from_frame(frame));
        keys.requested_cloud = Some(CloudShadowKey::from_frame(frame, resolution));
        keys.requested_environment = Some(EnvironmentKey::from_frame(frame, quality));
    }

    pub(crate) fn sky_needs_update(&self) -> bool {
        let keys = self.0.lock().expect("atmospheric pass cache");
        match (keys.rendered_sky, keys.requested_sky) {
            (Some(rendered), Some(requested)) => !rendered.reusable(requested),
            _ => true,
        }
    }

    pub(crate) fn cloud_shadow_needs_update(&self) -> bool {
        let keys = self.0.lock().expect("atmospheric pass cache");
        match (keys.rendered_cloud, keys.requested_cloud) {
            (Some(rendered), Some(requested)) => !rendered.reusable(requested),
            _ => true,
        }
    }

    pub(crate) fn environment_needs_update(&self) -> bool {
        let keys = self.0.lock().expect("atmospheric pass cache");
        match (keys.rendered_environment, keys.requested_environment) {
            (Some(rendered), Some(requested)) => !rendered.reusable(requested),
            _ => true,
        }
    }

    pub(crate) fn mark_sky_rendered(&self) {
        let mut keys = self.0.lock().expect("atmospheric pass cache");
        keys.rendered_sky = keys.requested_sky;
    }

    pub(crate) fn mark_cloud_shadow_rendered(&self) {
        let mut keys = self.0.lock().expect("atmospheric pass cache");
        keys.rendered_cloud = keys.requested_cloud;
    }

    pub(crate) fn mark_environment_rendered(&self) {
        let mut keys = self.0.lock().expect("atmospheric pass cache");
        keys.rendered_environment = keys.requested_environment;
        keys.environment_generation = keys.environment_generation.wrapping_add(1);
    }

    pub(crate) fn environment_generation(&self) -> u64 {
        self.0
            .lock()
            .expect("atmospheric pass cache")
            .environment_generation
    }

    pub(crate) fn indirect_sources_ready(&self) -> bool {
        !self.sky_needs_update()
            && !self.environment_needs_update()
            && !self.cloud_shadow_needs_update()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;

    fn frame() -> EnhancedFrameGpu {
        let mut frame = EnhancedFrameGpu::zeroed();
        frame.atmosphere.x = 1.0;
        frame.celestial = Vec3::Y.extend(0.0);
        frame.light_direction = Vec3::Y.extend(1.0);
        frame.camera_time.y = 64.0;
        frame.clouds = Vec4::new(0.5, 196.0, 64.0, 0.0);
        frame.cloud_shadow.z = 512.0;
        frame.flags.x = FEATURE_VOLUMETRIC_CLOUDS;
        frame
    }

    fn submitted(cache: &AtmosphereCache, frame: &EnhancedFrameGpu) {
        cache.prepare(frame);
        cache.mark_sky_rendered();
        cache.mark_cloud_shadow_rendered();
        cache.mark_environment_rendered();
    }

    #[test]
    fn indirect_transport_waits_for_current_atmosphere_cloud_shadow_and_incident_sky() {
        let cache = AtmosphereCache::default();
        let mut frame = frame();
        cache.prepare(&frame);
        assert!(!cache.indirect_sources_ready());
        cache.mark_cloud_shadow_rendered();
        assert!(!cache.indirect_sources_ready());
        cache.mark_sky_rendered();
        assert!(!cache.indirect_sources_ready());
        let generation = cache.environment_generation();
        cache.mark_environment_rendered();
        assert_eq!(cache.environment_generation(), generation + 1);
        assert!(cache.indirect_sources_ready());
        frame.projection.y += 1.0;
        cache.prepare(&frame);
        assert!(!cache.indirect_sources_ready());
        cache.mark_sky_rendered();
        cache.mark_environment_rendered();
        assert!(cache.indirect_sources_ready());
    }

    #[test]
    fn unchanged_inputs_do_not_redraw_persistent_atmospheric_targets() {
        let cache = AtmosphereCache::default();
        let mut frame = frame();
        cache.prepare(&frame);
        assert!(cache.sky_needs_update() && cache.cloud_shadow_needs_update());
        submitted(&cache, &frame);
        frame.camera_time.x = 3.0;
        frame.camera_time.z = 7.0;
        frame.temporal.x = 12.0;
        frame.camera_time.w = 0.1;
        frame.viewport = Vec4::new(1920.0, 1080.0, 0.0, 0.0);
        cache.prepare(&frame);
        assert!(!cache.sky_needs_update() && !cache.cloud_shadow_needs_update());
        assert!(!cache.environment_needs_update());
        frame.camera_time.w = 40.0;
        cache.prepare(&frame);
        assert!(cache.environment_needs_update());
        cache.mark_environment_rendered();
        cache.prepare(&frame);
        assert!(!cache.environment_needs_update());
    }

    #[test]
    fn source_irradiance_and_sky_fill_changes_refresh_sky_without_redrawing_cloud_density() {
        let baseline = frame();
        let changes: [fn(&mut EnhancedFrameGpu); 3] = [
            |frame| frame.projection.y += 1.0,
            |frame| frame.projection.z += 0.1,
            |frame| frame.light_colour.w += 0.01,
        ];
        for change in changes {
            let cache = AtmosphereCache::default();
            submitted(&cache, &baseline);
            let mut changed = baseline;
            change(&mut changed);
            cache.prepare(&changed);
            assert!(cache.sky_needs_update());
            assert!(cache.environment_needs_update());
            assert!(!cache.cloud_shadow_needs_update());
            cache.mark_sky_rendered();
            cache.mark_environment_rendered();
            cache.prepare(&changed);
            assert!(!cache.sky_needs_update());
            assert!(!cache.environment_needs_update());
        }
    }

    #[test]
    fn pending_passes_stay_dirty_until_recorded_and_accumulated_drift_refreshes() {
        let cache = AtmosphereCache::default();
        let mut frame = frame();
        cache.prepare(&frame);
        cache.prepare(&frame);
        assert!(cache.sky_needs_update() && cache.cloud_shadow_needs_update());
        submitted(&cache, &frame);
        frame.clouds.w = 0.1;
        cache.prepare(&frame);
        assert!(!cache.cloud_shadow_needs_update());
        frame.clouds.w = 0.6;
        cache.prepare(&frame);
        assert!(cache.cloud_shadow_needs_update());
        frame.celestial = Vec3::new(0.0004, 1.0, 0.0).normalize().extend(0.0);
        cache.prepare(&frame);
        assert!(cache.sky_needs_update());
    }

    #[test]
    fn absent_directional_light_caches_clear_shadows_until_the_source_returns() {
        let cache = AtmosphereCache::default();
        let mut frame = frame();
        submitted(&cache, &frame);
        frame.light_direction.w = 0.0;
        cache.prepare(&frame);
        assert!(cache.cloud_shadow_needs_update());
        cache.mark_cloud_shadow_rendered();
        frame.clouds.w += 8.0;
        frame.ambient_colour.w = 0.5;
        cache.prepare(&frame);
        assert!(!cache.cloud_shadow_needs_update());
        frame.light_direction.w = 0.01;
        cache.prepare(&frame);
        assert!(cache.cloud_shadow_needs_update());
    }

    #[test]
    fn flying_above_the_layer_keeps_the_same_shadow_receiver_plane() {
        let cache = AtmosphereCache::default();
        let mut frame = frame();
        frame.camera_time.y = frame.clouds.y;
        submitted(&cache, &frame);
        frame.camera_time.y = frame.clouds.y + frame.clouds.z + 64.0;
        cache.prepare(&frame);
        assert!(!cache.cloud_shadow_needs_update());
        frame.camera_time.y = frame.clouds.y - 32.0;
        cache.prepare(&frame);
        assert!(cache.cloud_shadow_needs_update());
    }

    #[test]
    fn weather_phase_layer_center_and_horizon_changes_invalidate_the_affected_pass() {
        let frame = frame();
        let alterations: [fn(&mut EnhancedFrameGpu); 2] = [
            |frame: &mut EnhancedFrameGpu| frame.ambient_colour.w = 0.2,
            |frame: &mut EnhancedFrameGpu| frame.atmosphere.x = 0.0,
        ];
        for alter in alterations {
            let cache = AtmosphereCache::default();
            submitted(&cache, &frame);
            let mut changed = frame;
            alter(&mut changed);
            cache.prepare(&changed);
            assert!(cache.sky_needs_update() && cache.cloud_shadow_needs_update());
        }
        let cache = AtmosphereCache::default();
        submitted(&cache, &frame);
        let mut changed = frame;
        changed.celestial.w = 4.0;
        cache.prepare(&changed);
        assert!(cache.sky_needs_update());
        assert!(!cache.cloud_shadow_needs_update());
        changed = frame;
        changed.cloud_shadow.x += 64.0;
        cache.prepare(&changed);
        assert!(cache.cloud_shadow_needs_update());
        changed = frame;
        changed.clouds.z += 8.0;
        cache.prepare(&changed);
        assert!(cache.cloud_shadow_needs_update());
        changed = frame;
        changed.light_direction = Vec3::new(1.0, 0.009, 0.0).normalize().extend(1.0);
        cache.prepare(&changed);
        assert!(cache.cloud_shadow_needs_update());
        changed = frame;
        changed.flags.x &= !FEATURE_VOLUMETRIC_CLOUDS;
        cache.prepare(&changed);
        assert!(cache.environment_needs_update());
        cache.mark_environment_rendered();
        changed.clouds.w += 8.0;
        cache.prepare(&changed);
        assert!(!cache.environment_needs_update());
        changed.flags.x |= FEATURE_VOLUMETRIC_CLOUDS;
        cache.prepare(&changed);
        assert!(cache.environment_needs_update());
    }
}
