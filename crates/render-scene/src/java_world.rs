//! Shared optional Java visual resources and the active world-style selection.

use assets::{
    AtmosphereRole, AtmosphereTexture, RuntimeAtmosphereAssets, java_world::JavaWorldAssets,
};
use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use std::sync::Arc;

/// Bedrock pack replacements retain priority over the Java preset's base sky textures.
#[derive(Resource, ExtractResource, Clone, Debug, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct JavaWorldVisuals {
    assets: Option<Arc<JavaWorldAssets>>,
    identity: [u8; 32],
    base_sky: [[u8; 32]; AtmosphereRole::ALL.len()],
    enabled: bool,
    base_tints: Arc<std::collections::BTreeMap<Box<str>, assets::LinearBiomeTints>>,
}

impl JavaWorldVisuals {
    /// Binds authenticated Java inputs to the unmodified Bedrock sky they may replace.
    pub fn new(
        assets: Arc<JavaWorldAssets>,
        identity: [u8; 32],
        bedrock: &RuntimeAtmosphereAssets,
        world: &assets::RuntimeAssets,
    ) -> Self {
        Self {
            assets: Some(assets),
            identity,
            base_sky: AtmosphereRole::ALL.map(|role| {
                bedrock
                    .texture(role)
                    .expect("validated sky role")
                    .pixels_sha256
            }),
            enabled: false,
            base_tints: Arc::new(
                world
                    .biome_assets()
                    .resolve_live(&[])
                    .map(|resolved| {
                        resolved
                            .biomes
                            .iter()
                            .zip(resolved.records.iter())
                            .filter_map(|(biome, tint)| {
                                Some((biome.as_ref()?.identifier.clone(), *tint))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
        }
    }

    /// Borrows the installed base colours so server-authored replacements retain priority.
    pub fn base_biome_tint(&self, identifier: &str) -> Option<&assets::LinearBiomeTints> {
        self.base_tints.get(identifier)
    }

    /// Applies the user's preference only when its local resources are available.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled && self.assets.is_some();
    }

    /// Reports whether the optional world preset can be selected.
    pub fn available(&self) -> bool {
        self.assets.is_some()
    }

    /// Reports the effective style, independent of player animations and quality controls.
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Borrows active Java input data without cloning its resource tables.
    pub fn assets(&self) -> Option<&JavaWorldAssets> {
        self.enabled.then_some(self.assets.as_deref()).flatten()
    }

    /// Includes style changes in the texture cache's stable identity.
    pub fn texture_identity(&self, mut bedrock: [u8; 32]) -> [u8; 32] {
        if self.enabled {
            for (byte, java) in bedrock.iter_mut().zip(self.identity) {
                *byte ^= java;
            }
        }
        bedrock
    }

    /// Selects Java base artwork while leaving a server-authored replacement untouched.
    pub fn sky_texture<'a>(&'a self, bedrock: &'a AtmosphereTexture) -> &'a AtmosphereTexture {
        self.assets()
            .filter(|_| self.base_sky[bedrock.role as usize - 1] == bedrock.pixels_sha256)
            .map_or(bedrock, |java| java.atmosphere_texture(bedrock.role))
    }
}
