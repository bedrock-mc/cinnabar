//! Registry-generation joins for biome and fog sampling.

use std::{collections::BTreeMap, sync::Arc};

use assets::{BiomeRule, BiomeVisualProfile, FogProfile, RuntimeAssets, RuntimeAtmosphereAssets};

use super::super::{EnvironmentProfileRoute, profile_lookup::dimension_fallback_biome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BiomeProfileIndex(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FogProfileIndex(pub usize);

#[derive(Clone)]
struct Binding {
    biome: BiomeProfileIndex,
    fog: Option<FogProfileIndex>,
    route: Arc<EnvironmentProfileRoute>,
}

/// Joins immutable registries once and retains their generation identity.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct FogBindings {
    atmosphere: Option<Arc<RuntimeAtmosphereAssets>>,
    biome_generation: Option<[u8; 32]>,
    by_id: BTreeMap<u32, Option<Binding>>,
    defaults: [Option<Binding>; 3],
    pub default_fog: Option<FogProfileIndex>,
}

impl FogBindings {
    /// Rebinds typed indices only when either owning registry is replaced.
    pub fn refresh(
        &mut self,
        world: &RuntimeAssets,
        atmosphere: Option<&Arc<RuntimeAtmosphereAssets>>,
    ) {
        let generation = world.provenance().biome_registry_sha256;
        let same = match (&self.atmosphere, atmosphere) {
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            (None, None) => true,
            _ => false,
        };
        if same && self.biome_generation == Some(generation) {
            return;
        }
        self.atmosphere = atmosphere.cloned();
        self.biome_generation = Some(generation);
        self.by_id.clear();
        self.defaults = Default::default();
        self.default_fog = None;
        let Some(atmosphere) = atmosphere else { return };
        self.compile(
            &world.biome_assets().rules,
            atmosphere.biome_profiles(),
            atmosphere.fog_profiles(),
        );
    }

    /// Compiles palette bindings and shared diagnostic routes from one generation.
    fn compile(
        &mut self,
        rules: &[BiomeRule],
        profiles: &[BiomeVisualProfile],
        fogs: &[FogProfile],
    ) {
        self.by_id.clear();
        let bind = |name: &str| {
            let biome = profiles
                .binary_search_by(|profile| profile.biome_identifier.as_ref().cmp(name))
                .ok()?;
            let profile = &profiles[biome];
            Some(Binding {
                biome: BiomeProfileIndex(biome),
                fog: fogs
                    .binary_search_by(|fog| fog.identifier.cmp(&profile.fog_identifier))
                    .ok()
                    .map(FogProfileIndex),
                route: Arc::new(EnvironmentProfileRoute {
                    biome_identifier: Some(Arc::from(profile.biome_identifier.as_ref())),
                    fog_identifier: Some(Arc::from(profile.fog_identifier.as_ref())),
                    atmosphere_identifier: Some(Arc::from(profile.atmosphere_identifier.as_ref())),
                    provisional_lighting_identifier: Some(Arc::from(
                        profile.lighting_identifier.as_ref(),
                    )),
                }),
            })
        };
        for rule in rules {
            self.by_id.insert(rule.id, bind(&rule.name));
        }
        self.defaults = std::array::from_fn(|dimension| {
            dimension_fallback_biome(dimension as i32).and_then(bind)
        });
        self.default_fog = fogs
            .binary_search_by(|fog| fog.identifier.as_ref().cmp("minecraft:fog_default"))
            .ok()
            .map(FogProfileIndex);
    }

    /// Builds synthetic registry bindings for behavior tests without texture carriers.
    #[cfg(test)]
    pub(super) fn for_profiles(
        rules: &[BiomeRule],
        profiles: &[BiomeVisualProfile],
        fogs: &[FogProfile],
    ) -> Self {
        let mut result = Self::default();
        result.compile(rules, profiles, fogs);
        result
    }

    /// Returns the sample's prebound fog; only unknown biome IDs use the dimension fallback.
    pub fn fog(&self, raw: Option<u32>, dimension: i32) -> Option<FogProfileIndex> {
        match raw.and_then(|raw| self.by_id.get(&raw)) {
            Some(binding) => binding.as_ref().and_then(|binding| binding.fog),
            None => self.fallback(dimension).and_then(|binding| binding.fog),
        }
    }

    /// Resolves the camera profile with its existing dimension fallback and shared route names.
    pub fn camera(
        &self,
        raw: Option<u32>,
        dimension: i32,
    ) -> (
        Option<BiomeProfileIndex>,
        Option<Arc<EnvironmentProfileRoute>>,
        Option<FogProfileIndex>,
    ) {
        let binding = raw
            .and_then(|raw| self.by_id.get(&raw))
            .and_then(|binding| binding.as_ref())
            .or_else(|| self.fallback(dimension));
        (
            binding.map(|binding| binding.biome),
            binding.map(|binding| Arc::clone(&binding.route)),
            binding.and_then(|binding| binding.fog),
        )
    }

    /// Returns the explicit fallback profile for supported dimension IDs.
    fn fallback(&self, dimension: i32) -> Option<&Binding> {
        self.defaults
            .get(usize::try_from(dimension).ok()?)?
            .as_ref()
    }
}

#[cfg(test)]
mod tests;
