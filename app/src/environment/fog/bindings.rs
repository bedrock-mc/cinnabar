//! Registry-generation joins for biome and fog sampling.

use std::{collections::BTreeMap, sync::Arc};

use assets::{BiomeVisualProfile, FogProfile, ResolvedBiomeTints, RuntimeAtmosphereAssets};

#[cfg(test)]
use assets::BiomeRule;

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

struct BiomeBinding {
    identifier: Arc<str>,
    profile: Option<Binding>,
}

/// Joins immutable registries once and retains their generation identity.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct FogBindings {
    atmosphere: Option<Arc<RuntimeAtmosphereAssets>>,
    biome_generation: Option<meshing::ChunkBiomeTintIdentity>,
    /// Published atmosphere identity that owns these indices and routes.
    pub(crate) atmosphere_generation: Option<[u8; 32]>,
    by_id: BTreeMap<u32, BiomeBinding>,
    defaults: [Option<Binding>; 3],
    pub default_fog: Option<FogProfileIndex>,
}

impl FogBindings {
    /// Rebinds admitted session names only when their registry or atmosphere changes.
    pub fn refresh(
        &mut self,
        registry: &ResolvedBiomeTints,
        generation: meshing::ChunkBiomeTintIdentity,
        atmosphere_assets: Option<&render::AtmosphereTextureAssets>,
    ) {
        let atmosphere = atmosphere_assets.and_then(|assets| assets.runtime());
        let atmosphere_generation = atmosphere_assets
            .filter(|assets| assets.runtime().is_some())
            .map(|assets| assets.identity());
        let same = match (&self.atmosphere, atmosphere) {
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            (None, None) => true,
            _ => false,
        };
        if same
            && self.biome_generation == Some(generation)
            && self.atmosphere_generation == atmosphere_generation
        {
            return;
        }
        self.atmosphere = atmosphere.cloned();
        self.atmosphere_generation = atmosphere_generation;
        self.biome_generation = Some(generation);
        self.by_id.clear();
        self.defaults = Default::default();
        self.default_fog = None;
        self.compile(
            Self::registry_identifiers(registry),
            atmosphere.map_or(&[][..], |assets| assets.biome_profiles()),
            atmosphere.map_or(&[][..], |assets| assets.fog_profiles()),
        );
    }

    /// Joins dense admitted records to their names without scanning missing raw-ID slots.
    fn registry_identifiers(registry: &ResolvedBiomeTints) -> impl Iterator<Item = (u32, &str)> {
        registry
            .records
            .iter()
            .zip(&registry.biomes)
            .filter_map(|(record, biome)| {
                biome
                    .as_ref()
                    .map(|biome| (record.raw_id, biome.identifier.as_ref()))
            })
    }

    /// Compiles palette bindings and shared diagnostic routes from one generation.
    fn compile<'a>(
        &mut self,
        names: impl IntoIterator<Item = (u32, &'a str)>,
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
        for (id, name) in names {
            let profile = bind(name);
            let identifier = profile
                .as_ref()
                .and_then(|binding| binding.route.biome_identifier.clone())
                .unwrap_or_else(|| Arc::from(name));
            self.by_id.insert(
                id,
                BiomeBinding {
                    identifier,
                    profile,
                },
            );
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
        let mut result = Self {
            atmosphere_generation: Some(Default::default()),
            ..Default::default()
        };
        result.compile(
            rules.iter().map(|rule| (rule.id, rule.name.as_ref())),
            profiles,
            fogs,
        );
        result
    }

    /// Shares the admitted biome name even when it has no visual profile.
    pub fn identifier(&self, raw: Option<u32>) -> Option<Arc<str>> {
        self.by_id
            .get(&raw?)
            .map(|binding| Arc::clone(&binding.identifier))
    }

    /// Uses the dimension profile when a sample has no resolved visual profile.
    pub fn fog(&self, raw: Option<u32>, dimension: i32) -> Option<FogProfileIndex> {
        self.binding(raw, dimension).and_then(|binding| binding.fog)
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
        let binding = self.binding(raw, dimension);
        (
            binding.map(|binding| binding.biome),
            binding.map(|binding| Arc::clone(&binding.route)),
            binding.and_then(|binding| binding.fog),
        )
    }

    /// Resolves an authored visual binding or the dimension default for unprofiled IDs.
    fn binding(&self, raw: Option<u32>, dimension: i32) -> Option<&Binding> {
        raw.and_then(|raw| self.by_id.get(&raw))
            .and_then(|binding| binding.profile.as_ref())
            .or_else(|| self.fallback(dimension))
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
