//! Resolves custom sampler names and shared texture-unit overrides.

use crate::buffers::{canonical_sampler_name, unit_zero_texture};
use crate::reflection::{SamplerClass, TextureDimension};
use std::collections::HashMap;

/// Custom sampler bindings, split between declared names and shared texture units.
#[derive(Default)]
pub(crate) struct SamplerClasses {
    /// Raw texture's declared name and shape → reflected name and class.
    pub named: HashMap<(String, TextureDimension), (String, SamplerClass)>,
    /// Canonical texture-unit name and shape (`None` for images) → sampler class.
    pub units: HashMap<(String, Option<TextureDimension>), SamplerClass>,
}

impl SamplerClasses {
    /// Resolves the declared sampler before falling back to the texture on unit zero.
    pub fn resolve(
        &self,
        name: &str,
        dimension: TextureDimension,
        unit_zero: bool,
        geometry: bool,
    ) -> (String, SamplerClass) {
        let declared = super::legacy::sampler_name(name);
        if let Some(binding) = self.binding(declared, dimension) {
            return binding;
        }
        let builtin = if declared == name {
            name.to_string()
        } else {
            canonical_sampler_name(declared)
        };
        let bound = if unit_zero {
            unit_zero_texture(&canonical_sampler_name(&builtin), geometry)
                .map_or_else(|| builtin.clone(), str::to_owned)
        } else {
            builtin
        };
        let class = self
            .unit(&bound, dimension)
            .unwrap_or_else(|| SamplerClass::for_name(&bound));
        (bound, class)
    }

    /// Finds a texture-unit override, with shape-specific textures taking priority over images.
    fn unit(&self, name: &str, dimension: TextureDimension) -> Option<SamplerClass> {
        let name = crate::buffers::canonical_sampler_name(name);
        self.units
            .get(&(name.clone(), Some(dimension)))
            .or_else(|| self.units.get(&(name, None)))
            .copied()
    }

    /// Resolves an explicitly named binding before a shared texture-unit override.
    fn binding(&self, name: &str, dimension: TextureDimension) -> Option<(String, SamplerClass)> {
        self.named
            .get(&(name.to_string(), dimension))
            .cloned()
            .or_else(|| {
                self.unit(name, dimension)
                    .map(|class| (name.to_string(), class))
            })
    }
}
