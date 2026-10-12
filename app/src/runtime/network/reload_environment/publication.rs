//! Publishes prepared environment resources within the accepted pack transaction.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Res, ResMut},
};
use client_presentation::camera::overlay_publish::ScreenOverlayBase;
use render_scene::{
    AtmosphereTextureAssets, ParticleSimulation, ScreenOverlayScene, WeatherTextureAssets,
};

use super::{EnvironmentBase, PreparedEnvironment};

#[derive(SystemParam)]
pub(crate) struct EnvironmentPublication<'w> {
    base: Option<Res<'w, EnvironmentBase>>,
    overlay_base: Option<Res<'w, ScreenOverlayBase>>,
    atmosphere: Option<ResMut<'w, AtmosphereTextureAssets>>,
    weather: Option<ResMut<'w, WeatherTextureAssets>>,
    screen_overlays: Option<ResMut<'w, ScreenOverlayScene>>,
    particles: Option<ResMut<'w, ParticleSimulation>>,
}

impl EnvironmentPublication<'_> {
    /// Copies startup resources for the worker, never the currently overridden scene.
    pub(in crate::runtime::network) fn base(&self) -> Option<EnvironmentBase> {
        let mut base = self.base.as_deref()?.clone();
        base.screen_overlays = self.overlay_base.as_deref().and_then(|base| base.0.clone());
        Some(base)
    }

    /// Applies only resources rebuilt by the accepted revision and session generation.
    pub(in crate::runtime::network) fn publish(&mut self, prepared: PreparedEnvironment) {
        if let Some(next) = prepared.atmosphere {
            if let Some(current) = self.atmosphere.as_mut()
                && current.identity() != next.atmosphere.identity()
            {
                **current = next.atmosphere;
            }
            if let Some(current) = self.weather.as_mut()
                && current.identity() != next.weather.identity()
            {
                **current = next.weather;
            }
            if let Some(scene) = self.screen_overlays.as_mut()
                && scene.textures != next.screen_overlays
            {
                scene.set_textures(next.screen_overlays);
            }
        }
        if let (Some(current), Some(next)) = (self.particles.as_mut(), prepared.particles) {
            current.0 = next;
        }
    }
}
