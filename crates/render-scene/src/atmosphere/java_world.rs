//! Adapts evaluated Java world values to the shared GPU frame.

use super::{AtmosphereFrame, SkyKind, celestial};
use assets::java_world::Attribute as A;
use bevy::prelude::Vec4;
use render_model::java_world::JavaWorldFrame;

impl AtmosphereFrame {
    /// Selects the optional world style while retaining camera medium and dimension ownership.
    pub fn with_java_world(
        mut self,
        frame: JavaWorldFrame,
        renderer_ticks: f64,
        cloud_width: u32,
    ) -> Self {
        let period = f64::from(cloud_width.max(1)) * 400.0;
        let scroll =
            renderer_ticks.max(0.0).rem_euclid(period) * meshing::java_clouds::SCROLL_PER_TICK;
        self.fog_end_time.z = -(scroll / super::CLOUD_TEXTURE_WORLD_PERIOD) as f32;
        let environment = frame.environment;
        let angle = environment.value(A::SunAngle).scalar() / 360.0;
        let sun = celestial::sun_direction(angle);
        let moon = celestial::sun_direction(environment.value(A::MoonAngle).scalar() / 360.0);
        self.sun_direction_daylight = Vec4::new(sun[0], sun[1], sun[2], frame.lightmap.sky_factor);
        self.moon_direction_phase =
            Vec4::new(moon[0], moon[1], moon[2], self.moon_direction_phase.w);
        let sky = celestial::rgb_to_linear(environment.value(A::SkyColour).rgb());
        self.sky_zenith_rain = Vec4::new(sky[0], sky[1], sky[2], self.sky_zenith_rain.w);
        let fog = celestial::rgb_to_linear(frame.fog_colour);
        self.fog_color_start = Vec4::new(fog[0], fog[1], fog[2], frame.render_fog[0]);
        self.sky_horizon_thunder = Vec4::new(fog[0], fog[1], fog[2], self.sky_horizon_thunder.w);
        self.fog_end_time.x = frame.render_fog[1];
        let sunrise = environment.value(A::Sunrise).0;
        let rgb = celestial::rgb_to_linear([sunrise[0], sunrise[1], sunrise[2]]);
        self.sunrise_band = Vec4::new(rgb[0], rgb[1], rgb[2], sunrise[3]);
        self.sky_extra.x = if self.sky_kind() == SkyKind::Overworld {
            environment.value(A::Stars).scalar()
        } else {
            0.0
        };
        self.sky_extra.y = angle;
        self.java_fog = Vec4::new(1.0, frame.fog[0], frame.fog[1], frame.sky_fog_end);
        self.java_sky = Vec4::new(
            environment.value(A::StarAngle).scalar() / 360.0,
            environment.value(A::CloudHeight).scalar(),
            frame.cloud_fog_end,
            0.0,
        );
        self.java_cloud = Vec4::from_array(environment.value(A::CloudColour).0);
        self
    }

    /// Reports the effective world preset to renderer-owned caches and shader paths.
    pub fn java_world_visuals(self) -> bool {
        self.java_fog.x > 0.0
    }

    /// Returns the active cloud underside without changing the default Bedrock geometry.
    pub fn cloud_height(self) -> f32 {
        if self.java_world_visuals() {
            self.java_sky.y
        } else {
            meshing::CLOUD_UNDERSIDE_Y
        }
    }
}

impl AtmosphereFrame {
    /// Returns the evaluated cloud visibility range for this frame.
    pub fn java_cloud_fog_end(self) -> f32 {
        self.java_sky.z
    }
}
